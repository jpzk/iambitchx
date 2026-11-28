//! # The BitchX client state machine
//!
//! Pure state: numbered windows, the input line, and slash-command dispatch.
//! Keys and Matrix events go in, [`Effect`]s for the Matrix side come out.
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};

use crate::bitchx::app::logo::header_line;
use crate::bitchx::channels::normalize_join;
use crate::bitchx::commands::{BitchxCommand, COMMAND_NAMES, parse};
use crate::bitchx::format::names_cell_width;
use crate::bitchx::input::Input;
use crate::bitchx::mention::Matcher;
use crate::bitchx::model::{
    Effect, Line, Member, Presence, register_me, same_localpart, whois_idle,
};
use crate::bitchx::status::{BxStatus, UserCounts};
use crate::bitchx::vars::{Vars, lookup};

mod admin;
mod away;
mod debug;
mod events;
mod ignore;
mod keys;
mod logging;
mod logo;
mod state;
mod theme;
mod verify;

pub(crate) use logo::{LOGO_START, Logo, startup_lines_for, utf8_terminal};
pub(crate) use state::{Confirm, DBG_PER_SEC, PendingVerify, Probe, Window, now_ms};
pub(crate) use theme::{PICKER_ITEMS, PICKER_ROWS, Picker};

/// The status window's name, shown where a channel window shows its
/// channel: prompt `[status]`, status bar `[status]`.
pub(crate) const STATUS_NAME: &str = "status";

/// Refnum of the status window.
pub(crate) const STATUS_REFNUM: usize = 1;

pub(crate) struct App {
    pub me: OwnedUserId,
    pub nick: String,
    pub windows: Vec<Window>,
    /// Index into `windows` (refnum - 1).
    pub current: usize,
    pub input: Input,
    pub away: Option<String>,
    pub lag: Option<u64>,
    /// F37: the sync loop stopped (no more events until a restart).
    pub sync_lost: bool,
    /// `/window split on`: show the `STATUS_FORMAT3` top bar (capture 19).
    pub split: bool,
    pub quit: bool,
    /// Rows of scrollback on screen; set by the renderer, used for paging.
    pub page: usize,
    /// Terminal width in columns; set by the renderer, used for `/list`.
    pub cols: usize,
    /// Fixed clock for tests; `None` uses local time.
    pub clock: Option<String>,
    /// A pending y/n question (`/encrypt`).
    pub confirm: Option<Confirm>,
    /// Key verifications in progress.
    pub verifications: Vec<PendingVerify>,
    /// `/set` variables.
    pub vars: Vars,
    /// Where `/set` persists (`config.toml`); `None`: not saved.
    pub config_toml: Option<PathBuf>,
    /// Unix millis of the last keypress: the `AUTO_AWAY` idle clock.
    pub last_input: i64,
    /// The current away was set by `AUTO_AWAY`; the next keypress clears it.
    pub auto_away: bool,
    /// When we went away (unix millis), for `You were /away for ...`.
    pub away_since: Option<i64>,
    /// MSGCOUNT: query messages received while away (`%A`).
    pub away_msgs: usize,
    /// `AUTO_AWAY` only runs in the interactive client: never in `--smoke`.
    pub auto_away_armed: bool,
    /// Fixed clock for tests (unix millis); `None` uses the real time.
    pub fake_now: Option<i64>,
    /// The last known presence of other users.
    pub presence: HashMap<OwnedUserId, Presence>,
    /// Fixed monotonic clock for tests (presence ages); `None`: real time.
    pub fake_mono: Option<Instant>,
    /// The user of the /whois being printed (from its `WhoisNick` line), so
    /// the idle row can be spliced in before the last row.
    pub whois_user: Option<OwnedUserId>,
    /// Our own Matrix display name, once known (silent self-whois at
    /// connect, `/nick`, profile changes seen in rooms).
    pub display_name: Option<String>,
    /// The silent self-whois that learns [`Self::display_name`].
    pub probe: Probe,
    /// A live mention arrived and `BEEP` is on: ring the bell once at the
    /// next draw (see [`Self::take_bell`]).
    pub bell: bool,
    /// `m.ignored_user_list`: users whose events are hidden.
    pub ignored: Vec<OwnedUserId>,
    /// The first ignore list arrived (later changes are announced).
    pub ignore_known: bool,
    /// Debug tap settings last pushed to [`crate::debug_tap::configure`]
    /// (starts at the tap's own defaults: off, all, raw off).
    dbg_applied: (bool, String, bool),
    /// DBG flood guard per window: (second, lines this second, suppressed).
    dbg_flood: HashMap<usize, (i64, u32, u32)>,
    /// Joins the server confirmed before the room synced with members: room
    /// -> when the join started (unix millis). The synched line and NAMES
    /// wait for the first snapshot that has members.
    pending_joins: HashMap<OwnedRoomId, i64>,
    /// The splash logo currently drawn, and where its lines sit in window 0.
    /// `/theme`: every theme that can be picked.
    pub themes: crate::bitchx::theme::Themes,
    /// The open `/theme` picker, if any.
    pub picker: Option<Picker>,
    /// `/set LOG`: the open log files.
    pub(crate) logger: crate::bitchx::logfile::Logger,
    /// `LOG_DIR` when empty: `$XDG_DATA_HOME/iambitchx/logs`; `None`
    /// (no data dir known): logging needs an explicit `LOG_DIR`.
    pub log_default_dir: Option<PathBuf>,
    logo: Logo,
    logo_start: usize,
    logo_len: usize,
}
impl App {
    pub(crate) fn new(me: OwnedUserId) -> Self {
        Self::new_with_logo(me, Logo::pick(utf8_terminal(), 0, 0))
    }

