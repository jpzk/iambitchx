//! # The BitchX client state machine
//!
//! Pure state: numbered windows, the input line, and slash-command dispatch.
//! Keys and Matrix events go in, [`Effect`]s for the Matrix side come out.
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};

use crate::bitchx::app::logo::header_line;
use crate::bitchx::channels::{list_lines, normalize_join};
use crate::bitchx::commands::{BitchxCommand, COMMAND_NAMES, VerifyCmd, parse};
use crate::bitchx::e2ee::{encrypt_prompt, sas_lines};
use crate::bitchx::format::names_cell_width;
use crate::bitchx::input::Input;
use crate::bitchx::mention::Matcher;
use crate::bitchx::model::{
    Effect, Line, Member, MxEvent, Presence, RoomSnapshot, VerifyAction, VerifyUpdate, nick_of,
    register_me, same_localpart, whois_idle,
};
use crate::bitchx::status::{BxStatus, UserCounts};
use crate::bitchx::text::{clean_line, clean_snapshot, one_line};
use crate::bitchx::vars::{Vars, expand_auto_away_msg, expand_away_nick, lookup, strip_away_nick};

mod logo;
mod state;

#[cfg(test)]
pub use logo::{BANNER_BLOCKS, BANNER_SHADOW};
pub use logo::{Logo, startup_lines_for, utf8_terminal};
pub use state::{Confirm, DBG_PER_SEC, PendingVerify, Probe, Window, now_ms};

/// F30/F31: everything from the Matrix side is remote text. Strip format
/// characters everywhere and keep one-line fields on one line before any of
/// it is stored or shown.
fn clean_event(ev: MxEvent) -> MxEvent {
    match ev {
        MxEvent::Room(snap) => MxEvent::Room(clean_snapshot(snap)),
        MxEvent::Lines { room_id, lines, backfill } => {
            let lines = lines.into_iter().map(|(id, ts, l)| (id, ts, clean_line(l))).collect();
            MxEvent::Lines { room_id, lines, backfill }
        },
        MxEvent::Info { room_id, line } => MxEvent::Info { room_id, line: clean_line(line) },
        MxEvent::OwnDisplayName(n) => MxEvent::OwnDisplayName(one_line(&n)),
        MxEvent::Away(a) => MxEvent::Away(a.map(|a| one_line(&a))),
        MxEvent::Presence { user, mut presence } => {
            presence.status_msg = presence.status_msg.map(|m| one_line(&m));
            MxEvent::Presence { user, presence }
        },
        MxEvent::Verify { user, update: VerifyUpdate::Cancelled(r) } => MxEvent::Verify {
            user,
            update: VerifyUpdate::Cancelled(one_line(&r)),
        },
        MxEvent::Verify { user, update: VerifyUpdate::Emoji(e) } => {
            let e = e.into_iter().map(|(a, b)| (one_line(&a), one_line(&b))).collect();
            MxEvent::Verify { user, update: VerifyUpdate::Emoji(e) }
        },
        ev => ev,
    }
}

/// The status window's name, shown where a channel window shows its
/// channel: prompt `[status]`, status bar `[status]`.
pub const STATUS_NAME: &str = "status";

/// Refnum of the status window.
pub const STATUS_REFNUM: usize = 1;

/// F37: the line `spawn_sync` sends when the sync loop breaks for good.
fn is_sync_death(line: &Line) -> bool {
    matches!(line, Line::Error(t) if t.starts_with("sync: "))
}

/// The flood guard's ` DBG (suppressed N)`.
fn suppressed_line(n: u32) -> Line {
    Line::Debug {
        cat: String::new(),
        text: format!("(suppressed {n})"),
    }
}

pub struct App {
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
    /// Unix millis of the last keypress: the AUTO_AWAY idle clock.
    pub last_input: i64,
    /// The current away was set by AUTO_AWAY; the next keypress clears it.
    pub auto_away: bool,
    /// When we went away (unix millis), for `You were /away for ...`.
    pub away_since: Option<i64>,
    /// MSGCOUNT: query messages received while away (`%A`).
    pub away_msgs: usize,
    /// AUTO_AWAY only runs in the interactive client: never in `--smoke`.
    pub auto_away_armed: bool,
    /// Fixed clock for tests (unix millis); `None` uses the real time.
    pub fake_now: Option<i64>,
    /// The last known presence of other users.
    pub presence: HashMap<OwnedUserId, Presence>,
    /// Fixed monotonic clock for tests (presence ages); `None`: real time.
    pub fake_mono: Option<Instant>,
    /// The user of the /whois being printed (from its WhoisNick line), so
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
    /// Debug tap settings last pushed to [`crate::debug_tap::configure`]
    /// (starts at the tap's own defaults: off, all, raw off).
    dbg_applied: (bool, String, bool),
    /// DBG flood guard per window: (second, lines this second, suppressed).
    dbg_flood: HashMap<usize, (i64, u32, u32)>,
    /// The splash logo currently drawn, and where its lines sit in window 0.
    logo: Logo,
    logo_start: usize,
    logo_len: usize,
}
impl App {
    pub fn new(me: OwnedUserId) -> Self {
        Self::new_with_logo(me, Logo::pick(utf8_terminal(), 0, 0))
    }

