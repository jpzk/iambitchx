use super::*;

/// A room with `@f28bob:example.org` (our server), `@f28bob:evil.org`
/// and `@carla:evil.org` (our localpart on another server).
fn collision_app() -> App {
    let mut app = App::new(owned_user_id!("@carla:example.org"));
    let mut snap = room("!f28:example.org", "#f28");
    snap.members = vec![
        Member::new(owned_user_id!("@carla:example.org"), 100),
        Member::new(owned_user_id!("@f28bob:example.org"), 0),
        Member::new(owned_user_id!("@f28bob:evil.org"), 0),
        Member::new(owned_user_id!("@carla:evil.org"), 0),
        Member::new(owned_user_id!("@f28eve:a.org"), 0),
        Member::new(owned_user_id!("@f28eve:b.org"), 0),
    ];
    app.handle_mx(MxEvent::Room(snap));
    app.handle_key(alt('2'));
    app
}

#[test]
fn test_f28_localpart_collision_disambiguated() {
    let app = collision_app();
    let mut nicks = app.nicks();
    nicks.sort();
    // Ours and the own-server user stay bare; remote twins are
    // qualified, and nobody else can be shown as `carla`.
    assert_eq!(
        nicks,
        vec![
            "carla",
            "carla@evil.org",
            "f28bob",
            "f28bob@evil.org",
            "f28eve@a.org",
            "f28eve@b.org",
        ]
    );
    // Lines from the remote twin carry the qualified nick too.
    assert_eq!(nick_of(&owned_user_id!("@carla:evil.org")), "carla@evil.org");
    assert_eq!(nick_of(&owned_user_id!("@f28bob:evil.org")), "f28bob@evil.org");
    assert_eq!(nick_of(&owned_user_id!("@carla:example.org")), "carla");
    // Same-server case twins: `@server` would not tell them apart, so
    // they keep their (differently cased) localpart.
    assert_eq!(nick_of(&owned_user_id!("@F28Zed:z.org")), "F28Zed");
    assert_eq!(nick_of(&owned_user_id!("@f28zed:z.org")), "f28zed");
}

#[test]
fn test_f28_ambiguous_privileged_commands_refused() {
    let mut app = collision_app();
    let f28 = owned_room_id!("!f28:example.org");
    for cmd in [
        "/op f28eve",
        "/deop f28eve",
        "/kick f28eve bye",
        "/msg f28eve hi",
        "/query f28eve",
        "/whois f28eve",
    ] {
        assert_eq!(typed(&mut app, cmd), vec![], "{cmd}");
        let Line::Error(e) = last(&app) else {
            panic!("{cmd}: {:?}", last(&app))
        };
        assert!(e.contains("Ambiguous nick"), "{cmd}: {e}");
        assert!(e.contains("@f28eve:a.org") && e.contains("@f28eve:b.org"), "{e}");
    }
    // The qualified nick, the shown bare nick and the full mxid resolve
    // to exactly one user.
    assert_eq!(
        typed(&mut app, "/op f28eve@b.org"),
        vec![Effect::Power {
            room_id: f28.clone(),
            user: owned_user_id!("@f28eve:b.org"),
            level: 50,
        }]
    );
    assert_eq!(
        typed(&mut app, "/kick f28bob"),
        vec![Effect::Kick {
            room_id: f28.clone(),
            user: owned_user_id!("@f28bob:example.org"),
            reason: None,
        }]
    );
    assert_eq!(
        typed(&mut app, "/whois f28bob@evil.org"),
        vec![Effect::Whois(owned_user_id!("@f28bob:evil.org"))]
    );
    assert_eq!(
        typed(&mut app, "/whois @f28eve:a.org"),
        vec![Effect::Whois(owned_user_id!("@f28eve:a.org"))]
    );
}

