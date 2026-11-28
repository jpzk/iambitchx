//! Per-window and per-session state types: numbered [`Window`]s, pending
//! y/n confirmations and key verifications, and the self-whois probe.

use std::collections::HashSet;

use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};

use crate::bitchx::media::{MEDIA_KEEP, MediaRef};
use crate::bitchx::model::{Line, RoomSnapshot};

pub(super) const MAX_LINES: usize = 2000;

/// Now, in unix millis.
pub(crate) fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// One numbered window: the status window (no room) or a room.
#[derive(Clone, Debug, Default)]
pub(crate) struct Window {
    pub room: RoomSnapshot,
    pub lines: Vec<Line>,
    /// When each of `lines` happened (unix millis): the event's
    /// `origin_server_ts`, or the time a client line was printed.
    pub times: Vec<i64>,
    pub(super) ids: HashSet<String>,
    pub unread: bool,
    /// Unread mention (BitchX `AUTO_REPLY)`: shown apart in `Act:`.
    pub hilite: bool,
    /// Rows scrolled up from the bottom (PageUp/PageDown).
    pub scroll: usize,
    pub backfill_requested: bool,
    /// F29: the room name cannot be trusted as a channel name (it clashes
    /// with another window, or poses as an alias on our own server, which
    /// real aliases never show), or (M-03) is a room name, not an alias, so
    /// it reads like one of our own aliases: the room ID is shown instead.
    pub spoof: bool,
    /// Lines pushed since the last [`super::App::flush_log`], for the log
    /// file: (time, line). Only [`crate::bitchx::logfile::loggable`] lines.
    pub(super) log_queue: Vec<(i64, Line)>,
    /// Media messages seen in this window, oldest first, for `/get`.
    pub media: Vec<MediaRef>,
    /// The last media number handed out (`[m.image #n]`).
    pub(super) media_seq: usize,
}

impl Window {
    pub(crate) fn room_id(&self) -> Option<&OwnedRoomId> {
        self.room.room_id.as_ref()
    }

    /// The `#channel` shown in the prompt and status line.
    pub(crate) fn channel(&self) -> Option<&str> {
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
    pub(super) fn query_line(&self, line: Line) -> Line {
        match (&self.room.dm_peer, line) {
            (Some(peer), Line::Public { nick, own, text }) => {
                let nick = if own { self.room.name.clone() } else { nick };
                Line::Private { nick, mxid: peer.to_string(), own, text }
            },
            (_, line) => line,
        }
    }

    /// A media line gets the next number of this window and becomes a
    /// `<nick> [m.image #n] name` line; its reference goes to `out` (for
    /// [`Window::add_media`]). An event already shown is not renumbered.
    /// Other lines pass through.
    pub(super) fn number_media(
        &mut self,
        id: Option<&str>,
        line: Line,
        out: &mut Vec<MediaRef>,
    ) -> Line {
        let Line::Media { nick, own, media } = line else {
            return line;
        };
        if id.is_some_and(|id| self.ids.contains(id)) {
            let text = format!("[{}] {}", media.kind, media.name);
            return Line::Public { nick, own, text };
        }
        self.media_seq += 1;
        let n = self.media_seq;
        let text = media.text(n);
        out.push(MediaRef { n, media });
        Line::Public { nick, own, text }
    }

    /// Remember numbered media: in front for backfill (older history),
    /// else at the end. At most [`MEDIA_KEEP`] are kept.
    pub(super) fn add_media(&mut self, mut refs: Vec<MediaRef>, older: bool) {
        if older {
            refs.append(&mut self.media);
            self.media = refs;
        } else {
            self.media.append(&mut refs);
        }
        if self.media.len() > MEDIA_KEEP {
            let n = self.media.len() - MEDIA_KEEP;
            self.media.drain(..n);
        }
    }

    /// Append a client-generated line, stamped now.
    pub(super) fn push(&mut self, id: Option<String>, line: Line) -> bool {
        self.push_at(id, now_ms(), line)
    }

    /// Append a line stamped `ts` (unix millis).
    pub(super) fn push_at(&mut self, id: Option<String>, ts: i64, line: Line) -> bool {
        if let Some(id) = id
            && !self.ids.insert(id)
        {
            return false;
        }
        if crate::bitchx::logfile::loggable(&line) {
            if self.log_queue.len() >= MAX_LINES {
                self.log_queue.remove(0);
            }
            self.log_queue.push((ts, line.clone()));
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

    pub(super) fn prepend(&mut self, lines: Vec<(Option<String>, i64, Line)>) {
        let mut fresh = vec![];
        let mut times = vec![];
        for (id, ts, line) in lines {
            if let Some(id) = id
                && !self.ids.insert(id)
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
    pub(super) fn clear(&mut self) {
        self.lines.clear();
        self.times.clear();
    }

    /// Members this nick may mean: the displayed (maybe qualified) nick or
    /// the bare localpart, case-insensitively.
    pub(super) fn members_by_nick(&self, nick: &str) -> Vec<OwnedUserId> {
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
pub(crate) enum Confirm {
    /// `/encrypt` in this room.
    Encrypt(OwnedRoomId),
    /// Create `#localpart` on our server after a `/join` 404.
    Create(String),
}

/// A key verification in progress with another user (or own device).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PendingVerify {
    pub user: OwnedUserId,
    pub flow_id: String,
    pub device: Option<String>,
    pub room_id: Option<OwnedRoomId>,
    pub we_started: bool,
    /// E4: what the emoji verify, shown in the SAS and done lines.
    pub subject: String,
    /// requested / ready / emoji / confirmed.
    pub state: &'static str,
    /// Several flows with this user reached SAS: the emoji shown cannot be
    /// tied to this flow, so `/verify confirm` refuses it (F44).
    pub ambiguous: bool,
}

/// DBG flood guard: at most this many DBG lines per second per window.
pub(crate) const DBG_PER_SEC: u32 = 20;

/// The connect-time self-whois: its lines are swallowed, the `ircname` row
/// becomes [`App::display_name`](crate::bitchx::app::App::display_name).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Probe {
    /// Not sent yet (sent on the first lag measurement, i.e. once online).
    #[default]
    Start,
    /// Sent; `rule` = the leading `WhoisRule` was swallowed.
    Pending {
        rule: bool,
    },
    /// Inside our own whois block.
    Block,
    Done,
}
