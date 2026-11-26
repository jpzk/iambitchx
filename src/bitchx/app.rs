//! # The BitchX client state machine
//!
//! Pure state: numbered windows, the input line, and slash-command dispatch.
//! Keys and Matrix events go in, [`Effect`]s for the Matrix side come out.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};

use crate::bitchx::channels::{list_lines, normalize_join, short_name};
use crate::bitchx::commands::{BitchxCommand, COMMAND_NAMES, VerifyCmd, parse};
use crate::bitchx::e2ee::{encrypt_prompt, sas_lines};
use crate::bitchx::input::Input;
use crate::bitchx::model::{
    Effect,
    Line,
    Member,
    MxEvent,
    RoomSnapshot,
    VerifyAction,
    VerifyUpdate,
};
use crate::bitchx::status::{BxStatus, UserCounts};
use crate::bitchx::vars::{Vars, expand_auto_away_msg, expand_away_nick, lookup, strip_away_nick};

/// Refnum of the status window.
pub const STATUS_REFNUM: usize = 1;

/// The iamBitchX logo shown in the status window, in figlet `slant` style
/// (47 columns). Rendered italic where the terminal supports it.
pub const BANNER: [&str; 5] = [
    r"    _               ____  _ __       __   _  __",
    r"   (_)___ _____ ___/ __ )(_) /______/ /_ | |/ /",
    r"  / / __ `/ __ `__ \ __  / / __/ ___/ __ \|   /",
    r" / / /_/ / / / / / / /_/ / / /_/ /__/ / / /   |",
    r"/_/\__,_/_/ /_/ /_/_____/_/\__/\___/_/ /_/_/|_|",
];

/// The startup lines of window 1, as in recon captures 00/01: version
/// header, logo with its `[ version]` credit, then ` iamBitchX:` lines.
pub fn startup_lines(nick: &str) -> Vec<Line> {
    let ver = env!("CARGO_PKG_VERSION");
    let sha = option_env!("VERGEN_GIT_SHA").unwrap_or("unknown");
    let term = std::env::var("TERM").unwrap_or_else(|_| "unknown".into());
    let mut v = vec![
        Line::Raw("iamBitchX - Matrix client in the style of BitchX by panasync".into()),
        Line::Raw(format!("Version (iambitchx-{ver}) -- Commit ({sha}).")),
        Line::Raw(format!("Process [{}]", std::process::id())),
        Line::Raw(format!("Using terminal type [{term}]")),
        Line::Raw(String::new()),
    ];
    v.extend(BANNER.iter().map(|b| Line::Logo(format!("    {b}"))));
    v.push(Line::Logo(format!("    [ iamBitchX-{ver}]           Matrix edition")));
    v.push(Line::Raw(String::new()));
    v.push(Line::Error(format!("Auto Response is set to - {nick}")));
    v.push(Line::Error("Type /help for help".into()));
    v
}

const MAX_LINES: usize = 2000;

/// Now, in unix millis.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// One numbered window: the status window (no room) or a room.
#[derive(Clone, Debug, Default)]
pub struct Window {
    pub room: RoomSnapshot,
    pub lines: Vec<Line>,
    /// When each of `lines` happened (unix millis): the event's
    /// `origin_server_ts`, or the time a client line was printed.
    pub times: Vec<i64>,
    ids: HashSet<String>,
    pub unread: bool,
    /// Rows scrolled up from the bottom (PageUp/PageDown).
    pub scroll: usize,
    pub backfill_requested: bool,
}

impl Window {
    pub fn room_id(&self) -> Option<&OwnedRoomId> {
        self.room.room_id.as_ref()
    }

    /// The `#channel` shown in the prompt and status line.
    pub fn channel(&self) -> Option<&str> {
        self.room.room_id.as_ref().map(|_| self.room.name.as_str())
    }

    /// In a query (DM) window, `<nick>` lines become BitchX query lines:
    /// `[msg(peer)] text` for ours and `[peer(mxid)] text` for theirs.
    fn query_line(&self, line: Line) -> Line {
        match (&self.room.dm_peer, line) {
            (Some(peer), Line::Public { nick, own, text }) => {
                let nick = if own { self.room.name.clone() } else { nick };
                Line::Private { nick, mxid: peer.to_string(), own, text }
            },
            (_, line) => line,
        }
    }

    /// Append a client-generated line, stamped now.
    fn push(&mut self, id: Option<String>, line: Line) -> bool {
        self.push_at(id, now_ms(), line)
    }

    /// Append a line stamped `ts` (unix millis).
    fn push_at(&mut self, id: Option<String>, ts: i64, line: Line) -> bool {
        if let Some(id) = id &&
            !self.ids.insert(id)
        {
            return false;
        }
        self.lines.push(line);
        self.times.push(ts);
        if self.lines.len() > MAX_LINES {
            let n = self.lines.len() - MAX_LINES;
            self.lines.drain(..n);
            self.times.drain(..n);
        }
        true
    }

    fn prepend(&mut self, lines: Vec<(Option<String>, i64, Line)>) {
        let mut fresh = vec![];
        let mut times = vec![];
        for (id, ts, line) in lines {
            if let Some(id) = id &&
                !self.ids.insert(id)
            {
                continue;
            }
            fresh.push(line);
            times.push(ts);
        }
        fresh.append(&mut self.lines);
        times.append(&mut self.times);
        self.lines = fresh;
        self.times = times;
    }

    /// Drop all scrollback (`/clear`).
    fn clear(&mut self) {
        self.lines.clear();
        self.times.clear();
    }

    fn member_by_nick(&self, nick: &str) -> Option<&Member> {
        let nick = nick.to_lowercase();
        self.room
            .members
            .iter()
            .find(|m| m.nick.to_lowercase() == nick)
            .or_else(|| {
                self.room
                    .members
                    .iter()
                    .find(|m| m.user_id.localpart().to_lowercase() == nick)
            })
    }
}

/// A y/n question waiting for the next input line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Confirm {
    /// `/encrypt` in this room.
    Encrypt(OwnedRoomId),
}

/// A key verification in progress with another user (or own device).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingVerify {
    pub user: OwnedUserId,
    pub flow_id: String,
    pub device: Option<String>,
    pub room_id: Option<OwnedRoomId>,
    pub we_started: bool,
    /// requested / ready / emoji / confirmed.
    pub state: &'static str,
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
    /// Other users whose presence is `unavailable`, with their status message.
    pub presence_away: HashMap<OwnedUserId, String>,
    /// Our own Matrix display name, once known (silent self-whois at
    /// connect, `/nick`, profile changes seen in rooms).
    pub display_name: Option<String>,
    /// The silent self-whois that learns [`Self::display_name`].
    pub probe: Probe,
}

/// The connect-time self-whois: its lines are swallowed, the `ircname` row
/// becomes [`App::display_name`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Probe {
    /// Not sent yet (sent on the first lag measurement, i.e. once online).
    #[default]
    Start,
    /// Sent; `rule` = the leading WhoisRule was swallowed.
    Pending {
        rule: bool,
    },
    /// Inside our own whois block.
    Block,
    Done,
}

