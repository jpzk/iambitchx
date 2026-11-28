//! Tests for /SET variables, timestamps and protocol debug (DBG) mode.

use super::*;

fn local(ts: i64, fmt: &str) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(ts).unwrap().format(fmt).to_string()
}

#[test]
fn test_timestamp_toggle_rerenders_with_origin_server_ts() {
    let mut app = app();
    app.handle_key(alt('2'));
    let room_id = owned_room_id!("!a:example.org");
    // Two events from different (past) times: history must show THEIR time.
    let (t1, t2) = (1_700_000_000_000, 1_700_000_000_000 + 5 * 3_600_000 + 7 * 60_000);
    let l = |t: &str| Line::Public { nick: "bob".into(), own: false, text: t.into() };
    app.handle_mx(MxEvent::Lines {
        room_id: room_id.clone(),
        lines: vec![(Some("$2".into()), Some(t2), l("second"))],
        backfill: false,
    });
    app.handle_mx(MxEvent::Lines {
        room_id,
        lines: vec![(Some("$1".into()), Some(t1), l("first"))],
        backfill: true,
    });
    let off = rows(&mut app);
    assert!(off.iter().any(|r| r.starts_with("<bob> first")), "{off:?}");

    typed(&mut app, "/set timestamp on");
    assert!(app.vars.timestamp);
    let on = rows(&mut app);
    let s1 = local(t1, "%I:%M%p ");
    let s2 = local(t2, "%I:%M%p ");
    assert!(on.contains(&format!("{s1}<bob> first")), "{on:?}");
    assert!(on.contains(&format!("{s2}<bob> second")), "{on:?}");
    // `-:-` lines (client output, stamped now) carry the prefix too.
    let now = local(now_ms(), "%I:%M%p ");
    assert!(
        on.iter()
            .any(|r| r.starts_with(&format!("{now} Value of TIMESTAMP set to ON")))
    );

    // Changing the format re-renders existing lines.
    typed(&mut app, "/set timestamp_string [%H:%M] ");
    let on = rows(&mut app);
    assert!(on.contains(&format!("{}<bob> first", local(t1, "[%H:%M] "))), "{on:?}");

    typed(&mut app, "/set timestamp off");
    let off = rows(&mut app);
    assert!(off.iter().any(|r| r.starts_with("<bob> first")), "{off:?}");
    assert!(off.iter().any(|r| r.starts_with(" Value of TIMESTAMP set to OFF")), "{off:?}");
}

#[test]
fn test_timestamp_action_and_status_window() {
    let mut app = app();
    app.vars.timestamp = true;
    app.handle_key(alt('2'));
    let ts = 1_750_000_000_000;
    app.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!a:example.org"),
        lines: vec![(
            None,
            Some(ts),
            Line::Action {
                nick: "bob".into(),
                own: false,
                text: "waves".into(),
            },
        )],
        backfill: false,
    });
    let r = rows(&mut app);
    let stamp = local(ts, "%I:%M%p ");
    assert!(
        r.iter()
            .any(|x| x.starts_with(&format!("{stamp} bob")) && x.ends_with(" waves")),
        "{r:?}"
    );
    // The startup logo stays unstamped; its ` iamBitchX:` lines do not.
    app.handle_key(alt('1'));
    let r = rows(&mut app);
    assert!(r[0].starts_with("iamBitchX - Matrix client"), "{r:?}");
    assert!(r.iter().any(|x| x.ends_with("iamBitchX: Type /help for help")), "{r:?}");
    assert!(!r.iter().any(|x| x.starts_with(" iamBitchX: Type")), "{r:?}");
}

