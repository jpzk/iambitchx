//! # Plain data shared by the Matrix side and the UI
//!
//! Nothing here touches the SDK or the terminal, so the whole UI can be
//! driven by tests.
use std::time::{Duration, Instant};

use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};

use crate::bitchx::channels::{JoinTarget, ListArgs, ListEntry};
pub(crate) use crate::bitchx::commands::Secret;

/// A terminal colour, for the splash art.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Ink {
    Default,
    Black,
    Red,
    Blue,
    Magenta,
    Cyan,
    White,
}

/// One run of the splash art: text with a foreground, a background and a
/// bold flag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Run {
    pub text: String,
    pub fg: Ink,
    pub bg: Ink,
    pub bold: bool,
}

/// One scrollback line, already translated from Matrix into IRC terms.
/// The channel name is filled in by the formatter from the window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Line {
    /// PUBLIC / `SEND_PUBLIC`: `<nick> text`.
    Public { nick: String, own: bool, text: String },
    /// ACTION / `SEND_ACTION`: ` nick text` / ` nick/#chan text`.
    Action { nick: String, own: bool, text: String },
    /// Query (DM) message. `SEND_MSG` `[msg(nick)] text` when `own` (`nick`
    /// is the peer), MSG `[nick(mxid)] text` otherwise.
    Private {
        nick: String,
        mxid: String,
        own: bool,
        text: String,
    },
    /// NOTICE: `-nick(mxid)- text`.
    Notice { nick: String, mxid: String, text: String },
    /// JOIN: ` nick [mxid] has joined #chan`.
    Join { nick: String, mxid: String },
    /// LEAVE: ` nick [mxid] has left #chan [reason]`.
    Part { nick: String, mxid: String, reason: Option<String> },
    /// KICK: ` victim was kicked off #chan by kicker (reason)`.
    Kick {
        victim: String,
        kicker: String,
        reason: Option<String>,
    },
    /// ` nick changed display name to name` -- the nick (mxid localpart)
    /// itself cannot change; `name: None` means the display name was removed.
    DisplayName { nick: String, name: Option<String> },
    /// TOPIC / `TOPIC_CHANGE`: ` Topic (#chan): [changed by nick:] topic`.
    Topic { nick: Option<String>, topic: String },
    /// MODE: ` mode/#chan [modes] by nick`.
    Mode { modes: String, nick: String },
    /// INVITE: ` nick invited target to #chan`.
    Invite { nick: String, target: String },
    /// `/set` listing row: `    name                        value`.
    SetVar { name: String, value: Option<String> },
    /// Any other ` text` line (server/login info, command output).
    Info(String),
    /// An error line: ` iamBitchX: text`.
    Error(String),
    /// An event we could not decrypt: ` [unable to decrypt: reason]`.
    Undecryptable { nick: String, reason: String },
    /// One SAS emoji: ` SAS n/7  🐶  Dog`.
    Sas { n: usize, emoji: String, name: String },
    /// A raw line without prefix (startup header), default colours.
    Raw(String),
    /// One row of the startup logo: no prefix, bold blue.
    Logo(String),
    /// One row of the startup logo's shadow: no prefix, dim blue.
    LogoDim(String),
    /// One row of the startup art in the original's own colours: a run of
    /// text per colour change (the BitchX logo shades its 3D with
    /// background colours).
    LogoRuns(Vec<Run>),
    /// NAMES header: ` [Users(#chan:n/m)]`.
    Users { chan: String, shown: usize, total: usize },
    /// One NAMES grid row: `[@nick ] [ nick (zZzZ 4h) ]`, cells padded to `width`.
    Names {
        cells: Vec<(char, String, Option<PresenceMark>)>,
        width: usize,
    },
    /// The WHOIS ruler `------------- --  -`.
    WhoisRule,
    /// WHOIS first row: `| nick (mxid) (info)`.
    WhoisNick { nick: String, mxid: String, info: String },
    /// WHOIS first row of an away/offline peer, with the presence marker:
    /// `| nick (mxid) (info) (zZzZ 4h)`.
    WhoisMark {
        nick: String,
        mxid: String,
        info: String,
        mark: PresenceMark,
    },
    /// WHOIS field row n (1-based): `{lead}{key:<8} : value`, the lead
    /// alternating ` `/`|` and fading green to grey, `:` on the last row.
    WhoisRow { row: u8, last: bool, key: String, value: String },
    /// Protocol debug line (`/set debug on`): ` DBG cat text`, magenta tag;
    /// `cat` empty for the flood guard's `DBG (suppressed N)`.
    Debug { cat: String, text: String },
}

