//! Tests for windows: numbering, switching, activity, hilites and backfill order.

use super::*;

#[test]
fn test_query_window() {
    let mut app = app();
    let dm = owned_room_id!("!c:example.org");
    let idx = app.window_of(&dm).unwrap();
    // Capture 29: ` Starting conversation with nick`, window named after the peer.
    assert_eq!(app.windows[idx].lines[0], Line::Info("Starting conversation with armin".into()));
    app.switch(idx);
    assert_eq!(app.prompt(), "[armin] ");
    assert_eq!(app.status().query.as_deref(), Some("armin"));
    assert_eq!(app.status().channel, None);
    // `<nick>` lines in a DM become `[msg(peer)]` / `[peer(mxid)]`.
    let public = |nick: &str, own| Line::Public { nick: nick.into(), own, text: "t".into() };
    app.handle_mx(MxEvent::Lines {
        room_id: dm.clone(),
        lines: vec![
            (None, None, public("carla", true)),
            (None, None, public("armin", false)),
        ],
        backfill: false,
    });
    let n = app.cur().lines.len();
    assert_eq!(
        app.cur().lines[n - 2],
        Line::Private {
            nick: "armin".into(),
            mxid: "@armin:remote.tld".into(),
            own: true,
            text: "t".into()
        }
    );
    assert_eq!(
        app.cur().lines[n - 1],
        Line::Private {
            nick: "armin".into(),
            mxid: "@armin:remote.tld".into(),
            own: false,
            text: "t".into()
        }
    );
    // Channel windows keep `<nick>`.
    app.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!a:example.org"),
        lines: vec![(None, None, public("bob", false))],
        backfill: false,
    });
    assert_eq!(app.windows[1].lines.last(), Some(&public("bob", false)));
    // `/query` alone: ` Ending conversation with nick`, back to window 1.
    typed(&mut app, "/query");
    assert_eq!(app.refnum(), 1);
    assert_eq!(last(&app), Line::Info("Ending conversation with armin".into()));
    // ...and does nothing outside a query.
    let before = app.cur().lines.len();
    assert!(typed(&mut app, "/query").is_empty());
    assert_eq!(app.cur().lines.len(), before);
    // `/msg user text` with no DM window echoes `[msg(user)] text`.
    typed(&mut app, "/msg bob yo");
    assert_eq!(
        last(&app),
        Line::Private {
            nick: "bob".into(),
            mxid: "@bob:example.org".into(),
            own: true,
            text: "yo".into()
        }
    );
}

#[test]
fn test_windows_numbered() {
    let app = app();
    assert_eq!(app.windows.len(), 4);
    assert_eq!(app.refnum(), STATUS_REFNUM);
    assert_eq!(app.windows[1].channel(), Some("#room"));
    assert_eq!(app.cur().channel(), None);
    assert_eq!(app.prompt(), "[status] ");
}

#[test]
fn test_alt_digit_switches() {
    let mut app = app();
    let fx = app.handle_key(alt('2'));
    assert_eq!(app.refnum(), 2);
    assert_eq!(app.prompt(), "[#room] ");
    assert_eq!(fx, vec![Effect::Backfill(owned_room_id!("!a:example.org"))]);
    // History is only requested once.
    app.handle_key(alt('1'));
    assert!(app.handle_key(alt('2')).is_empty());
    app.handle_key(alt('4'));
    assert_eq!(app.prompt(), "[armin] ");
    // No such window: stay, print an error.
    app.handle_key(alt('9'));
    assert_eq!(app.refnum(), 4);
    assert!(matches!(last(&app), Line::Error(_)));
}

#[test]
fn test_ctrl_n_p_cycle() {
    let mut app = app();
    app.handle_key(ctrl('n'));
    assert_eq!(app.refnum(), 2);
    app.handle_key(ctrl('p'));
    app.handle_key(ctrl('p'));
    assert_eq!(app.refnum(), 4);
    app.handle_key(ctrl('n'));
    assert_eq!(app.refnum(), 1);
}

#[test]
fn test_window_command() {
    let mut app = app();
    typed(&mut app, "/window 3");
    assert_eq!(app.refnum(), 3);
    typed(&mut app, "/window #room");
    assert_eq!(app.refnum(), 2);
    typed(&mut app, "/window next");
    assert_eq!(app.refnum(), 3);
    typed(&mut app, "/window prev");
    assert_eq!(app.refnum(), 2);
    typed(&mut app, "/window 7");
    assert_eq!(app.refnum(), 2);
    assert_eq!(last(&app), Line::Error("No such window: 7".into()));
}