#[test]
fn test_set_list_show_errors() {
    let mut app = app();
    let n = app.cur().lines.len();
    typed(&mut app, "/set");
    assert_eq!(
        &app.cur().lines[n..],
        &[
            Line::SetVar { name: "AUTO_AWAY".into(), value: Some("ON".into()) },
            Line::SetVar {
                name: "AUTO_AWAY_MSG".into(),
                value: Some("Auto-Away after %t mins".into())
            },
            Line::SetVar {
                name: "AUTO_AWAY_TIME".into(),
                value: Some("600".into())
            },
            Line::SetVar { name: "AUTO_REPLY".into(), value: None },
            Line::SetVar {
                name: "AWAY_MSG".into(),
                value: Some("away".into())
            },
            Line::SetVar {
                name: "AWAY_NICK".into(),
                value: Some("%n^afk".into())
            },
            Line::SetVar { name: "BEEP".into(), value: Some("ON".into()) },
            Line::SetVar { name: "DEBUG".into(), value: Some("OFF".into()) },
            Line::SetVar {
                name: "DEBUG_FILTER".into(),
                value: Some("all".into())
            },
            Line::SetVar {
                name: "DEBUG_RAW".into(),
                value: Some("OFF".into())
            },
            Line::SetVar { name: "LOG".into(), value: Some("OFF".into()) },
            Line::SetVar { name: "LOG_DIR".into(), value: None },
            Line::SetVar {
                name: "SEND_RECEIPTS".into(),
                value: Some("ON".into())
            },
            Line::SetVar {
                name: "SEND_TYPING".into(),
                value: Some("ON".into())
            },
            Line::SetVar {
                name: "TIMESTAMP".into(),
                value: Some("OFF".into())
            },
            Line::SetVar {
                name: "TIMESTAMP_STRING".into(),
                value: Some("%I:%M%p ".into())
            },
        ]
    );
    typed(&mut app, "/set timestamp_s");
    assert_eq!(
        last(&app),
        Line::SetVar {
            name: "TIMESTAMP_STRING".into(),
            value: Some("%I:%M%p ".into())
        }
    );
    typed(&mut app, "/set bogus 1");
    assert_eq!(last(&app), Line::Info("No such variable \"BOGUS\"".into()));
    typed(&mut app, "/set timestamp maybe");
    assert_eq!(last(&app), Line::Info("Value must be either ON, OFF, or TOGGLE".into()));
    assert!(!app.vars.timestamp);
    let r = rows(&mut app);
    assert!(r.iter().any(|x| x.starts_with("     timestamp_string            %I:%M%p")), "{r:?}");
}