/// IRC-style nick for a Matrix user: the mxid localpart (`@carla:x` ->
/// `carla`), never the display name, sanitized for the terminal.
///
/// F28: localparts are only unique per homeserver. When another known user
/// shares the localpart (case-insensitively), the nick is qualified
/// IRC-style as `localpart@server` -- except for ourselves and, when the
/// localpart is not ours, for the user on our own homeserver. So with
/// `@bob:own` and `@bob:evil.org` in sight they read `bob` and
/// `bob@evil.org`, and a remote `@me:evil.org` can never show up as `me`.
/// Case twins on the same server (`@Bob:x`, `@bob:x`) stay bare: `@server`
/// cannot tell them apart, their case does (and nick commands refuse them
/// as ambiguous).
pub(crate) fn nick_of(user: &UserId) -> String {
    nicks::nick(user)
}

/// Remember `me` as one of our own accounts (preferred bare nick, own
/// homeserver).
pub(crate) fn register_me(me: &UserId) {
    nicks::with(|r| {
        r.mine.insert(me.to_owned());
        r.see(me);
    });
}

/// All users seen with this (case-insensitive) localpart.
pub(crate) fn same_localpart(localpart: &str) -> Vec<OwnedUserId> {
    nicks::with(|r| r.by_local.get(&localpart.to_lowercase()).cloned().unwrap_or_default())
        .into_iter()
        .collect()
}

mod nicks {
    use std::collections::{BTreeSet, HashMap, HashSet};

    use matrix_sdk::ruma::{OwnedUserId, UserId};

    /// Every user ID seen (members, senders), by lowercased localpart.
    #[derive(Default)]
    pub(super) struct Registry {
        pub mine: HashSet<OwnedUserId>,
        pub by_local: HashMap<String, BTreeSet<OwnedUserId>>,
    }

    impl Registry {
        pub(super) fn see(&mut self, user: &UserId) {
            self.by_local
                .entry(user.localpart().to_lowercase())
                .or_default()
                .insert(user.to_owned());
        }

        /// The bare localpart is only shown when it cannot be confused.
        fn bare(&self, user: &UserId) -> bool {
            let key = user.localpart().to_lowercase();
            let others = self
                .by_local
                .get(&key)
                .is_some_and(|s| s.iter().any(|u| u.server_name() != user.server_name()));
            if !others || self.mine.contains(user) {
                return true;
            }
            // Someone else has this name. Ours wins; else the user on our
            // own homeserver (unique there) keeps it.
            let ours = |u: &OwnedUserId| self.mine.contains(u);
            let taken_by_me = self.by_local[&key].iter().any(ours);
            !taken_by_me && self.mine.iter().any(|m| m.server_name() == user.server_name())
        }
    }

    // Tests run one thread per test: a thread-local registry keeps them
    // independent. The real client renders on one thread and converts
    // events on others, so it shares one registry.
    #[cfg(test)]
    thread_local! {
        static REG: std::cell::RefCell<Registry> = std::cell::RefCell::new(Registry::default());
    }

    #[cfg(test)]
    pub(super) fn with<T>(f: impl FnOnce(&mut Registry) -> T) -> T {
        REG.with(|r| f(&mut r.borrow_mut()))
    }

    #[cfg(not(test))]
    pub(super) fn with<T>(f: impl FnOnce(&mut Registry) -> T) -> T {
        static REG: std::sync::LazyLock<std::sync::Mutex<Registry>> =
            std::sync::LazyLock::new(Default::default);
        let mut r = REG.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut r)
    }

    pub(super) fn nick(user: &UserId) -> String {
        let bare = with(|r| {
            r.see(user);
            r.bare(user)
        });
        let local = crate::bitchx::text::one_line(user.localpart());
        if bare {
            local
        } else {
            format!("{local}@{}", user.server_name())
        }
    }
}

/// Matrix presence state of another user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PresenceState {
    Online,
    Unavailable,
    Offline,
}

/// The last known presence of another user. `last_active_ago` was measured
/// by the homeserver when the event arrived at `received_at`, so the age
/// ticks forward locally: `age = last_active_ago + (now - received_at)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Presence {
    pub state: PresenceState,
    pub status_msg: Option<String>,
    pub last_active_ago: Option<Duration>,
    pub received_at: Instant,
    pub currently_active: bool,
}