    /// [`App::new`] with an explicit logo.
    pub(crate) fn new_with_logo(me: OwnedUserId, logo: Logo) -> Self {
        let lines = startup_lines_for(me.localpart(), logo);
        // The logo slot is positional, not found by line kind: `None` has no
        // lines and the original art is `LogoRuns`, so a scan for `Logo`
        // lines would land on the `[ iamBitchX-ver]` credit instead.
        let logo_start = LOGO_START;
        let logo_len = logo.lines(env!("CARGO_PKG_VERSION")).len();
        Self::with_startup(me, lines, logo, logo_start, logo_len)
    }

    /// Re-pick the splash logo now that the terminal size is known: the
    /// original BitchX art needs 78 columns and about 36 rows.
    ///
    /// `utf8` is passed in rather than read from the environment, so the
    /// choice is testable and does not depend on the ambient `TERM`.
    pub(crate) fn fit_logo(&mut self, utf8: bool, width: u16, height: u16) {
        let want = Logo::pick(utf8, width, height);
        if want == self.logo {
            return;
        }
        let lines = want.lines(env!("CARGO_PKG_VERSION"));
        let start = self.logo_start.min(self.windows[0].lines.len());
        let end = (self.logo_start + self.logo_len).min(self.windows[0].lines.len());
        let ts = now_ms();
        self.windows[0].lines.splice(start..end, lines.iter().cloned());
        self.windows[0].times.splice(start..end, vec![ts; lines.len()]);
        // The header names the logo, so it has to follow.
        if let Some(line) = self.windows[0].lines.get_mut(3) {
            *line = header_line(want);
        }
        self.logo = want;
        self.logo_start = start;
        self.logo_len = lines.len();
    }

    fn with_startup(
        me: OwnedUserId,
        lines: Vec<Line>,
        logo: Logo,
        logo_start: usize,
        logo_len: usize,
    ) -> Self {
        register_me(&me);
        let mut status = Window::default();
        for line in lines {
            status.push(None, line);
        }

        App {
            nick: me.localpart().to_string(),
            me,
            windows: vec![status],
            current: 0,
            input: Input::default(),
            away: None,
            lag: None,
            sync_lost: false,
            split: false,
            quit: false,
            page: 20,
            cols: 80,
            clock: None,
            confirm: None,
            verifications: vec![],
            vars: Vars::default(),
            config_toml: None,
            last_input: now_ms(),
            auto_away: false,
            away_since: None,
            away_msgs: 0,
            auto_away_armed: false,
            fake_now: None,
            presence: HashMap::new(),
            fake_mono: None,
            whois_user: None,
            display_name: None,
            probe: Probe::Start,
            bell: false,
            ignored: vec![],
            ignore_known: false,
            dbg_applied: (false, crate::bitchx::vars::DEBUG_ALL.to_string(), false),
            dbg_flood: HashMap::new(),
            pending_joins: HashMap::new(),
            themes: crate::bitchx::theme::Themes::load(None).0,
            picker: None,
            logger: crate::bitchx::logfile::Logger::default(),
            log_default_dir: None,
            logo,
            logo_start,
            logo_len,
        }
    }

    pub(crate) fn cur(&self) -> &Window {
        &self.windows[self.current]
    }

    fn cur_mut(&mut self) -> &mut Window {
        &mut self.windows[self.current]
    }

    pub(crate) fn refnum(&self) -> usize {
        self.current + 1
    }

    pub(crate) fn window_of(&self, room_id: &OwnedRoomId) -> Option<usize> {
        self.windows.iter().position(|w| w.room_id() == Some(room_id))
    }

    /// Print a `-:-` line in the current window.
    pub(crate) fn info(&mut self, text: impl Into<String>) {
        self.cur_mut().push(None, Line::Info(text.into()));
    }

