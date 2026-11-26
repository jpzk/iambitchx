//! # Plain data shared by the Matrix side and the UI
//!
//! Nothing here touches the SDK or the terminal, so the whole UI can be
//! driven by tests.
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};

use crate::bitchx::channels::{JoinTarget, ListArgs, ListEntry};
pub use crate::bitchx::commands::Secret;

/// One scrollback line, already translated from Matrix into IRC terms.
/// The channel name is filled in by the formatter from the window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Line {
    /// PUBLIC / SEND_PUBLIC: `<nick> text`.
    Public { nick: String, own: bool, text: String },
    /// ACTION / SEND_ACTION: ` nick text` / ` nick/#chan text`.
    Action { nick: String, own: bool, text: String },
    /// Query (DM) message. SEND_MSG `[msg(nick)] text` when `own` (`nick`
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
    /// TOPIC / TOPIC_CHANGE: ` Topic (#chan): [changed by nick:] topic`.
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
    /// NAMES header: ` [Users(#chan:n/m)]`.
    Users { chan: String, shown: usize, total: usize },
    /// One NAMES grid row: `[@nick ] [ nick ]`, nicks padded to `width`.
    Names { cells: Vec<(char, String)>, width: usize },
    /// The WHOIS ruler `------------- --  -`.
    WhoisRule,
    /// WHOIS first row: `| nick (mxid) (info)`.
    WhoisNick { nick: String, mxid: String, info: String },
    /// WHOIS field row n (1-based): `{lead}{key:<8} : value`, the lead
    /// alternating ` `/`|` and fading green to grey, `:` on the last row.
    WhoisRow { row: u8, last: bool, key: String, value: String },
}

/// IRC-style nick for a Matrix user: the mxid localpart (`@carla:x` ->
/// `carla`), never the display name, sanitized for the terminal.
pub fn nick_of(user: &UserId) -> String {
    crate::bitchx::format::sanitize(user.localpart())
}

/// Matrix presence as IRC away: `unavailable` is away (BitchX shows
/// `away : nick - msg`, the message defaulting to `away`); `online`,
/// `offline` and unknown states are not.
pub fn away_of(presence: &str, status_msg: Option<&str>) -> Option<String> {
    (presence == "unavailable").then(|| {
        let msg = crate::bitchx::format::sanitize(status_msg.unwrap_or("").trim());
        if msg.is_empty() {
            "away".to_string()
        } else {
            msg
        }
    })
}

/// A line with the event id it came from (for dedup) and the event's
/// `origin_server_ts` in unix millis (`None`: client-generated, use now).
pub type Stamped = (Option<String>, Option<i64>, Line);

/// A room member as the UI needs it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Member {
    pub user_id: OwnedUserId,
    pub nick: String,
    pub power: i64,
}

impl Member {
    /// A member whose nick is the mxid localpart. The display name (which may
    /// be capitalised, e.g. `Carla`) is deliberately NOT used; BitchX shows it
    /// only as the ircname in /whois.
    pub fn new(user_id: OwnedUserId, power: i64) -> Self {
        Member { nick: nick_of(&user_id), user_id, power }
    }

    /// `@` in IRC terms: power level 50 or more.
    pub fn is_op(&self) -> bool {
        self.power >= 50
    }

    /// `+` in IRC terms: above the default, below op.
    pub fn is_voice(&self) -> bool {
        self.power > 0 && self.power < 50
    }
}

/// Everything the UI shows about a room.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RoomSnapshot {
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
pub enum MxEvent {
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
    /// Presence of another user: `Some(status_msg)` while `unavailable`.
    Presence { user: OwnedUserId, away: Option<String> },
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
}

/// Something the UI wants the Matrix side to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
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
pub enum VerifyAction {
    Accept,
    Start { device: Option<String> },
    Confirm,
    Mismatch,
    Cancel,
}

/// Progress of a key verification, as reported by the Matrix side.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerifyUpdate {
    /// A request exists (theirs or ours). In-room requests carry the room.
    Requested {
        flow_id: String,
        device: Option<String>,
        room_id: Option<OwnedRoomId>,
        we_started: bool,
    },
    /// Both sides are ready; SAS can start.
    Ready,
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

    #[test]
    fn test_away_of_presence() {
        assert_eq!(away_of("unavailable", Some("lunch")), Some("lunch".into()));
        assert_eq!(away_of("unavailable", Some("  ")), Some("away".into()));
        assert_eq!(away_of("unavailable", None), Some("away".into()));
        assert_eq!(away_of("unavailable", Some("a\x1b[31mb")), Some("a [31mb".into()));
        assert_eq!(away_of("online", Some("hi")), None);
        assert_eq!(away_of("offline", None), None);
    }
}
