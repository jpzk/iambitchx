//! Tests for channel creation and admin: /create, the /join 404 create
//! prompt, /alias, /publish, /ban /unban /kickban, /power and /mode.

use super::*;

fn a() -> OwnedRoomId {
    owned_room_id!("!a:example.org")
}

/// The app with `#room` (we are carla, power 100) current.
fn in_room() -> App {
    let mut app = app();
    app.handle_key(alt('2'));
    app
}

fn errored(app: &App, what: &str) -> bool {
    matches!(last(app), Line::Error(t) if t.contains(what))
}

#[test]
fn test_create_private_encrypted_by_default() {
    let mut app = in_room();
    let fx = typed(&mut app, "/create #new");
    assert_eq!(fx, vec![Effect::CreateRoom { alias: "new".into(), public: false, encrypt: true }]);
    // explicit own server, any case
    let fx = typed(&mut app, "/create #new:Example.org -noencrypt");
    assert_eq!(fx, vec![Effect::CreateRoom { alias: "new".into(), public: false, encrypt: false }]);
}

#[test]
fn test_create_public_warns_about_history() {
    let mut app = in_room();
    let fx = typed(&mut app, "/create #pub -public");
    assert_eq!(fx, vec![Effect::CreateRoom { alias: "pub".into(), public: true, encrypt: true }]);
    let warned = app
        .cur()
        .lines
        .iter()
        .any(|l| matches!(l, Line::Info(t) if t.contains("history is unreadable")));
    assert!(warned);
}

#[test]
fn test_create_refuses_other_server() {
    let mut app = in_room();
    assert_eq!(typed(&mut app, "/create #x:matrix.org"), vec![]);
    assert!(errored(&app, "only be created on your own server (example.org)"));
}

#[test]
fn test_join_404_offers_create_y() {
    let mut app = in_room();
    app.handle_mx(MxEvent::OfferCreate("#fresh:example.org".into()));
    assert!(
        matches!(last(&app), Line::Info(t) if t.contains("#fresh:example.org") && t.contains("[y/n]"))
    );
    assert_eq!(app.confirm, Some(Confirm::Create("fresh".into())));
    assert_eq!(
        typed(&mut app, "y"),
        vec![Effect::CreateRoom {
            alias: "fresh".into(),
            public: false,
            encrypt: true
        }]
    );
    assert_eq!(app.confirm, None);
}

#[test]
fn test_join_404_offer_declined_and_foreign_ignored() {
    let mut app = in_room();
    app.handle_mx(MxEvent::OfferCreate("#fresh:example.org".into()));
    assert_eq!(typed(&mut app, "n"), vec![]);
    assert_eq!(app.confirm, None);
    // never a prompt for another server, even if the backend asked
    app.handle_mx(MxEvent::OfferCreate("#x:evil.org".into()));
    assert_eq!(app.confirm, None);
}

#[test]
fn test_alias_and_publish() {
    let mut app = in_room();
    assert_eq!(
        typed(&mut app, "/alias add #more:example.org"),
        vec![Effect::AliasAdd { room_id: a(), alias: "#more:example.org".into() }]
    );
    assert_eq!(
        typed(&mut app, "/alias del #more"),
        vec![Effect::AliasDel("#more:example.org".into())]
    );
    assert_eq!(typed(&mut app, "/publish on"), vec![Effect::Publish { room_id: a(), on: true }]);
    assert_eq!(typed(&mut app, "/publish off"), vec![Effect::Publish { room_id: a(), on: false }]);
}

#[test]
fn test_admin_needs_a_channel() {
    let mut app = app();
    for cmd in [
        "/publish on",
        "/ban bob",
        "/unban bob",
        "/alias add #x",
        "/mode +i",
    ] {
        assert_eq!(typed(&mut app, cmd), vec![], "{cmd}");
    }
}

#[test]
fn test_ban_unban_kickban() {
    let mut app = in_room();
    let bob = owned_user_id!("@bob:example.org");
    assert_eq!(
        typed(&mut app, "/ban bob spam"),
        vec![Effect::Ban {
            room_id: a(),
            user: bob.clone(),
            reason: Some("spam".into())
        }]
    );
    assert_eq!(
        typed(&mut app, "/kickban bob"),
        vec![Effect::Ban { room_id: a(), user: bob.clone(), reason: None }]
    );
    assert_eq!(typed(&mut app, "/unban bob"), vec![Effect::Unban { room_id: a(), user: bob }]);
}

#[test]
fn test_power_refuses_above_own_level() {
    let mut app = in_room();
    let bob = owned_user_id!("@bob:example.org");
    assert_eq!(
        typed(&mut app, "/power bob 75"),
        vec![Effect::Power { room_id: a(), user: bob, level: 75 }]
    );
    // bob (power 10) cannot raise anyone above 10
    let mut app = App::new_with_logo(owned_user_id!("@bob:example.org"), Logo::None);
    app.handle_mx(MxEvent::Room(room("!a:example.org", "#room")));
    app.handle_key(alt('2'));
    assert_eq!(typed(&mut app, "/power armin 11"), vec![]);
    assert!(errored(&app, "above your own (10)"));
    assert_eq!(typed(&mut app, "/power armin 10").len(), 1);
}

#[test]
fn test_mode_bare_prints_modes() {
    let mut app = app();
    let mut r = room("!a:example.org", "#room");
    r.invite_only = true;
    r.topic_lock = true;
    r.encrypted = true;
    app.handle_mx(MxEvent::Room(r));
    app.handle_key(alt('2'));
    assert_eq!(typed(&mut app, "/mode"), vec![]);
    assert_eq!(last(&app), Line::Info("Mode for #room: +it (encrypted)".into()));
    // status bar channel field carries them too
    assert_eq!(app.status().modes, "ite");
}

#[test]
fn test_mode_maps_to_matrix_state() {
    let mut app = in_room();
    let bob = owned_user_id!("@bob:example.org");
    assert_eq!(
        typed(&mut app, "/mode +imt"),
        vec![
            Effect::JoinRule { room_id: a(), invite: true },
            Effect::EventsDefault { room_id: a(), level: 50 },
            Effect::TopicLevel { room_id: a(), level: 50 },
        ]
    );
    assert_eq!(
        typed(&mut app, "/mode -imt"),
        vec![
            Effect::JoinRule { room_id: a(), invite: false },
            Effect::EventsDefault { room_id: a(), level: 0 },
            Effect::TopicLevel { room_id: a(), level: 0 },
        ]
    );
    assert_eq!(
        typed(&mut app, "/mode #room +o-o+b-b bob bob bob bob"),
        vec![
            Effect::Power { room_id: a(), user: bob.clone(), level: 50 },
            Effect::Power { room_id: a(), user: bob.clone(), level: 0 },
            Effect::Ban { room_id: a(), user: bob.clone(), reason: None },
            Effect::Unban { room_id: a(), user: bob },
        ]
    );
    assert_eq!(
        last(&app),
        Line::Info("Setting mode [+o bob -o bob +b bob -b bob] on #room".into())
    );
}

#[test]
fn test_mode_refusals() {
    let mut app = in_room();
    assert_eq!(typed(&mut app, "/mode +k"), vec![]);
    assert!(errored(&app, "Unknown mode 'k' (supported: +i +m +t +o +b)"));
    assert_eq!(typed(&mut app, "/mode +o"), vec![]);
    assert!(errored(&app, "+o needs a nick"));
    // an unknown nick drops the whole line, nothing half-applied
    assert_eq!(typed(&mut app, "/mode +i+o nosuchnick"), vec![]);
}