    pub(crate) fn error(&mut self, text: impl Into<String>) {
        self.cur_mut().push(None, Line::Error(text.into()));
    }

    /// The status window header above the numbered channel list (recon 7):
    /// BitchX prints its server list grouped by network; we print the
    /// joined channels grouped by homeserver, numbered by window.
    pub(crate) fn channel_list_header(&mut self, homeserver: &str) {
        self.status_info(format!("Current homeserver: {homeserver}"));
        self.status_info("Channel list:");
        let server = self.me.server_name().to_string();
        self.status_info(format!("[{server}]"));
    }

    /// Print a `-:-` line in the status window.
    pub(crate) fn status_info(&mut self, text: impl Into<String>) {
        self.windows[0].push(None, Line::Info(text.into()));
    }

    /// Switch to window index `idx`; asks for history the first time a room
    /// window is shown.
    pub(crate) fn switch(&mut self, idx: usize) -> Vec<Effect> {
        if idx >= self.windows.len() {
            self.error(format!("No such window: {}", idx + 1));
            return vec![];
        }
        self.current = idx;
        let win = &mut self.windows[idx];
        win.unread = false;
        win.hilite = false;
        win.scroll = 0;
        self.backfill_current()
    }

    pub(crate) fn backfill_current(&mut self) -> Vec<Effect> {
        let win = self.cur_mut();
        match win.room.room_id.clone() {
            Some(room_id) if !win.backfill_requested => {
                win.backfill_requested = true;
                vec![Effect::Backfill(room_id)]
            },
            _ => vec![],
        }
    }

    pub(crate) fn next_window(&mut self) -> Vec<Effect> {
        self.switch((self.current + 1) % self.windows.len())
    }

    pub(crate) fn prev_window(&mut self) -> Vec<Effect> {
        let n = self.windows.len();
        self.switch((self.current + n - 1) % n)
    }

    /// The mention matcher: our mxid, display name and `AUTO_REPLY`.
    pub(crate) fn matcher(&self) -> Matcher {
        Matcher::new(&self.me, self.display_name.as_deref(), &self.vars.auto_reply)
    }

    /// Whether the bell is due; clears it.
    pub(crate) fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    /// Refnums of other windows with an unread mention (shown highlighted
    /// in `Act:`).
    pub(crate) fn hilite(&self) -> Vec<usize> {
        self.windows
            .iter()
            .enumerate()
            .filter(|(i, w)| *i != self.current && w.hilite)
            .map(|(i, _)| i + 1)
            .collect()
    }

    /// Refnums of other windows with unread activity (`Act:`).
    pub(crate) fn act(&self) -> Vec<usize> {
        self.windows
            .iter()
            .enumerate()
            .filter(|(i, w)| *i != self.current && w.unread)
            .map(|(i, _)| i + 1)
            .collect()
    }

    /// Nicks for Tab completion in the current window.
    pub(crate) fn nicks(&self) -> Vec<String> {
        self.cur().room.members.iter().map(|m| m.nick.clone()).collect()
    }

    pub(crate) fn status(&self) -> BxStatus {
        let win = self.cur();
        let me = win.room.members.iter().find(|m| m.user_id == self.me);
        let mut modes = String::new();
        let room_modes = crate::bitchx::modes::RoomModes {
            invite_only: win.room.invite_only,
            moderated: win.room.moderated,
            topic_lock: win.room.topic_lock,
        };
        modes.push_str(&room_modes.letters());
        if win.room.encrypted {
            modes.push('e');
        }
        let users = win.room.room_id.as_ref().map(|_| UserCounts {
            ops: win.room.members.iter().filter(|m| m.is_op()).count(),
            nonops: win.room.members.iter().filter(|m| !m.is_op()).count(),
            voice: win.room.members.iter().filter(|m| m.is_voice()).count(),
        });

        BxStatus {
            time: self
                .clock
                .clone()
                .unwrap_or_else(|| crate::bitchx::status::clock(&chrono::Local::now())),
            nick: self.nick.clone(),
            umodes: String::new(),
            away: self.away.is_some(),
            away_msgs: self.away_msgs,
            chanop: me.is_some_and(Member::is_op),
            voice: me.is_some_and(Member::is_voice),
            channel: win.channel().filter(|_| win.room.dm_peer.is_none()).map(str::to_string),
            window: win.room.room_id.is_none().then(|| STATUS_NAME.to_string()),
            query: win.room.dm_peer.as_ref().map(|_| win.room.name.clone()),
            query_away: win
                .room
                .dm_peer
                .as_ref()
                .and_then(|p| self.presence.get(p))
                .and_then(|p| p.mark(self.mono())),
            modes,
            act: self.act(),
            hilite: self.hilite(),
            users,
            lag: self.lag,
            sync_lost: self.sync_lost,
        }
    }