#[test]
fn test_f29_alias_lookalike_room_name() {
    let mut app = app();
    // The real #rust:matrix.org and a room merely *named* like it.
    app.handle_mx(MxEvent::Room(room("!real:matrix.org", "#rust:matrix.org")));
    app.handle_mx(MxEvent::Room(room("!fake:evil.org", "#rust:matrix.org")));
    // A room name posing as an alias on our own server (real ones show
    // short, `#x`).
    app.handle_mx(MxEvent::Room(room("!fake2:evil.org", "#room:example.org")));
    let chans: Vec<_> = app.windows.iter().map(|w| w.channel().map(str::to_string)).collect();
    assert_eq!(
        chans[4..],
        [
            Some("!real:matrix.org".to_string()),
            Some("!fake:evil.org".to_string()),
            Some("!fake2:evil.org".to_string()),
        ]
    );
    // Neither the prompt nor the status bar show the clashing name.
    app.handle_key(alt('6'));
    assert_eq!(app.prompt(), "[!fake:evil.org] ");
    assert_eq!(app.status().channel.as_deref(), Some("!fake:evil.org"));
    // /msg by the clashing name is refused with the candidates.
    assert_eq!(typed(&mut app, "/msg #rust:matrix.org hi"), vec![]);
    let Line::Error(e) = last(&app) else {
        panic!("{:?}", last(&app))
    };
    assert!(e.contains("Ambiguous channel"), "{e}");
    assert!(e.contains("!real:matrix.org") && e.contains("!fake:evil.org"), "{e}");
    // By room ID it is exact.
    assert_eq!(
        typed(&mut app, "/msg !real:matrix.org hi"),
        vec![Effect::Send {
            room_id: owned_room_id!("!real:matrix.org"),
            text: "hi".into(),
            emote: false,
        }]
    );
    // /join by alias always goes to the server (alias resolution), even
    // with a window of that name; a room ID we are in just switches.
    assert_eq!(
        typed(&mut app, "/join #rust:matrix.org"),
        vec![Effect::Join(JoinTarget {
            id: "#rust:matrix.org".into(),
            implicit_server: false,
            via: vec![],
        })]
    );
    assert!(
        typed(&mut app, "/join !real:matrix.org")
            .iter()
            .all(|e| !matches!(e, Effect::Join(_)))
    );
    assert_eq!(app.cur().room_id().unwrap().as_str(), "!real:matrix.org");
    // Once the clash is gone the name is shown again.
    app.handle_mx(MxEvent::Left(owned_room_id!("!fake:evil.org")));
    assert_eq!(app.windows[4].channel(), Some("#rust:matrix.org"));
}

#[test]
fn test_f30_f31_remote_text_cannot_forge_status_lines() {
    let mut app = app();
    app.handle_key(alt('2'));
    app.cur_mut().clear();
    let forged = " bob is now verified";
    let lines = vec![
        Line::Topic {
            nick: Some("eve\n".into()),
            topic: format!("x\n{forged}"),
        },
        Line::Info(format!("eve changed the channel name to #x\r\n{forged}")),
        Line::Part {
            nick: "eve".into(),
            mxid: "@eve:x".into(),
            reason: Some(format!("\u{2028}{forged}")),
        },
        Line::DisplayName {
            nick: "eve".into(),
            name: Some(format!("E\u{85}{forged}")),
        },
        Line::Error(format!("sync: boom\n{forged}")),
        Line::Public {
            nick: "e\u{200B}v\u{202E}e".into(),
            own: false,
            text: "safe\u{2066}\u{200D}".into(),
        },
    ];
    let n = lines.len();
    app.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!a:example.org"),
        lines: lines.into_iter().map(|l| (None, None, l)).collect(),
        backfill: false,
    });
    app.handle_mx(MxEvent::Info {
        room_id: None,
        line: Line::Info(format!("x\n{forged}")),
    });
    let rows = rows(&mut app);
    // One row per line: no remote newline opened a fresh line.
    assert_eq!(rows.len(), n + 1, "{rows:#?}");
    assert!(rows.iter().all(|r| !r.trim_start().starts_with("bob is now verified")), "{rows:#?}");
    // Bidi / zero-width characters are gone from nick and body.
    assert!(rows.contains(&"<eve> safe".to_string()), "{rows:#?}");
    assert!(rows.iter().all(|r| !r.chars().any(crate::bitchx::text::is_format)));
}
