//! Reactions, typing notices / the typing status, read receipts and their
//! `/set SEND_TYPING` / `SEND_RECEIPTS` switches.

use super::*;
use crate::bitchx::format::{format_line, plain};
use crate::bitchx::status::typing_text;

fn rid() -> OwnedRoomId {
    owned_room_id!("!a:example.org")
}

fn msg(id: &str, nick: &str, text: &str) -> (Option<String>, Option<i64>, Line) {
    let own = nick == "carla";
    (Some(id.into()), Some(1), Line::Public { nick: nick.into(), own, text: text.into() })
}

fn reaction(id: &str, nick: &str, key: &str, target: &str) -> (Option<String>, Option<i64>, Line) {
    let line = Line::Reaction {
        nick: nick.into(),
        own: nick == "carla",
        key: key.into(),
        target: target.into(),
        quote: None,
    };
    (Some(id.into()), Some(2), line)
}

fn lines(app: &mut App, lines: Vec<(Option<String>, Option<i64>, Line)>) -> Vec<Effect> {
    app.handle_mx(MxEvent::Lines { room_id: rid(), lines, backfill: false })
}

fn shown(line: &Line) -> String {
    plain(&format_line(line, "#room")).join("\n")
}

// ---- reactions -------------------------------------------------------

#[test]
fn test_reaction_line_format() {
    let l = Line::Reaction {
        nick: "armin".into(),
        own: false,
        key: "👍".into(),
        target: "$m1".into(),
        quote: Some("hello world".into()),
    };
    assert_eq!(shown(&l), "* armin reacted 👍 to \"hello world\"");
    let Line::Reaction { nick, own, key, target, .. } = l else {
        unreachable!()
    };
    let unknown = Line::Reaction { nick, own, key, target, quote: None };
    assert_eq!(shown(&unknown), "* armin reacted 👍 to a message");
}

#[test]
fn test_reaction_quotes_the_target_message() {
    let mut app = app();
    app.handle_key(alt('2'));
    lines(&mut app, vec![msg("$m1", "armin", "hello world")]);
    lines(&mut app, vec![reaction("$r1", "bob", "😂", "$m1")]);
    assert_eq!(shown(&last(&app)), "* bob reacted 😂 to \"hello world\"");
    // Target and reaction in the same batch (backfill is oldest first).
    app.handle_mx(MxEvent::Lines {
        room_id: rid(),
        lines: vec![
            msg("$m0", "bob", "older"),
            reaction("$r0", "armin", "+1", "$m0"),
        ],
        backfill: true,
    });
    assert!(
        rows(&mut app)
            .iter()
            .any(|r| r.contains("* armin reacted +1 to \"older\""))
    );
    // Unknown target: no quote.
    lines(&mut app, vec![reaction("$r2", "bob", "x", "$gone")]);
    assert_eq!(shown(&last(&app)), "* bob reacted x to a message");
}

#[test]
fn test_reaction_is_sanitised_and_clipped() {
    let mut app = app();
    app.handle_key(alt('2'));
    let long = "first line is quite a bit longer than thirty chars\nsecond";
    lines(&mut app, vec![msg("$m1", "armin", long)]);
    lines(
        &mut app,
        vec![reaction(
            "$r1",
            "bob",
            "\x1b[31mred\u{202e}key-that-is-way-too-long",
            "$m1",
        )],
    );
    let row = shown(&last(&app));
    assert_eq!(row, "* bob reacted  [31mredkey-t... to \"first line is quite a bit l...\"");
    crate::bitchx::text::assert_inert(&row);
    // The first non-empty line, controls blanked.
    lines(&mut app, vec![msg("$m2", "armin", "  \nb\x07c\nmore")]);
    lines(&mut app, vec![reaction("$r2", "bob", "k", "$m2")]);
    assert_eq!(shown(&last(&app)), "* bob reacted k to \"b c\"");
}

#[test]
fn test_convert_m_reaction() {
    let me = owned_user_id!("@carla:example.org");
    let raw = serde_json::json!({
        "type": "m.reaction",
        "event_id": "$r1",
        "sender": "@armin:remote.tld",
        "origin_server_ts": 5,
        "content": {"m.relates_to": {"rel_type": "m.annotation", "event_id": "$m1", "key": "👍"}}
    });
    let raw = matrix_sdk::ruma::serde::Raw::from_json_string(raw.to_string()).unwrap();
    let got = crate::matrix::convert_raw(&me, &raw);
    assert_eq!(
        got,
        vec![(
            Some("$r1".to_string()),
            Some(5),
            Line::Reaction {
                nick: "armin".into(),
                own: false,
                key: "👍".into(),
                target: "$m1".into(),
                quote: None,
            }
        )]
    );
}

// ---- /react ------------------------------------------------------------