    /// The input prompt: `[#channel] `.
    pub(crate) fn prompt(&self) -> String {
        match self.cur().channel() {
            Some(c) => format!("[{c}] "),
            None => format!("[{STATUS_NAME}] "),
        }
    }

    // ---- clocks ----------------------------------------------------------

    /// The monotonic clock presence ages tick on (the fake one in tests).
    pub(crate) fn mono(&self) -> Instant {
        self.fake_mono.unwrap_or_else(Instant::now)
    }

    /// Now in unix millis (the fake clock in tests).
    pub(crate) fn now(&self) -> i64 {
        self.fake_now.unwrap_or_else(now_ms)
    }

    // ---- input line ---------------------------------------------------

    /// Handle a submitted input line: a `/command` or text for the room.
    pub(crate) fn submit(&mut self, line: &str) -> Vec<Effect> {
        if line.trim().is_empty() {
            return vec![];
        }

        if let Some(confirm) = self.confirm.take() {
            return self.answer(confirm, line);
        }

        if let Some(rest) = line.strip_prefix('/')
            && !rest.starts_with('/')
        {
            return match parse(rest) {
                Ok(Some(cmd)) => self.command(cmd),
                Ok(None) => vec![],
                Err(e) => {
                    self.error(e.to_string());
                    vec![]
                },
            };
        }

        // `//text` sends `/text` literally.
        let text = line.strip_prefix('/').filter(|r| r.starts_with('/')).unwrap_or(line);
        self.say(text.to_string(), false)
    }

    fn current_room(&mut self) -> Option<OwnedRoomId> {
        let room = self.cur().room_id().cloned();
        if room.is_none() {
            self.error("You're not on a channel. Use /join #channel or Alt-2..9");
        }
        room
    }

    fn say(&mut self, text: String, emote: bool) -> Vec<Effect> {
        match self.current_room() {
            Some(room_id) => {
                self.cur_mut().scroll = 0;
                vec![Effect::Send { room_id, text, emote }]
            },
            None => vec![],
        }
    }

    /// Resolve `@user:server`, `nick@server`, or a nick/localpart in the
    /// current window (then any window), to exactly one user ID.
    ///
    /// F28: a nick is only a localpart, unique per homeserver. When it fits
    /// more than one user (in the current window, or across windows when it
    /// is not in the current one) the command is refused with the
    /// candidates rather than guessing: `/op`, `/kick`, `/msg` ... must hit
    /// the user the operator meant.
    pub(crate) fn resolve_user(&self, target: &str) -> Result<OwnedUserId, String> {
        if target.starts_with('@') {
            return UserId::parse(target).map_err(|e| format!("Invalid user ID {target}: {e}"));
        }
        if let Some((local, server)) = target.split_once('@') {
            let id = format!("@{local}:{server}");
            return UserId::parse(&id).map_err(|e| format!("Invalid nick {target}: {e}"));
        }

        let one = |cands: Vec<OwnedUserId>| -> Option<Result<OwnedUserId, String>> {
            let mut cands = cands;
            cands.sort();
            cands.dedup();
            match cands.len() {
                0 => None,
                1 => Some(Ok(cands.remove(0))),
                _ => {
                    let list: Vec<String> = cands.iter().map(ToString::to_string).collect();
                    Some(Err(format!(
                        "{target}: Ambiguous nick, matches {} -- use the full user ID",
                        list.join(" ")
                    )))
                },
            }
        };
        if let Some(r) = one(self.cur().members_by_nick(target)) {
            return r;
        }
        let everywhere = self.windows.iter().flat_map(|w| w.members_by_nick(target));
        let dms = self
            .windows
            .iter()
            .filter(|w| w.room.name.eq_ignore_ascii_case(target))
            .filter_map(|w| w.room.dm_peer.clone());
        if let Some(r) = one(everywhere.chain(dms).collect()) {
            return r;
        }
        // Not in any window: still refuse if we have seen several.
        let seen = same_localpart(target);
        if seen.len() > 1
            && let Some(r) = one(seen)
        {
            return r;
        }
        Err(format!("{target}: No such nick"))
    }

    /// Find a window by refnum, `#name`, room ID or alias. A name that fits
    /// several windows finds none (see [`Self::window_clash`]).
    pub(crate) fn find_window(&self, target: &str) -> Option<usize> {
        if let Ok(n) = target.parse::<usize>() {
            return (n >= 1 && n <= self.windows.len()).then(|| n - 1);
        }
        if let Some(i) = self
            .windows
            .iter()
            .position(|w| w.room_id().is_some_and(|r| r.as_str() == target))
        {
            return Some(i);
        }
        let mut by_name = self
            .windows
            .iter()
            .enumerate()
            .filter(|(_, w)| w.room.name.eq_ignore_ascii_case(target));
        match (by_name.next(), by_name.next()) {
            (Some((i, _)), None) => Some(i),
            _ => None,
        }
    }