/// The compact marker next to a peer: `zZzZ 4h` (unavailable) or `off 2d`
/// (offline); the age is missing when the homeserver sent none.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PresenceMark {
    pub offline: bool,
    pub age: Option<String>,
}

impl PresenceMark {
    /// The marker word: `zZzZ` (BitchX `STATUS_AWAY`) or `off`.
    pub(crate) fn word(&self) -> &'static str {
        if self.offline { "off" } else { "zZzZ" }
    }

    /// `zZzZ 4h`, `off 2d`, `zZzZ`.
    pub(crate) fn text(&self) -> String {
        match &self.age {
            Some(age) => format!("{} {age}", self.word()),
            None => self.word().to_string(),
        }
    }
}

impl Presence {
    /// The away message while `unavailable` (BitchX `away : nick - msg`).
    pub(crate) fn away(&self) -> Option<String> {
        (self.state == PresenceState::Unavailable).then(|| {
            let msg = self.status_msg.as_deref().unwrap_or("").trim();
            if msg.is_empty() {
                "away".to_string()
            } else {
                msg.to_string()
            }
        })
    }

    /// Time since the user was last active, as of `now`; `None` when the
    /// homeserver sent no `last_active_ago`.
    pub(crate) fn age(&self, now: Instant) -> Option<Duration> {
        let since = now.saturating_duration_since(self.received_at);
        self.last_active_ago.map(|ago| ago + since)
    }

    /// The status bar / NAMES marker; nothing for online or currently active.
    pub(crate) fn mark(&self, now: Instant) -> Option<PresenceMark> {
        if self.currently_active || self.state == PresenceState::Online {
            return None;
        }
        Some(PresenceMark {
            offline: self.state == PresenceState::Offline,
            age: self.age(now).map(compact_age),
        })
    }
}

/// Compact age, largest unit only, rounded down: `45s 12m 4h 2d 3w`.
pub(crate) fn compact_age(age: Duration) -> String {
    let s = age.as_secs();
    match s {
        0..60 => format!("{s}s"),
        60..3_600 => format!("{}m", s / 60),
        3_600..86_400 => format!("{}h", s / 3_600),
        86_400..604_800 => format!("{}d", s / 86_400),
        _ => format!("{}w", s / 604_800),
    }
}

/// `WHOIS_IDLE` `: idle     : $0 hours $1 mins $2 secs` (`include/color.h:731`,
/// split as in `source/numbers.c:870-877`). Matrix has no signon time, so
/// the `(signon: ...)` part is left out.
pub(crate) fn whois_idle(idle: Duration) -> String {
    let secs = idle.as_secs();
    let hours = secs / 3600;
    let minutes = (secs - hours * 3600) / 60;
    let seconds = secs % 60;
    format!("{hours} hours {minutes} mins {seconds} secs")
}

/// Matrix presence as the UI keeps it. `unavailable` is away (BitchX shows
/// `away : nick - msg`, the message defaulting to `away`); unknown states
/// count as offline. The status message is sanitized for the terminal.
pub(crate) fn away_of(
    presence: &str,
    status_msg: Option<&str>,
    last_active_ago: Option<Duration>,
    currently_active: Option<bool>,
    received_at: Instant,
) -> Presence {
    let state = match presence {
        "online" => PresenceState::Online,
        "unavailable" => PresenceState::Unavailable,
        _ => PresenceState::Offline,
    };
    Presence {
        state,
        status_msg: status_msg.map(crate::bitchx::format::sanitize),
        last_active_ago,
        received_at,
        currently_active: currently_active.unwrap_or(false),
    }
}

/// A line with the event id it came from (for dedup) and the event's
/// `origin_server_ts` in unix millis (`None`: client-generated, use now).
pub(crate) type Stamped = (Option<String>, Option<i64>, Line);

/// A room member as the UI needs it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Member {
    pub user_id: OwnedUserId,
    pub nick: String,
    pub power: i64,
}

impl Member {
    /// A member whose nick is the mxid localpart. The display name (which may
    /// be capitalised, e.g. `Carla`) is deliberately NOT used; BitchX shows it
    /// only as the ircname in /whois.
    pub(crate) fn new(user_id: OwnedUserId, power: i64) -> Self {
        Member { nick: nick_of(&user_id), user_id, power }
    }