    /// [`App::new`] with an explicit logo.
    pub fn new_with_logo(me: OwnedUserId, logo: Logo) -> Self {
        let lines = startup_lines_for(me.localpart(), logo);
        let logo_start = lines
            .iter()
            .position(|l| matches!(l, Line::Logo(_) | Line::LogoDim(_)))
            .unwrap_or(0);
        let logo_len = lines[logo_start..]
            .iter()
            .take_while(|l| matches!(l, Line::Logo(_) | Line::LogoDim(_)))
            .count();
        Self::with_startup(me, lines, logo, logo_start, logo_len)
    }

    /// Re-pick the splash logo now that the terminal size is known: the
    /// original BitchX art needs 78 columns and about 36 rows.
    ///
    /// `utf8` is passed in rather than read from the environment, so the
    /// choice is testable and does not depend on the ambient `TERM`.
    pub fn fit_logo(&mut self, utf8: bool, width: u16, height: u16) {
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
            dbg_applied: (false, crate::bitchx::vars::DEBUG_ALL.to_string(), false),
            dbg_flood: HashMap::new(),
            logo,
            logo_start,
            logo_len,
        }
    }

    pub fn cur(&self) -> &Window {
        &self.windows[self.current]
    }

    fn cur_mut(&mut self) -> &mut Window {
        &mut self.windows[self.current]
    }

    pub fn refnum(&self) -> usize {
        self.current + 1
    }

    pub fn window_of(&self, room_id: &OwnedRoomId) -> Option<usize> {
        self.windows.iter().position(|w| w.room_id() == Some(room_id))
    }

    /// Print a `-:-` line in the current window.
    pub fn info(&mut self, text: impl Into<String>) {
        self.cur_mut().push(None, Line::Info(text.into()));
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.cur_mut().push(None, Line::Error(text.into()));
    }

    /// The status window header above the numbered channel list (recon 7):
    /// BitchX prints its server list grouped by network; we print the
    /// joined channels grouped by homeserver, numbered by window.
    pub fn channel_list_header(&mut self, homeserver: &str) {
        self.status_info(format!("Current homeserver: {homeserver}"));
        self.status_info("Channel list:");
        let server = self.me.server_name().to_string();
        self.status_info(format!("[{server}]"));
    }

    /// Print a `-:-` line in the status window.
    pub fn status_info(&mut self, text: impl Into<String>) {
        self.windows[0].push(None, Line::Info(text.into()));
    }

    /// Switch to window index `idx`; asks for history the first time a room
    /// window is shown.
    pub fn switch(&mut self, idx: usize) -> Vec<Effect> {
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

    pub fn backfill_current(&mut self) -> Vec<Effect> {
        let win = self.cur_mut();
        match win.room.room_id.clone() {
            Some(room_id) if !win.backfill_requested => {
                win.backfill_requested = true;
                vec![Effect::Backfill(room_id)]
            },
            _ => vec![],
        }
    }

    pub fn next_window(&mut self) -> Vec<Effect> {
        self.switch((self.current + 1) % self.windows.len())
    }

    pub fn prev_window(&mut self) -> Vec<Effect> {
        let n = self.windows.len();
        self.switch((self.current + n - 1) % n)
    }

    /// The mention matcher: our mxid, display name and `AUTO_REPLY`.
    pub fn matcher(&self) -> Matcher {
        Matcher::new(&self.me, self.display_name.as_deref(), &self.vars.auto_reply)
    }

    /// Whether the bell is due; clears it.
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    /// Refnums of other windows with an unread mention (shown highlighted
    /// in `Act:`).
    pub fn hilite(&self) -> Vec<usize> {
        self.windows
            .iter()
            .enumerate()
            .filter(|(i, w)| *i != self.current && w.hilite)
            .map(|(i, _)| i + 1)
            .collect()
    }

    /// Refnums of other windows with unread activity (`Act:`).
    pub fn act(&self) -> Vec<usize> {
        self.windows
            .iter()
            .enumerate()
            .filter(|(i, w)| *i != self.current && w.unread)
            .map(|(i, _)| i + 1)
            .collect()
    }

    /// Nicks for Tab completion in the current window.
    pub fn nicks(&self) -> Vec<String> {
        self.cur().room.members.iter().map(|m| m.nick.clone()).collect()
    }

    pub fn status(&self) -> BxStatus {
        let win = self.cur();
        let me = win.room.members.iter().find(|m| m.user_id == self.me);
        let mut modes = String::new();
        if win.room.invite_only {
            modes.push('i');
        }
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
                .unwrap_or_else(|| crate::bitchx::status::clock(chrono::Local::now())),
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
    pub fn prompt(&self) -> String {
        match self.cur().channel() {
            Some(c) => format!("[{c}] "),
            None => format!("[{STATUS_NAME}] "),
        }
    }

    // ---- Matrix events ------------------------------------------------

    pub fn handle_mx(&mut self, ev: MxEvent) -> Vec<Effect> {
        let ev = clean_event(ev);
        self.apply_debug();
        match ev {
            MxEvent::Debug(line) => self.debug_line(line),
            MxEvent::Room(snap) => return self.upsert_room(snap),
            MxEvent::Lines { room_id, lines, backfill } => {
                let Some(idx) = self.window_of(&room_id) else {
                    return vec![];
                };
                // our own profile change seen in a room (e.g. from another client)
                let own_name = lines.iter().rev().find_map(|(_, _, l)| match l {
                    Line::DisplayName { nick, name: Some(n) } if *nick == self.nick => {
                        Some(n.clone())
                    },
                    _ => None,
                });
                if let Some(name) = own_name.filter(|_| !backfill) {
                    self.own_name(name);
                }
                let matcher = self.matcher();
                let win = &mut self.windows[idx];
                let now = now_ms();
                let lines = lines
                    .into_iter()
                    .map(|(id, ts, l)| (id, ts.unwrap_or(now), win.query_line(l)))
                    .collect();
                if backfill {
                    win.prepend(lines);
                } else {
                    let mut added = false;
                    let mut msgs = 0;
                    let mut mentioned = false;
                    for (id, ts, line) in lines {
                        let incoming = matches!(line, Line::Private { own: false, .. });
                        let mention = matcher.is_mention(&line);
                        if win.push_at(id, ts, line) {
                            added = true;
                            msgs += usize::from(incoming);
                            mentioned |= mention;
                        }
                    }
                    if self.away.is_some() {
                        self.away_msgs += msgs;
                    }
                    if added && idx != self.current {
                        win.unread = true;
                    }
                    // BitchX `do_beep` after an AR line (`source/parse.c:596`).
                    if mentioned {
                        win.hilite |= idx != self.current;
                        self.bell |= self.vars.beep;
                    }
                }
            },
            MxEvent::Left(room_id) => {
                if let Some(idx) = self.window_of(&room_id) {
                    let name = self.windows[idx].room.name.clone();
                    self.windows.remove(idx);
                    self.mark_spoofs();
                    if self.current >= idx && self.current > 0 {
                        self.current -= 1;
                    }
                    self.status_info(format!("You have left {name} (window {})", idx + 1));
                }
            },
            MxEvent::Focus(room_id) => {
                if let Some(idx) = self.window_of(&room_id) {
                    return self.switch(idx);
                }
            },
            MxEvent::Synched { room_id, millis } => {
                if let Some(idx) = self.window_of(&room_id) {
                    self.names_in(idx);
                    let chan = self.windows[idx].room.name.clone();
                    let secs = millis as f64 / 1000.0;
                    let line =
                        Line::Error(format!("Join to {chan} was synched in {secs:.3} secs!!"));
                    self.windows[idx].push(None, line);
                    return self.switch(idx);
                }
            },
            MxEvent::Info { room_id, line } => {
                if self.probe_line(&line) {
                    return vec![];
                }
                if room_id.is_none() && is_sync_death(&line) {
                    self.sync_died(line);
                    return vec![];
                }
                let idx = room_id.and_then(|r| self.window_of(&r)).unwrap_or(self.current);
                for line in self.whois_splice(line) {
                    self.windows[idx].push(None, line);
                }
            },
            MxEvent::OwnDisplayName(name) => {
                self.own_name(name.clone());
                // The nick is the localpart and never changes; /nick only
                // sets the Matrix display name.
                let line = Line::DisplayName { nick: self.nick.clone(), name: Some(name) };
                self.windows[0].push(None, line);
            },
            MxEvent::Away(away) => self.away_changed(away),
            MxEvent::Presence { user, presence } => {
                self.presence.insert(user, presence);
            },
            MxEvent::Lag(lag) => {
                self.lag = Some(lag);
                if self.probe == Probe::Start {
                    self.probe = Probe::Pending { rule: false };
                    return vec![Effect::Whois(self.me.clone())];
                }
            },
            MxEvent::Verify { user, update } => self.verify_update(user, update),
            MxEvent::List { server, search, entries } => {
                // Text width after the line glyph.
                let width = self.cols.saturating_sub(2).max(20);
                for l in list_lines(&server, search.as_deref(), &entries, width) {
                    self.info(l);
                }
            },
        }
        vec![]
    }

    fn upsert_room(&mut self, mut snap: RoomSnapshot) -> Vec<Effect> {
        let Some(room_id) = snap.room_id.clone() else {
            return vec![];
        };
        // F28: nicks are qualified only once every member is known.
        for m in &snap.members {
            nick_of(&m.user_id);
        }
        for m in &mut snap.members {
            m.nick = nick_of(&m.user_id);
        }

        if let Some(idx) = self.window_of(&room_id) {
            self.windows[idx].room = snap;
            self.mark_spoofs();
            return vec![];
        }

        let refnum = self.windows.len() + 1;
        let what = match &snap.dm_peer {
            Some(peer) => format!("query with {peer}"),
            None => format!("{} users, {room_id}", snap.members.len()),
        };
        self.status_info(format!("{refnum:>6}) {} [{what}]", snap.name));

        let mut win = Window { room: snap, ..Default::default() };
        if win.room.dm_peer.is_some() {
            let peer = win.room.name.clone();
            win.push(None, Line::Info(format!("Starting conversation with {peer}")));
        }
        if let Some(topic) = win.room.topic.clone() {
            win.push(None, Line::Topic { nick: None, topic });
        }
        self.windows.push(win);
        self.mark_spoofs();
        vec![]
    }

    // ---- keys ---------------------------------------------------------

    // ---- away / AUTO_AWAY ----------------------------------------------

    /// The monotonic clock presence ages tick on (the fake one in tests).
    pub fn mono(&self) -> Instant {
        self.fake_mono.unwrap_or_else(Instant::now)
    }

    /// Now in unix millis (the fake clock in tests).
    pub fn now(&self) -> i64 {
        self.fake_now.unwrap_or_else(now_ms)
    }

    /// The timer: `check_auto_away` (`source/misc.c:1204-1225`). After
    /// AUTO_AWAY_TIME seconds without a keypress, go away with
    /// `Auto-Away after N mins`, unless already away (by hand or not).
    pub fn tick(&mut self) -> Vec<Effect> {
        self.flush_debug();
        let secs = self.vars.auto_away_time;
        let now = self.now();
        if !self.auto_away_armed
            || !self.vars.auto_away
            || secs == 0
            || self.away.is_some()
            || now - self.last_input < i64::from(secs) * 1000
        {
            return vec![];
        }
        let msg = expand_auto_away_msg(&self.vars.auto_away_msg, secs / 60);
        let nick = self.away_nick_on();
        self.auto_away = true;
        self.away = Some(msg.clone());
        self.away_since = Some(now);
        let mut fx = vec![Effect::Away(Some(msg))];
        fx.extend(nick);
        fx
    }

    /// Keyboard activity: resets the idle clock; ends an automatic away
    /// (a `/away` by hand stays until `/back`).
    pub fn activity(&mut self) -> Vec<Effect> {
        self.last_input = self.now();
        if self.auto_away && self.away.is_some() {
            self.auto_away = false;
            let mut fx = vec![Effect::Away(None)];
            fx.extend(self.away_nick_off());
            return fx;
        }
        vec![]
    }

    // ---- AWAY_NICK -----------------------------------------------------

    /// Going away: switch the display name to AWAY_NICK, remembering (and
    /// persisting) the original. Nothing while already away, when off, or
    /// while our display name is unknown. Never stacks: an already-away
    /// name (`carla^afk`) is unwrapped to its original first.
    fn away_nick_on(&mut self) -> Vec<Effect> {
        let pattern = self.vars.away_nick.clone();
        if self.away.is_some() || self.vars.away_nick_orig.is_some() || pattern.is_empty() {
            return vec![];
        }
        let Some(cur) = self.display_name.clone() else {
            self.error("AWAY_NICK skipped: display name not known yet");
            return vec![];
        };
        let orig = strip_away_nick(&pattern, &cur).unwrap_or_else(|| cur.clone());
        let Some(nick) = expand_away_nick(&pattern, &orig) else {
            return vec![];
        };
        self.vars.away_nick_orig = Some(orig);
        self.save_vars();
        if nick == cur {
            return vec![];
        }
        vec![Effect::Nick(nick)]
    }

    /// Back: restore exactly the display name from before the away nick.
    fn away_nick_off(&mut self) -> Vec<Effect> {
        let Some(orig) = self.vars.away_nick_orig.take() else {
            return vec![];
        };
        self.save_vars();
        if self.display_name.as_deref() == Some(orig.as_str()) {
            return vec![];
        }
        vec![Effect::Nick(orig)]
    }

    /// Our display name is now `name`: one ` iamBitchX:` line per change.
    fn own_name(&mut self, name: String) {
        if self.display_name.as_deref() == Some(name.as_str()) {
            return;
        }
        let msg = match &self.vars.away_nick_orig {
            Some(orig) if *orig != name => {
                format!("Display name is now {name} (away; /back restores {orig})")
            },
            _ => format!("Display name is now {name}"),
        };
        self.display_name = Some(name);
        self.windows[0].push(None, Line::Error(msg));
    }

    /// Swallow the lines of the connect-time self-whois; `true` if `line`
    /// was one of them.
    fn probe_line(&mut self, line: &Line) -> bool {
        match (self.probe, line) {
            (Probe::Pending { .. }, Line::WhoisRule) => {
                self.probe = Probe::Pending { rule: true };
                true
            },
            (Probe::Pending { .. }, Line::WhoisNick { mxid, .. }) if *mxid == self.me.as_str() => {
                self.probe = Probe::Block;
                true
            },
            (Probe::Pending { rule: true }, Line::WhoisNick { .. }) => {
                // someone else's whois: give back the rule we took
                self.probe = Probe::Pending { rule: false };
                self.cur_mut().push(None, Line::WhoisRule);
                false
            },
            (Probe::Block, Line::WhoisRow { key, value, last, .. }) => {
                if key == "ircname" {
                    self.display_name = Some(value.clone());
                }
                if *last {
                    self.probe = Probe::Done;
                    self.startup_offer();
                }
                true
            },
            _ => false,
        }
    }

    /// At connect, not away, but the display name is still the away nick
    /// (crash or restart while away): offer `/back` to restore it.
    fn startup_offer(&mut self) {
        let (Some(cur), None) = (self.display_name.clone(), &self.away) else {
            return;
        };
        let pattern = self.vars.away_nick.clone();
        if self.vars.away_nick_orig.as_deref() == Some(cur.as_str()) {
            // already restored
            self.vars.away_nick_orig = None;
            self.save_vars();
            return;
        }
        if self.vars.away_nick_orig.is_none() {
            let Some(base) = strip_away_nick(&pattern, &cur) else {
                return;
            };
            self.vars.away_nick_orig = Some(base);
            self.save_vars();
        }
        let orig = self.vars.away_nick_orig.clone().unwrap_or_default();
        self.error(format!(
            "Display name is still the away nick {cur}: type /back to restore {orig}"
        ));
    }

    /// Push `DEBUG` / `DEBUG_FILTER` / `DEBUG_RAW` to the debug tap when they
    /// differ from what it has (cheap; runs before every Matrix event and
    /// after `/set`).
    pub fn apply_debug(&mut self) {
        let v = &self.vars;
        let want = (v.debug, v.debug_filter.clone(), v.debug_raw);
        if want != self.dbg_applied {
            crate::debug_tap::configure(want.0, &want.1, want.2);
            self.dbg_applied = want;
        }
    }

    /// A DBG line: room-scoped lines go to that room's window, the rest to
    /// the status window; at most [`DBG_PER_SEC`] per window and second,
    /// then one `DBG (suppressed N)` when the next second starts.
    fn debug_line(&mut self, line: crate::debug_tap::DbgLine) {
        let idx = line
            .room
            .as_deref()
            .and_then(|r| OwnedRoomId::try_from(r).ok())
            .and_then(|r| self.window_of(&r))
            .unwrap_or(0);
        let sec = self.now().div_euclid(1000);
        let st = self.dbg_flood.entry(idx).or_insert((sec, 0, 0));
        let mut suppressed = 0;
        if st.0 != sec {
            suppressed = st.2;
            *st = (sec, 0, 0);
        }
        let show = st.1 < DBG_PER_SEC;
        if show {
            st.1 += 1;
        } else {
            st.2 += 1;
        }
        if suppressed > 0 {
            self.windows[idx].push(None, suppressed_line(suppressed));
        }
        if show {
            let cat = line.cat.name().to_string();
            self.windows[idx].push(None, Line::Debug { cat, text: line.text });
        }
    }

    /// Print the pending `DBG (suppressed N)` of windows whose second is over
    /// (from [`Self::tick`], so a burst's tail is not lost).
    fn flush_debug(&mut self) {
        let sec = self.now().div_euclid(1000);
        let mut out = vec![];
        for (idx, st) in &mut self.dbg_flood {
            if st.0 != sec && st.2 > 0 {
                out.push((*idx, st.2));
                *st = (sec, 0, 0);
            }
        }
        for (idx, n) in out {
            if let Some(w) = self.windows.get_mut(idx) {
                w.push(None, suppressed_line(n));
            }
        }
    }

    /// Persist [`Self::vars`] (the away nick original) if there is a
    /// `config.toml`; quietly skipped otherwise.
    fn save_vars(&mut self) {
        if let Some(path) = self.config_toml.clone()
            && let Err(e) = self.vars.persist(&path)
        {
            self.error(format!("Could not save {}: {e:#}", path.display()));
        }
    }

    /// The homeserver confirmed an away change: the 306/305 lines, and on
    /// return BitchX's `You were /away for ...` (`source/commands.c:3124`).
    fn away_changed(&mut self, away: Option<String>) {
        let now = self.now();
        match &away {
            Some(r) => {
                self.away_since.get_or_insert(now);
                self.info(format!("You have been marked as being away ({r})"));
            },
            None => {
                if let Some(since) = self.away_since.take() {
                    let t = (now - since).max(0) / 1000;
                    let (h, m, s) = (t / 3600, (t % 3600) / 60, t % 60);
                    self.info(format!(
                        "You were /away for {h} hours {m} minutes and {s} seconds. [BX-MsgLog Off]"
                    ));
                }
                self.info("You are no longer marked as being away");
                self.auto_away = false;
                self.away_msgs = 0;
            },
        }
        self.away = away;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.kind == KeyEventKind::Release {
            return vec![];
        }
        let mut fx = self.activity();
        fx.extend(self.key(key));
        fx
    }

    fn key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if key.code != KeyCode::Tab {
            self.input.reset_tab();
        }

        match key.code {
            KeyCode::Char(c @ '0'..='9') if alt => {
                let n = if c == '0' {
                    10
                } else {
                    c as usize - '0' as usize
                };
                return self.switch(n - 1);
            },
            KeyCode::Char('n') if ctrl => return self.next_window(),
            KeyCode::Char('p') if ctrl => return self.prev_window(),
            KeyCode::Char('a') if ctrl => self.input.home(),
            KeyCode::Char('e') if ctrl => self.input.end(),
            KeyCode::Char('u') if ctrl => self.input.kill_to_start(),
            KeyCode::Char('k') if ctrl => self.input.kill_to_end(),
            KeyCode::Char('w') if ctrl => self.input.kill_word(),
            KeyCode::Char('b') if ctrl => self.input.left(),
            KeyCode::Char('f') if ctrl => self.input.right(),
            KeyCode::Char('h') if ctrl => self.input.backspace(),
            KeyCode::Char('d') if ctrl => {
                if self.input.is_empty() {
                    self.quit = true;
                    return vec![Effect::Quit];
                }
                self.input.delete();
            },
            KeyCode::Char(_) if ctrl || alt => {},
            KeyCode::Char(c) => self.input.insert(c),
            KeyCode::Left => self.input.left(),
            KeyCode::Right => self.input.right(),
            KeyCode::Home => self.input.home(),
            KeyCode::End => self.input.end(),
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::Up => self.input.history_prev(),
            KeyCode::Down => self.input.history_next(),
            KeyCode::Tab => {
                let nicks = self.nicks();
                self.input.complete(&nicks);
            },
            KeyCode::PageUp => {
                let page = self.page.max(1);
                self.cur_mut().scroll += page;
            },
            KeyCode::PageDown => {
                let page = self.page.max(1);
                let win = self.cur_mut();
                win.scroll = win.scroll.saturating_sub(page);
            },
            KeyCode::Enter => {
                let line = self.input.submit();
                return self.submit(&line);
            },
            _ => {},
        }
        vec![]
    }

    // ---- input line ---------------------------------------------------

    /// Handle a submitted input line: a `/command` or text for the room.
    pub fn submit(&mut self, line: &str) -> Vec<Effect> {
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
    pub fn resolve_user(&self, target: &str) -> Result<OwnedUserId, String> {
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
    pub fn find_window(&self, target: &str) -> Option<usize> {
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
    pub fn window_clash(&self, target: &str) -> Option<String> {
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
            w.spoof = clash || fake_own;
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

    pub fn command(&mut self, cmd: BitchxCommand) -> Vec<Effect> {
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
                match idx.and_then(|i| self.windows[i].room_id().cloned()) {
                    Some(room_id) => vec![Effect::Part { room_id, reason }],
                    None => {
                        self.error("You're not on that channel");
                        vec![]
                    },
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
            BitchxCommand::Window { target: Some(t) } => match self.window_arg(&t) {
                Some(idx) => self.switch(idx),
                None => {
                    self.error(format!("No such window: {t}"));
                    vec![]
                },
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
            BitchxCommand::Help => {
                self.info(format!("iamBitchX commands: /{}", COMMAND_NAMES.join(" /")));
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
            BitchxCommand::Recover { key } => {
                // Never echo the key.
                self.info("Recovering secrets from secure backup with the given key (********)");
                vec![Effect::Recover(key)]
            },
            BitchxCommand::Mode { .. }
            | BitchxCommand::Ctcp { .. }
            | BitchxCommand::Ignore { .. }
            | BitchxCommand::Unignore { .. } => {
                self.error("Not supported on Matrix");
                vec![]
            },
        }
    }

    // ---- end-to-end encryption -----------------------------------------

    /// `/encrypt`: ask first; enabling encryption can never be undone.
    fn encrypt(&mut self) -> Vec<Effect> {
        let Some(room_id) = self.current_room() else {
            return vec![];
        };
        if self.cur().room.encrypted {
            self.info(format!("{} is already encrypted (+e)", self.cur().room.name));
            return vec![];
        }
        let name = self.cur().room.name.clone();
        self.info(encrypt_prompt(&name));
        self.confirm = Some(Confirm::Encrypt(room_id));
        vec![]
    }

    /// The line typed after a y/n question.
    fn answer(&mut self, confirm: Confirm, line: &str) -> Vec<Effect> {
        let yes = matches!(line.trim().to_lowercase().as_str(), "y" | "yes");
        match confirm {
            Confirm::Encrypt(room_id) if yes => {
                self.info("Enabling end-to-end encryption (megolm)...");
                vec![Effect::EnableEncryption(room_id)]
            },
            Confirm::Encrypt(_) => {
                self.info("Encryption not enabled");
                vec![]
            },
        }
    }

    /// F37: the Matrix sync loop broke (`sync: <err>`, it is not restarted).
    /// Status bar turns `[SYNC LOST]` for good and the status window plus the
    /// current one get a -:- line, so a frozen client never looks live.
    fn sync_died(&mut self, line: Line) {
        self.windows[0].push(None, line);
        if self.sync_lost {
            return;
        }
        self.sync_lost = true;
        let warn = Line::Info(
            "Sync with the homeserver stopped: no new messages arrive until iamBitchX is restarted"
                .into(),
        );
        if self.current != 0 {
            self.windows[self.current].push(None, warn.clone());
        }
        self.windows[0].push(None, warn);
    }

    /// The pending flows with `user` (F44: one entry per flow, not per user).
    fn flows_of(&self, user: &OwnedUserId) -> Vec<&PendingVerify> {
        self.verifications.iter().filter(|v| &v.user == user).collect()
    }

    /// The one flow a `/verify <sub> <user> [flow]` means. `flow` is a flow id
    /// or a unique prefix of one. Several flows and no `flow`: refused, listing
    /// them, so a confirm can never land on a flow whose emoji were not shown.
    fn pick_flow(&mut self, user: &OwnedUserId, flow: Option<&str>) -> Option<PendingVerify> {
        let flows = self.flows_of(user);
        let picked: Vec<&PendingVerify> = match flow {
            Some(f) => {
                let exact: Vec<_> = flows.iter().copied().filter(|v| v.flow_id == f).collect();
                if exact.is_empty() {
                    flows.iter().copied().filter(|v| v.flow_id.starts_with(f)).collect()
                } else {
                    exact
                }
            },
            None => flows,
        };
        match picked.as_slice() {
            [one] => Some((*one).clone()),
            [] => {
                let what = flow.map(|f| format!(" (flow {f})")).unwrap_or_default();
                self.error(format!("No pending verification with {user}{what} -- see /verify"));
                None
            },
            many => {
                let list: Vec<String> = many
                    .iter()
                    .map(|v| {
                        let dev =
                            v.device.as_deref().map(|d| format!(" [{d}]")).unwrap_or_default();
                        format!("{}{dev}", v.flow_id)
                    })
                    .collect();
                self.error(format!(
                    "{user} has {} pending verifications: {} -- add the flow id",
                    many.len(),
                    list.join(", ")
                ));
                None
            },
        }
    }

    fn verify_cmd(&mut self, cmd: VerifyCmd) -> Vec<Effect> {
        let (target, flow, action) = match cmd {
            VerifyCmd::List => {
                if self.verifications.is_empty() {
                    self.info("No pending verifications");
                }
                let rows: Vec<String> = self
                    .verifications
                    .iter()
                    .map(|v| {
                        let dir = if v.we_started { "to" } else { "from" };
                        let dev =
                            v.device.as_deref().map(|d| format!(" [{d}]")).unwrap_or_default();
                        let amb = if v.ambiguous { ", emoji ambiguous" } else { "" };
                        format!(
                            "Verification {dir} {}{dev}: {}{amb} (flow {})",
                            v.user, v.state, v.flow_id
                        )
                    })
                    .collect();
                for r in rows {
                    self.info(r);
                }
                return vec![];
            },
            VerifyCmd::Accept { user, flow } => (user, flow, VerifyAction::Accept),
            VerifyCmd::Start { user, device } => (user, None, VerifyAction::Start { device }),
            VerifyCmd::Confirm { user, flow } => (user, flow, VerifyAction::Confirm),
            VerifyCmd::Mismatch { user, flow } => (user, flow, VerifyAction::Mismatch),
            VerifyCmd::Cancel { user, flow } => (user, flow, VerifyAction::Cancel),
        };
        let Some(user) = self.user_cmd(&target) else {
            return vec![];
        };
        if let VerifyAction::Start { .. } = action {
            let flow_id = self.flows_of(&user).last().map(|v| v.flow_id.clone());
            return vec![Effect::Verify { user, flow_id, action }];
        }
        let Some(v) = self.pick_flow(&user, flow.as_deref()) else {
            return vec![];
        };
        // The emoji on screen cannot be tied to this flow (several flows with
        // this user reached SAS): never confirm it, start over.
        if v.ambiguous && action == VerifyAction::Confirm {
            self.error(format!(
                "Emoji shown for {user} may belong to another flow -- /verify cancel {user} {} and start again",
                v.flow_id
            ));
            return vec![];
        }
        if matches!(action, VerifyAction::Cancel | VerifyAction::Mismatch)
            && let Some(p) = self.verifications.iter_mut().find(|p| p.flow_id == v.flow_id)
        {
            // the Matrix side's `Cancelled` (no flow id) then removes this one
            p.state = "cancelled";
        }
        vec![Effect::Verify { user, flow_id: Some(v.flow_id), action }]
    }

    /// Print verification lines in the room of an in-room request (else the
    /// status window), and in the current window too so they are not missed.
    fn verify_lines(&mut self, room_id: Option<OwnedRoomId>, lines: Vec<Line>) {
        let idx = room_id.and_then(|r| self.window_of(&r)).unwrap_or(0);
        for l in &lines {
            self.windows[idx].push(None, l.clone());
        }
        if idx != self.current {
            self.windows[idx].unread = true;
            for l in lines {
                self.cur_mut().push(None, l);
            }
        }
    }

    /// F44: the Matrix side reports progress per user (no flow id yet), so an
    /// update is tied to a flow only while that user has exactly one. With
    /// several flows, SAS emoji mark them all `ambiguous` (confirm refused).
    fn verify_update(&mut self, user: OwnedUserId, update: VerifyUpdate) {
        let n = self.flows_of(&user).len();
        let room_id = self.flows_of(&user).last().and_then(|v| v.room_id.clone());
        let set_state = |app: &mut App, state| {
            if n == 1
                && let Some(v) = app.verifications.iter_mut().find(|v| v.user == user)
            {
                v.state = state;
            }
        };
        let lines = match update {
            VerifyUpdate::Requested { flow_id, device, room_id, we_started } => {
                let dev = device.as_deref().map(|d| format!(" [{d}]")).unwrap_or_default();
                let mut lines = vec![Line::Info(if we_started {
                    format!("Verification request sent to {user}{dev}, waiting for them to accept")
                } else {
                    format!(
                        "Verification request from {user}{dev} -- /verify accept {user} or /verify cancel {user}"
                    )
                })];
                self.verifications.retain(|v| v.flow_id != flow_id);
                if !self.flows_of(&user).is_empty() {
                    lines.push(Line::Error(format!(
                        "{user} has another verification pending -- commands now need the flow id ({flow_id}), see /verify"
                    )));
                }
                self.verifications.push(PendingVerify {
                    user: user.clone(),
                    flow_id,
                    device,
                    room_id: room_id.clone(),
                    we_started,
                    state: "requested",
                    ambiguous: false,
                });
                return self.verify_lines(room_id, lines);
            },
            VerifyUpdate::Ready => {
                set_state(self, "ready");
                vec![Line::Info(format!(
                    "Verification with {user} is ready, starting emoji (SAS)"
                ))]
            },
            VerifyUpdate::Emoji(emojis) => {
                set_state(self, "emoji");
                let mut lines = sas_lines(user.as_str(), &emojis);
                if n > 1 {
                    for v in self.verifications.iter_mut().filter(|v| v.user == user) {
                        v.ambiguous = true;
                    }
                    lines.push(Line::Error(format!(
                        "{n} verifications with {user} are pending: these emoji cannot be tied to one of them, confirm is refused -- cancel them and start again"
                    )));
                }
                lines
            },
            VerifyUpdate::Confirmed => {
                set_state(self, "confirmed");
                vec![Line::Info(format!(
                    "Waiting for {user} to confirm the emoji..."
                ))]
            },
            VerifyUpdate::Done => {
                if n == 1 {
                    self.verifications.retain(|v| v.user != user);
                } else {
                    // The flow we confirmed (never an ambiguous one) finished.
                    let done = self
                        .verifications
                        .iter()
                        .position(|v| v.user == user && v.state == "confirmed");
                    if let Some(i) = done {
                        self.verifications.remove(i);
                    }
                }
                vec![Line::Info(format!("{user} is now verified"))]
            },
            VerifyUpdate::Cancelled(reason) => {
                let mut lines = vec![Line::Error(format!(
                    "Verification with {user} cancelled: {reason}"
                ))];
                let ours = self
                    .verifications
                    .iter()
                    .position(|v| v.user == user && v.state == "cancelled");
                if n == 1 {
                    self.verifications.retain(|v| v.user != user);
                } else if let Some(i) = ours {
                    self.verifications.remove(i);
                } else if n > 1 {
                    // Which flow ended is unknown: keep them, see /verify.
                    for v in self.verifications.iter_mut().filter(|v| v.user == user) {
                        v.ambiguous = true;
                    }
                    lines.push(Line::Error(format!(
                        "One of {n} verifications with {user} ended -- /verify cancel {user} <flow> the rest"
                    )));
                }
                lines
            },
        };
        self.verify_lines(room_id, lines);
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
        match idx.filter(|i| *i > 0) {
            Some(i) => Some(i),
            None => {
                self.error("No channel");
                None
            },
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
    /// WHOIS_IDLE row (`source/numbers.c:870-877`) before the closing row.
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
                format!("{}: {} [{what}]", i + 1, w.room.name)
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
    pub fn cur_scroll_for_test(&mut self, scroll: usize) {
        self.cur_mut().scroll = scroll;
    }
}

#[cfg(test)]
pub mod tests;