    /// F29: room names are free text, so `#rust:matrix.org` may be the name
    /// of more than one room. Commands that pick a window by name refuse
    /// such a name and list the room IDs.
    pub(crate) fn window_clash(&self, target: &str) -> Option<String> {
        let ids: Vec<String> = self
            .windows
            .iter()
            .enumerate()
            .filter(|(_, w)| w.room.name.eq_ignore_ascii_case(target))
            .filter_map(|(i, w)| w.room_id().map(|r| format!("{r} (window {})", i + 1)))
            .collect();
        (ids.len() > 1).then(|| {
            format!(
                "{target}: Ambiguous channel, matches {} -- use the room ID or window number",
                ids.join(", ")
            )
        })
    }

    /// [`Self::find_window`] for a command argument: prints the clash.
    fn window_arg(&mut self, target: &str) -> Option<usize> {
        if let Some(e) = self.window_clash(target) {
            self.error(e);
            return None;
        }
        self.find_window(target)
    }

    /// Recompute [`Window::spoof`] after rooms came or went.
    fn mark_spoofs(&mut self) {
        let own = format!(":{}", self.me.server_name());
        let names: Vec<String> = self.windows.iter().map(|w| w.room.name.to_lowercase()).collect();
        for (i, w) in self.windows.iter_mut().enumerate().skip(1) {
            let name = w.room.name.to_lowercase();
            let clash = names.iter().enumerate().any(|(j, n)| j != i && *n == name);
            // Our own aliases are shown short (`#x`), so `#x:<our server>`
            // is a room name dressed up as one.
            let fake_own = name.starts_with('#') && name.ends_with(&own);
            // M-03: a `#name` taken from the room name rather than an alias
            // is byte-identical to one of our own aliases.
            w.spoof = clash || fake_own || w.room.from_room_name;
        }
    }

    fn user_cmd(&mut self, target: &str) -> Option<OwnedUserId> {
        match self.resolve_user(target) {
            Ok(u) => Some(u),
            Err(e) => {
                self.error(e);
                None
            },
        }
    }

    /// `/set` (list), `/set NAME` (show), `/set NAME value` (set + persist).
    fn set(&mut self, name: Option<String>, value: Option<String>) {
        let show = |app: &mut Self, name: &str| {
            let line = Line::SetVar { name: name.to_string(), value: app.vars.get(name) };
            app.cur_mut().push(None, line);
        };
        let Some(name) = name else {
            for name in crate::bitchx::vars::NAMES {
                show(self, name);
            }
            return;
        };
        // `/set -NAME` clears a variable, as in BitchX.
        let (name, value) = match name.strip_prefix('-') {
            Some(n) => (n.to_string(), Some(String::new())),
            None => (name, value),
        };
        let name = match lookup(&name) {
            Ok(n) => n,
            Err(e) => return self.info(e),
        };
        let Some(value) = value else {
            return show(self, name);
        };
        if let Err(e) = self.vars.set(name, &value) {
            return self.info(e);
        }
        self.apply_debug();
        if name.starts_with("LOG") {
            self.log_settings_changed();
        }
        let shown = self.vars.get(name).unwrap_or_else(|| "<EMPTY>".into());
        self.info(format!("Value of {name} set to {shown}"));
        match self.config_toml.clone() {
            Some(path) => {
                if let Err(e) = self.vars.persist(&path) {
                    self.error(format!("Could not save {}: {e:#}", path.display()));
                }
            },
            None => self.error("Not saved: no config.toml (JSON config)"),
        }
    }