    /// `@` in IRC terms: power level 50 or more.
    pub(crate) fn is_op(&self) -> bool {
        self.power >= 50
    }

    /// `+` in IRC terms: above the default, below op.
    pub(crate) fn is_voice(&self) -> bool {
        self.power > 0 && self.power < 50
    }
}

/// Everything the UI shows about a room.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RoomSnapshot {
    pub room_id: Option<OwnedRoomId>,
    /// `#alias`, or the room name, or the DM peer's nick.
    pub name: String,
    /// The other user, when this room is a direct message.
    pub dm_peer: Option<OwnedUserId>,
    pub topic: Option<String>,
    pub encrypted: bool,
    pub invite_only: bool,
    pub members: Vec<Member>,
}

/// Something that happened on the Matrix side.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MxEvent {
    /// A joined room appeared or changed.
    Room(RoomSnapshot),
    /// New lines for a room. `backfill` lines are older than everything shown.
    Lines {
        room_id: OwnedRoomId,
        lines: Vec<Stamped>,
        backfill: bool,
    },
    /// We left (or were removed from) a room.
    Left(OwnedRoomId),
    /// Switch to this room's window (after /join, /query).
    Focus(OwnedRoomId),
    /// Our /join of this room completed in `millis`: focus it, print NAMES
    /// and ` iamBitchX: Join to #chan was synched in X secs!!`.
    Synched { room_id: OwnedRoomId, millis: u64 },
    /// A `-:-` line for a room window, or the status window.
    Info { room_id: Option<OwnedRoomId>, line: Line },
    /// Our own display name changed (`/nick`); the nick stays the localpart.
    OwnDisplayName(String),
    /// Away state changed.
    Away(Option<String>),
    /// Presence of another user, stamped with when it arrived.
    Presence { user: OwnedUserId, presence: Presence },
    /// Round-trip time to the homeserver, in seconds.
    Lag(u64),
    /// A key verification with `user` progressed.
    Verify { user: OwnedUserId, update: VerifyUpdate },
    /// The public channel directory of `server` (`/list`).
    List {
        server: String,
        search: Option<String>,
        entries: Vec<ListEntry>,
    },
    /// A protocol debug line from [`crate::debug_tap`] (`/set debug on`).
    Debug(crate::debug_tap::DbgLine),
}

/// Something the UI wants the Matrix side to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Effect {
    Send {
        room_id: OwnedRoomId,
        text: String,
        emote: bool,
    },
    Join(JoinTarget),
    /// `/list`: fetch the public channel directory.
    List(ListArgs),
    Part {
        room_id: OwnedRoomId,
        reason: Option<String>,
    },
    Topic {
        room_id: OwnedRoomId,
        topic: String,
    },
    Nick(String),
    Msg {
        user: OwnedUserId,
        text: String,
    },
    Query(OwnedUserId),
    Invite {
        room_id: OwnedRoomId,
        user: OwnedUserId,
    },
    Kick {
        room_id: OwnedRoomId,
        user: OwnedUserId,
        reason: Option<String>,
    },
    Power {
        room_id: OwnedRoomId,
        user: OwnedUserId,
        level: i64,
    },
    Away(Option<String>),
    Whois(OwnedUserId),
    Backfill(OwnedRoomId),
    /// Enable megolm in a room (after the user confirmed: irreversible).
    EnableEncryption(OwnedRoomId),
    /// Drive a SAS verification with `user`. `flow_id` is the pending
    /// request, if any.
    Verify {
        user: OwnedUserId,
        flow_id: Option<String>,
        action: VerifyAction,
    },
    /// List a user's devices with verified flags.
    Devices(OwnedUserId),
    /// Show our own cross-signing status.
    Trust,
    /// Import secrets from secure backup; bootstrap cross-signing if missing.
    Recover(Secret),
    Quit,
}

/// What `/verify ...` asks the Matrix side to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VerifyAction {
    Accept,
    Start { device: Option<String> },
    Confirm,
    Mismatch,
    Cancel,
}

/// E3: the device named in an incoming request, as resolved by the SDK.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ReqDevice {
    /// In the SDK's device list for the user: its id and ed25519 key.
    Known { id: String, ed25519: Option<String> },
    /// The sender-chosen id is not in the user's device list: display only,
    /// never trusted or matched.
    Unknown(String),
}

