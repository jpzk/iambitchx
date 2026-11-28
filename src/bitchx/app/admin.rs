//! Channel creation and admin: `/create`, the create prompt after a `/join`
//! 404 on our own server, `/alias`, `/publish`, `/ban` `/unban` `/kickban`,
//! `/power` and `/mode` (IRC modes mapped by [`crate::bitchx::modes`]).

use matrix_sdk::ruma::OwnedRoomId;

use crate::bitchx::app::{App, Confirm};
use crate::bitchx::model::Effect;
use crate::bitchx::modes::{MatrixChange, ModeChange, RoomModes, SUPPORTED, parse_modes};

impl App {
    /// The localpart of `#x` or `#x:<our server>`; any other server is refused:
    /// we can only create aliases on our own homeserver.
    pub(super) fn own_alias(&mut self, room: &str) -> Option<String> {
        let own = self.me.server_name().to_string();
        let bare = room.strip_prefix('#').unwrap_or(room);
        let (local, server) = match bare.split_once(':') {
            Some((l, s)) => (l, Some(s)),
            None => (bare, None),
        };
        if let Some(s) = server.filter(|s| !s.eq_ignore_ascii_case(&own)) {
            self.error(format!(
                "Cannot create #{local}:{s}: channels can only be created on your own server ({own})"
            ));
            return None;
        }
        if local.is_empty() || local.chars().any(|c| c.is_whitespace() || c.is_control()) {
            self.error(format!("Bad channel name: {room}"));
            return None;
        }
        Some(local.to_string())
    }

    /// `/create #x [-public] [-noencrypt]`.
    pub(super) fn create(&mut self, room: &str, public: bool, encrypt: bool) -> Vec<Effect> {
        let Some(alias) = self.own_alias(room) else {
            return vec![];
        };
        let own = self.me.server_name().to_string();
        if public && encrypt {
            self.info(
                "Public + encrypted is fine, but history is unreadable to anyone who joins later \
                 (use -noencrypt for a readable public channel)",
            );
        }
        let what = match (public, encrypt) {
            (true, true) => "public, encrypted",
            (true, false) => "public, NOT encrypted",
            (false, true) => "invite-only, encrypted",
            (false, false) => "invite-only, NOT encrypted",
        };
        self.info(format!("Creating channel #{alias}:{own} ({what})..."));
        vec![Effect::CreateRoom { alias, public, encrypt }]
    }

    /// A `/join` of `#x:<our server>` got 404: offer to create it.
    pub(super) fn offer_create(&mut self, alias: &str) {
        let Some(local) = self.own_alias(alias) else {
            return;
        };
        let own = self.me.server_name().to_string();
        self.info(format!(
            "Channel #{local}:{own} does not exist. Create it (invite-only, encrypted)? [y/n]"
        ));
        self.confirm = Some(Confirm::Create(local));
    }

    /// `/alias add|del #x[:server]` (bare `#x` means our server).
    pub(super) fn alias(&mut self, add: bool, alias: &str) -> Vec<Effect> {
        let full = if alias.contains(':') {
            alias.to_string()
        } else {
            format!("#{}:{}", alias.trim_start_matches('#'), self.me.server_name())
        };
        if !full.starts_with('#') {
            self.error(format!("Bad alias {alias}: must start with #"));
            return vec![];
        }
        if !add {
            return vec![Effect::AliasDel(full)];
        }
        self.current_room()
            .map(|room_id| Effect::AliasAdd { room_id, alias: full })
            .into_iter()
            .collect()
    }

    pub(super) fn publish(&mut self, on: bool) -> Vec<Effect> {
        self.current_room()
            .map(|room_id| Effect::Publish { room_id, on })
            .into_iter()
            .collect()
    }

    pub(super) fn ban(&mut self, target: &str, reason: Option<String>) -> Vec<Effect> {
        let Some(room_id) = self.current_room() else {
            return vec![];
        };
        self.user_cmd(target)
            .map(|user| Effect::Ban { room_id, user, reason })
            .into_iter()
            .collect()
    }

    pub(super) fn unban(&mut self, target: &str) -> Vec<Effect> {
        let Some(room_id) = self.current_room() else {
            return vec![];
        };
        self.user_cmd(target)
            .map(|user| Effect::Unban { room_id, user })
            .into_iter()
            .collect()
    }

    /// Our own power level in the current window (0 if unknown).
    pub(super) fn own_power(&self) -> i64 {
        self.cur()
            .room
            .members
            .iter()
            .find(|m| m.user_id == self.me)
            .map_or(0, |m| m.power)
    }

    /// `/power nick <level>`: refused above our own level (the server would
    /// refuse it too, but this says why).
    pub(super) fn raw_power(&mut self, target: &str, level: i64) -> Vec<Effect> {
        let own = self.own_power();
        if level > own {
            self.error(format!(
                "Cannot set power level {level} for {target}: above your own ({own})"
            ));
            return vec![];
        }
        self.power(target, level)
    }

    /// `/mode [#chan] [modes]`: bare prints the room's modes, otherwise each
    /// change becomes one Matrix state change.
    pub(super) fn mode(&mut self, target: Option<String>, modes: Option<String>) -> Vec<Effect> {
        let Some(idx) = self.target_window(target) else {
            return vec![];
        };
        let Some(room_id) = self.windows[idx].room_id().cloned() else {
            return vec![];
        };
        let room = &self.windows[idx].room;
        let chan = self.windows[idx].channel().unwrap_or(&room.name).to_string();
        let modes = modes.filter(|m| !m.is_empty());
        let Some(modes) = modes else {
            let m = RoomModes {
                invite_only: room.invite_only,
                moderated: room.moderated,
                topic_lock: room.topic_lock,
            };
            let e = if room.encrypted { " (encrypted)" } else { "" };
            self.info(format!("Mode for {chan}: {}{e}", m.irc()));
            return vec![];
        };
        let changes = match parse_modes(&modes) {
            Ok(c) if !c.is_empty() => c,
            Ok(_) => {
                self.error(format!("No mode change in {modes}; supported: {SUPPORTED}"));
                return vec![];
            },
            Err(e) => {
                self.error(e);
                return vec![];
            },
        };
        let what: Vec<String> = changes.iter().map(ModeChange::irc).collect();
        let mut fx = vec![];
        for change in changes {
            match self.mode_effect(&room_id, change.to_matrix()) {
                Some(e) => fx.push(e),
                // nothing half-applied: one bad nick drops the whole line
                None => return vec![],
            }
        }
        self.info(format!("Setting mode [{}] on {chan}", what.join(" ")));
        fx
    }

    fn mode_effect(&mut self, room_id: &OwnedRoomId, change: MatrixChange) -> Option<Effect> {
        let room_id = room_id.clone();
        Some(match change {
            MatrixChange::JoinRule { invite } => Effect::JoinRule { room_id, invite },
            MatrixChange::EventsDefault(level) => Effect::EventsDefault { room_id, level },
            MatrixChange::TopicLevel(level) => Effect::TopicLevel { room_id, level },
            MatrixChange::UserPower { nick, level } => {
                Effect::Power { room_id, user: self.user_cmd(&nick)?, level }
            },
            MatrixChange::Ban { nick } => {
                Effect::Ban { room_id, user: self.user_cmd(&nick)?, reason: None }
            },
            MatrixChange::Unban { nick } => Effect::Unban { room_id, user: self.user_cmd(&nick)? },
        })
    }
}
