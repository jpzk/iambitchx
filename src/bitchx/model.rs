//! # Plain data shared by the Matrix side and the UI
//!
//! Nothing here touches the SDK or the terminal, so the whole UI can be
//! driven by tests.
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};

/// One scrollback line, already translated from Matrix into IRC terms.
/// The channel name is filled in by the formatter from the window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Line {
    /// PUBLIC / SEND_PUBLIC: `<nick> text`.
    Public { nick: String, own: bool, text: String },
    /// ACTION / SEND_ACTION: `* nick text`.
    Action { nick: String, own: bool, text: String },
    /// PUBLIC_NOTICE: `-nick:#chan- text`.
    Notice { nick: String, text: String },
    /// JOIN: `-:- nick [mxid] has joined #chan`.
    Join { nick: String, mxid: String },
    /// LEAVE: `-:- nick [mxid] has left #chan [reason]`.
    Part { nick: String, mxid: String, reason: Option<String> },
    /// KICK: `-:- victim was kicked off #chan by kicker (reason)`.
    Kick {
        victim: String,
        kicker: String,
        reason: Option<String>,
    },
    /// NICKNAME: `-:- old is now known as new`.
    Nick { old: String, new: String },
    /// TOPIC / TOPIC_CHANGE: `-:- Topic (#chan): [changed by nick:] topic`.
    Topic { nick: Option<String>, topic: String },
    /// MODE: `-:- mode/#chan [modes] by nick`.
    Mode { modes: String, nick: String },
    /// INVITE: `-:- nick invited target to #chan`.
    Invite { nick: String, target: String },
    /// Any other `-:- text` line (server/login info, command output).
    Info(String),
    /// A `-:- text` error line.
    Error(String),
    /// A raw line without prefix (banner).
    Raw(String),
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
    /// A `-:-` line for a room window, or the status window.
    Info { room_id: Option<OwnedRoomId>, line: Line },
    /// Our own nick changed.
    OwnNick(String),
    /// Away state changed.
    Away(Option<String>),
    /// Round-trip time to the homeserver, in seconds.
    Lag(u64),
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
    Quit,
}
