use super::*;

#[test]
fn test_plain_text_sends() {
    let mut app = app();
    assert!(typed(&mut app, "hello").is_empty());
    assert!(matches!(last(&app), Line::Error(_)));
    app.handle_key(alt('2'));
    let fx = typed(&mut app, "hello world");
    assert_eq!(fx, vec![Effect::Send {
        room_id: owned_room_id!("!a:example.org"),
        text: "hello world".into(),
        emote: false,
    }]);
    let fx = typed(&mut app, "/me waves");
    assert_eq!(fx, vec![Effect::Send {
        room_id: owned_room_id!("!a:example.org"),
        text: "waves".into(),
        emote: true,
    }]);
    let fx = typed(&mut app, "//slash");
    assert!(matches!(&fx[0], Effect::Send { text, .. } if text == "/slash"));
}

#[test]
fn test_unknown_command_is_error_line() {
    let mut app = app();
    assert!(typed(&mut app, "/frob").is_empty());
    assert_eq!(last(&app), Line::Error("Unknown command: FROB".into()));
}

#[test]
fn test_commands_to_effects() {
    let mut app = app();
    app.handle_key(alt('2'));
    let a = owned_room_id!("!a:example.org");
    let armin = owned_user_id!("@armin:remote.tld");
    assert_eq!(typed(&mut app, "/join #new:x"), vec![Effect::Join(JoinTarget {
        id: "#new:x".into(),
        implicit_server: false,
        via: vec![],
    })]);
    // No server: our own homeserver.
    assert_eq!(typed(&mut app, "/join #new"), vec![Effect::Join(JoinTarget {
        id: "#new:example.org".into(),
        implicit_server: true,
        via: vec![],
    })]);
    // An alias always goes to the server (F29: room names can pose as
    // aliases); a joined room ID just switches.
    assert_eq!(typed(&mut app, "/join #other:example.org"), vec![Effect::Join(JoinTarget {
        id: "#other:example.org".into(),
        implicit_server: false,
        via: vec![],
    })]);
    assert!(
        typed(&mut app, "/join !b:example.org")
            .iter()
            .all(|e| !matches!(e, Effect::Join(_)))
    );
    assert_eq!(app.cur().room.name, "#other");
    app.handle_key(alt('2'));
    assert_eq!(typed(&mut app, "/list matrix.org -s rust"), vec![Effect::List(ListArgs {
        server: Some("matrix.org".into()),
        search: Some("rust".into()),
    })]);
    assert_eq!(typed(&mut app, "/topic hi there"), vec![Effect::Topic {
        room_id: a.clone(),
        topic: "hi there".into()
    }]);
    assert_eq!(typed(&mut app, "/op armin"), vec![Effect::Power {
        room_id: a.clone(),
        user: armin.clone(),
        level: 50
    }]);
    assert_eq!(typed(&mut app, "/deop ARMIN"), vec![Effect::Power {
        room_id: a.clone(),
        user: armin.clone(),
        level: 0
    }]);
    assert_eq!(typed(&mut app, "/kick bob spam"), vec![Effect::Kick {
        room_id: a.clone(),
        user: owned_user_id!("@bob:example.org"),
        reason: Some("spam".into())
    }]);
    assert_eq!(typed(&mut app, "/invite @x:y"), vec![Effect::Invite {
        room_id: a.clone(),
        user: owned_user_id!("@x:y")
    }]);
    assert_eq!(typed(&mut app, "/whois armin"), vec![Effect::Whois(armin.clone())]);
    // An open DM window takes the message; unknown users get a new DM.
    assert_eq!(typed(&mut app, "/msg armin yo"), vec![Effect::Send {
        room_id: owned_room_id!("!c:example.org"),
        text: "yo".into(),
        emote: false
    }]);
    assert_eq!(typed(&mut app, "/msg bob yo"), vec![Effect::Msg {
        user: owned_user_id!("@bob:example.org"),
        text: "yo".into()
    }]);
    assert_eq!(typed(&mut app, "/nick neo"), vec![Effect::Nick("neo".into())]);
    assert_eq!(typed(&mut app, "/away lunch"), vec![Effect::Away(Some("lunch".into()))]);
    assert_eq!(typed(&mut app, "/back"), vec![Effect::Away(None)]);
    assert_eq!(typed(&mut app, "/part"), vec![Effect::Part { room_id: a, reason: None }]);
    assert!(typed(&mut app, "/kick nobody").is_empty());
    assert_eq!(last(&app), Line::Error("nobody: No such nick".into()));
    // An existing DM window is reused by /query.
    assert_eq!(typed(&mut app, "/query armin"), vec![Effect::Backfill(owned_room_id!(
        "!c:example.org"
    ))]);
    assert_eq!(app.refnum(), 4);
    assert_eq!(typed(&mut app, "/query @new:x"), vec![Effect::Query(owned_user_id!("@new:x"))]);
    assert_eq!(typed(&mut app, "/quit"), vec![Effect::Quit]);
    assert!(app.quit);
}