#[test]
fn test_parse_react() {
    let p = |s| crate::bitchx::commands::parse(s);
    let r = |key: &str, nth| Ok(Some(BitchxCommand::React { key: key.into(), nth }));
    assert_eq!(p("react 👍"), r("👍", 1));
    assert_eq!(p("react 👍 3"), r("👍", 3));
    assert_eq!(p("react lol ok 2"), r("lol ok", 2));
    assert_eq!(p("react +1"), r("+1", 1));
    assert_eq!(p("react 2"), r("2", 1), "a lone number is the key");
    assert!(p("react x 0").is_err());
    assert!(p("react").is_err());
}

#[test]
fn test_react_to_last_and_nth_last_message() {
    let mut app = app();
    app.handle_key(alt('2'));
    lines(&mut app, vec![msg("$m1", "armin", "one"), msg("$m2", "carla", "two")]);
    // Non-message events are skipped when counting.
    lines(
        &mut app,
        vec![(
            Some("$j".into()),
            Some(3),
            Line::Join {
                nick: "bob".into(),
                mxid: "@bob:example.org".into(),
            },
        )],
    );
    let react = |key: &str, id: &str| Effect::React {
        room_id: rid(),
        event_id: id.into(),
        key: key.into(),
    };
    assert_eq!(typed(&mut app, "/react 👍"), vec![react("👍", "$m2")]);
    assert_eq!(typed(&mut app, "/react nice one 2"), vec![react("nice one", "$m1")]);
    assert_eq!(typed(&mut app, "/react x 3"), vec![]);
    assert_eq!(last(&app), Line::Error("No message 3 to react to (2 in this window)".into()));
    // The status window has nothing to react to.
    app.handle_key(alt('1'));
    assert_eq!(typed(&mut app, "/react x"), vec![]);
}

// ---- typing --------------------------------------------------------------

#[test]
fn test_typing_status_bar_text() {
    assert_eq!(typing_text(&[]), None);
    let n = |v: &[&str]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(typing_text(&n(&["armin"])).as_deref(), Some("typing: armin"));
    assert_eq!(typing_text(&n(&["armin", "bob"])).as_deref(), Some("typing: armin, bob"));
    assert_eq!(typing_text(&n(&["a", "b", "c", "d", "e"])).as_deref(), Some("typing: a, b, c +2"));
}

#[test]
fn test_typing_shows_in_status_bar_of_active_window() {
    let mut app = app();
    app.handle_key(alt('2'));
    let users = vec![
        owned_user_id!("@armin:remote.tld"),
        owned_user_id!("@carla:example.org"),
        owned_user_id!("@bob:example.org"),
    ];
    app.handle_mx(MxEvent::Typing { room_id: rid(), users });
    assert_eq!(app.status().typing, vec!["armin".to_string(), "bob".to_string()], "not us");
    let l2: String = app.status().line2().spans.iter().map(|s| s.content.to_string()).collect();
    assert!(l2.ends_with(" [typing: armin, bob]"), "{l2:?}");
    // Another window does not show it.
    app.handle_key(alt('3'));
    assert!(app.status().typing.is_empty());
    let l2: String = app.status().line2().spans.iter().map(|s| s.content.to_string()).collect();
    assert!(!l2.contains("typing"), "{l2:?}");
    // Back, then they stop.
    app.handle_key(alt('2'));
    app.handle_mx(MxEvent::Typing { room_id: rid(), users: vec![] });
    assert!(app.status().typing.is_empty());
}

fn typing_app() -> App {
    let mut app = app();
    app.vars.send_typing = true;
    app.fake_now = Some(1_000_000);
    app.handle_key(alt('2'));
    app
}

fn typing(on: bool) -> Effect {
    Effect::Typing { room_id: rid(), typing: on }
}

#[test]
fn test_typing_notice_throttled_and_stopped_on_send() {
    let mut app = typing_app();
    assert_eq!(app.handle_key(key(KeyCode::Char('h'))), vec![typing(true)]);
    assert_eq!(app.handle_key(key(KeyCode::Char('i'))), vec![], "throttled");
    app.fake_now = Some(1_000_000 + TYPING_RESEND_MS);
    assert_eq!(app.handle_key(key(KeyCode::Char('!'))), vec![typing(true)], "refreshed");
    let fx = app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        fx,
        vec![
            Effect::Send { room_id: rid(), text: "hi!".into(), emote: false },
            typing(false),
        ]
    );
    assert_eq!(app.typing_room, None);
}