    pub(crate) fn command(&mut self, cmd: BitchxCommand) -> Vec<Effect> {
        match cmd {
            BitchxCommand::Me { text } | BitchxCommand::Action { text } => self.say(text, true),
            BitchxCommand::Msg { target, text } => {
                if let Some(e) = self.window_clash(&target) {
                    self.error(e);
                    return vec![];
                }
                if let Some(idx) = self.find_window(&target).filter(|i| *i > 0) {
                    let room_id = self.windows[idx].room_id().cloned().unwrap();
                    let chan = self.windows[idx].channel().unwrap_or_default().to_string();
                    self.info(format!("-> [{chan}] {text}"));
                    return vec![Effect::Send { room_id, text, emote: false }];
                }
                match self.user_cmd(&target) {
                    Some(user) => {
                        let line = Line::Private {
                            nick: target,
                            mxid: user.to_string(),
                            own: true,
                            text: text.clone(),
                        };
                        self.cur_mut().push(None, line);
                        vec![Effect::Msg { user, text }]
                    },
                    None => vec![],
                }
            },
            BitchxCommand::Query { target: None } => {
                // `/query` alone ends the conversation. A Matrix DM is its own
                // window, so fall back to window 1 and print the BitchX line
                // there.
                if self.current > 0 && self.cur().room.dm_peer.is_some() {
                    let peer = self.cur().room.name.clone();
                    let fx = self.switch(0);
                    self.info(format!("Ending conversation with {peer}"));
                    return fx;
                }
                vec![]
            },
            BitchxCommand::Query { target: Some(target) } => {
                if let Some(e) = self.window_clash(&target) {
                    self.error(e);
                    return vec![];
                }
                if let Some(idx) = self.find_window(&target).filter(|i| *i > 0) {
                    return self.switch(idx);
                }
                self.user_cmd(&target).map(Effect::Query).into_iter().collect()
            },
            BitchxCommand::Join { room, .. } => self.join(&room),
            BitchxCommand::Part { room, reason } => {
                let idx = match room {
                    Some(r) => self.window_arg(&r).filter(|i| *i > 0),
                    None => Some(self.current).filter(|i| *i > 0),
                };
                if let Some(room_id) = idx.and_then(|i| self.windows[i].room_id().cloned()) {
                    vec![Effect::Part { room_id, reason }]
                } else {
                    self.error("You're not on that channel");
                    vec![]
                }
            },
            BitchxCommand::Leave | BitchxCommand::Close => {
                self.command(BitchxCommand::Part { room: None, reason: None })
            },
            BitchxCommand::Quit { .. } => {
                self.quit = true;
                vec![Effect::Quit]
            },
            BitchxCommand::Nick { nick } => vec![Effect::Nick(nick)],
            BitchxCommand::Topic { topic } => self
                .current_room()
                .map(|room_id| Effect::Topic { room_id, topic })
                .into_iter()
                .collect(),
            BitchxCommand::Away { reason } => {
                let msg = reason
                    .or_else(|| Some(self.vars.away_msg.clone()).filter(|m| !m.is_empty()))
                    .unwrap_or_else(|| "away".to_string());
                let nick = self.away_nick_on();
                self.auto_away = false;
                let mut fx = vec![Effect::Away(Some(msg))];
                fx.extend(nick);
                fx
            },
            BitchxCommand::Back => {
                self.auto_away = false;
                let mut fx = vec![Effect::Away(None)];
                fx.extend(self.away_nick_off());
                fx
            },
            BitchxCommand::Clear => {
                let win = self.cur_mut();
                win.clear();
                win.scroll = 0;
                vec![]
            },
            BitchxCommand::Names { target } => {
                self.names(target);
                vec![]
            },
            BitchxCommand::Who { target } => {
                self.who(target);
                vec![]
            },
            BitchxCommand::Whois { target } => {
                if matches!(self.probe, Probe::Pending { .. }) {
                    self.probe = Probe::Done;
                }
                self.user_cmd(&target).map(Effect::Whois).into_iter().collect()
            },
            BitchxCommand::Channels => {
                self.channels();
                vec![]
            },
            BitchxCommand::List(args) => vec![Effect::List(args)],
            BitchxCommand::Window { target: None } => {
                let w = self.cur();
                let name = w.channel().unwrap_or("(status)").to_string();
                self.info(format!("Window {}: {name}", self.refnum()));
                vec![]
            },
            BitchxCommand::Window { target: Some(t) } => {
                if let Some(idx) = self.window_arg(&t) {
                    self.switch(idx)
                } else {
                    self.error(format!("No such window: {t}"));
                    vec![]
                }
            },
            BitchxCommand::WindowSplit(on) => {
                self.split = on.unwrap_or(!self.split);
                let state = if self.split { "ON" } else { "OFF" };
                self.info(format!("Window SPLIT is {state}"));
                vec![]
            },
            BitchxCommand::NextWindow => self.next_window(),
            BitchxCommand::PrevWindow => self.prev_window(),
            BitchxCommand::Invite { target, room } => {
                let room_id = match room {
                    Some(r) => self.window_arg(&r).and_then(|i| self.windows[i].room_id().cloned()),
                    None => self.cur().room_id().cloned(),
                };
                let Some(room_id) = room_id else {
                    self.error("You're not on that channel");
                    return vec![];
                };
                self.user_cmd(&target)
                    .map(|user| Effect::Invite { room_id, user })
                    .into_iter()
                    .collect()
            },
            BitchxCommand::Kick { target, reason } => {
                let Some(room_id) = self.current_room() else {
                    return vec![];
                };
                self.user_cmd(&target)
                    .map(|user| Effect::Kick { room_id, user, reason })
                    .into_iter()
                    .collect()
            },
            BitchxCommand::Op { target } => self.power(&target, 50),
            BitchxCommand::Deop { target } => self.power(&target, 0),
            BitchxCommand::Theme { name } => {
                self.theme_cmd(name);
                vec![]
            },
            BitchxCommand::Help => {
                self.info(format!("iamBitchX commands: /{}", COMMAND_NAMES.join(" /")));
                self.info("/theme [name]: colour themes, Up/Down preview, Enter keep, Esc cancel");
                self.info("/theme terminal: use your terminal's own colours");
                self.info(
                    "/create #x [-public] [-noencrypt]: new channel on your server \
                     (invite-only + encrypted by default)",
                );
                self.info(
                    "/alias add|del #x[:server], /publish on|off: channel aliases, channel list",
                );
                self.info("/ban NICK [reason], /unban NICK, /kickban NICK [reason]");
                self.info("/power NICK 0..100: raw power level (not above your own)");
                self.info(
                    "/ignore [NICK|@user:server], /unignore NICK: account ignore list \
                     (m.ignored_user_list); bare /ignore lists it",
                );
                self.info(
                    "/lastlog [-i] TEXT: search this window (-i any case), /lastlog -clear: drop it",
                );
                self.info(
                    "/set LOG ON|OFF, /set LOG_DIR path: log windows to LOG_DIR/server/window.log",
                );
                self.info(
                    "/mode [+-imtob] [NICK]: i invite-only, m moderated, t topic lock, \
                     o op, b ban; bare /mode shows the modes",
                );
                self.info(
                    "Keys: Alt-1..9 window, Ctrl-N/Ctrl-P next/prev, PgUp/PgDn scroll, Tab nick",
                );
                vec![]
            },
            BitchxCommand::Version => {
                self.info(format!(
                    "iamBitchX {} (iambitchx) - in the style of BitchX by panasync",
                    env!("CARGO_PKG_VERSION")
                ));
                vec![]
            },
            BitchxCommand::Encrypt => self.encrypt(),
            BitchxCommand::Verify(v) => self.verify_cmd(v),
            BitchxCommand::Devices { user } => {
                let user = match user {
                    Some(u) => self.user_cmd(&u),
                    None => Some(self.me.clone()),
                };
                user.map(Effect::Devices).into_iter().collect()
            },
            BitchxCommand::Trust => vec![Effect::Trust],
            BitchxCommand::Set { name, value } => {
                self.set(name, value);
                vec![]
            },
            BitchxCommand::Lastlog { pattern, ignore_case } => {
                self.lastlog(&pattern, ignore_case);
                vec![]
            },
            BitchxCommand::LastlogClear => {
                self.lastlog_clear();
                vec![]
            },
            BitchxCommand::Recover { key } => {
                // Never echo the key.
                self.info("Recovering secrets from secure backup with the given key (********)");
                vec![Effect::Recover(key)]
            },
            BitchxCommand::Mode { target, modes } => self.mode(target, modes),
            BitchxCommand::Create { room, public, encrypt } => self.create(&room, public, encrypt),
            BitchxCommand::Alias { add, alias } => self.alias(add, &alias),
            BitchxCommand::Publish(on) => self.publish(on),
            // On Matrix a ban also removes the member: /kickban is /ban.
            BitchxCommand::Ban { target, reason } | BitchxCommand::Kickban { target, reason } => {
                self.ban(&target, reason)
            },
            BitchxCommand::Unban { target } => self.unban(&target),
            BitchxCommand::Power { target, level } => self.raw_power(&target, level),
            BitchxCommand::Ignore { pattern } => self.ignore(pattern),
            BitchxCommand::Unignore { pattern } => self.unignore(&pattern),
            BitchxCommand::Ctcp { .. } => {
                self.error("Not supported on Matrix");
                vec![]
            },
        }
    }