#[test]
fn test_set_persists_to_config_toml() {
    let dir = std::env::temp_dir().join(format!("bx-set-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "[settings]\nlog_level = \"info\"\n[profiles.p]\nuser_id = \"@a:b\"\n")
        .unwrap();
    let mut app = app();
    app.config_toml = Some(path.clone());
    typed(&mut app, "/set timestamp on");
    typed(&mut app, "/set timestamp_string %H:%M:%S ");
    typed(&mut app, "/set auto_reply creed,bx");
    typed(&mut app, "/set beep off");
    assert!(!matches!(last(&app), Line::Error(_)), "{:?}", last(&app));

    let cfg = crate::config::Config::parse_toml(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let settings = cfg.settings.unwrap();
    assert_eq!(settings.log_level.as_deref(), Some("info"));
    assert!(cfg.profiles.contains_key("p"));
    let vars = Vars::from_config(&settings.bitchx.unwrap());
    assert_eq!(vars, app.vars);
    assert_eq!(vars.timestamp_string, "%H:%M:%S ");
    assert!(vars.timestamp);
    assert_eq!(vars.auto_reply, "creed,bx");
    assert!(!vars.beep);
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- protocol debug mode (DBG lines) --------------------------------

fn dbg(room: Option<&str>, text: &str) -> MxEvent {
    MxEvent::Debug(crate::debug_tap::DbgLine {
        cat: crate::debug_tap::Cat::Http,
        room: room.map(str::to_owned),
        text: text.into(),
    })
}

fn dbg_texts(w: &Window) -> Vec<String> {
    w.lines
        .iter()
        .filter_map(|l| match l {
            Line::Debug { cat, text } if cat.is_empty() => Some(text.clone()),
            Line::Debug { cat, text } => Some(format!("{cat} {text}")),
            _ => None,
        })
        .collect()
}

#[test]
fn test_debug_routing_and_flood_guard() {
    const T0: i64 = 1_800_000_000_000;
    let mut app = app();
    app.fake_now = Some(T0);
    let a = app.window_of(&owned_room_id!("!a:example.org")).unwrap();
    app.handle_mx(dbg(None, "GET /_matrix/client/v3/sync 200 1.2s"));
    app.handle_mx(dbg(Some("!nope:example.org"), "unknown room"));
    assert_eq!(
        dbg_texts(&app.windows[0]),
        vec![
            "http GET /_matrix/client/v3/sync 200 1.2s",
            "http [!nope:example.org] unknown room"
        ],
        "every line goes to the status window, room lines tagged"
    );

    for i in 0..25 {
        app.handle_mx(dbg(Some("!a:example.org"), &format!("l{i}")));
    }
    let shown = dbg_texts(&app.windows[0]);
    assert_eq!(shown.len(), DBG_PER_SEC as usize);
    assert_eq!(shown.last().unwrap(), "http [!a:example.org] l17");
    assert!(dbg_texts(&app.windows[a]).is_empty());

    // next second: the suppressed count first, then the line
    app.fake_now = Some(T0 + 1000);
    app.handle_mx(dbg(Some("!a:example.org"), "next"));
    let shown = dbg_texts(&app.windows[0]);
    assert_eq!(
        shown[20..],
        [
            "(suppressed 7)".to_string(),
            "http [!a:example.org] next".to_string()
        ]
    );

    // a burst's tail is flushed by the timer
    for i in 0..22 {
        app.handle_mx(dbg(Some("!a:example.org"), &format!("m{i}")));
    }
    app.fake_now = Some(T0 + 2500);
    app.tick();
    let shown = dbg_texts(&app.windows[0]);
    assert_eq!(shown.last().unwrap(), "(suppressed 3)");
    assert_eq!(shown.len(), 22 + 19 + 1);
    app.tick();
    assert_eq!(dbg_texts(&app.windows[0]).len(), 42, "flushed once");
    assert!(dbg_texts(&app.windows[a]).is_empty());

    let tl = crate::bitchx::format::format_line(
        &Line::Debug {
            cat: "sync".into(),
            text: "next_batch=s1.. joined=3".into(),
        },
        "#r",
    );
    let s: String = tl
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(s, " DBG sync next_batch=s1.. joined=3");
    let tag = tl[0].spans.iter().find(|s| s.content == "DBG").unwrap();
    assert_eq!(tag.style.fg, Some(ratatui::style::Color::Magenta));
}

#[test]
fn test_debug_lines_stay_out_of_room_scrollback() {
    // F-06: room-scoped tap lines were pushed into the room window, between
    // the messages of the conversation
    let mut app = app();
    let a = app.window_of(&owned_room_id!("!a:example.org")).unwrap();
    assert_ne!(a, 0);
    let before = app.windows[a].lines.len();
    app.handle_mx(dbg(Some("!a:example.org"), "raw {\"type\":\"m.room.message\"}"));
    assert_eq!(app.windows[a].lines.len(), before, "room scrollback untouched");
    assert!(dbg_texts(&app.windows[a]).is_empty());
    assert_eq!(
        dbg_texts(&app.windows[0]),
        vec!["http [!a:example.org] raw {\"type\":\"m.room.message\"}"]
    );
}

#[test]
fn test_set_debug_configures_tap() {
    let _tap = crate::debug_tap::tests::harness();
    let mut app = app();
    assert!(!crate::debug_tap::on());
    typed(&mut app, "/set debug on");
    assert!(crate::debug_tap::on());
    assert!(crate::debug_tap::wants(crate::debug_tap::Cat::Sync));
    typed(&mut app, "/set debug_filter http,auth");
    assert!(crate::debug_tap::wants(crate::debug_tap::Cat::Http));
    assert!(crate::debug_tap::wants(crate::debug_tap::Cat::Auth));
    assert!(!crate::debug_tap::wants(crate::debug_tap::Cat::Sync));
    typed(&mut app, "/set debug_raw on");
    assert!(crate::debug_tap::raw());
    typed(&mut app, "/set debug off");
    assert!(!crate::debug_tap::on());
    assert!(!crate::debug_tap::wants(crate::debug_tap::Cat::Http));
    crate::debug_tap::configure(false, "all", false);
}
