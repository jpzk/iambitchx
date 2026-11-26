//! # The BitchX client state machine
//!
//! Pure state: numbered windows, the input line, and slash-command dispatch.
//! Keys and Matrix events go in, [`Effect`]s for the Matrix side come out.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};

use crate::bitchx::channels::{list_lines, normalize_join};
use crate::bitchx::commands::{BitchxCommand, COMMAND_NAMES, VerifyCmd, parse};
use crate::bitchx::e2ee::{encrypt_prompt, sas_lines};
use crate::bitchx::format::names_cell_width;
use crate::bitchx::input::Input;
use crate::bitchx::mention::Matcher;
use crate::bitchx::model::{
    Effect,
    Line,
    Member,
    MxEvent,
    Presence,
    RoomSnapshot,
    VerifyAction,
    VerifyUpdate,
    nick_of,
    register_me,
    same_localpart,
    whois_idle,
};
use crate::bitchx::status::{BxStatus, UserCounts};
use crate::bitchx::text::{clean_line, clean_snapshot, one_line};
use crate::bitchx::vars::{Vars, expand_auto_away_msg, expand_away_nick, lookup, strip_away_nick};

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
        MxEvent::Verify { user, update: VerifyUpdate::Cancelled(r) } => {
            MxEvent::Verify {
                user,
                update: VerifyUpdate::Cancelled(one_line(&r)),
            }
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

/// The iamBitchX logo shown in the status window, in figlet `slant` style
/// (47 columns). Rendered italic where the terminal supports it.
pub const BANNER: [&str; 5] = [
    r"    _               ____  _ __       __   _  __",
    r"   (_)___ _____ ___/ __ )(_) /______/ /_ | |/ /",
    r"  / / __ `/ __ `__ \ __  / / __/ ___/ __ \|   /",
    r" / / /_/ / / / / / / /_/ / / /_/ /__/ / / /   |",
    r"/_/\__,_/_/ /_/ /_/_____/_/\__/\___/_/ /_/_/|_|",
];

/// F37: the line `spawn_sync` sends when the sync loop breaks for good.
fn is_sync_death(line: &Line) -> bool {
    matches!(line, Line::Error(t) if t.starts_with("sync: "))
}

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

/// The flood guard's ` DBG (suppressed N)`.
fn suppressed_line(n: u32) -> Line {
    Line::Debug {
        cat: String::new(),
        text: format!("(suppressed {n})"),
    }
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
    /// Unread mention (BitchX AUTO_REPLY): shown apart in `Act:`.
    pub hilite: bool,
    /// Rows scrolled up from the bottom (PageUp/PageDown).
    pub scroll: usize,
    pub backfill_requested: bool,
    /// F29: the room name cannot be trusted as a channel name (it clashes
    /// with another window, or poses as an alias on our own server, which
    /// real aliases never show): the room ID is shown instead.
    pub spoof: bool,
}

impl Window {
    pub fn room_id(&self) -> Option<&OwnedRoomId> {
        self.room.room_id.as_ref()
    }

    /// The `#channel` shown in the prompt and status line.
    pub fn channel(&self) -> Option<&str> {
        self.room.room_id.as_ref().map(|id| {
            if self.spoof {
                id.as_str()
            } else {
                self.room.name.as_str()
            }
        })
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

    /// Members this nick may mean: the displayed (maybe qualified) nick or
    /// the bare localpart, case-insensitively.
    fn members_by_nick(&self, nick: &str) -> Vec<OwnedUserId> {
        let nick = nick.to_lowercase();
        let exact: Vec<OwnedUserId> = self
            .room
            .members
            .iter()
            .filter(|m| m.nick.to_lowercase() == nick)
            .map(|m| m.user_id.clone())
            .collect();
        let local: Vec<OwnedUserId> = self
            .room
            .members
            .iter()
            .filter(|m| m.user_id.localpart().to_lowercase() == nick)
            .map(|m| m.user_id.clone())
            .collect();
        // The shown nick is already disambiguated (`bob` next to
        // `bob@evil.org`); a bare localpart shown nowhere (both users
        // qualified) matches all of them.
        if exact.is_empty() { local } else { exact }
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
    /// Several flows with this user reached SAS: the emoji shown cannot be
    /// tied to this flow, so `/verify confirm` refuses it (F44).
    pub ambiguous: bool,
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
}

/// DBG flood guard: at most this many DBG lines per second per window.
pub const DBG_PER_SEC: u32 = 20;

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
        register_me(&me);
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
        if seen.len() > 1 &&
            let Some(r) = one(seen)
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
                match self.window_arg(&t) {
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
        if matches!(action, VerifyAction::Cancel | VerifyAction::Mismatch) &&
            let Some(p) = self.verifications.iter_mut().find(|p| p.flow_id == v.flow_id)
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
            if n == 1 &&
                let Some(v) = app.verifications.iter_mut().find(|v| v.user == user)
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

    /// A presence event for `user`, last active `ago` seconds before the
    /// app's fake monotonic clock.
    pub fn presence_ev(user: &OwnedUserId, state: &str, ago: Option<u64>) -> MxEvent {
        presence_at(user, state, ago, None, T_MONO.with(|t| *t))
    }

    fn presence_at(
        user: &OwnedUserId,
        state: &str,
        ago: Option<u64>,
        active: Option<bool>,
        at: Instant,
    ) -> MxEvent {
        let ago = ago.map(std::time::Duration::from_secs);
        let presence = crate::bitchx::model::away_of(state, None, ago, active, at);
        MxEvent::Presence { user: user.clone(), presence }
    }

    thread_local! {
        static T_MONO: Instant = Instant::now();
    }

    fn mono_app() -> App {
        let mut app = app();
        app.fake_mono = Some(T_MONO.with(|t| *t));
        app
    }

    fn advance(app: &mut App, secs: u64) {
        let t = app.fake_mono.unwrap() + std::time::Duration::from_secs(secs);
        app.fake_mono = Some(t);
    }

    fn query_mark(app: &App) -> Option<String> {
        app.status().query_away.map(|m| m.text())
    }

    #[test]
    fn test_query_presence_ticks_with_fake_clock() {
        let mut app = mono_app();
        let armin = owned_user_id!("@armin:remote.tld");
        let q = app.find_window("armin").unwrap();
        app.switch(q);
        app.handle_mx(presence_ev(&armin, "unavailable", Some(45)));
        assert_eq!(query_mark(&app).as_deref(), Some("zZzZ 45s"));
        advance(&mut app, 15);
        assert_eq!(query_mark(&app).as_deref(), Some("zZzZ 1m"));
        advance(&mut app, 11 * 60);
        assert_eq!(query_mark(&app).as_deref(), Some("zZzZ 12m"));
        advance(&mut app, 4 * 3_600);
        assert_eq!(query_mark(&app).as_deref(), Some("zZzZ 4h"));
        // offline vs unavailable vs online
        app.handle_mx(presence_ev(&armin, "offline", Some(2 * 86_400)));
        let off = app.status().query_away.unwrap();
        assert!(off.offline);
        assert_eq!(off.text(), "off 2d");
        app.handle_mx(presence_ev(&armin, "offline", Some(3 * 604_800)));
        assert_eq!(query_mark(&app).as_deref(), Some("off 3w"));
        app.handle_mx(presence_ev(&armin, "online", Some(5)));
        assert_eq!(query_mark(&app), None);
        let now = app.mono();
        app.handle_mx(presence_at(&armin, "unavailable", Some(99), Some(true), now));
        assert_eq!(query_mark(&app), None);
        // our own away keeps the BitchX `(zZzZ N)` message counter
        app.away = Some("gone".into());
        app.away_msgs = 3;
        assert!(app.status().away);
        assert_eq!(app.status().away_msgs, 3);
    }

    #[test]
    fn test_names_show_presence_marks() {
        let mut app = mono_app();
        app.handle_key(alt('2'));
        let armin = owned_user_id!("@armin:remote.tld");
        let bob = owned_user_id!("@bob:example.org");
        let me = app.me.clone();
        app.handle_mx(presence_ev(&armin, "unavailable", Some(4 * 3_600)));
        app.handle_mx(presence_ev(&bob, "offline", Some(2 * 86_400)));
        app.handle_mx(presence_ev(&me, "online", Some(1)));
        typed(&mut app, "/names");
        let Line::Names { cells, width } = last(&app) else {
            panic!("{:?}", last(&app));
        };
        let marks: Vec<_> = cells
            .iter()
            .map(|(_, n, m)| (n.clone(), m.as_ref().map(|m| m.text())))
            .collect();
        assert_eq!(marks, vec![
            ("carla".into(), None),
            ("bob".into(), Some("off 2d".into())),
            ("armin".into(), Some("zZzZ 4h".into())),
        ]);
        // `armin (zZzZ 4h)` is the widest cell
        assert_eq!(width, 15);
        let text = crate::bitchx::format::plain(&crate::bitchx::format::format_line(
            &Line::Names { cells, width },
            "#room",
        ))
        .concat();
        assert_eq!(text.trim_end(), "[@carla           ] [+bob (off 2d)    ] [ armin (zZzZ 4h) ]");
    }

    fn whois_of(app: &mut App, mxid: &str) {
        let row = |row: u8, last: bool, key: &str, value: &str| {
            Line::WhoisRow { row, last, key: key.into(), value: value.into() }
        };
        let lines = vec![
            Line::WhoisRule,
            Line::WhoisNick {
                nick: "armin".into(),
                mxid: mxid.into(),
                info: "unverified".into(),
            },
            row(1, false, "ircname", "Armin"),
            row(2, false, "server", "remote.tld"),
            row(3, true, "devices", "2"),
        ];
        for line in lines {
            app.handle_mx(MxEvent::Info { room_id: None, line });
        }
    }

    #[test]
    fn test_whois_idle_line_and_mark() {
        let mut app = mono_app();
        let armin = owned_user_id!("@armin:remote.tld");
        app.handle_mx(presence_ev(&armin, "unavailable", Some(4 * 3_600 + 15 * 60)));
        advance(&mut app, 7);
        whois_of(&mut app, "@armin:remote.tld");
        let lines = &app.cur().lines;
        let n = lines.len();
        assert_eq!(lines[n - 2], Line::WhoisRow {
            row: 3,
            last: false,
            key: "idle".into(),
            value: "4 hours 15 mins 7 secs".into(),
        });
        assert_eq!(lines[n - 1], Line::WhoisRow {
            row: 4,
            last: true,
            key: "devices".into(),
            value: "2".into(),
        });
        let nick = lines.iter().rev().find(|l| matches!(l, Line::WhoisMark { .. })).unwrap();
        let text =
            crate::bitchx::format::plain(&crate::bitchx::format::format_line(nick, "")).concat();
        assert_eq!(text.trim_end(), "| armin (@armin:remote.tld) (unverified) (zZzZ 4h)");
        let idle =
            crate::bitchx::format::plain(&crate::bitchx::format::format_line(&lines[n - 2], ""))
                .concat();
        assert_eq!(idle.trim_end(), " idle     : 4 hours 15 mins 7 secs");

        // online: idle line still there, no marker on the nick row
        // stamped at the (advanced) fake now, so no extra elapsed time
        let now = app.fake_mono.unwrap();
        app.handle_mx(presence_at(&armin, "online", Some(30), None, now));
        whois_of(&mut app, "@armin:remote.tld");
        let lines = &app.cur().lines;
        let n = lines.len();
        assert!(
            matches!(&lines[n - 2], Line::WhoisRow { key, value, .. } if key == "idle" && value == "0 hours 0 mins 30 secs")
        );
        assert!(matches!(&lines[n - 5], Line::WhoisNick { .. }), "{:?}", &lines[n - 6..]);

        // unknown presence: no idle line, rows untouched
        whois_of(&mut app, "@bob:example.org");
        let lines = &app.cur().lines;
        let n = lines.len();
        assert!(matches!(&lines[n - 1], Line::WhoisRow { row: 3, last: true, .. }));
        assert!(matches!(&lines[n - 2], Line::WhoisRow { key, .. } if key == "server"));
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
        assert_eq!(app.prompt(), "[status] ");
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
    fn test_mention_hilites_window_and_beeps_once() {
        let mut app = app();
        app.handle_key(alt('2'));
        let say = |app: &mut App, room: &str, id: &str, own: bool, text: &str| {
            app.handle_mx(MxEvent::Lines {
                room_id: OwnedRoomId::try_from(room).unwrap(),
                lines: vec![(Some(id.into()), None, Line::Public {
                    nick: "bob".into(),
                    own,
                    text: text.into(),
                })],
                backfill: false,
            })
        };
        say(&mut app, "!b:example.org", "$1", false, "plain chatter");
        say(&mut app, "!c:example.org", "$2", false, "carla: look");
        say(&mut app, "!c:example.org", "$3", false, "CARLA again");
        assert_eq!(app.act(), vec![3, 4]);
        assert_eq!(app.hilite(), vec![4], "only the mention window is highlighted");
        assert_eq!(app.status().hilite, vec![4]);
        assert!(app.take_bell(), "bell rings");
        assert!(!app.take_bell(), "...once");
        // A duplicate (already seen event) neither beeps nor highlights.
        say(&mut app, "!b:example.org", "$1", false, "carla");
        assert!(!app.take_bell());
        // Own messages and near-misses never highlight.
        say(&mut app, "!b:example.org", "$4", true, "i am carla");
        say(&mut app, "!b:example.org", "$5", false, "carlax is not me");
        assert_eq!(app.hilite(), vec![4]);
        assert!(!app.take_bell());
        // Switching to the window clears its highlight.
        app.handle_key(alt('4'));
        assert_eq!(app.hilite(), Vec::<usize>::new());
        // In the current window: beep, but nothing to mark.
        say(&mut app, "!c:example.org", "$6", false, "@carla:example.org hi");
        assert!(app.take_bell());
        assert!(!app.windows[3].hilite);
        // BEEP off: highlighted, silent.
        typed(&mut app, "/set beep off");
        say(&mut app, "!b:example.org", "$7", false, "hey carla");
        assert_eq!(app.hilite(), vec![3]);
        assert!(!app.take_bell());
        // AUTO_REPLY words and the display name highlight too.
        typed(&mut app, "/set auto_reply creed, bx");
        assert_eq!(app.vars.auto_reply, "creed,bx");
        app.display_name = Some("Big Boss".into());
        assert!(app.matcher().matches("yo creed"));
        assert!(app.matcher().matches("big boss"));
        typed(&mut app, "/set -auto_reply");
        assert_eq!(app.vars.auto_reply, "");
        assert!(!app.matcher().matches("yo creed"));
    }

    #[test]
    fn test_backfilled_mention_is_silent() {
        let mut app = app();
        app.handle_mx(MxEvent::Lines {
            room_id: owned_room_id!("!b:example.org"),
            lines: vec![(Some("$old".into()), None, Line::Public {
                nick: "bob".into(),
                own: false,
                text: "carla".into(),
            })],
            backfill: true,
        });
        assert!(!app.take_bell());
        assert_eq!(app.hilite(), Vec::<usize>::new());
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
        // An alias always goes to the server (F29: room names can pose as
        // aliases); a joined room ID just switches.
        assert_eq!(typed(&mut app, "/join #other:example.org"), vec![Effect::Join(JoinTarget {
            id: "#other:example.org".into(),
            implicit_server: false,
            via: vec![],
        })]);
        assert!(
            typed(&mut app, "/join !b:example.org")
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
                    ('@', "carla".into(), None),
                    ('+', "bob".into(), None),
                    (' ', "armin".into(), None),
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

    /// F37: a dead sync loop turns the status bar `[SYNC LOST]` for good and
    /// leaves a -:- line; other room-less errors do not.
    #[test]
    fn test_f37_sync_death_marker() {
        let mut app = app();
        let bar = |app: &App| -> String {
            app.status().line2().spans.iter().map(|s| s.content.to_string()).collect()
        };
        assert!(bar(&app).contains("[Lag ??]"), "{}", bar(&app));
        app.handle_mx(MxEvent::Info { room_id: None, line: Line::Error("other".into()) });
        assert!(!app.sync_lost);
        app.handle_mx(MxEvent::Info {
            room_id: None,
            line: Line::Error("sync: connection reset".into()),
        });
        assert!(app.sync_lost);
        assert!(bar(&app).contains("[SYNC LOST]"), "{}", bar(&app));
        assert!(!bar(&app).contains("Lag"), "{}", bar(&app));
        let warns = |app: &App| {
            app.windows[0]
                .lines
                .iter()
                .filter(|l| matches!(l, Line::Info(t) if t.contains("Sync with the homeserver stopped")))
                .count()
        };
        assert_eq!(warns(&app), 1);
        // A lag sample does not hide it; a second error does not repeat it.
        app.handle_mx(MxEvent::Lag(0));
        app.handle_mx(MxEvent::Info {
            room_id: None,
            line: Line::Error("sync: again".into()),
        });
        assert!(bar(&app).contains("[SYNC LOST]"));
        assert_eq!(warns(&app), 1);
    }

    /// F44: a second flow with the same user no longer replaces the first;
    /// commands then need the flow id, and emoji that cannot be tied to one
    /// flow can never be confirmed.
    #[test]
    fn test_f44_verification_keyed_by_flow() {
        let mut app = app();
        let me2 = owned_user_id!("@armin:remote.tld");
        let req = |flow: &str, dev: &str| {
            MxEvent::Verify {
                user: me2.clone(),
                update: VerifyUpdate::Requested {
                    flow_id: flow.into(),
                    device: Some(dev.into()),
                    room_id: None,
                    we_started: false,
                },
            }
        };
        app.handle_mx(req("flowA", "DEVA"));
        app.handle_mx(req("flowB", "DEVB"));
        assert_eq!(app.verifications.len(), 2);
        assert!(
            app.windows[0]
                .lines
                .iter()
                .any(|l| matches!(l, Line::Error(t) if t.contains("another verification pending")))
        );
        // No flow id: refused, both listed.
        for cmd in [
            "/verify accept armin",
            "/verify confirm armin",
            "/verify cancel armin",
        ] {
            assert_eq!(typed(&mut app, cmd), vec![], "{cmd}");
            assert!(
                matches!(last(&app), Line::Error(t)
                if t.contains("2 pending verifications") && t.contains("flowA [DEVA]")
                    && t.contains("flowB [DEVB]")),
                "{cmd}"
            );
        }
        // The flow id (or a unique prefix) picks exactly that flow.
        assert_eq!(typed(&mut app, "/verify accept armin flowA"), vec![Effect::Verify {
            user: me2.clone(),
            flow_id: Some("flowA".into()),
            action: VerifyAction::Accept,
        }]);
        assert_eq!(typed(&mut app, "/verify accept armin flowB")[0], Effect::Verify {
            user: me2.clone(),
            flow_id: Some("flowB".into()),
            action: VerifyAction::Accept,
        });
        assert_eq!(typed(&mut app, "/verify accept armin flowC"), vec![]);
        // Emoji arrive while two flows are pending: they cannot be tied to a
        // flow, so confirm is refused even with an explicit flow id.
        let emojis: Vec<(String, String)> =
            (0..7).map(|i| (format!("e{i}"), format!("n{i}"))).collect();
        app.handle_mx(MxEvent::Verify {
            user: me2.clone(),
            update: VerifyUpdate::Emoji(emojis),
        });
        assert!(app.verifications.iter().all(|v| v.ambiguous));
        assert_eq!(typed(&mut app, "/verify confirm armin flowA"), vec![]);
        assert!(matches!(last(&app), Line::Error(t) if t.contains("may belong to another flow")));
        // Cancelling one: the Matrix side's Cancelled removes that flow only.
        assert_eq!(typed(&mut app, "/verify cancel armin flowB")[0], Effect::Verify {
            user: me2.clone(),
            flow_id: Some("flowB".into()),
            action: VerifyAction::Cancel,
        });
        app.handle_mx(MxEvent::Verify {
            user: me2.clone(),
            update: VerifyUpdate::Cancelled("user".into()),
        });
        assert_eq!(app.verifications.len(), 1);
        assert_eq!(app.verifications[0].flow_id, "flowA");
        // flowA stays tainted: its emoji were ambiguous.
        assert_eq!(typed(&mut app, "/verify confirm armin"), vec![]);
        // A single fresh flow confirms normally.
        app.handle_mx(MxEvent::Verify {
            user: me2.clone(),
            update: VerifyUpdate::Cancelled("user".into()),
        });
        assert!(app.verifications.is_empty());
        app.handle_mx(req("flowC", "DEVA"));
        assert_eq!(typed(&mut app, "/verify confirm armin"), vec![Effect::Verify {
            user: me2,
            flow_id: Some("flowC".into()),
            action: VerifyAction::Confirm,
        }]);
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
            Line::SetVar { name: "AUTO_REPLY".into(), value: None },
            Line::SetVar {
                name: "AWAY_MSG".into(),
                value: Some("away".into())
            },
            Line::SetVar {
                name: "AWAY_NICK".into(),
                value: Some("%n^afk".into())
            },
            Line::SetVar { name: "BEEP".into(), value: Some("ON".into()) },
            Line::SetVar { name: "DEBUG".into(), value: Some("OFF".into()) },
            Line::SetVar {
                name: "DEBUG_FILTER".into(),
                value: Some("all".into())
            },
            Line::SetVar {
                name: "DEBUG_RAW".into(),
                value: Some("OFF".into())
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
        typed(&mut app, "/set auto_reply creed,bx");
        typed(&mut app, "/set beep off");
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
        assert_eq!(vars.auto_reply, "creed,bx");
        assert!(!vars.beep);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ---- protocol debug mode (DBG lines) --------------------------------

    fn dbg(room: Option<&str>, text: &str) -> MxEvent {
        MxEvent::Debug(crate::debug_tap::DbgLine {
            cat: crate::debug_tap::Cat::Http,
            room: room.map(str::to_owned),
            text: text.into(),
        })
    }

    fn dbg_texts(w: &Window) -> Vec<String> {
        w.lines
            .iter()
            .filter_map(|l| {
                match l {
                    Line::Debug { cat, text } if cat.is_empty() => Some(text.clone()),
                    Line::Debug { cat, text } => Some(format!("{cat} {text}")),
                    _ => None,
                }
            })
            .collect()
    }

    #[test]
    fn test_debug_routing_and_flood_guard() {
        const T0: i64 = 1_800_000_000_000;
        let mut app = app();
        app.fake_now = Some(T0);
        let a = app.window_of(&owned_room_id!("!a:example.org")).unwrap();
        app.handle_mx(dbg(None, "GET /_matrix/client/v3/sync 200 1.2s"));
        app.handle_mx(dbg(Some("!nope:example.org"), "unknown room"));
        assert_eq!(
            dbg_texts(&app.windows[0]),
            vec![
                "http GET /_matrix/client/v3/sync 200 1.2s",
                "http unknown room"
            ],
            "non-room / unknown-room lines go to the status window"
        );
        assert!(dbg_texts(&app.windows[a]).is_empty());

        for i in 0..25 {
            app.handle_mx(dbg(Some("!a:example.org"), &format!("l{i}")));
        }
        let shown = dbg_texts(&app.windows[a]);
        assert_eq!(shown.len(), DBG_PER_SEC as usize);
        assert_eq!(shown.last().unwrap(), "http l19");
        // the status window has its own budget
        app.handle_mx(dbg(None, "status still flows"));
        assert_eq!(dbg_texts(&app.windows[0]).len(), 3);

        // next second: the suppressed count first, then the line
        app.fake_now = Some(T0 + 1000);
        app.handle_mx(dbg(Some("!a:example.org"), "next"));
        let shown = dbg_texts(&app.windows[a]);
        assert_eq!(shown[20..], ["(suppressed 5)".to_string(), "http next".to_string()]);

        // a burst's tail is flushed by the timer
        for i in 0..22 {
            app.handle_mx(dbg(Some("!a:example.org"), &format!("m{i}")));
        }
        app.fake_now = Some(T0 + 2500);
        app.tick();
        let shown = dbg_texts(&app.windows[a]);
        assert_eq!(shown.last().unwrap(), "(suppressed 3)");
        assert_eq!(shown.len(), 22 + 19 + 1);
        app.tick();
        assert_eq!(dbg_texts(&app.windows[a]).len(), 42, "flushed once");

        let tl = crate::bitchx::format::format_line(
            &Line::Debug {
                cat: "sync".into(),
                text: "next_batch=s1.. joined=3".into(),
            },
            "#r",
        );
        let s: String = tl
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(s, " DBG sync next_batch=s1.. joined=3");
        let tag = tl[0].spans.iter().find(|s| s.content == "DBG").unwrap();
        assert_eq!(tag.style.fg, Some(ratatui::style::Color::Magenta));
    }

    #[test]
    fn test_set_debug_configures_tap() {
        let _tap = crate::debug_tap::tests::harness();
        let mut app = app();
        assert!(!crate::debug_tap::on());
        typed(&mut app, "/set debug on");
        assert!(crate::debug_tap::on());
        assert!(crate::debug_tap::wants(crate::debug_tap::Cat::Sync));
        typed(&mut app, "/set debug_filter http,auth");
        assert!(crate::debug_tap::wants(crate::debug_tap::Cat::Http));
        assert!(crate::debug_tap::wants(crate::debug_tap::Cat::Auth));
        assert!(!crate::debug_tap::wants(crate::debug_tap::Cat::Sync));
        typed(&mut app, "/set debug_raw on");
        assert!(crate::debug_tap::raw());
        typed(&mut app, "/set debug off");
        assert!(!crate::debug_tap::on());
        assert!(!crate::debug_tap::wants(crate::debug_tap::Cat::Http));
        crate::debug_tap::configure(false, "all", false);
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
        assert_eq!(app.status().query_away, None);
        let armin = owned_user_id!("@armin:remote.tld");
        app.handle_mx(presence_ev(&armin, "unavailable", None));
        assert_eq!(app.status().query_away.map(|m| m.text()).as_deref(), Some("zZzZ"));
        app.handle_mx(presence_ev(&armin, "online", None));
        assert_eq!(app.status().query_away, None);
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
        assert_eq!(typed(&mut app, "/away lunch"), vec![away("lunch"), nick("Carla^afk")]);
        assert_eq!(app.vars.away_nick_orig.as_deref(), Some("Carla"));
        app.handle_mx(MxEvent::Away(Some("lunch".into())));
        app.handle_mx(MxEvent::OwnDisplayName("Carla^afk".into()));
        let shown = app.windows[0].lines.iter().any(|l| {
            *l == Line::Error("Display name is now Carla^afk (away; /back restores Carla)".into())
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
        let mut app = named("Carla^afk");
        assert!(matches!(last(&app), Line::Error(e) if e.contains("/back to restore Carla")));
        assert_eq!(app.vars.away_nick_orig.as_deref(), Some("Carla"));
        assert_eq!(typed(&mut app, "/away"), vec![away("away")], "no 'Carla^afk^afk'");
        assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None), nick("Carla")]);

        // a display name that already is an away nick, no orig recorded
        let mut app = super::tests::app();
        app.display_name = Some("Carla^afk".into());
        assert_eq!(typed(&mut app, "/away"), vec![away("away")]);
        assert_eq!(app.vars.away_nick_orig.as_deref(), Some("Carla"));

        // many rounds: always the same two names
        let mut app = named("Carla");
        for _ in 0..3 {
            let fx = typed(&mut app, "/away");
            assert_eq!(fx, vec![away("away"), nick("Carla^afk")]);
            app.handle_mx(MxEvent::Away(Some("away".into())));
            app.handle_mx(MxEvent::OwnDisplayName("Carla^afk".into()));
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
        assert_eq!(app.tick(), vec![auto_msg(), nick("Carla^afk")]);
        assert_eq!(load().away_nick_orig.as_deref(), Some("Carla"), "persisted");
        drop(app);

        // run 2: not away, display name still the away nick -> offer + /back
        let mut app = super::tests::app();
        app.vars = load();
        app.config_toml = Some(path.clone());
        probe(&mut app, "Carla^afk");
        assert_eq!(
            last(&app),
            Line::Error(
                "Display name is still the away nick Carla^afk: type /back to restore Carla".into()
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

    /// A room with `@f28bob:example.org` (our server), `@f28bob:evil.org`
    /// and `@carla:evil.org` (our localpart on another server).
    fn collision_app() -> App {
        let mut app = App::new(owned_user_id!("@carla:example.org"));
        let mut snap = room("!f28:example.org", "#f28");
        snap.members = vec![
            Member::new(owned_user_id!("@carla:example.org"), 100),
            Member::new(owned_user_id!("@f28bob:example.org"), 0),
            Member::new(owned_user_id!("@f28bob:evil.org"), 0),
            Member::new(owned_user_id!("@carla:evil.org"), 0),
            Member::new(owned_user_id!("@f28eve:a.org"), 0),
            Member::new(owned_user_id!("@f28eve:b.org"), 0),
        ];
        app.handle_mx(MxEvent::Room(snap));
        app.handle_key(alt('2'));
        app
    }

    #[test]
    fn test_f28_localpart_collision_disambiguated() {
        let app = collision_app();
        let mut nicks = app.nicks();
        nicks.sort();
        // Ours and the own-server user stay bare; remote twins are
        // qualified, and nobody else can be shown as `carla`.
        assert_eq!(nicks, vec![
            "carla",
            "carla@evil.org",
            "f28bob",
            "f28bob@evil.org",
            "f28eve@a.org",
            "f28eve@b.org",
        ]);
        // Lines from the remote twin carry the qualified nick too.
        assert_eq!(nick_of(&owned_user_id!("@carla:evil.org")), "carla@evil.org");
        assert_eq!(nick_of(&owned_user_id!("@f28bob:evil.org")), "f28bob@evil.org");
        assert_eq!(nick_of(&owned_user_id!("@carla:example.org")), "carla");
        // Same-server case twins: `@server` would not tell them apart, so
        // they keep their (differently cased) localpart.
        assert_eq!(nick_of(&owned_user_id!("@F28Zed:z.org")), "F28Zed");
        assert_eq!(nick_of(&owned_user_id!("@f28zed:z.org")), "f28zed");
    }

    #[test]
    fn test_f28_ambiguous_privileged_commands_refused() {
        let mut app = collision_app();
        let f28 = owned_room_id!("!f28:example.org");
        for cmd in [
            "/op f28eve",
            "/deop f28eve",
            "/kick f28eve bye",
            "/msg f28eve hi",
            "/query f28eve",
            "/whois f28eve",
        ] {
            assert_eq!(typed(&mut app, cmd), vec![], "{cmd}");
            let Line::Error(e) = last(&app) else {
                panic!("{cmd}: {:?}", last(&app))
            };
            assert!(e.contains("Ambiguous nick"), "{cmd}: {e}");
            assert!(e.contains("@f28eve:a.org") && e.contains("@f28eve:b.org"), "{e}");
        }
        // The qualified nick, the shown bare nick and the full mxid resolve
        // to exactly one user.
        assert_eq!(typed(&mut app, "/op f28eve@b.org"), vec![Effect::Power {
            room_id: f28.clone(),
            user: owned_user_id!("@f28eve:b.org"),
            level: 50,
        }]);
        assert_eq!(typed(&mut app, "/kick f28bob"), vec![Effect::Kick {
            room_id: f28.clone(),
            user: owned_user_id!("@f28bob:example.org"),
            reason: None,
        }]);
        assert_eq!(typed(&mut app, "/whois f28bob@evil.org"), vec![Effect::Whois(owned_user_id!(
            "@f28bob:evil.org"
        ))]);
        assert_eq!(typed(&mut app, "/whois @f28eve:a.org"), vec![Effect::Whois(owned_user_id!(
            "@f28eve:a.org"
        ))]);
    }

    #[test]
    fn test_f29_alias_lookalike_room_name() {
        let mut app = app();
        // The real #rust:matrix.org and a room merely *named* like it.
        app.handle_mx(MxEvent::Room(room("!real:matrix.org", "#rust:matrix.org")));
        app.handle_mx(MxEvent::Room(room("!fake:evil.org", "#rust:matrix.org")));
        // A room name posing as an alias on our own server (real ones show
        // short, `#x`).
        app.handle_mx(MxEvent::Room(room("!fake2:evil.org", "#room:example.org")));
        let chans: Vec<_> = app.windows.iter().map(|w| w.channel().map(str::to_string)).collect();
        assert_eq!(chans[4..], [
            Some("!real:matrix.org".to_string()),
            Some("!fake:evil.org".to_string()),
            Some("!fake2:evil.org".to_string()),
        ]);
        // Neither the prompt nor the status bar show the clashing name.
        app.handle_key(alt('6'));
        assert_eq!(app.prompt(), "[!fake:evil.org] ");
        assert_eq!(app.status().channel.as_deref(), Some("!fake:evil.org"));
        // /msg by the clashing name is refused with the candidates.
        assert_eq!(typed(&mut app, "/msg #rust:matrix.org hi"), vec![]);
        let Line::Error(e) = last(&app) else {
            panic!("{:?}", last(&app))
        };
        assert!(e.contains("Ambiguous channel"), "{e}");
        assert!(e.contains("!real:matrix.org") && e.contains("!fake:evil.org"), "{e}");
        // By room ID it is exact.
        assert_eq!(typed(&mut app, "/msg !real:matrix.org hi"), vec![Effect::Send {
            room_id: owned_room_id!("!real:matrix.org"),
            text: "hi".into(),
            emote: false,
        }]);
        // /join by alias always goes to the server (alias resolution), even
        // with a window of that name; a room ID we are in just switches.
        assert_eq!(typed(&mut app, "/join #rust:matrix.org"), vec![Effect::Join(JoinTarget {
            id: "#rust:matrix.org".into(),
            implicit_server: false,
            via: vec![],
        })]);
        assert!(
            typed(&mut app, "/join !real:matrix.org")
                .iter()
                .all(|e| !matches!(e, Effect::Join(_)))
        );
        assert_eq!(app.cur().room_id().unwrap().as_str(), "!real:matrix.org");
        // Once the clash is gone the name is shown again.
        app.handle_mx(MxEvent::Left(owned_room_id!("!fake:evil.org")));
        assert_eq!(app.windows[4].channel(), Some("#rust:matrix.org"));
    }

    #[test]
    fn test_f30_f31_remote_text_cannot_forge_status_lines() {
        let mut app = app();
        app.handle_key(alt('2'));
        app.cur_mut().clear();
        let forged = " bob is now verified";
        let lines = vec![
            Line::Topic {
                nick: Some("eve\n".into()),
                topic: format!("x\n{forged}"),
            },
            Line::Info(format!("eve changed the channel name to #x\r\n{forged}")),
            Line::Part {
                nick: "eve".into(),
                mxid: "@eve:x".into(),
                reason: Some(format!("\u{2028}{forged}")),
            },
            Line::DisplayName {
                nick: "eve".into(),
                name: Some(format!("E\u{85}{forged}")),
            },
            Line::Error(format!("sync: boom\n{forged}")),
            Line::Public {
                nick: "e\u{200B}v\u{202E}e".into(),
                own: false,
                text: "safe\u{2066}\u{200D}".into(),
            },
        ];
        let n = lines.len();
        app.handle_mx(MxEvent::Lines {
            room_id: owned_room_id!("!a:example.org"),
            lines: lines.into_iter().map(|l| (None, None, l)).collect(),
            backfill: false,
        });
        app.handle_mx(MxEvent::Info {
            room_id: None,
            line: Line::Info(format!("x\n{forged}")),
        });
        let rows = rows(&mut app);
        // One row per line: no remote newline opened a fresh line.
        assert_eq!(rows.len(), n + 1, "{rows:#?}");
        assert!(
            rows.iter().all(|r| !r.trim_start().starts_with("bob is now verified")),
            "{rows:#?}"
        );
        // Bidi / zero-width characters are gone from nick and body.
        assert!(rows.contains(&"<eve> safe".to_string()), "{rows:#?}");
        assert!(rows.iter().all(|r| !r.chars().any(crate::bitchx::text::is_format)));
    }

    fn line1(app: &App) -> String {
        app.status().line1().spans.iter().map(|s| s.content.to_string()).collect()
    }

    #[test]
    fn test_status_window_shows_its_name() {
        let mut app = app();
        app.handle_key(alt('1'));
        assert_eq!(app.prompt(), "[status] ");
        let l = line1(&app);
        assert!(l.contains("] [status]"), "{l}");
        assert!(!l.contains("[]") && !l.contains("[0]"), "{l}");
    }

    #[test]
    fn test_query_status_shows_plus_e() {
        let mut app = app();
        app.handle_key(alt('4'));
        let mut dm = room("!c:example.org", "armin");
        dm.dm_peer = Some(owned_user_id!("@armin:remote.tld"));
        dm.encrypted = true;
        app.handle_mx(MxEvent::Room(dm));
        let l = line1(&app);
        assert!(l.contains("[Query: armin(+e)]"), "{l}");
        assert!(!l.contains("[]"), "{l}");
    }
}
