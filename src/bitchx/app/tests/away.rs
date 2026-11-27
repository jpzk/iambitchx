//! Tests for `AUTO_AWAY` timing, `AWAY_NICK` and the self-whois probe.

use super::*;

// ---- AUTO_AWAY (fake clock) -----------------------------------------

const T0: i64 = 1_764_000_000_000;

/// An armed app on the fake clock at `T0`, last keypress at `T0`.
fn idle_app() -> App {
    let mut app = app();
    app.fake_now = Some(T0);
    app.last_input = T0;
    app.auto_away_armed = true;
    app
}

fn auto_msg() -> Effect {
    Effect::Away(Some("Auto-Away after 10 mins".into()))
}

#[test]
fn test_auto_away_after_threshold() {
    let mut app = idle_app();
    app.fake_now = Some(T0 + 599_999);
    assert_eq!(app.tick(), vec![]);
    assert!(app.away.is_none());
    app.fake_now = Some(T0 + 600_000);
    assert_eq!(app.tick(), vec![auto_msg()]);
    assert!(app.auto_away);
    assert!(app.status().away);
    // Only once.
    app.fake_now = Some(T0 + 700_000);
    assert_eq!(app.tick(), vec![]);
    // The homeserver echo prints the 306 line with the reason.
    app.handle_mx(MxEvent::Away(Some("Auto-Away after 10 mins".into())));
    assert_eq!(
        last(&app),
        Line::Info("You have been marked as being away (Auto-Away after 10 mins)".into())
    );
    // A configured time: /set auto_away_time 15 -> 15 minutes.
    let mut app = idle_app();
    typed(&mut app, "/set auto_away_time 15");
    assert_eq!(app.vars.auto_away_time, 900);
    app.last_input = T0;
    app.fake_now = Some(T0 + 899_000);
    assert_eq!(app.tick(), vec![]);
    app.fake_now = Some(T0 + 900_000);
    assert_eq!(app.tick(), vec![Effect::Away(Some("Auto-Away after 15 mins".into()))]);
}

#[test]
fn test_auto_back_on_input() {
    let mut app = idle_app();
    app.fake_now = Some(T0 + 600_000);
    app.tick();
    app.handle_mx(MxEvent::Away(Some("Auto-Away after 10 mins".into())));
    app.fake_now = Some(T0 + 600_000 + 3_723_000);
    // First keypress: back, and the key still types.
    assert_eq!(app.handle_key(key(KeyCode::Char('x'))), vec![Effect::Away(None)]);
    assert_eq!(app.input.text(), "x");
    assert!(!app.auto_away);
    // Second keypress: nothing more.
    assert_eq!(app.handle_key(key(KeyCode::Char('y'))), vec![]);
    app.handle_mx(MxEvent::Away(None));
    let n = app.cur().lines.len();
    assert_eq!(
        &app.cur().lines[n - 2..],
        &[
            Line::Info(
                "You were /away for 1 hours 2 minutes and 3 seconds. [BX-MsgLog Off]".into()
            ),
            Line::Info("You are no longer marked as being away".into()),
        ]
    );
    assert!(app.away.is_none());
    // The idle clock restarted at the keypress.
    let back = T0 + 600_000 + 3_723_000;
    app.fake_now = Some(back + 599_000);
    assert_eq!(app.tick(), vec![]);
    app.fake_now = Some(back + 600_000);
    assert_eq!(app.tick(), vec![auto_msg()]);
}

#[test]
fn test_manual_away_not_cleared_by_input() {
    let mut app = idle_app();
    assert_eq!(typed(&mut app, "/away lunch"), vec![Effect::Away(Some("lunch".into()))]);
    app.handle_mx(MxEvent::Away(Some("lunch".into())));
    // Typing does not bring us back, and the timer leaves it alone.
    assert_eq!(app.handle_key(key(KeyCode::Char('a'))), vec![]);
    app.fake_now = Some(T0 + 3_600_000);
    assert_eq!(app.tick(), vec![]);
    assert_eq!(app.away.as_deref(), Some("lunch"));
    // A manual /away during an auto-away is manual too.
    let mut app = idle_app();
    app.fake_now = Some(T0 + 600_000);
    app.tick();
    typed(&mut app, "/away gone");
    app.handle_mx(MxEvent::Away(Some("gone".into())));
    assert_eq!(app.handle_key(key(KeyCode::Char('a'))), vec![]);
    // Only /back ends it.
    app.handle_key(ctrl('u'));
    assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None)]);
}