#[test]
fn test_act_shows_unread() {
    let mut app = app();
    app.handle_key(alt('2'));
    let line = Line::Public { nick: "bob".into(), own: false, text: "hi".into() };
    for room in ["!b:example.org", "!c:example.org", "!a:example.org"] {
        app.handle_mx(MxEvent::Lines {
            room_id: OwnedRoomId::try_from(room).unwrap(),
            lines: vec![(Some(format!("$e{room}")), None, line.clone())],
            backfill: false,
        });
    }
    assert_eq!(app.act(), vec![3, 4]);
    assert_eq!(app.status().act, vec![3, 4]);
    app.handle_key(alt('3'));
    assert_eq!(app.act(), vec![4]);
}

#[test]
fn test_mention_hilites_window_and_beeps_once() {
    let mut app = app();
    app.handle_key(alt('2'));
    let say = |app: &mut App, room: &str, id: &str, own: bool, text: &str| {
        app.handle_mx(MxEvent::Lines {
            room_id: OwnedRoomId::try_from(room).unwrap(),
            lines: vec![(
                Some(id.into()),
                None,
                Line::Public { nick: "bob".into(), own, text: text.into() },
            )],
            backfill: false,
        })
    };
    say(&mut app, "!b:example.org", "$1", false, "plain chatter");
    say(&mut app, "!c:example.org", "$2", false, "carla: look");
    say(&mut app, "!c:example.org", "$3", false, "CARLA again");
    assert_eq!(app.act(), vec![3, 4]);
    assert_eq!(app.hilite(), vec![4], "only the mention window is highlighted");
    assert_eq!(app.status().hilite, vec![4]);
    assert!(app.take_bell(), "bell rings");
    assert!(!app.take_bell(), "...once");
    // A duplicate (already seen event) neither beeps nor highlights.
    say(&mut app, "!b:example.org", "$1", false, "carla");
    assert!(!app.take_bell());
    // Own messages and near-misses never highlight.
    say(&mut app, "!b:example.org", "$4", true, "i am carla");
    say(&mut app, "!b:example.org", "$5", false, "carlax is not me");
    assert_eq!(app.hilite(), vec![4]);
    assert!(!app.take_bell());
    // Switching to the window clears its highlight.
    app.handle_key(alt('4'));
    assert_eq!(app.hilite(), Vec::<usize>::new());
    // In the current window: beep, but nothing to mark.
    say(&mut app, "!c:example.org", "$6", false, "@carla:example.org hi");
    assert!(app.take_bell());
    assert!(!app.windows[3].hilite);
    // BEEP off: highlighted, silent.
    typed(&mut app, "/set beep off");
    say(&mut app, "!b:example.org", "$7", false, "hey carla");
    assert_eq!(app.hilite(), vec![3]);
    assert!(!app.take_bell());
    // AUTO_REPLY words and the display name highlight too.
    typed(&mut app, "/set auto_reply creed, bx");
    assert_eq!(app.vars.auto_reply, "creed,bx");
    app.display_name = Some("Big Boss".into());
    assert!(app.matcher().matches("yo creed"));
    assert!(app.matcher().matches("big boss"));
    typed(&mut app, "/set -auto_reply");
    assert_eq!(app.vars.auto_reply, "");
    assert!(!app.matcher().matches("yo creed"));
}

#[test]
fn test_backfilled_mention_is_silent() {
    let mut app = app();
    app.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!b:example.org"),
        lines: vec![(
            Some("$old".into()),
            None,
            Line::Public {
                nick: "bob".into(),
                own: false,
                text: "carla".into(),
            },
        )],
        backfill: true,
    });
    assert!(!app.take_bell());
    assert_eq!(app.hilite(), Vec::<usize>::new());
}

#[test]
fn test_dedupe_and_backfill_order() {
    let mut app = app();
    let room_id = owned_room_id!("!a:example.org");
    let l = |t: &str| Line::Public { nick: "a".into(), own: false, text: t.into() };
    app.handle_mx(MxEvent::Lines {
        room_id: room_id.clone(),
        lines: vec![(Some("$2".into()), None, l("new"))],
        backfill: false,
    });
    app.handle_mx(MxEvent::Lines {
        room_id: room_id.clone(),
        lines: vec![
            (Some("$1".into()), None, l("old")),
            (Some("$2".into()), None, l("new")),
        ],
        backfill: true,
    });
    let texts: Vec<_> = app.windows[1].lines.clone();
    assert_eq!(texts, vec![l("old"), l("new")]);
}