    fn power(&mut self, target: &str, level: i64) -> Vec<Effect> {
        let Some(room_id) = self.current_room() else {
            return vec![];
        };
        self.user_cmd(target)
            .map(|user| Effect::Power { room_id, user, level })
            .into_iter()
            .collect()
    }

    fn target_window(&mut self, target: Option<String>) -> Option<usize> {
        let idx = match target {
            Some(t) => self.window_arg(&t),
            None => Some(self.current),
        };
        if let Some(i) = idx.filter(|i| *i > 0) {
            Some(i)
        } else {
            self.error("No channel");
            None
        }
    }

    /// NAMES: ` [Users(#chan:n/m)]` then a grid of `[@nick ]` cells (capture 25).
    fn names(&mut self, target: Option<String>) {
        let Some(idx) = self.target_window(target) else {
            return;
        };
        self.names_in(idx);
    }

    fn names_in(&mut self, idx: usize) {
        let now = self.mono();
        let room = &self.windows[idx].room;
        let mut members = room.members.clone();
        members.sort_by(|a, b| b.power.cmp(&a.power).then(a.nick.cmp(&b.nick)));
        let total = members.len();
        let cells: Vec<_> = members
            .iter()
            .map(|m| {
                let flag = if m.is_op() {
                    '@'
                } else if m.is_voice() {
                    '+'
                } else {
                    ' '
                };
                let mark = self.presence.get(&m.user_id).and_then(|p| p.mark(now));
                (flag, m.nick.clone(), mark)
            })
            .collect();
        let width = cells
            .iter()
            .map(|(_, nick, mark)| names_cell_width(nick, mark.as_ref()))
            .max()
            .unwrap_or(0);
        let mut lines = vec![Line::Users { chan: room.name.clone(), shown: total, total }];
        for chunk in cells.chunks(5) {
            lines.push(Line::Names { cells: chunk.to_vec(), width });
        }
        for l in lines {
            self.windows[idx].push(None, l);
        }
    }