#[test]
fn test_auto_away_off_disables() {
    let mut app = idle_app();
    typed(&mut app, "/set auto_away off");
    assert!(!app.vars.auto_away);
    app.fake_now = Some(T0 + 86_400_000);
    assert_eq!(app.tick(), vec![]);
    typed(&mut app, "/set auto_away on");
    app.fake_now = Some(app.last_input + 600_000);
    assert_eq!(app.tick(), vec![auto_msg()]);
    // AUTO_AWAY_TIME 0 disables too.
    let mut app = idle_app();
    typed(&mut app, "/set auto_away_time 0");
    app.fake_now = Some(T0 + 86_400_000);
    assert_eq!(app.tick(), vec![]);
    // Never in --smoke: an unarmed app (the default) never goes away.
    let mut app = self::app();
    app.fake_now = Some(app.last_input + 86_400_000);
    assert_eq!(app.tick(), vec![]);
}

#[test]
fn test_away_msgcount_and_presence() {
    let mut app = idle_app();
    app.fake_now = Some(T0 + 600_000);
    app.tick();
    let dm = owned_room_id!("!c:example.org");
    let msg = |id: &str| {
        (
            Some(id.to_string()),
            Some(T0),
            Line::Public {
                nick: "armin".into(),
                own: false,
                text: "yo".into(),
            },
        )
    };
    app.handle_mx(MxEvent::Lines {
        room_id: dm.clone(),
        lines: vec![msg("$1"), msg("$2"), msg("$1")],
        backfill: false,
    });
    assert_eq!(app.status().away_msgs, 2);
    // Channel lines do not count.
    app.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!a:example.org"),
        lines: vec![msg("$3")],
        backfill: false,
    });
    assert_eq!(app.status().away_msgs, 2);
    app.handle_key(key(KeyCode::Char('x')));
    app.handle_mx(MxEvent::Away(None));
    assert_eq!(app.status().away_msgs, 0);

    // The query window shows the peer's presence.
    let q = app.find_window("armin").unwrap();
    app.switch(q);
    assert_eq!(app.status().query_away, None);
    let armin = owned_user_id!("@armin:remote.tld");
    app.handle_mx(presence_ev(&armin, "unavailable", None));
    assert_eq!(app.status().query_away.map(|m| m.text()).as_deref(), Some("zZzZ"));
    app.handle_mx(presence_ev(&armin, "online", None));
    assert_eq!(app.status().query_away, None);
}

// ---- AWAY_NICK ------------------------------------------------------

fn whois_lines(me: &str, name: &str) -> Vec<Line> {
    let row = |row: u8, last: bool, key: &str, value: &str| Line::WhoisRow {
        row,
        last,
        key: key.into(),
        value: value.into(),
    };
    vec![
        Line::WhoisRule,
        Line::WhoisNick {
            nick: "carla".into(),
            mxid: me.into(),
            info: "unknown".into(),
        },
        row(1, false, "ircname", name),
        row(2, true, "server", "example.org"),
    ]
}

/// Connect: the first lag sample sends the silent self-whois.
fn probe(app: &mut App, name: &str) {
    assert_eq!(app.handle_mx(MxEvent::Lag(0)), vec![Effect::Whois(app.me.clone())]);
    let me = app.me.to_string();
    for line in whois_lines(&me, name) {
        app.handle_mx(MxEvent::Info { room_id: None, line });
    }
}

fn named(name: &str) -> App {
    let mut app = app();
    probe(&mut app, name);
    app
}

fn nick(s: &str) -> Effect {
    Effect::Nick(s.into())
}

fn away(s: &str) -> Effect {
    Effect::Away(Some(s.into()))
}