/// Progress of a key verification, as reported by the Matrix side.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VerifyUpdate {
    /// A request exists (theirs or ours). In-room requests carry the room.
    Requested {
        flow_id: String,
        device: Option<ReqDevice>,
        room_id: Option<OwnedRoomId>,
        we_started: bool,
        /// E4: the whole cross-signing identity is being verified (trusts
        /// every device of the user), not one device.
        identity: bool,
    },
    /// Both sides are ready; SAS can start.
    Ready,
    /// E2: the peer started emoji (SAS); it waits for `/verify accept`.
    SasOffered,
    /// The short auth string: 7 (emoji, name) pairs.
    Emoji(Vec<(String, String)>),
    /// We confirmed; waiting for the other side.
    Confirmed,
    Done,
    Cancelled(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(state: &str, ago: Option<u64>, active: Option<bool>, at: Instant) -> Presence {
        away_of(state, None, ago.map(Duration::from_secs), active, at)
    }

    #[test]
    fn test_away_of_presence() {
        let t = Instant::now();
        let away = |s, m| away_of(s, m, None, None, t).away();
        assert_eq!(away("unavailable", Some("lunch")), Some("lunch".into()));
        assert_eq!(away("unavailable", Some("  ")), Some("away".into()));
        assert_eq!(away("unavailable", None), Some("away".into()));
        assert_eq!(away("unavailable", Some("a\x1b[31mb")), Some("a [31mb".into()));
        assert_eq!(away("online", Some("hi")), None);
        assert_eq!(away("offline", None), None);
        assert_eq!(away_of("weird", None, None, None, t).state, PresenceState::Offline);
    }

    #[test]
    fn test_compact_age_boundaries() {
        let a = |s| compact_age(Duration::from_secs(s));
        assert_eq!(a(0), "0s");
        assert_eq!(a(45), "45s");
        assert_eq!(a(59), "59s");
        assert_eq!(a(60), "1m");
        assert_eq!(a(12 * 60 + 59), "12m");
        assert_eq!(a(3_599), "59m");
        assert_eq!(a(3_600), "1h");
        assert_eq!(a(4 * 3_600 + 15 * 60), "4h");
        assert_eq!(a(86_399), "23h");
        assert_eq!(a(86_400), "1d");
        assert_eq!(a(2 * 86_400 + 5), "2d");
        assert_eq!(a(604_799), "6d");
        assert_eq!(a(604_800), "1w");
        assert_eq!(a(3 * 604_800 + 86_400), "3w");
        assert_eq!(compact_age(Duration::from_millis(999)), "0s");
    }

    #[test]
    fn test_presence_age_ticks_with_fake_clock() {
        let t0 = Instant::now();
        let pr = p("unavailable", Some(30), None, t0);
        let at = |s| pr.mark(t0 + Duration::from_secs(s)).unwrap().text();
        assert_eq!(at(0), "zZzZ 30s");
        assert_eq!(at(15), "zZzZ 45s");
        assert_eq!(at(30), "zZzZ 1m");
        assert_eq!(at(4 * 3_600), "zZzZ 4h");
        // a clock before the receive time never goes negative
        assert_eq!(pr.age(t0), Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_presence_mark_states() {
        let t = Instant::now();
        let h4 = Some(4 * 3_600);
        assert_eq!(p("unavailable", h4, None, t).mark(t).unwrap().text(), "zZzZ 4h");
        assert_eq!(p("offline", Some(2 * 86_400), None, t).mark(t).unwrap().text(), "off 2d");
        assert_eq!(p("unavailable", None, None, t).mark(t).unwrap().text(), "zZzZ");
        assert_eq!(p("offline", None, None, t).mark(t).unwrap().text(), "off");
        assert_eq!(p("online", h4, None, t).mark(t), None);
        assert_eq!(p("online", None, Some(true), t).mark(t), None);
        assert_eq!(p("unavailable", h4, Some(true), t).mark(t), None);
        assert!(p("offline", None, None, t).mark(t).unwrap().offline);
    }

    #[test]
    fn test_whois_idle_format() {
        let i = |s| whois_idle(Duration::from_secs(s));
        assert_eq!(i(0), "0 hours 0 mins 0 secs");
        assert_eq!(i(4 * 3_600 + 15 * 60 + 7), "4 hours 15 mins 7 secs");
        assert_eq!(i(50 * 3_600 + 59), "50 hours 0 mins 59 secs");
    }
}
