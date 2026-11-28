//! Unit tests for the BitchX `App`, split by topic; shared fixtures live here.

use super::logo::*;
use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use matrix_sdk::ruma::{owned_room_id, owned_user_id};

use crate::bitchx::channels::{JoinTarget, ListArgs};
use crate::bitchx::model::{Ink, MxEvent, RoomSnapshot, Run, VerifyUpdate, nick_of};

pub(crate) fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub(crate) fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

pub(crate) fn alt(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
}

pub(crate) fn member(id: &str, nick: &str, power: i64) -> Member {
    Member {
        user_id: UserId::parse(id).unwrap(),
        nick: nick.into(),
        power,
    }
}

pub(crate) fn room(id: &str, name: &str) -> RoomSnapshot {
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
pub(crate) fn app() -> App {
    let mut app = App::new_with_logo(owned_user_id!("@carla:example.org"), Logo::None);
    app.clock = Some("09:24pm".into());
    app.handle_mx(MxEvent::Room(room("!a:example.org", "#room")));
    app.handle_mx(MxEvent::Room(room("!b:example.org", "#other")));
    let mut dm = room("!c:example.org", "armin");
    dm.dm_peer = Some(owned_user_id!("@armin:remote.tld"));
    app.handle_mx(MxEvent::Room(dm));
    app
}

pub(crate) fn typed(app: &mut App, s: &str) -> Vec<Effect> {
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
pub(crate) fn presence_ev(user: &OwnedUserId, state: &str, ago: Option<u64>) -> MxEvent {
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

fn rows(app: &mut App) -> Vec<String> {
    let lines = crate::bitchx::ui::scrollback(app, 200, 500);
    crate::bitchx::format::plain(&lines)
}

mod admin;
mod away;
mod commands;
mod ignore;
mod logging;
mod logo;
mod media;
mod presence;
mod security;
mod vars;
mod verify;
mod windows;