#[test]
fn test_away_nick_probe_is_silent() {
    let mut app = app();
    let before: Vec<usize> = app.windows.iter().map(|w| w.lines.len()).collect();
    probe(&mut app, "Carla");
    let after: Vec<usize> = app.windows.iter().map(|w| w.lines.len()).collect();
    assert_eq!(before, after, "self-whois lines are swallowed");
    assert_eq!(app.display_name.as_deref(), Some("Carla"));
    assert_eq!(app.probe, Probe::Done);
    // only once
    assert_eq!(app.handle_mx(MxEvent::Lag(1)), vec![]);
    // a later /whois is shown in full
    let me = app.me.to_string();
    for line in whois_lines(&me, "Carla") {
        app.handle_mx(MxEvent::Info { room_id: None, line });
    }
    assert!(matches!(last(&app), Line::WhoisRow { last: true, .. }));
}

#[test]
fn test_away_nick_probe_gives_back_foreign_whois() {
    let mut app = app();
    app.handle_mx(MxEvent::Lag(0));
    for line in whois_lines("@armin:remote.tld", "Armin") {
        app.handle_mx(MxEvent::Info { room_id: None, line });
    }
    let lines = &app.cur().lines;
    let n = lines.len();
    assert!(matches!(lines[n - 4], Line::WhoisRule), "{:?}", &lines[n - 4..]);
    assert!(matches!(lines[n - 3], Line::WhoisNick { .. }));
    assert_eq!(app.display_name, None);
}

#[test]
fn test_away_nick_manual_away_and_back() {
    let mut app = named("Carla");
    assert_eq!(typed(&mut app, "/away lunch"), vec![away("lunch"), nick("Carla^afk")]);
    assert_eq!(app.vars.away_nick_orig.as_deref(), Some("Carla"));
    app.handle_mx(MxEvent::Away(Some("lunch".into())));
    app.handle_mx(MxEvent::OwnDisplayName("Carla^afk".into()));
    let shown = app.windows[0].lines.iter().any(|l| {
        *l == Line::Error("Display name is now Carla^afk (away; /back restores Carla)".into())
    });
    assert!(shown, "iamBitchX line on the change");
    // already away: a new reason, but the nick is left alone
    assert_eq!(typed(&mut app, "/away dinner"), vec![away("dinner")]);
    assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None), nick("Carla")]);
    assert_eq!(app.vars.away_nick_orig, None);
    app.handle_mx(MxEvent::Away(None));
    app.handle_mx(MxEvent::OwnDisplayName("Carla".into()));
    let n = app.windows[0].lines.len();
    assert_eq!(app.windows[0].lines[n - 2], Line::Error("Display name is now Carla".into()));
    // /back again: nothing left to restore
    assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None)]);
}

#[test]
fn test_away_nick_never_stacks() {
    // crash while away left the away nick on, the original unknown
    let mut app = named("Carla^afk");
    assert!(matches!(last(&app), Line::Error(e) if e.contains("/back to restore Carla")));
    assert_eq!(app.vars.away_nick_orig.as_deref(), Some("Carla"));
    assert_eq!(typed(&mut app, "/away"), vec![away("away")], "no 'Carla^afk^afk'");
    assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None), nick("Carla")]);

    // a display name that already is an away nick, no orig recorded
    let mut app = super::tests::app();
    app.display_name = Some("Carla^afk".into());
    assert_eq!(typed(&mut app, "/away"), vec![away("away")]);
    assert_eq!(app.vars.away_nick_orig.as_deref(), Some("Carla"));

    // many rounds: always the same two names
    let mut app = named("Carla");
    for _ in 0..3 {
        let fx = typed(&mut app, "/away");
        assert_eq!(fx, vec![away("away"), nick("Carla^afk")]);
        app.handle_mx(MxEvent::Away(Some("away".into())));
        app.handle_mx(MxEvent::OwnDisplayName("Carla^afk".into()));
        assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None), nick("Carla")]);
        app.handle_mx(MxEvent::Away(None));
        app.handle_mx(MxEvent::OwnDisplayName("Carla".into()));
    }
}

#[test]
fn test_away_nick_empty_disables() {
    let mut app = named("Carla");
    app.vars.away_nick = String::new();
    assert_eq!(typed(&mut app, "/away"), vec![away("away")]);
    assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None)]);
    assert_eq!(app.vars.away_nick_orig, None);
    // auto-away too
    app.handle_mx(MxEvent::Away(None));
    app.auto_away_armed = true;
    app.fake_now = Some(app.last_input + 600_000);
    assert_eq!(app.tick(), vec![auto_msg()]);
    assert_eq!(app.activity(), vec![Effect::Away(None)]);
}

