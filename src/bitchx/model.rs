//! # Plain data shared by the Matrix side and the UI
//!
//! Nothing here touches the SDK or the terminal, so the whole UI can be
//! driven by tests.
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};

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
    /// NICKNAME: ` old is now known as new`.
    Nick { old: String, new: String },
    /// TOPIC / TOPIC_CHANGE: ` Topic (#chan): [changed by nick:] topic`.
    Topic { nick: Option<String>, topic: String },
    /// MODE: ` mode/#chan [modes] by nick`.
    Mode { modes: String, nick: String },
    /// INVITE: ` nick invited target to #chan`.
    Invite { nick: String, target: String },
    /// Any other ` text` line (server/login info, command output).
    Info(String),
    /// An error line: ` BitchX: text`.
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

/// A room member as the UI needs it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Member {
    pub user_id: OwnedUserId,
    pub nick: String,
    pub power: i64,
}

impl Member {
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
        lines: Vec<(Option<String>, Line)>,
        backfill: bool,
    },
    /// We left (or were removed from) a room.
    Left(OwnedRoomId),
    /// Switch to this room's window (after /join, /query).
    Focus(OwnedRoomId),
    /// Our /join of this room completed in `millis`: focus it, print NAMES
    /// and ` BitchX: Join to #chan was synched in X secs!!`.
    Synched { room_id: OwnedRoomId, millis: u64 },
    /// A `-:-` line for a room window, or the status window.
    Info { room_id: Option<OwnedRoomId>, line: Line },
    /// Our own nick changed.
    OwnNick(String),
    /// Away state changed.
    Away(Option<String>),
    /// Round-trip time to the homeserver, in seconds.
    Lag(u64),
    /// A key verification with `user` progressed.
    Verify { user: OwnedUserId, update: VerifyUpdate },
}

/// Something the UI wants the Matrix side to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    Send {
        room_id: OwnedRoomId,
        text: String,
        emote: bool,
    },
    Join(String),
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
