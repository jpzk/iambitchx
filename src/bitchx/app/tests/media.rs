//! Media numbering, `/get` and `/upload`.

use super::*;

use crate::bitchx::media::{GetArgs, Media, Pick};

fn media(name: &str) -> Media {
    Media {
        kind: "m.image".into(),
        name: name.into(),
        caption: None,
        source: format!("{{\"url\":\"mxc://x/{name}\"}}"),
    }
}

fn media_ev(id: &str, nick: &str, name: &str, backfill: bool) -> MxEvent {
    MxEvent::Lines {
        room_id: owned_room_id!("!a:example.org"),
        lines: vec![(
            Some(id.into()),
            Some(1),
            Line::Media { nick: nick.into(), own: false, media: media(name) },
        )],
        backfill,
    }
}

/// `app()` focused on `#room` (window 2).
fn in_room() -> App {
    let mut app = app();
    app.handle_key(alt('2'));
    assert_eq!(app.cur().room.name, "#room");
    app
}

#[test]
fn test_media_lines_are_numbered_per_window() {
    let mut app = in_room();
    app.handle_mx(media_ev("$1", "bob", "cat.png", false));
    app.handle_mx(media_ev("$2", "bob", "dog.png", false));
    // a duplicate event keeps its number and is not shown twice
    app.handle_mx(media_ev("$2", "bob", "dog.png", false));
    let texts: Vec<_> = app
        .cur()
        .lines
        .iter()
        .filter_map(|l| match l {
            Line::Public { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["[m.image #1] cat.png", "[m.image #2] dog.png"]);
    assert_eq!(app.cur().media.len(), 2);
    // another window starts at #1
    app.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!b:example.org"),
        lines: vec![(
            Some("$3".into()),
            Some(1),
            Line::Media {
                nick: "bob".into(),
                own: false,
                media: media("x.png"),
            },
        )],
        backfill: false,
    });
    let other = app.windows.iter().find(|w| w.room.name == "#other").unwrap();
    assert_eq!(other.media[0].n, 1);
}

#[test]
fn test_backfilled_media_are_older() {
    let mut app = in_room();
    app.handle_mx(media_ev("$new", "bob", "new.png", false));
    app.handle_mx(media_ev("$old", "bob", "old.png", true));
    // /get (last) is the live one, /get 2 the backfilled one
    let fx = typed(&mut app, "/get");
    assert!(matches!(&fx[..], [Effect::Get { media, .. }] if media.name == "new.png"), "{fx:?}");
    let fx = typed(&mut app, "/get 2");
    assert!(matches!(&fx[..], [Effect::Get { media, .. }] if media.name == "old.png"), "{fx:?}");
}

#[test]
fn test_get_picks_nth_last_or_number() {
    let mut app = in_room();
    for (i, n) in ["a.png", "b.png", "c.png"].iter().enumerate() {
        app.handle_mx(media_ev(&format!("${i}"), "bob", n, false));
    }
    let room_id = owned_room_id!("!a:example.org");
    assert_eq!(
        typed(&mut app, "/get"),
        vec![Effect::Get {
            room_id: room_id.clone(),
            n: 3,
            media: media("c.png"),
            open: false
        }]
    );
    assert_eq!(
        typed(&mut app, "/get 3"),
        vec![Effect::Get {
            room_id: room_id.clone(),
            n: 1,
            media: media("a.png"),
            open: false
        }]
    );
    assert_eq!(
        typed(&mut app, "/get -o #2"),
        vec![Effect::Get { room_id, n: 2, media: media("b.png"), open: true }]
    );
    assert_eq!(typed(&mut app, "/get 4"), vec![]);
    assert_eq!(last(&app), Line::Error("No media 4 back in this window".into()));
    assert_eq!(typed(&mut app, "/get #9"), vec![]);
    assert_eq!(last(&app), Line::Error("No media #9 in this window".into()));
}

#[test]
fn test_get_without_media_or_room() {
    let mut app = in_room();
    assert_eq!(typed(&mut app, "/get"), vec![]);
    assert_eq!(last(&app), Line::Error("No media in this window".into()));
    app.handle_key(alt('1'));
    assert_eq!(typed(&mut app, "/get"), vec![]);
}

#[test]
fn test_upload_effect() {
    let mut app = in_room();
    assert_eq!(
        typed(&mut app, "/upload ~/cat.png look"),
        vec![Effect::Upload {
            room_id: owned_room_id!("!a:example.org"),
            path: "~/cat.png".into(),
            caption: Some("look".into()),
        }]
    );
    assert_eq!(typed(&mut app, "/upload"), vec![]);
}

#[test]
fn test_parse_get_upload_commands() {
    use crate::bitchx::commands::{BitchxCommand, parse};
    assert_eq!(
        parse("get").unwrap(),
        Some(BitchxCommand::Get(GetArgs { pick: Pick::Last(1), open: false }))
    );
    assert_eq!(
        parse("get -o 2").unwrap(),
        Some(BitchxCommand::Get(GetArgs { pick: Pick::Last(2), open: true }))
    );
    assert!(parse("get zero").is_err());
    assert_eq!(
        parse("upload \"/a b/c.png\" hi there").unwrap(),
        Some(BitchxCommand::Upload {
            path: "/a b/c.png".into(),
            caption: Some("hi there".into())
        })
    );
    assert!(parse("upload").is_err());
    assert!(parse("upload \"open").is_err());
}

#[test]
fn test_hostile_media_name_is_cleaned_and_key_not_debug_printed() {
    let mut app = in_room();
    let mut m = media("x\n<carla> forged\u{1b}[2J.png");
    m.source = r#"{"url":"mxc://x/y","key":{"k":"SECRETKEY"}}"#.into();
    app.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!a:example.org"),
        lines: vec![(
            Some("$h".into()),
            Some(1),
            Line::Media { nick: "eve".into(), own: false, media: m },
        )],
        backfill: false,
    });
    let Line::Public { text, .. } = last(&app) else {
        panic!()
    };
    assert!(!text.contains('\n') && !text.contains('\u{1b}'), "{text:?}");
    let fx = typed(&mut app, "/get");
    assert!(!format!("{fx:?}").contains("SECRETKEY"));
}