#[test]
fn test_away_nick_unknown_display_name_skips() {
    let mut app = app();
    assert_eq!(typed(&mut app, "/away"), vec![away("away")]);
    assert_eq!(last(&app), Line::Error("AWAY_NICK skipped: display name not known yet".into()));
}

#[test]
fn test_away_messages_manual_vs_auto() {
    let mut app = named("Carla");
    app.vars.away_nick = "[%n]".into();
    assert_eq!(typed(&mut app, "/away"), vec![away("away"), nick("[Carla]")]);
    typed(&mut app, "/back");
    typed(&mut app, "/set away_msg gone fishing");
    assert_eq!(app.vars.away_msg, "gone fishing");
    assert_eq!(typed(&mut app, "/away")[0], away("gone fishing"));
    typed(&mut app, "/back");
    assert_eq!(typed(&mut app, "/away brb")[0], away("brb"), "text wins over AWAY_MSG");
    typed(&mut app, "/back");

    // auto-away: AUTO_AWAY_MSG with %t, plus the away nick
    app.handle_mx(MxEvent::Away(None));
    typed(&mut app, "/set auto_away_msg idle %t min");
    typed(&mut app, "/set auto_away_time 900");
    app.auto_away_armed = true;
    app.fake_now = Some(app.last_input + 900_000);
    assert_eq!(app.tick(), vec![away("idle 15 min"), nick("[Carla]")]);
    app.handle_mx(MxEvent::OwnDisplayName("[Carla]".into()));
    // auto-back restores
    assert_eq!(app.activity(), vec![Effect::Away(None), nick("Carla")]);
}

#[test]
fn test_away_nick_restored_after_restart() {
    let dir = std::env::temp_dir().join(format!("bx-awaynick-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    let _ = std::fs::remove_file(&path);
    let load = || {
        let s = std::fs::read_to_string(&path).unwrap();
        let cfg = crate::config::Config::parse_toml(&s).unwrap();
        Vars::from_config(&cfg.settings.unwrap().bitchx.unwrap())
    };

    // run 1: auto-away switches the nick, then the client dies
    let mut app = named("Carla");
    app.config_toml = Some(path.clone());
    app.auto_away_armed = true;
    app.fake_now = Some(app.last_input + 600_000);
    assert_eq!(app.tick(), vec![auto_msg(), nick("Carla^afk")]);
    assert_eq!(load().away_nick_orig.as_deref(), Some("Carla"), "persisted");
    drop(app);

    // run 2: not away, display name still the away nick -> offer + /back
    let mut app = super::tests::app();
    app.vars = load();
    app.config_toml = Some(path.clone());
    probe(&mut app, "Carla^afk");
    assert_eq!(
        last(&app),
        Line::Error(
            "Display name is still the away nick Carla^afk: type /back to restore Carla".into()
        )
    );
    assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None), nick("Carla")]);
    assert_eq!(load().away_nick_orig, None, "cleared on disk");

    // a custom original survives too (not derivable from the away nick)
    let mut app = named("Neo");
    app.config_toml = Some(path.clone());
    app.vars.away_nick = "zzz".into();
    assert_eq!(typed(&mut app, "/away"), vec![away("away"), nick("zzz")]);
    let mut app = app_after_restart(&path, load(), "zzz");
    assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None), nick("Neo")]);

    // restarted after the restore already happened: no offer, orig dropped
    let mut app = named("Neo");
    app.config_toml = Some(path.clone());
    typed(&mut app, "/away");
    let app = app_after_restart(&path, load(), "Neo");
    assert!(!matches!(last(&app), Line::Error(e) if e.contains("/back")));
    assert_eq!(load().away_nick_orig, None);
    std::fs::remove_dir_all(&dir).unwrap();
}

fn app_after_restart(path: &std::path::Path, vars: Vars, name: &str) -> App {
    let mut app = app();
    app.vars = vars;
    app.config_toml = Some(path.to_path_buf());
    probe(&mut app, name);
    app
}
