//! Matrix-side input: sync events, room snapshots, and the sync loop
//! dying. Everything remote is cleaned before it touches a window.

use matrix_sdk::ruma::OwnedRoomId;

use crate::bitchx::app::{App, Probe, Window, now_ms};
use crate::bitchx::channels::list_lines;
use crate::bitchx::model::{Effect, Line, MxEvent, RoomSnapshot, VerifyUpdate, nick_of};
use crate::bitchx::text::{clean_line, clean_list_entry, clean_snapshot, one_line};

impl App {
    pub(crate) fn handle_mx(&mut self, ev: MxEvent) -> Vec<Effect> {
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
                // Hidden client-side too, in case the server still delivers.
                let lines: Vec<_> =
                    lines.into_iter().filter(|(_, _, l)| !self.from_ignored(l)).collect();
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
                match self.window_of(&room_id) {
                    Some(idx) if !self.windows[idx].room.members.is_empty() => {
                        return self.synched(idx, millis);
                    },
                    // Joined, but the room has not synced its members yet:
                    // no `[Users(#x:0/0)]`, wait for the first full snapshot.
                    _ => {
                        let start = self.now() - i64::try_from(millis).unwrap_or(0);
                        self.pending_joins.insert(room_id, start);
                    },
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
            MxEvent::OfferCreate(alias) => self.offer_create(&alias),
            MxEvent::OwnDisplayName(name) => {
                self.own_name(name.clone());
                // The nick is the localpart and never changes; /nick only
                // sets the Matrix display name.
                let line = Line::DisplayName { nick: self.nick.clone(), name: Some(name) };
                self.windows[0].push(None, line);
            },
            MxEvent::Away(away) => self.away_changed(away),
            MxEvent::Ignored(users) => self.ignored_changed(users),
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
            MxEvent::Verify { user, update } => self.verify_update(&user, update),
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

    pub(super) fn upsert_room(&mut self, mut snap: RoomSnapshot) -> Vec<Effect> {
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
            return self.join_completed(idx, &room_id);
        }

        let refnum = self.windows.len() + 1;
        let what = match &snap.dm_peer {
            Some(peer) => format!("query with {peer}"),
            None => format!("{} users, {room_id}", snap.members.len()),
        };
        let space = if snap.space { " [space]" } else { "" };
        self.status_info(format!("{refnum:>6}) {} [{what}]{space}", snap.name));

        let mut win = Window { room: snap, ..Default::default() };
        if win.room.space {
            let line = format!("[space] {} is a Matrix space (a group of rooms)", win.room.name);
            win.push(None, Line::Info(line));
        }
        if win.room.dm_peer.is_some() {
            let peer = win.room.name.clone();
            win.push(None, Line::Info(format!("Starting conversation with {peer}")));
        }
        if let Some(topic) = win.room.topic.clone() {
            win.push(None, Line::Topic { nick: None, topic });
        }
        self.windows.push(win);
        self.mark_spoofs();
        self.join_completed(self.windows.len() - 1, &room_id)
    }

    /// A pending `/join` whose room now has members: print the synched line
    /// and NAMES (once).
    fn join_completed(&mut self, idx: usize, room_id: &OwnedRoomId) -> Vec<Effect> {
        if self.windows[idx].room.members.is_empty() {
            return vec![];
        }
        let Some(start) = self.pending_joins.remove(room_id) else {
            return vec![];
        };
        let millis = u64::try_from(self.now() - start).unwrap_or(0);
        self.synched(idx, millis)
    }

    /// NAMES plus ` iamBitchX: Join to #chan was synched in X secs!!`, then
    /// switch to the window.
    fn synched(&mut self, idx: usize, millis: u64) -> Vec<Effect> {
        self.names_in(idx);
        let chan = self.windows[idx].room.name.clone();
        #[allow(
            clippy::cast_precision_loss,
            reason = "join latency in ms is far below 2^52; display-only"
        )]
        let secs = millis as f64 / 1000.0;
        let line = Line::Error(format!("Join to {chan} was synched in {secs:.3} secs!!"));
        self.windows[idx].push(None, line);
        self.switch(idx)
    }

    /// F37: the Matrix sync loop broke (`sync: <err>`, it is not restarted).
    /// Status bar turns `[SYNC LOST]` for good and the status window plus the
    /// current one get a -:- line, so a frozen client never looks live.
    pub(super) fn sync_died(&mut self, line: Line) {
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
}

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
        MxEvent::OfferCreate(a) => MxEvent::OfferCreate(one_line(&a)),
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
        // F-04: directory entries (alias, name, topic) are remote text too.
        MxEvent::List { server, search, entries } => MxEvent::List {
            server,
            search,
            entries: entries.into_iter().map(clean_list_entry).collect(),
        },
        ev => ev,
    }
}

/// F37: the line `spawn_sync` sends when the sync loop breaks for good.
fn is_sync_death(line: &Line) -> bool {
    matches!(line, Line::Error(t) if t.starts_with("sync: "))
}