impl App {
    pub fn new(me: OwnedUserId) -> Self {
        let mut status = Window::default();
        for line in startup_lines(me.localpart()) {
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
            presence_away: HashMap::new(),
            display_name: None,
            probe: Probe::Start,
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
        let users = win.room.room_id.as_ref().map(|_| {
            UserCounts {
                ops: win.room.members.iter().filter(|m| m.is_op()).count(),
                nonops: win.room.members.iter().filter(|m| !m.is_op()).count(),
                voice: win.room.members.iter().filter(|m| m.is_voice()).count(),
            }
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
            query: win.room.dm_peer.as_ref().map(|_| win.room.name.clone()),
            query_away: win
                .room
                .dm_peer
                .as_ref()
                .is_some_and(|p| self.presence_away.contains_key(p)),
            modes,
            act: self.act(),
            users,
            lag: self.lag,
        }
    }

    /// The input prompt: `[#channel] `.
    pub fn prompt(&self) -> String {
        match self.cur().channel() {
            Some(c) => format!("[{c}] "),
            None => "[0] ".to_string(),
        }
    }

    // ---- Matrix events ------------------------------------------------

    pub fn handle_mx(&mut self, ev: MxEvent) -> Vec<Effect> {
        match ev {
            MxEvent::Room(snap) => return self.upsert_room(snap),
            MxEvent::Lines { room_id, lines, backfill } => {
                let Some(idx) = self.window_of(&room_id) else {
                    return vec![];
                };
                // our own profile change seen in a room (e.g. from another client)
                let own_name = lines.iter().rev().find_map(|(_, _, l)| {
                    match l {
                        Line::DisplayName { nick, name: Some(n) } if *nick == self.nick => {
                            Some(n.clone())
                        },
                        _ => None,
                    }
                });
                if let Some(name) = own_name.filter(|_| !backfill) {
                    self.own_name(name);
                }
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
                    for (id, ts, line) in lines {
                        let incoming = matches!(line, Line::Private { own: false, .. });
                        if win.push_at(id, ts, line) {
                            added = true;
                            msgs += usize::from(incoming);
                        }
                    }
                    if self.away.is_some() {
                        self.away_msgs += msgs;
                    }
                    if added && idx != self.current {
                        win.unread = true;
                    }
                }
            },
            MxEvent::Left(room_id) => {
                if let Some(idx) = self.window_of(&room_id) {
                    let name = self.windows[idx].room.name.clone();
                    self.windows.remove(idx);
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
                let idx = room_id.and_then(|r| self.window_of(&r)).unwrap_or(self.current);
                self.windows[idx].push(None, line);
            },
            MxEvent::OwnDisplayName(name) => {
                self.own_name(name.clone());
                // The nick is the localpart and never changes; /nick only
                // sets the Matrix display name.
                let line = Line::DisplayName { nick: self.nick.clone(), name: Some(name) };
                self.windows[0].push(None, line);
            },
            MxEvent::Away(away) => self.away_changed(away),
            MxEvent::Presence { user, away } => {
                match away {
                    Some(msg) => self.presence_away.insert(user, msg),
                    None => self.presence_away.remove(&user),
                };
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

    fn upsert_room(&mut self, snap: RoomSnapshot) -> Vec<Effect> {
        let Some(room_id) = snap.room_id.clone() else {
            return vec![];
        };

        if let Some(idx) = self.window_of(&room_id) {
            self.windows[idx].room = snap;
            return vec![];
        }

        let refnum = self.windows.len() + 1;
        let what = match &snap.dm_peer {
            Some(peer) => format!("query with {peer}"),
            None => format!("{} users", snap.members.len()),
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
        vec![]
    }

    // ---- keys ---------------------------------------------------------

    // ---- away / AUTO_AWAY ----------------------------------------------

    /// Now in unix millis (the fake clock in tests).
    pub fn now(&self) -> i64 {
        self.fake_now.unwrap_or_else(now_ms)
    }

    /// The timer: `check_auto_away` (`source/misc.c:1204-1225`). After
    /// AUTO_AWAY_TIME seconds without a keypress, go away with
    /// `Auto-Away after N mins`, unless already away (by hand or not).
    pub fn tick(&mut self) -> Vec<Effect> {
        let secs = self.vars.auto_away_time;
        let now = self.now();
        if !self.auto_away_armed ||
            !self.vars.auto_away ||
            secs == 0 ||
            self.away.is_some() ||
            now - self.last_input < i64::from(secs) * 1000
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
    /// name (`carla afk`) is unwrapped to its original first.
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

    /// Persist [`Self::vars`] (the away nick original) if there is a
    /// `config.toml`; quietly skipped otherwise.
    fn save_vars(&mut self) {
        if let Some(path) = self.config_toml.clone() &&
            let Err(e) = self.vars.persist(&path)
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

        if let Some(rest) = line.strip_prefix('/') &&
            !rest.starts_with('/')
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

    /// Resolve `@user:server`, or a nick/localpart in the current window
    /// (then any window), to a user ID.
    pub fn resolve_user(&self, target: &str) -> Result<OwnedUserId, String> {
        if target.starts_with('@') {
            return UserId::parse(target).map_err(|e| format!("Invalid user ID {target}: {e}"));
        }

        std::iter::once(self.cur())
            .chain(self.windows.iter())
            .find_map(|w| w.member_by_nick(target))
            .map(|m| m.user_id.clone())
            .or_else(|| {
                self.windows
                    .iter()
                    .find(|w| w.room.name.eq_ignore_ascii_case(target))
                    .and_then(|w| w.room.dm_peer.clone())
            })
            .ok_or_else(|| format!("{target}: No such nick"))
    }

    /// Find a window by refnum, `#name`, room ID or alias.
    pub fn find_window(&self, target: &str) -> Option<usize> {
        if let Ok(n) = target.parse::<usize>() {
            return (n >= 1 && n <= self.windows.len()).then(|| n - 1);
        }
        self.windows.iter().position(|w| {
            w.room.name.eq_ignore_ascii_case(target) ||
                w.room_id().is_some_and(|r| r.as_str() == target)
        })
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
                if let Some(idx) = self.find_window(&target).filter(|i| *i > 0) {
                    let room_id = self.windows[idx].room_id().cloned().unwrap();
                    self.info(format!("-> [{}] {text}", self.windows[idx].room.name));
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
                if let Some(idx) = self.find_window(&target).filter(|i| *i > 0) {
                    return self.switch(idx);
                }
                self.user_cmd(&target).map(Effect::Query).into_iter().collect()
            },
            BitchxCommand::Join { room, .. } => self.join(&room),
            BitchxCommand::Part { room, reason } => {
                let idx = match room {
                    Some(r) => self.find_window(&r).filter(|i| *i > 0),
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
            BitchxCommand::Topic { topic } => {
                self.current_room()
                    .map(|room_id| Effect::Topic { room_id, topic })
                    .into_iter()
                    .collect()
            },
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
                match self.find_window(&t) {
                    Some(idx) => self.switch(idx),
                    None => {
                        self.error(format!("No such window: {t}"));
                        vec![]
                    },
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
                    Some(r) => {
                        self.find_window(&r).and_then(|i| self.windows[i].room_id().cloned())
                    },
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
            BitchxCommand::Mode { .. } |
            BitchxCommand::Ctcp { .. } |
            BitchxCommand::Ignore { .. } |
            BitchxCommand::Unignore { .. } => {
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

    fn pending_verify(&self, user: &OwnedUserId) -> Option<&PendingVerify> {
        self.verifications.iter().find(|v| &v.user == user)
    }

    fn verify_cmd(&mut self, cmd: VerifyCmd) -> Vec<Effect> {
        let (target, action) = match cmd {
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
                        format!("Verification {dir} {}{dev}: {}", v.user, v.state)
                    })
                    .collect();
                for r in rows {
                    self.info(r);
                }
                return vec![];
            },
            VerifyCmd::Accept { user } => (user, VerifyAction::Accept),
            VerifyCmd::Start { user, device } => (user, VerifyAction::Start { device }),
            VerifyCmd::Confirm { user } => (user, VerifyAction::Confirm),
            VerifyCmd::Mismatch { user } => (user, VerifyAction::Mismatch),
            VerifyCmd::Cancel { user } => (user, VerifyAction::Cancel),
        };
        let Some(user) = self.user_cmd(&target) else {
            return vec![];
        };
        let flow_id = self.pending_verify(&user).map(|v| v.flow_id.clone());
        if flow_id.is_none() && !matches!(action, VerifyAction::Start { .. }) {
            self.error(format!("No pending verification with {user} -- see /verify"));
            return vec![];
        }
        vec![Effect::Verify { user, flow_id, action }]
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

    fn verify_update(&mut self, user: OwnedUserId, update: VerifyUpdate) {
        let room_id = self.pending_verify(&user).and_then(|v| v.room_id.clone());
        let set_state = |app: &mut App, state| {
            if let Some(v) = app.verifications.iter_mut().find(|v| v.user == user) {
                v.state = state;
            }
        };
        let lines = match update {
            VerifyUpdate::Requested { flow_id, device, room_id, we_started } => {
                let dev = device.as_deref().map(|d| format!(" [{d}]")).unwrap_or_default();
                let line = if we_started {
                    format!("Verification request sent to {user}{dev}, waiting for them to accept")
                } else {
                    format!(
                        "Verification request from {user}{dev} -- /verify accept {user} or /verify cancel {user}"
                    )
                };
                self.verifications.retain(|v| v.user != user);
                self.verifications.push(PendingVerify {
                    user: user.clone(),
                    flow_id,
                    device,
                    room_id: room_id.clone(),
                    we_started,
                    state: "requested",
                });
                return self.verify_lines(room_id, vec![Line::Info(line)]);
            },
            VerifyUpdate::Ready => {
                set_state(self, "ready");
                vec![Line::Info(format!(
                    "Verification with {user} is ready, starting emoji (SAS)"
                ))]
            },
            VerifyUpdate::Emoji(emojis) => {
                set_state(self, "emoji");
                sas_lines(user.as_str(), &emojis)
            },
            VerifyUpdate::Confirmed => {
                set_state(self, "confirmed");
                vec![Line::Info(format!(
                    "Waiting for {user} to confirm the emoji..."
                ))]
            },
            VerifyUpdate::Done => {
                self.verifications.retain(|v| v.user != user);
                vec![Line::Info(format!("{user} is now verified"))]
            },
            VerifyUpdate::Cancelled(reason) => {
                self.verifications.retain(|v| v.user != user);
                vec![Line::Error(format!(
                    "Verification with {user} cancelled: {reason}"
                ))]
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
            Some(t) => self.find_window(&t),
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
        let room = &self.windows[idx].room;
        let mut members = room.members.clone();
        members.sort_by(|a, b| b.power.cmp(&a.power).then(a.nick.cmp(&b.nick)));
        let total = members.len();
        let width = members.iter().map(|m| m.nick.chars().count()).max().unwrap_or(0);
        let mut lines = vec![Line::Users { chan: room.name.clone(), shown: total, total }];
        for chunk in members.chunks(5) {
            let cells = chunk
                .iter()
                .map(|m| {
                    let flag = if m.is_op() {
                        '@'
                    } else if m.is_voice() {
                        '+'
                    } else {
                        ' '
                    };
                    (flag, m.nick.clone())
                })
                .collect();
            lines.push(Line::Names { cells, width });
        }
        for l in lines {
            self.windows[idx].push(None, l);
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

    /// `/join`: an already joined channel just switches to its window;
    /// otherwise ask the Matrix side, which opens a window on success.
    fn join(&mut self, target: &str) -> Vec<Effect> {
        let own = self.me.server_name().to_string();
        let t = match normalize_join(target, &own) {
            Ok(t) => t,
            Err(e) => {
                self.error(e);
                return vec![];
            },
        };
        let short = short_name(&t.id, &own);
        let idx = self.find_window(&short).or_else(|| self.find_window(&t.id));
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
pub mod tests {
    use matrix_sdk::ruma::{owned_room_id, owned_user_id};

    use super::*;
    use crate::bitchx::channels::{JoinTarget, ListArgs};

    pub fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    pub fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    pub fn alt(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
    }

    pub fn member(id: &str, nick: &str, power: i64) -> Member {
        Member {
            user_id: UserId::parse(id).unwrap(),
            nick: nick.into(),
            power,
        }
    }

    pub fn room(id: &str, name: &str) -> RoomSnapshot {
        RoomSnapshot {
            room_id: Some(OwnedRoomId::try_from(id).unwrap()),
            name: name.into(),
            members: vec![
                member("@carla:example.org", "carla", 100),
                member("@armin:remote.tld", "armin", 0),
                member("@bob:example.org", "bob", 10),
            ],
            ..Default::default()
        }
    }

    /// An app with the status window and three rooms.
    pub fn app() -> App {
        let mut app = App::new(owned_user_id!("@carla:example.org"));
        app.clock = Some("09:24pm".into());
        app.handle_mx(MxEvent::Room(room("!a:example.org", "#room")));
        app.handle_mx(MxEvent::Room(room("!b:example.org", "#other")));
        let mut dm = room("!c:example.org", "armin");
        dm.dm_peer = Some(owned_user_id!("@armin:remote.tld"));
        app.handle_mx(MxEvent::Room(dm));
        app
    }

    pub fn typed(app: &mut App, s: &str) -> Vec<Effect> {
        for c in s.chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        app.handle_key(key(KeyCode::Enter))
    }

    fn last(app: &App) -> Line {
        app.cur().lines.last().cloned().unwrap()
    }

    #[test]
    fn test_query_window() {
        let mut app = app();
        let dm = owned_room_id!("!c:example.org");
        let idx = app.window_of(&dm).unwrap();
        // Capture 29: ` Starting conversation with nick`, window named after the peer.
        assert_eq!(
            app.windows[idx].lines[0],
            Line::Info("Starting conversation with armin".into())
        );
        app.switch(idx);
        assert_eq!(app.prompt(), "[armin] ");
        assert_eq!(app.status().query.as_deref(), Some("armin"));
        assert_eq!(app.status().channel, None);
        // `<nick>` lines in a DM become `[msg(peer)]` / `[peer(mxid)]`.
        let public = |nick: &str, own| Line::Public { nick: nick.into(), own, text: "t".into() };
        app.handle_mx(MxEvent::Lines {
            room_id: dm.clone(),
            lines: vec![
                (None, None, public("carla", true)),
                (None, None, public("armin", false)),
            ],
            backfill: false,
        });
        let n = app.cur().lines.len();
        assert_eq!(app.cur().lines[n - 2], Line::Private {
            nick: "armin".into(),
            mxid: "@armin:remote.tld".into(),
            own: true,
            text: "t".into()
        });
        assert_eq!(app.cur().lines[n - 1], Line::Private {
            nick: "armin".into(),
            mxid: "@armin:remote.tld".into(),
            own: false,
            text: "t".into()
        });
        // Channel windows keep `<nick>`.
        app.handle_mx(MxEvent::Lines {
            room_id: owned_room_id!("!a:example.org"),
            lines: vec![(None, None, public("bob", false))],
            backfill: false,
        });
        assert_eq!(app.windows[1].lines.last(), Some(&public("bob", false)));
        // `/query` alone: ` Ending conversation with nick`, back to window 1.
        typed(&mut app, "/query");
        assert_eq!(app.refnum(), 1);
        assert_eq!(last(&app), Line::Info("Ending conversation with armin".into()));
        // ...and does nothing outside a query.
        let before = app.cur().lines.len();
        assert!(typed(&mut app, "/query").is_empty());
        assert_eq!(app.cur().lines.len(), before);
        // `/msg user text` with no DM window echoes `[msg(user)] text`.
        typed(&mut app, "/msg bob yo");
        assert_eq!(last(&app), Line::Private {
            nick: "bob".into(),
            mxid: "@bob:example.org".into(),
            own: true,
            text: "yo".into()
        });
    }

    #[test]
    fn test_windows_numbered() {
        let app = app();
        assert_eq!(app.windows.len(), 4);
        assert_eq!(app.refnum(), STATUS_REFNUM);
        assert_eq!(app.windows[1].channel(), Some("#room"));
        assert_eq!(app.cur().channel(), None);
        assert_eq!(app.prompt(), "[0] ");
    }

    #[test]
    fn test_alt_digit_switches() {
        let mut app = app();
        let fx = app.handle_key(alt('2'));
        assert_eq!(app.refnum(), 2);
        assert_eq!(app.prompt(), "[#room] ");
        assert_eq!(fx, vec![Effect::Backfill(owned_room_id!("!a:example.org"))]);
        // History is only requested once.
        app.handle_key(alt('1'));
        assert!(app.handle_key(alt('2')).is_empty());
        app.handle_key(alt('4'));
        assert_eq!(app.prompt(), "[armin] ");
        // No such window: stay, print an error.
        app.handle_key(alt('9'));
        assert_eq!(app.refnum(), 4);
        assert!(matches!(last(&app), Line::Error(_)));
    }

    #[test]
    fn test_ctrl_n_p_cycle() {
        let mut app = app();
        app.handle_key(ctrl('n'));
        assert_eq!(app.refnum(), 2);
        app.handle_key(ctrl('p'));
        app.handle_key(ctrl('p'));
        assert_eq!(app.refnum(), 4);
        app.handle_key(ctrl('n'));
        assert_eq!(app.refnum(), 1);
    }

    #[test]
    fn test_window_command() {
        let mut app = app();
        typed(&mut app, "/window 3");
        assert_eq!(app.refnum(), 3);
        typed(&mut app, "/window #room");
        assert_eq!(app.refnum(), 2);
        typed(&mut app, "/window next");
        assert_eq!(app.refnum(), 3);
        typed(&mut app, "/window prev");
        assert_eq!(app.refnum(), 2);
        typed(&mut app, "/window 7");
        assert_eq!(app.refnum(), 2);
        assert_eq!(last(&app), Line::Error("No such window: 7".into()));
    }

    #[test]
    fn test_act_shows_unread() {
        let mut app = app();
        app.handle_key(alt('2'));
        let line = Line::Public { nick: "bob".into(), own: false, text: "hi".into() };
        for room in ["!b:example.org", "!c:example.org", "!a:example.org"] {
            app.handle_mx(MxEvent::Lines {
                room_id: OwnedRoomId::try_from(room).unwrap(),
                lines: vec![(Some(format!("$e{room}")), None, line.clone())],
                backfill: false,
            });
        }
        assert_eq!(app.act(), vec![3, 4]);
        assert_eq!(app.status().act, vec![3, 4]);
        app.handle_key(alt('3'));
        assert_eq!(app.act(), vec![4]);
    }

    #[test]
    fn test_dedupe_and_backfill_order() {
        let mut app = app();
        let room_id = owned_room_id!("!a:example.org");
        let l = |t: &str| Line::Public { nick: "a".into(), own: false, text: t.into() };
        app.handle_mx(MxEvent::Lines {
            room_id: room_id.clone(),
            lines: vec![(Some("$2".into()), None, l("new"))],
            backfill: false,
        });
        app.handle_mx(MxEvent::Lines {
            room_id: room_id.clone(),
            lines: vec![
                (Some("$1".into()), None, l("old")),
                (Some("$2".into()), None, l("new")),
            ],
            backfill: true,
        });
        let texts: Vec<_> = app.windows[1].lines.to_vec();
        assert_eq!(texts, vec![l("old"), l("new")]);
    }

    #[test]
    fn test_plain_text_sends() {
        let mut app = app();
        assert!(typed(&mut app, "hello").is_empty());
        assert!(matches!(last(&app), Line::Error(_)));
        app.handle_key(alt('2'));
        let fx = typed(&mut app, "hello world");
        assert_eq!(fx, vec![Effect::Send {
            room_id: owned_room_id!("!a:example.org"),
            text: "hello world".into(),
            emote: false,
        }]);
        let fx = typed(&mut app, "/me waves");
        assert_eq!(fx, vec![Effect::Send {
            room_id: owned_room_id!("!a:example.org"),
            text: "waves".into(),
            emote: true,
        }]);
        let fx = typed(&mut app, "//slash");
        assert!(matches!(&fx[0], Effect::Send { text, .. } if text == "/slash"));
    }

    #[test]
    fn test_unknown_command_is_error_line() {
        let mut app = app();
        assert!(typed(&mut app, "/frob").is_empty());
        assert_eq!(last(&app), Line::Error("Unknown command: FROB".into()));
    }

    #[test]
    fn test_commands_to_effects() {
        let mut app = app();
        app.handle_key(alt('2'));
        let a = owned_room_id!("!a:example.org");
        let armin = owned_user_id!("@armin:remote.tld");
        assert_eq!(typed(&mut app, "/join #new:x"), vec![Effect::Join(JoinTarget {
            id: "#new:x".into(),
            implicit_server: false,
            via: vec![],
        })]);
        // No server: our own homeserver.
        assert_eq!(typed(&mut app, "/join #new"), vec![Effect::Join(JoinTarget {
            id: "#new:example.org".into(),
            implicit_server: true,
            via: vec![],
        })]);
        // Already joined (window name is the short alias): just switch.
        assert!(
            typed(&mut app, "/join #other:example.org")
                .iter()
                .all(|e| !matches!(e, Effect::Join(_)))
        );
        assert_eq!(app.cur().room.name, "#other");
        app.handle_key(alt('2'));
        assert_eq!(typed(&mut app, "/list matrix.org -s rust"), vec![Effect::List(ListArgs {
            server: Some("matrix.org".into()),
            search: Some("rust".into()),
        })]);
        assert_eq!(typed(&mut app, "/topic hi there"), vec![Effect::Topic {
            room_id: a.clone(),
            topic: "hi there".into()
        }]);
        assert_eq!(typed(&mut app, "/op armin"), vec![Effect::Power {
            room_id: a.clone(),
            user: armin.clone(),
            level: 50
        }]);
        assert_eq!(typed(&mut app, "/deop ARMIN"), vec![Effect::Power {
            room_id: a.clone(),
            user: armin.clone(),
            level: 0
        }]);
        assert_eq!(typed(&mut app, "/kick bob spam"), vec![Effect::Kick {
            room_id: a.clone(),
            user: owned_user_id!("@bob:example.org"),
            reason: Some("spam".into())
        }]);
        assert_eq!(typed(&mut app, "/invite @x:y"), vec![Effect::Invite {
            room_id: a.clone(),
            user: owned_user_id!("@x:y")
        }]);
        assert_eq!(typed(&mut app, "/whois armin"), vec![Effect::Whois(armin.clone())]);
        // An open DM window takes the message; unknown users get a new DM.
        assert_eq!(typed(&mut app, "/msg armin yo"), vec![Effect::Send {
            room_id: owned_room_id!("!c:example.org"),
            text: "yo".into(),
            emote: false
        }]);
        assert_eq!(typed(&mut app, "/msg bob yo"), vec![Effect::Msg {
            user: owned_user_id!("@bob:example.org"),
            text: "yo".into()
        }]);
        assert_eq!(typed(&mut app, "/nick neo"), vec![Effect::Nick("neo".into())]);
        assert_eq!(typed(&mut app, "/away lunch"), vec![Effect::Away(Some("lunch".into()))]);
        assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None)]);
        assert_eq!(typed(&mut app, "/part"), vec![Effect::Part { room_id: a, reason: None }]);
        assert!(typed(&mut app, "/kick nobody").is_empty());
        assert_eq!(last(&app), Line::Error("nobody: No such nick".into()));
        // An existing DM window is reused by /query.
        assert_eq!(typed(&mut app, "/query armin"), vec![Effect::Backfill(owned_room_id!(
            "!c:example.org"
        ))]);
        assert_eq!(app.refnum(), 4);
        assert_eq!(typed(&mut app, "/query @new:x"), vec![Effect::Query(owned_user_id!("@new:x"))]);
        assert_eq!(typed(&mut app, "/quit"), vec![Effect::Quit]);
        assert!(app.quit);
    }

    #[test]
    fn test_join_synched() {
        let mut app = app();
        assert_eq!(app.refnum(), 1);
        app.handle_mx(MxEvent::Synched {
            room_id: OwnedRoomId::try_from("!b:example.org").unwrap(),
            millis: 319,
        });
        assert_eq!(app.cur().room.name, "#other");
        let n = app.cur().lines.len();
        assert!(matches!(&app.cur().lines[n - 3], Line::Users { shown: 3, .. }));
        assert!(matches!(&app.cur().lines[n - 2], Line::Names { .. }));
        assert_eq!(
            app.cur().lines[n - 1],
            Line::Error("Join to #other was synched in 0.319 secs!!".into())
        );
    }

    #[test]
    fn test_local_commands() {
        let mut app = app();
        app.handle_key(alt('2'));
        typed(&mut app, "/names");
        assert!(
            app.cur()
                .lines
                .iter()
                .any(|l| { *l == Line::Users { chan: "#room".into(), shown: 3, total: 3 } })
        );
        assert!(app.cur().lines.iter().any(|l| {
            *l == Line::Names {
                cells: vec![
                    ('@', "carla".into()),
                    ('+', "bob".into()),
                    (' ', "armin".into()),
                ],
                width: 5,
            }
        }));
        typed(&mut app, "/who");
        assert!(
            app.cur()
                .lines
                .iter()
                .any(|l| matches!(l, Line::Info(t) if t.contains("armin H @armin:remote.tld")))
        );
        typed(&mut app, "/rooms");
        assert!(
            app.cur()
                .lines
                .iter()
                .any(|l| matches!(l, Line::Info(t) if t.starts_with("4: armin")))
        );
        typed(&mut app, "/help");
        assert!(matches!(last(&app), Line::Info(_)));
        typed(&mut app, "/clear");
        assert!(app.cur().lines.is_empty());
    }

    #[test]
    fn test_left_room_closes_window() {
        let mut app = app();
        app.handle_key(alt('4'));
        app.handle_mx(MxEvent::Left(owned_room_id!("!b:example.org")));
        assert_eq!(app.windows.len(), 3);
        assert_eq!(app.prompt(), "[armin] ");
        assert_eq!(app.refnum(), 3);
    }

    #[test]
    fn test_input_keys_through_app() {
        let mut app = app();
        for c in "hello world".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        app.handle_key(ctrl('w'));
        assert_eq!(app.input.text(), "hello ");
        app.handle_key(ctrl('a'));
        app.handle_key(key(KeyCode::Char('>')));
        app.handle_key(ctrl('e'));
        app.handle_key(key(KeyCode::Char('!')));
        assert_eq!(app.input.text(), ">hello !");
        app.handle_key(ctrl('u'));
        assert_eq!(app.input.text(), "");
        typed(&mut app, "/window 2");
        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.input.text(), "/window 2");
        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.input.text(), "");
        for c in "ar".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.input.text(), "armin: ");
    }

    #[test]
    fn test_paging() {
        let mut app = app();
        app.page = 10;
        app.handle_key(key(KeyCode::PageUp));
        app.handle_key(key(KeyCode::PageUp));
        assert_eq!(app.cur().scroll, 20);
        app.handle_key(key(KeyCode::PageDown));
        assert_eq!(app.cur().scroll, 10);
    }

    #[test]
    fn test_status_counts() {
        let mut app = app();
        app.handle_key(alt('2'));
        let st = app.status();
        assert_eq!(st.channel.as_deref(), Some("#room"));
        assert!(st.chanop);
        assert_eq!(st.users.unwrap(), UserCounts { ops: 1, nonops: 2, voice: 1 });
        assert_eq!(app.refnum(), 2);
        assert_eq!(st.lag, None);
    }

    #[test]
    fn test_encrypt_confirm_flow() {
        let mut app = app();
        app.handle_key(alt('2'));
        let a = owned_room_id!("!a:example.org");
        // Asking does nothing yet; the question says it is irreversible.
        assert_eq!(typed(&mut app, "/encrypt"), vec![]);
        assert!(
            matches!(last(&app), Line::Info(t) if t.contains("IRREVERSIBLE") && t.contains("[y/n]"))
        );
        assert_eq!(app.confirm, Some(Confirm::Encrypt(a.clone())));
        // "n" declines and is not sent as a message.
        assert_eq!(typed(&mut app, "n"), vec![]);
        assert_eq!(app.confirm, None);
        assert_eq!(last(&app), Line::Info("Encryption not enabled".into()));
        // Anything but y/yes declines.
        typed(&mut app, "/encrypt");
        assert_eq!(typed(&mut app, "sure"), vec![]);
        // "y" enables.
        typed(&mut app, "/encrypt");
        assert_eq!(typed(&mut app, "Y"), vec![Effect::EnableEncryption(a.clone())]);
        typed(&mut app, "/encrypt");
        assert_eq!(typed(&mut app, "yes"), vec![Effect::EnableEncryption(a)]);
        // After answering, text is sent normally again.
        assert!(matches!(&typed(&mut app, "hi")[0], Effect::Send { .. }));
        // Not in the status window; not twice in an encrypted room.
        app.handle_key(alt('1'));
        assert_eq!(typed(&mut app, "/encrypt"), vec![]);
        assert!(app.confirm.is_none());
        app.handle_key(alt('2'));
        let mut snap = room("!a:example.org", "#room");
        snap.encrypted = true;
        app.handle_mx(MxEvent::Room(snap));
        assert_eq!(typed(&mut app, "/encrypt"), vec![]);
        assert!(app.confirm.is_none());
        assert!(matches!(last(&app), Line::Info(t) if t.contains("already encrypted")));
    }

    #[test]
    fn test_status_plus_e() {
        let mut app = app();
        app.handle_key(alt('2'));
        assert_eq!(app.status().modes, "");
        let mut snap = room("!a:example.org", "#room");
        snap.encrypted = true;
        app.handle_mx(MxEvent::Room(snap.clone()));
        assert_eq!(app.status().modes, "e");
        snap.invite_only = true;
        app.handle_mx(MxEvent::Room(snap));
        assert_eq!(app.status().modes, "ie");
        let text: String =
            app.status().line1().spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("#room(+ie)"), "{text}");
    }

    #[test]
    fn test_verify_dispatch() {
        let mut app = app();
        let armin = owned_user_id!("@armin:remote.tld");
        // Nothing pending.
        assert_eq!(typed(&mut app, "/verify"), vec![]);
        assert_eq!(last(&app), Line::Info("No pending verifications".into()));
        assert_eq!(typed(&mut app, "/verify accept armin"), vec![]);
        assert!(matches!(last(&app), Line::Error(t) if t.contains("No pending verification")));
        // Start needs no pending request.
        assert_eq!(typed(&mut app, "/verify start armin DEV1"), vec![Effect::Verify {
            user: armin.clone(),
            flow_id: None,
            action: VerifyAction::Start { device: Some("DEV1".into()) },
        }]);
        // An incoming to-device request shows an info line in the status window.
        app.handle_mx(MxEvent::Verify {
            user: armin.clone(),
            update: VerifyUpdate::Requested {
                flow_id: "f1".into(),
                device: Some("DEV1".into()),
                room_id: None,
                we_started: false,
            },
        });
        assert!(matches!(app.windows[0].lines.last(), Some(Line::Info(t))
            if t.contains("Verification request from @armin:remote.tld [DEV1]")
                && t.contains("/verify accept")));
        typed(&mut app, "/verify");
        assert!(
            matches!(last(&app), Line::Info(t) if t.contains("from @armin:remote.tld [DEV1]: requested"))
        );
        let fx = |action| {
            vec![Effect::Verify {
                user: armin.clone(),
                flow_id: Some("f1".into()),
                action,
            }]
        };
        assert_eq!(typed(&mut app, "/verify accept armin"), fx(VerifyAction::Accept));
        assert_eq!(typed(&mut app, "/verify confirm @armin:remote.tld"), fx(VerifyAction::Confirm));
        assert_eq!(typed(&mut app, "/verify mismatch armin"), fx(VerifyAction::Mismatch));
        assert_eq!(typed(&mut app, "/verify cancel armin"), fx(VerifyAction::Cancel));
        assert!(matches!(&typed(&mut app, "/verify start armin")[0], Effect::Verify {
            flow_id: Some(_),
            action: VerifyAction::Start { device: None },
            ..
        }));
        assert_eq!(typed(&mut app, "/verify accept nobody"), vec![]);

        // Emoji go to the request's window and the current one.
        app.handle_key(alt('3'));
        let emojis: Vec<(String, String)> =
            (0..7).map(|i| (format!("e{i}"), format!("n{i}"))).collect();
        app.handle_mx(MxEvent::Verify {
            user: armin.clone(),
            update: VerifyUpdate::Emoji(emojis),
        });
        assert_eq!(app.verifications[0].state, "emoji");
        let sas = |w: &Window| w.lines.iter().filter(|l| matches!(l, Line::Sas { .. })).count();
        assert_eq!(sas(&app.windows[0]), 7);
        assert_eq!(sas(app.cur()), 7);
        app.handle_mx(MxEvent::Verify { user: armin.clone(), update: VerifyUpdate::Done });
        assert!(app.verifications.is_empty());
        assert_eq!(last(&app), Line::Info("@armin:remote.tld is now verified".into()));

        // In-room request lands in that room's window; cancel clears it.
        app.handle_key(alt('1'));
        app.handle_mx(MxEvent::Verify {
            user: armin.clone(),
            update: VerifyUpdate::Requested {
                flow_id: "f2".into(),
                device: None,
                room_id: Some(owned_room_id!("!c:example.org")),
                we_started: false,
            },
        });
        assert!(
            matches!(app.windows[3].lines.last(), Some(Line::Info(t)) if t.contains("Verification request from"))
        );
        assert!(app.windows[3].unread);
        app.handle_mx(MxEvent::Verify {
            user: armin,
            update: VerifyUpdate::Cancelled("mismatch".into()),
        });
        assert!(app.verifications.is_empty());
        assert!(
            matches!(app.windows[3].lines.last(), Some(Line::Error(t)) if t.contains("cancelled: mismatch"))
        );
    }

    #[test]
    fn test_devices_trust_recover_dispatch() {
        let mut app = app();
        assert_eq!(typed(&mut app, "/devices"), vec![Effect::Devices(owned_user_id!(
            "@carla:example.org"
        ))]);
        assert_eq!(typed(&mut app, "/devices armin"), vec![Effect::Devices(owned_user_id!(
            "@armin:remote.tld"
        ))]);
        assert_eq!(typed(&mut app, "/trust"), vec![Effect::Trust]);
        let key = "EsTc 9xQ2 kLm7 Zz11";
        let fx = typed(&mut app, &format!("/recover {key}"));
        assert_eq!(fx, vec![Effect::Recover(crate::bitchx::commands::Secret::new(key))]);
        // The key is in no scrollback line of any window, nor in Debug output.
        for w in &app.windows {
            for l in &w.lines {
                let s = format!("{l:?}");
                assert!(!s.contains("EsTc") && !s.contains("kLm7"), "{s}");
            }
        }
        assert!(!format!("{fx:?}").contains("EsTc"));
        // A missing key is an error that echoes nothing secret.
        typed(&mut app, "/recover");
        assert!(matches!(last(&app), Line::Error(t) if t.contains("missing argument")));
    }

    fn rows(app: &mut App) -> Vec<String> {
        let lines = crate::bitchx::ui::scrollback(app, 200, 500);
        crate::bitchx::format::plain(&lines)
    }

    fn local(ts: i64, fmt: &str) -> String {
        use chrono::TimeZone;
        chrono::Local.timestamp_millis_opt(ts).unwrap().format(fmt).to_string()
    }

    #[test]
    fn test_timestamp_toggle_rerenders_with_origin_server_ts() {
        let mut app = app();
        app.handle_key(alt('2'));
        let room_id = owned_room_id!("!a:example.org");
        // Two events from different (past) times: history must show THEIR time.
        let (t1, t2) = (1_700_000_000_000, 1_700_000_000_000 + 5 * 3_600_000 + 7 * 60_000);
        let l = |t: &str| Line::Public { nick: "bob".into(), own: false, text: t.into() };
        app.handle_mx(MxEvent::Lines {
            room_id: room_id.clone(),
            lines: vec![(Some("$2".into()), Some(t2), l("second"))],
            backfill: false,
        });
        app.handle_mx(MxEvent::Lines {
            room_id,
            lines: vec![(Some("$1".into()), Some(t1), l("first"))],
            backfill: true,
        });
        let off = rows(&mut app);
        assert!(off.iter().any(|r| r.starts_with("<bob> first")), "{off:?}");

        typed(&mut app, "/set timestamp on");
        assert!(app.vars.timestamp);
        let on = rows(&mut app);
        let s1 = local(t1, "%I:%M%p ");
        let s2 = local(t2, "%I:%M%p ");
        assert!(on.contains(&format!("{s1}<bob> first")), "{on:?}");
        assert!(on.contains(&format!("{s2}<bob> second")), "{on:?}");
        // `-:-` lines (client output, stamped now) carry the prefix too.
        let now = local(now_ms(), "%I:%M%p ");
        assert!(
            on.iter()
                .any(|r| r.starts_with(&format!("{now} Value of TIMESTAMP set to ON")))
        );

        // Changing the format re-renders existing lines.
        typed(&mut app, "/set timestamp_string [%H:%M] ");
        let on = rows(&mut app);
        assert!(on.contains(&format!("{}<bob> first", local(t1, "[%H:%M] "))), "{on:?}");

        typed(&mut app, "/set timestamp off");
        let off = rows(&mut app);
        assert!(off.iter().any(|r| r.starts_with("<bob> first")), "{off:?}");
        assert!(off.iter().any(|r| r.starts_with(" Value of TIMESTAMP set to OFF")), "{off:?}");
    }

    #[test]
    fn test_timestamp_action_and_status_window() {
        let mut app = app();
        app.vars.timestamp = true;
        app.handle_key(alt('2'));
        let ts = 1_750_000_000_000;
        app.handle_mx(MxEvent::Lines {
            room_id: owned_room_id!("!a:example.org"),
            lines: vec![(None, Some(ts), Line::Action {
                nick: "bob".into(),
                own: false,
                text: "waves".into(),
            })],
            backfill: false,
        });
        let r = rows(&mut app);
        let stamp = local(ts, "%I:%M%p ");
        assert!(
            r.iter()
                .any(|x| x.starts_with(&format!("{stamp} bob")) && x.ends_with(" waves")),
            "{r:?}"
        );
        // The startup logo stays unstamped; its ` iamBitchX:` lines do not.
        app.handle_key(alt('1'));
        let r = rows(&mut app);
        assert!(r[0].starts_with("iamBitchX - Matrix client"), "{r:?}");
        assert!(r.iter().any(|x| x.ends_with("iamBitchX: Type /help for help")), "{r:?}");
        assert!(!r.iter().any(|x| x.starts_with(" iamBitchX: Type")), "{r:?}");
    }

    #[test]
    fn test_set_list_show_errors() {
        let mut app = app();
        let n = app.cur().lines.len();
        typed(&mut app, "/set");
        assert_eq!(&app.cur().lines[n..], &[
            Line::SetVar { name: "AUTO_AWAY".into(), value: Some("ON".into()) },
            Line::SetVar {
                name: "AUTO_AWAY_MSG".into(),
                value: Some("Auto-Away after %t mins".into())
            },
            Line::SetVar {
                name: "AUTO_AWAY_TIME".into(),
                value: Some("600".into())
            },
            Line::SetVar {
                name: "AWAY_MSG".into(),
                value: Some("away".into())
            },
            Line::SetVar {
                name: "AWAY_NICK".into(),
                value: Some("%n afk".into())
            },
            Line::SetVar {
                name: "TIMESTAMP".into(),
                value: Some("OFF".into())
            },
            Line::SetVar {
                name: "TIMESTAMP_STRING".into(),
                value: Some("%I:%M%p ".into())
            },
        ]);
        typed(&mut app, "/set timestamp_s");
        assert_eq!(last(&app), Line::SetVar {
            name: "TIMESTAMP_STRING".into(),
            value: Some("%I:%M%p ".into())
        });
        typed(&mut app, "/set bogus 1");
        assert_eq!(last(&app), Line::Info("No such variable \"BOGUS\"".into()));
        typed(&mut app, "/set timestamp maybe");
        assert_eq!(last(&app), Line::Info("Value must be either ON, OFF, or TOGGLE".into()));
        assert!(!app.vars.timestamp);
        let r = rows(&mut app);
        assert!(
            r.iter().any(|x| x.starts_with("     timestamp_string            %I:%M%p")),
            "{r:?}"
        );
    }

    #[test]
    fn test_set_persists_to_config_toml() {
        let dir = std::env::temp_dir().join(format!("bx-set-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "[settings]\nlog_level = \"info\"\n[profiles.p]\nuser_id = \"@a:b\"\n",
        )
        .unwrap();
        let mut app = app();
        app.config_toml = Some(path.clone());
        typed(&mut app, "/set timestamp on");
        typed(&mut app, "/set timestamp_string %H:%M:%S ");
        assert!(!matches!(last(&app), Line::Error(_)), "{:?}", last(&app));

        let cfg =
            crate::config::Config::parse_toml(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let settings = cfg.settings.unwrap();
        assert_eq!(settings.log_level.as_deref(), Some("info"));
        assert!(cfg.profiles.contains_key("p"));
        let vars = Vars::from_config(&settings.bitchx.unwrap());
        assert_eq!(vars, app.vars);
        assert_eq!(vars.timestamp_string, "%H:%M:%S ");
        assert!(vars.timestamp);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ---- AUTO_AWAY (fake clock) -----------------------------------------

    const T0: i64 = 1_764_000_000_000;

    /// An armed app on the fake clock at `T0`, last keypress at `T0`.
    fn idle_app() -> App {
        let mut app = app();
        app.fake_now = Some(T0);
        app.last_input = T0;
        app.auto_away_armed = true;
        app
    }

    fn auto_msg() -> Effect {
        Effect::Away(Some("Auto-Away after 10 mins".into()))
    }

    #[test]
    fn test_auto_away_after_threshold() {
        let mut app = idle_app();
        app.fake_now = Some(T0 + 599_999);
        assert_eq!(app.tick(), vec![]);
        assert!(app.away.is_none());
        app.fake_now = Some(T0 + 600_000);
        assert_eq!(app.tick(), vec![auto_msg()]);
        assert!(app.auto_away);
        assert!(app.status().away);
        // Only once.
        app.fake_now = Some(T0 + 700_000);
        assert_eq!(app.tick(), vec![]);
        // The homeserver echo prints the 306 line with the reason.
        app.handle_mx(MxEvent::Away(Some("Auto-Away after 10 mins".into())));
        assert_eq!(
            last(&app),
            Line::Info("You have been marked as being away (Auto-Away after 10 mins)".into())
        );
        // A configured time: /set auto_away_time 15 -> 15 minutes.
        let mut app = idle_app();
        typed(&mut app, "/set auto_away_time 15");
        assert_eq!(app.vars.auto_away_time, 900);
        app.last_input = T0;
        app.fake_now = Some(T0 + 899_000);
        assert_eq!(app.tick(), vec![]);
        app.fake_now = Some(T0 + 900_000);
        assert_eq!(app.tick(), vec![Effect::Away(Some("Auto-Away after 15 mins".into()))]);
    }

    #[test]
    fn test_auto_back_on_input() {
        let mut app = idle_app();
        app.fake_now = Some(T0 + 600_000);
        app.tick();
        app.handle_mx(MxEvent::Away(Some("Auto-Away after 10 mins".into())));
        app.fake_now = Some(T0 + 600_000 + 3_723_000);
        // First keypress: back, and the key still types.
        assert_eq!(app.handle_key(key(KeyCode::Char('x'))), vec![Effect::Away(None)]);
        assert_eq!(app.input.text(), "x");
        assert!(!app.auto_away);
        // Second keypress: nothing more.
        assert_eq!(app.handle_key(key(KeyCode::Char('y'))), vec![]);
        app.handle_mx(MxEvent::Away(None));
        let n = app.cur().lines.len();
        assert_eq!(&app.cur().lines[n - 2..], &[
            Line::Info(
                "You were /away for 1 hours 2 minutes and 3 seconds. [BX-MsgLog Off]".into()
            ),
            Line::Info("You are no longer marked as being away".into()),
        ]);
        assert!(app.away.is_none());
        // The idle clock restarted at the keypress.
        let back = T0 + 600_000 + 3_723_000;
        app.fake_now = Some(back + 599_000);
        assert_eq!(app.tick(), vec![]);
        app.fake_now = Some(back + 600_000);
        assert_eq!(app.tick(), vec![auto_msg()]);
    }

    #[test]
    fn test_manual_away_not_cleared_by_input() {
        let mut app = idle_app();
        assert_eq!(typed(&mut app, "/away lunch"), vec![Effect::Away(Some("lunch".into()))]);
        app.handle_mx(MxEvent::Away(Some("lunch".into())));
        // Typing does not bring us back, and the timer leaves it alone.
        assert_eq!(app.handle_key(key(KeyCode::Char('a'))), vec![]);
        app.fake_now = Some(T0 + 3_600_000);
        assert_eq!(app.tick(), vec![]);
        assert_eq!(app.away.as_deref(), Some("lunch"));
        // A manual /away during an auto-away is manual too.
        let mut app = idle_app();
        app.fake_now = Some(T0 + 600_000);
        app.tick();
        typed(&mut app, "/away gone");
        app.handle_mx(MxEvent::Away(Some("gone".into())));
        assert_eq!(app.handle_key(key(KeyCode::Char('a'))), vec![]);
        // Only /back ends it.
        app.handle_key(ctrl('u'));
        assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None)]);
    }

    #[test]
    fn test_auto_away_off_disables() {
        let mut app = idle_app();
        typed(&mut app, "/set auto_away off");
        assert!(!app.vars.auto_away);
        app.fake_now = Some(T0 + 86_400_000);
        assert_eq!(app.tick(), vec![]);
        typed(&mut app, "/set auto_away on");
        app.fake_now = Some(app.last_input + 600_000);
        assert_eq!(app.tick(), vec![auto_msg()]);
        // AUTO_AWAY_TIME 0 disables too.
        let mut app = idle_app();
        typed(&mut app, "/set auto_away_time 0");
        app.fake_now = Some(T0 + 86_400_000);
        assert_eq!(app.tick(), vec![]);
        // Never in --smoke: an unarmed app (the default) never goes away.
        let mut app = self::app();
        app.fake_now = Some(app.last_input + 86_400_000);
        assert_eq!(app.tick(), vec![]);
    }

    #[test]
    fn test_away_msgcount_and_presence() {
        let mut app = idle_app();
        app.fake_now = Some(T0 + 600_000);
        app.tick();
        let dm = owned_room_id!("!c:example.org");
        let msg = |id: &str| {
            (Some(id.to_string()), Some(T0), Line::Public {
                nick: "armin".into(),
                own: false,
                text: "yo".into(),
            })
        };
        app.handle_mx(MxEvent::Lines {
            room_id: dm.clone(),
            lines: vec![msg("$1"), msg("$2"), msg("$1")],
            backfill: false,
        });
        assert_eq!(app.status().away_msgs, 2);
        // Channel lines do not count.
        app.handle_mx(MxEvent::Lines {
            room_id: owned_room_id!("!a:example.org"),
            lines: vec![msg("$3")],
            backfill: false,
        });
        assert_eq!(app.status().away_msgs, 2);
        app.handle_key(key(KeyCode::Char('x')));
        app.handle_mx(MxEvent::Away(None));
        assert_eq!(app.status().away_msgs, 0);

        // The query window shows the peer's presence.
        let q = app.find_window("armin").unwrap();
        app.switch(q);
        assert!(!app.status().query_away);
        let armin = owned_user_id!("@armin:remote.tld");
        app.handle_mx(MxEvent::Presence { user: armin.clone(), away: Some("brb".into()) });
        assert!(app.status().query_away);
        app.handle_mx(MxEvent::Presence { user: armin, away: None });
        assert!(!app.status().query_away);
    }

    // ---- AWAY_NICK ------------------------------------------------------

    fn whois_lines(me: &str, name: &str) -> Vec<Line> {
        let row = |row: u8, last: bool, key: &str, value: &str| {
            Line::WhoisRow { row, last, key: key.into(), value: value.into() }
        };
        vec![
            Line::WhoisRule,
            Line::WhoisNick {
                nick: "carla".into(),
                mxid: me.into(),
                info: "unknown".into(),
            },
            row(1, false, "ircname", name),
            row(2, true, "server", "example.org"),
        ]
    }

    /// Connect: the first lag sample sends the silent self-whois.
    fn probe(app: &mut App, name: &str) {
        assert_eq!(app.handle_mx(MxEvent::Lag(0)), vec![Effect::Whois(app.me.clone())]);
        let me = app.me.to_string();
        for line in whois_lines(&me, name) {
            app.handle_mx(MxEvent::Info { room_id: None, line });
        }
    }

    fn named(name: &str) -> App {
        let mut app = app();
        probe(&mut app, name);
        app
    }

    fn nick(s: &str) -> Effect {
        Effect::Nick(s.into())
    }

    fn away(s: &str) -> Effect {
        Effect::Away(Some(s.into()))
    }

    #[test]
    fn test_away_nick_probe_is_silent() {
        let mut app = app();
        let before: Vec<usize> = app.windows.iter().map(|w| w.lines.len()).collect();
        probe(&mut app, "Carla");
        let after: Vec<usize> = app.windows.iter().map(|w| w.lines.len()).collect();
        assert_eq!(before, after, "self-whois lines are swallowed");
        assert_eq!(app.display_name.as_deref(), Some("Carla"));
        assert_eq!(app.probe, Probe::Done);
        // only once
        assert_eq!(app.handle_mx(MxEvent::Lag(1)), vec![]);
        // a later /whois is shown in full
        let me = app.me.to_string();
        for line in whois_lines(&me, "Carla") {
            app.handle_mx(MxEvent::Info { room_id: None, line });
        }
        assert!(matches!(last(&app), Line::WhoisRow { last: true, .. }));
    }

    #[test]
    fn test_away_nick_probe_gives_back_foreign_whois() {
        let mut app = app();
        app.handle_mx(MxEvent::Lag(0));
        for line in whois_lines("@armin:remote.tld", "Armin") {
            app.handle_mx(MxEvent::Info { room_id: None, line });
        }
        let lines = &app.cur().lines;
        let n = lines.len();
        assert!(matches!(lines[n - 4], Line::WhoisRule), "{:?}", &lines[n - 4..]);
        assert!(matches!(lines[n - 3], Line::WhoisNick { .. }));
        assert_eq!(app.display_name, None);
    }

    #[test]
    fn test_away_nick_manual_away_and_back() {
        let mut app = named("Carla");
        assert_eq!(typed(&mut app, "/away lunch"), vec![away("lunch"), nick("Carla afk")]);
        assert_eq!(app.vars.away_nick_orig.as_deref(), Some("Carla"));
        app.handle_mx(MxEvent::Away(Some("lunch".into())));
        app.handle_mx(MxEvent::OwnDisplayName("Carla afk".into()));
        let shown = app.windows[0].lines.iter().any(|l| {
            *l == Line::Error("Display name is now Carla afk (away; /back restores Carla)".into())
        });
        assert!(shown, "iamBitchX line on the change");
        // already away: a new reason, but the nick is left alone
        assert_eq!(typed(&mut app, "/away dinner"), vec![away("dinner")]);
        assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None), nick("Carla")]);
        assert_eq!(app.vars.away_nick_orig, None);
        app.handle_mx(MxEvent::Away(None));
        app.handle_mx(MxEvent::OwnDisplayName("Carla".into()));
        let n = app.windows[0].lines.len();
        assert_eq!(app.windows[0].lines[n - 2], Line::Error("Display name is now Carla".into()));
        // /back again: nothing left to restore
        assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None)]);
    }

    #[test]
    fn test_away_nick_never_stacks() {
        // crash while away left the away nick on, the original unknown
        let mut app = named("Carla afk");
        assert!(matches!(last(&app), Line::Error(e) if e.contains("/back to restore Carla")));
        assert_eq!(app.vars.away_nick_orig.as_deref(), Some("Carla"));
        assert_eq!(typed(&mut app, "/away"), vec![away("away")], "no 'Carla afk afk'");
        assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None), nick("Carla")]);

        // a display name that already is an away nick, no orig recorded
        let mut app = super::tests::app();
        app.display_name = Some("Carla afk".into());
        assert_eq!(typed(&mut app, "/away"), vec![away("away")]);
        assert_eq!(app.vars.away_nick_orig.as_deref(), Some("Carla"));

        // many rounds: always the same two names
        let mut app = named("Carla");
        for _ in 0..3 {
            let fx = typed(&mut app, "/away");
            assert_eq!(fx, vec![away("away"), nick("Carla afk")]);
            app.handle_mx(MxEvent::Away(Some("away".into())));
            app.handle_mx(MxEvent::OwnDisplayName("Carla afk".into()));
            assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None), nick("Carla")]);
            app.handle_mx(MxEvent::Away(None));
            app.handle_mx(MxEvent::OwnDisplayName("Carla".into()));
        }
    }

    #[test]
    fn test_away_nick_empty_disables() {
        let mut app = named("Carla");
        app.vars.away_nick = String::new();
        assert_eq!(typed(&mut app, "/away"), vec![away("away")]);
        assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None)]);
        assert_eq!(app.vars.away_nick_orig, None);
        // auto-away too
        app.handle_mx(MxEvent::Away(None));
        app.auto_away_armed = true;
        app.fake_now = Some(app.last_input + 600_000);
        assert_eq!(app.tick(), vec![auto_msg()]);
        assert_eq!(app.activity(), vec![Effect::Away(None)]);
    }

    #[test]
    fn test_away_nick_unknown_display_name_skips() {
        let mut app = app();
        assert_eq!(typed(&mut app, "/away"), vec![away("away")]);
        assert_eq!(last(&app), Line::Error("AWAY_NICK skipped: display name not known yet".into()));
    }

    #[test]
    fn test_away_messages_manual_vs_auto() {
        let mut app = named("Carla");
        app.vars.away_nick = "[%n]".into();
        assert_eq!(typed(&mut app, "/away"), vec![away("away"), nick("[Carla]")]);
        typed(&mut app, "/back");
        typed(&mut app, "/set away_msg gone fishing");
        assert_eq!(app.vars.away_msg, "gone fishing");
        assert_eq!(typed(&mut app, "/away")[0], away("gone fishing"));
        typed(&mut app, "/back");
        assert_eq!(typed(&mut app, "/away brb")[0], away("brb"), "text wins over AWAY_MSG");
        typed(&mut app, "/back");

        // auto-away: AUTO_AWAY_MSG with %t, plus the away nick
        app.handle_mx(MxEvent::Away(None));
        typed(&mut app, "/set auto_away_msg idle %t min");
        typed(&mut app, "/set auto_away_time 900");
        app.auto_away_armed = true;
        app.fake_now = Some(app.last_input + 900_000);
        assert_eq!(app.tick(), vec![away("idle 15 min"), nick("[Carla]")]);
        app.handle_mx(MxEvent::OwnDisplayName("[Carla]".into()));
        // auto-back restores
        assert_eq!(app.activity(), vec![Effect::Away(None), nick("Carla")]);
    }

    #[test]
    fn test_away_nick_restored_after_restart() {
        let dir = std::env::temp_dir().join(format!("bx-awaynick-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let _ = std::fs::remove_file(&path);
        let load = || {
            let s = std::fs::read_to_string(&path).unwrap();
            let cfg = crate::config::Config::parse_toml(&s).unwrap();
            Vars::from_config(&cfg.settings.unwrap().bitchx.unwrap())
        };

        // run 1: auto-away switches the nick, then the client dies
        let mut app = named("Carla");
        app.config_toml = Some(path.clone());
        app.auto_away_armed = true;
        app.fake_now = Some(app.last_input + 600_000);
        assert_eq!(app.tick(), vec![auto_msg(), nick("Carla afk")]);
        assert_eq!(load().away_nick_orig.as_deref(), Some("Carla"), "persisted");
        drop(app);

        // run 2: not away, display name still the away nick -> offer + /back
        let mut app = super::tests::app();
        app.vars = load();
        app.config_toml = Some(path.clone());
        probe(&mut app, "Carla afk");
        assert_eq!(
            last(&app),
            Line::Error(
                "Display name is still the away nick Carla afk: type /back to restore Carla".into()
            )
        );
        assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None), nick("Carla")]);
        assert_eq!(load().away_nick_orig, None, "cleared on disk");

        // a custom original survives too (not derivable from the away nick)
        let mut app = named("Neo");
        app.config_toml = Some(path.clone());
        app.vars.away_nick = "zzz".into();
        assert_eq!(typed(&mut app, "/away"), vec![away("away"), nick("zzz")]);
        let mut app = app_after_restart(&path, load(), "zzz");
        assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None), nick("Neo")]);

        // restarted after the restore already happened: no offer, orig dropped
        let mut app = named("Neo");
        app.config_toml = Some(path.clone());
        typed(&mut app, "/away");
        let app = app_after_restart(&path, load(), "Neo");
        assert!(!matches!(last(&app), Line::Error(e) if e.contains("/back")));
        assert_eq!(load().away_nick_orig, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn app_after_restart(path: &std::path::Path, vars: Vars, name: &str) -> App {
        let mut app = app();
        app.vars = vars;
        app.config_toml = Some(path.to_path_buf());
        probe(&mut app, name);
        app
    }
}