#[test]
fn test_join_synched() {
    let mut app = app();
    assert_eq!(app.refnum(), 1);
    app.handle_mx(MxEvent::Synched {
        room_id: OwnedRoomId::try_from("!b:example.org").unwrap(),
        millis: 319,
    });
    assert_eq!(app.cur().room.name, "#other");
    let n = app.cur().lines.len();
    assert!(matches!(&app.cur().lines[n - 3], Line::Users { shown: 3, .. }));
    assert!(matches!(&app.cur().lines[n - 2], Line::Names { .. }));
    assert_eq!(
        app.cur().lines[n - 1],
        Line::Error("Join to #other was synched in 0.319 secs!!".into())
    );
}

#[test]
fn test_local_commands() {
    let mut app = app();
    app.handle_key(alt('2'));
    typed(&mut app, "/names");
    assert!(
        app.cur()
            .lines
            .iter()
            .any(|l| { *l == Line::Users { chan: "#room".into(), shown: 3, total: 3 } })
    );
    assert!(app.cur().lines.iter().any(|l| {
        *l == Line::Names {
            cells: vec![
                ('@', "carla".into(), None),
                ('+', "bob".into(), None),
                (' ', "armin".into(), None),
            ],
            width: 5,
        }
    }));
    typed(&mut app, "/who");
    assert!(
        app.cur()
            .lines
            .iter()
            .any(|l| matches!(l, Line::Info(t) if t.contains("armin H @armin:remote.tld")))
    );
    typed(&mut app, "/rooms");
    assert!(
        app.cur()
            .lines
            .iter()
            .any(|l| matches!(l, Line::Info(t) if t.starts_with("4: armin")))
    );
    typed(&mut app, "/help");
    assert!(matches!(last(&app), Line::Info(_)));
    typed(&mut app, "/clear");
    assert!(app.cur().lines.is_empty());
}

#[test]
fn test_left_room_closes_window() {
    let mut app = app();
    app.handle_key(alt('4'));
    app.handle_mx(MxEvent::Left(owned_room_id!("!b:example.org")));
    assert_eq!(app.windows.len(), 3);
    assert_eq!(app.prompt(), "[armin] ");
    assert_eq!(app.refnum(), 3);
}

#[test]
fn test_input_keys_through_app() {
    let mut app = app();
    for c in "hello world".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(ctrl('w'));
    assert_eq!(app.input.text(), "hello ");
    app.handle_key(ctrl('a'));
    app.handle_key(key(KeyCode::Char('>')));
    app.handle_key(ctrl('e'));
    app.handle_key(key(KeyCode::Char('!')));
    assert_eq!(app.input.text(), ">hello !");
    app.handle_key(ctrl('u'));
    assert_eq!(app.input.text(), "");
    typed(&mut app, "/window 2");
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.input.text(), "/window 2");
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.input.text(), "");
    for c in "ar".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.input.text(), "armin: ");
}

#[test]
fn test_paging() {
    let mut app = app();
    app.page = 10;
    app.handle_key(key(KeyCode::PageUp));
    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.cur().scroll, 20);
    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.cur().scroll, 10);
}

#[test]
fn test_status_counts() {
    let mut app = app();
    app.handle_key(alt('2'));
    let st = app.status();
    assert_eq!(st.channel.as_deref(), Some("#room"));
    assert!(st.chanop);
    assert_eq!(st.users.unwrap(), UserCounts { ops: 1, nonops: 2, voice: 1 });
    assert_eq!(app.refnum(), 2);
    assert_eq!(st.lag, None);
}