#[test]
fn test_typing_stops_on_clear_and_skips_commands() {
    let mut app = typing_app();
    assert_eq!(app.handle_key(key(KeyCode::Char('x'))), vec![typing(true)]);
    assert_eq!(app.handle_key(ctrl('u')), vec![typing(false)], "line cleared");
    assert_eq!(app.handle_key(key(KeyCode::Char('y'))), vec![typing(true)]);
    assert_eq!(app.handle_key(key(KeyCode::Backspace)), vec![typing(false)]);
    // A /command is not a message.
    for c in "/whois bob".chars() {
        assert_eq!(app.handle_key(key(KeyCode::Char(c))), vec![]);
    }
    app.handle_key(ctrl('u'));
    // `//text` is.
    app.handle_key(key(KeyCode::Char('/')));
    assert_eq!(app.handle_key(key(KeyCode::Char('/'))), vec![typing(true)]);
    // Switching windows stops it in the old room (#other gets its own).
    let fx = app.handle_key(alt('3'));
    assert!(fx.contains(&typing(false)), "{fx:?}");
    assert!(fx.contains(&Effect::Typing {
        room_id: owned_room_id!("!b:example.org"),
        typing: true
    }));
    // Nothing in the status window.
    let mut app = typing_app();
    app.handle_key(alt('1'));
    assert_eq!(app.handle_key(key(KeyCode::Char('x'))), vec![]);
}

#[test]
fn test_send_typing_off_sends_nothing() {
    let mut app = typing_app();
    typed(&mut app, "/set send_typing off");
    assert_eq!(last(&app), Line::Error("Not saved: no config.toml (JSON config)".into()));
    assert!(!app.vars.send_typing);
    assert_eq!(app.handle_key(key(KeyCode::Char('x'))), vec![]);
    assert_eq!(
        app.handle_key(key(KeyCode::Enter)),
        vec![Effect::Send { room_id: rid(), text: "x".into(), emote: false }]
    );
    // Turning it off mid-line stops the notice on the next key.
    typed(&mut app, "/set send_typing on");
    assert_eq!(app.handle_key(key(KeyCode::Char('a'))), vec![typing(true)]);
    app.vars.set("SEND_TYPING", "OFF").unwrap();
    assert_eq!(app.handle_key(key(KeyCode::Char('b'))), vec![typing(false)]);
}

// ---- receipts -----------------------------------------------------------

fn receipt(id: &str) -> Effect {
    Effect::Receipt { room_id: rid(), event_id: id.into() }
}

#[test]
fn test_read_receipt_for_active_window() {
    let mut app = app();
    app.vars.send_receipts = true;
    // Window 2 is not active: its lines send nothing.
    assert_eq!(lines(&mut app, vec![msg("$m1", "armin", "one")]), vec![]);
    // Opening it acks the newest event (after the backfill request).
    let fx = app.handle_key(alt('2'));
    assert_eq!(fx, vec![Effect::Backfill(rid()), receipt("$m1")]);
    // New events while it is active are acked as they arrive, once.
    assert_eq!(lines(&mut app, vec![msg("$m2", "bob", "two")]), vec![receipt("$m2")]);
    assert_eq!(lines(&mut app, vec![msg("$m2", "bob", "two")]), vec![], "dup");
    app.handle_key(alt('1'));
    assert_eq!(app.handle_key(alt('2')), vec![], "already acked");
    // Backfill does not move the receipt back.
    let fx = app.handle_mx(MxEvent::Lines {
        room_id: rid(),
        lines: vec![msg("$m0", "bob", "old")],
        backfill: true,
    });
    assert_eq!(fx, vec![]);
    assert_eq!(app.cur().last_event.as_deref(), Some("$m2"));
}

#[test]
fn test_send_receipts_off_sends_nothing() {
    let mut app = app();
    app.vars.send_receipts = true;
    typed(&mut app, "/set SEND_RECEIPTS OFF");
    assert_eq!(app.vars.get("SEND_RECEIPTS").as_deref(), Some("OFF"));
    lines(&mut app, vec![msg("$m1", "armin", "one")]);
    assert_eq!(app.handle_key(alt('2')), vec![Effect::Backfill(rid())]);
    assert_eq!(lines(&mut app, vec![msg("$m2", "bob", "two")]), vec![]);
    // Back on: the next activation acks the newest event.
    typed(&mut app, "/set SEND_RECEIPTS ON");
    app.handle_key(alt('1'));
    assert_eq!(app.handle_key(alt('2')), vec![receipt("$m2")]);
}

#[test]
fn test_set_lists_send_vars() {
    let mut app = app();
    typed(&mut app, "/set");
    let names: Vec<String> = app
        .cur()
        .lines
        .iter()
        .filter_map(|l| match l {
            Line::SetVar { name, value } => {
                Some(format!("{name}={}", value.clone().unwrap_or_default()))
            },
            _ => None,
        })
        .collect();
    assert!(names.contains(&"SEND_TYPING=OFF".to_string()), "{names:?}");
    assert!(names.contains(&"SEND_RECEIPTS=OFF".to_string()), "fixture has them off");
    assert!(Vars::default().send_typing && Vars::default().send_receipts, "default ON");
}