    /// A /whois from the Matrix side, with what only the UI knows spliced
    /// in: the peer's presence marker on the nick row, and BitchX's
    /// `WHOIS_IDLE` row (`source/numbers.c:870-877`) before the closing row.
    fn whois_splice(&mut self, line: Line) -> Vec<Line> {
        match line {
            Line::WhoisNick { nick, mxid, info } => {
                let user = UserId::parse(mxid.as_str()).ok();
                let mark = user
                    .as_ref()
                    .and_then(|u| self.presence.get(u))
                    .and_then(|p| p.mark(self.mono()));
                self.whois_user = user;
                match mark {
                    Some(mark) => vec![Line::WhoisMark { nick, mxid, info, mark }],
                    None => vec![Line::WhoisNick { nick, mxid, info }],
                }
            },
            Line::WhoisRow { row, last: true, key, value } => {
                let idle = self
                    .whois_user
                    .take()
                    .and_then(|u| self.presence.get(&u))
                    .and_then(|p| p.age(self.mono()));
                let end = |row| Line::WhoisRow { row, last: true, key, value };
                match idle {
                    Some(idle) => {
                        vec![
                            Line::WhoisRow {
                                row,
                                last: false,
                                key: "idle".into(),
                                value: whois_idle(idle),
                            },
                            end(row.saturating_add(1)),
                        ]
                    },
                    None => vec![end(row)],
                }
            },
            line => vec![line],
        }
    }

    /// WHO: one line per member: `#chan nick H@ mxid (PL n)`.
    fn who(&mut self, target: Option<String>) {
        let Some(idx) = self.target_window(target) else {
            return;
        };
        let room = self.windows[idx].room.clone();
        for m in &room.members {
            let flag = if m.is_op() {
                "H@"
            } else if m.is_voice() {
                "H+"
            } else {
                "H"
            };
            self.info(format!("{} {} {flag} {} (PL {})", room.name, m.nick, m.user_id, m.power));
        }
        self.info(format!("End of /WHO list for {}", room.name));
    }

    /// `/join`: a room ID we are in just switches to its window; anything
    /// else asks the Matrix side, which opens (or focuses) the window on
    /// success. F29: an alias is always resolved by the server, never
    /// matched against room names, which anyone can set to `#rust:x`.
    fn join(&mut self, target: &str) -> Vec<Effect> {
        let own = self.me.server_name().to_string();
        let t = match normalize_join(target, &own) {
            Ok(t) => t,
            Err(e) => {
                self.error(e);
                return vec![];
            },
        };
        let idx = self
            .windows
            .iter()
            .position(|w| w.room_id().is_some_and(|r| r.as_str() == t.id));
        if let Some(idx) = idx.filter(|i| *i > 0) {
            return self.switch(idx);
        }
        self.info(format!("Joining {}...", t.id));
        vec![Effect::Join(t)]
    }

    fn channels(&mut self) {
        let rows: Vec<String> = self
            .windows
            .iter()
            .enumerate()
            .skip(STATUS_REFNUM)
            .map(|(i, w)| {
                let what = match &w.room.dm_peer {
                    Some(p) => format!("query {p}"),
                    None => format!("{} users", w.room.members.len()),
                };
                let space = if w.room.space { " [space]" } else { "" };
                format!("{}: {} [{what}]{space}", i + 1, w.room.name)
            })
            .collect();
        if rows.is_empty() {
            self.info("No channels joined");
        }
        for r in rows {
            self.info(r);
        }
    }
}

#[cfg(test)]
impl App {
    pub(crate) fn cur_scroll_for_test(&mut self, scroll: usize) {
        self.cur_mut().scroll = scroll;
    }
}

#[cfg(test)]
pub(crate) mod tests;
