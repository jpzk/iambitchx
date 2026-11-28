//! Tests for /ignore and /unignore: nick resolution against the window
//! members, the ignorance list, and hiding lines from ignored senders.

use super::*;

fn spam() -> OwnedUserId {
    owned_user_id!("@spam:evil.tld")
}

/// The app with `#room` current; `spam` is a member of `#room` only.
fn in_room() -> App {
    let mut app = app();
    let mut r = room("!a:example.org", "#room");
    r.members.push(member("@spam:evil.tld", "spam", 0));
    app.handle_mx(MxEvent::Room(r));
    app.handle_key(alt('2'));
    // the startup list from the first sync: silent
    app.handle_mx(MxEvent::Ignored(vec![]));
    app
}

fn infos(app: &App) -> Vec<String> {
    app.cur()
        .lines
        .iter()
        .filter_map(|l| match l {
            Line::Info(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn test_ignore_resolves_nick_in_current_window() {
    let mut app = in_room();
    assert_eq!(typed(&mut app, "/ignore spam"), vec![Effect::Ignore(spam())]);
    assert_eq!(typed(&mut app, "/ignore SPAM"), vec![Effect::Ignore(spam())]);
    // a member of another window resolves too
    assert_eq!(
        typed(&mut app, "/ignore armin"),
        vec![Effect::Ignore(owned_user_id!("@armin:remote.tld"))]
    );
    // full user IDs and nick@server need no window
    assert_eq!(
        typed(&mut app, "/ignore @x:far.away"),
        vec![Effect::Ignore(owned_user_id!("@x:far.away"))]
    );
    assert_eq!(
        typed(&mut app, "/ignore x@far.away"),
        vec![Effect::Ignore(owned_user_id!("@x:far.away"))]
    );
}

#[test]
fn test_ignore_ambiguous_nick_refused() {
    let mut app = in_room();
    let mut r = room("!a:example.org", "#room");
    r.members.push(member("@spam:evil.tld", "spam", 0));
    r.members.push(member("@spam:other.tld", "spam", 0));
    app.handle_mx(MxEvent::Room(r));
    assert_eq!(typed(&mut app, "/ignore spam"), vec![]);
    assert!(matches!(last(&app), Line::Error(t) if t.contains("Ambiguous nick")));
}

#[test]
fn test_ignore_refuses_self_unknown_and_duplicates() {
    let mut app = in_room();
    assert_eq!(typed(&mut app, "/ignore carla"), vec![]);
    assert!(matches!(last(&app), Line::Error(t) if t.contains("can't ignore yourself")));
    assert_eq!(typed(&mut app, "/ignore @bad id"), vec![]);
    app.handle_mx(MxEvent::Ignored(vec![spam()]));
    assert_eq!(typed(&mut app, "/ignore spam"), vec![]);
    assert!(matches!(last(&app), Line::Info(t) if t.contains("already being ignored")));
}

#[test]
fn test_ignore_list_and_announcements() {
    let mut app = in_room();
    assert_eq!(typed(&mut app, "/ignore"), vec![]);
    assert_eq!(infos(&app).last().unwrap(), "There are no nicknames being ignored");
    app.handle_mx(MxEvent::Ignored(vec![spam()]));
    assert_eq!(infos(&app).last().unwrap(), "Now ignoring ALL from spam (@spam:evil.tld)");
    assert_eq!(typed(&mut app, "/ignore"), vec![]);
    let i = infos(&app);
    assert_eq!(i[i.len() - 2], "Ignorance List:");
    assert!(i[i.len() - 1].contains("spam") && i[i.len() - 1].contains("@spam:evil.tld"));
    // the same list again (the sync catching up): nothing printed
    let n = app.cur().lines.len();
    app.handle_mx(MxEvent::Ignored(vec![spam()]));
    assert_eq!(app.cur().lines.len(), n);
    app.handle_mx(MxEvent::Ignored(vec![]));
    assert_eq!(infos(&app).last().unwrap(), "spam (@spam:evil.tld) removed from ignorance list");
}

#[test]
fn test_first_ignore_list_is_silent() {
    let mut app = app();
    app.handle_key(alt('2'));
    let n = app.cur().lines.len();
    app.handle_mx(MxEvent::Ignored(vec![spam()]));
    assert_eq!(app.cur().lines.len(), n);
    assert_eq!(app.ignored, vec![spam()]);
}

#[test]
fn test_unignore_resolves_ignored_nick_outside_windows() {
    let mut app = app();
    app.handle_key(alt('2'));
    app.handle_mx(MxEvent::Ignored(vec![owned_user_id!("@gone:far.away")]));
    // in no window, but on the ignore list
    assert_eq!(
        typed(&mut app, "/unignore gone"),
        vec![Effect::Unignore(owned_user_id!("@gone:far.away"))]
    );
    // a member who is not ignored
    assert_eq!(typed(&mut app, "/unignore bob"), vec![]);
    assert!(matches!(last(&app), Line::Info(t) if t.contains("is not being ignored")));
}

#[test]
fn test_ignored_sender_lines_hidden() {
    let mut app = in_room();
    app.handle_mx(MxEvent::Ignored(vec![spam()]));
    let n = app.cur().lines.len();
    let lines = vec![
        (
            Some("$1".into()),
            Some(1),
            Line::Public {
                nick: "spam".into(),
                own: false,
                text: "buy".into(),
            },
        ),
        (
            Some("$2".into()),
            Some(2),
            Line::Action {
                nick: "spam".into(),
                own: false,
                text: "waves".into(),
            },
        ),
        (
            Some("$3".into()),
            Some(3),
            Line::Notice {
                nick: "spam".into(),
                mxid: "@spam:evil.tld".into(),
                text: "x".into(),
            },
        ),
        (
            Some("$4".into()),
            Some(4),
            Line::Undecryptable { nick: "spam".into(), reason: "r".into() },
        ),
        (
            Some("$5".into()),
            Some(5),
            Line::Public { nick: "bob".into(), own: false, text: "hi".into() },
        ),
        // membership stays visible
        (
            Some("$6".into()),
            Some(6),
            Line::Join { nick: "spam".into(), mxid: "@spam:evil.tld".into() },
        ),
    ];
    app.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!a:example.org"),
        lines,
        backfill: false,
    });
    let shown: Vec<Line> = app.cur().lines[n..].to_vec();
    assert_eq!(shown.len(), 2, "{shown:?}");
    assert!(matches!(&shown[0], Line::Public { nick, .. } if nick == "bob"));
    assert!(matches!(&shown[1], Line::Join { .. }));
    // backfill is filtered as well
    let old = vec![(
        Some("$0".into()),
        Some(0),
        Line::Public {
            nick: "spam".into(),
            own: false,
            text: "old".into(),
        },
    )];
    app.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!a:example.org"),
        lines: old,
        backfill: true,
    });
    assert!(
        !app.cur()
            .lines
            .iter()
            .any(|l| matches!(l, Line::Public { text, .. } if text == "old"))
    );
    // unignored: shown again
    app.handle_mx(MxEvent::Ignored(vec![]));
    let again = vec![(
        Some("$7".into()),
        Some(7),
        Line::Public {
            nick: "spam".into(),
            own: false,
            text: "back".into(),
        },
    )];
    app.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!a:example.org"),
        lines: again,
        backfill: false,
    });
    assert!(
        app.cur()
            .lines
            .iter()
            .any(|l| matches!(l, Line::Public { text, .. } if text == "back"))
    );
}

#[test]
fn test_ignored_query_messages_hidden() {
    let mut app = app();
    app.handle_mx(MxEvent::Ignored(vec![]));
    app.handle_mx(MxEvent::Ignored(vec![owned_user_id!("@armin:remote.tld")]));
    let idx = app.windows.iter().position(|w| w.room.name == "armin").unwrap();
    let n = app.windows[idx].lines.len();
    let lines = vec![(
        Some("$q".into()),
        Some(1),
        Line::Public {
            nick: "armin".into(),
            own: false,
            text: "psst".into(),
        },
    )];
    app.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!c:example.org"),
        lines,
        backfill: false,
    });
    assert_eq!(app.windows[idx].lines.len(), n);
}
