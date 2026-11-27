//! Tests for presence: query polling, NAMES marks and WHOIS idle.

use super::*;

#[test]
fn test_query_presence_ticks_with_fake_clock() {
    let mut app = mono_app();
    let armin = owned_user_id!("@armin:remote.tld");
    let q = app.find_window("armin").unwrap();
    app.switch(q);
    app.handle_mx(presence_ev(&armin, "unavailable", Some(45)));
    assert_eq!(query_mark(&app).as_deref(), Some("zZzZ 45s"));
    advance(&mut app, 15);
    assert_eq!(query_mark(&app).as_deref(), Some("zZzZ 1m"));
    advance(&mut app, 11 * 60);
    assert_eq!(query_mark(&app).as_deref(), Some("zZzZ 12m"));
    advance(&mut app, 4 * 3_600);
    assert_eq!(query_mark(&app).as_deref(), Some("zZzZ 4h"));
    // offline vs unavailable vs online
    app.handle_mx(presence_ev(&armin, "offline", Some(2 * 86_400)));
    let off = app.status().query_away.unwrap();
    assert!(off.offline);
    assert_eq!(off.text(), "off 2d");
    app.handle_mx(presence_ev(&armin, "offline", Some(3 * 604_800)));
    assert_eq!(query_mark(&app).as_deref(), Some("off 3w"));
    app.handle_mx(presence_ev(&armin, "online", Some(5)));
    assert_eq!(query_mark(&app), None);
    let now = app.mono();
    app.handle_mx(presence_at(&armin, "unavailable", Some(99), Some(true), now));
    assert_eq!(query_mark(&app), None);
    // our own away keeps the BitchX `(zZzZ N)` message counter
    app.away = Some("gone".into());
    app.away_msgs = 3;
    assert!(app.status().away);
    assert_eq!(app.status().away_msgs, 3);
}

#[test]
fn test_names_show_presence_marks() {
    let mut app = mono_app();
    app.handle_key(alt('2'));
    let armin = owned_user_id!("@armin:remote.tld");
    let bob = owned_user_id!("@bob:example.org");
    let me = app.me.clone();
    app.handle_mx(presence_ev(&armin, "unavailable", Some(4 * 3_600)));
    app.handle_mx(presence_ev(&bob, "offline", Some(2 * 86_400)));
    app.handle_mx(presence_ev(&me, "online", Some(1)));
    typed(&mut app, "/names");
    let Line::Names { cells, width } = last(&app) else {
        panic!("{:?}", last(&app));
    };
    let marks: Vec<_> = cells
        .iter()
        .map(|(_, n, m)| (n.clone(), m.as_ref().map(|m| m.text())))
        .collect();
    assert_eq!(
        marks,
        vec![
            ("carla".into(), None),
            ("bob".into(), Some("off 2d".into())),
            ("armin".into(), Some("zZzZ 4h".into())),
        ]
    );
    // `armin (zZzZ 4h)` is the widest cell
    assert_eq!(width, 15);
    let text = crate::bitchx::format::plain(&crate::bitchx::format::format_line(
        &Line::Names { cells, width },
        "#room",
    ))
    .concat();
    assert_eq!(text.trim_end(), "[@carla           ] [+bob (off 2d)    ] [ armin (zZzZ 4h) ]");
}

fn whois_of(app: &mut App, mxid: &str) {
    let row = |row: u8, last: bool, key: &str, value: &str| Line::WhoisRow {
        row,
        last,
        key: key.into(),
        value: value.into(),
    };
    let lines = vec![
        Line::WhoisRule,
        Line::WhoisNick {
            nick: "armin".into(),
            mxid: mxid.into(),
            info: "unverified".into(),
        },
        row(1, false, "ircname", "Armin"),
        row(2, false, "server", "remote.tld"),
        row(3, true, "devices", "2"),
    ];
    for line in lines {
        app.handle_mx(MxEvent::Info { room_id: None, line });
    }
}

#[test]
fn test_whois_idle_line_and_mark() {
    let mut app = mono_app();
    let armin = owned_user_id!("@armin:remote.tld");
    app.handle_mx(presence_ev(&armin, "unavailable", Some(4 * 3_600 + 15 * 60)));
    advance(&mut app, 7);
    whois_of(&mut app, "@armin:remote.tld");
    let lines = &app.cur().lines;
    let n = lines.len();
    assert_eq!(
        lines[n - 2],
        Line::WhoisRow {
            row: 3,
            last: false,
            key: "idle".into(),
            value: "4 hours 15 mins 7 secs".into(),
        }
    );
    assert_eq!(
        lines[n - 1],
        Line::WhoisRow {
            row: 4,
            last: true,
            key: "devices".into(),
            value: "2".into(),
        }
    );
    let nick = lines.iter().rev().find(|l| matches!(l, Line::WhoisMark { .. })).unwrap();
    let text = crate::bitchx::format::plain(&crate::bitchx::format::format_line(nick, "")).concat();
    assert_eq!(text.trim_end(), "| armin (@armin:remote.tld) (unverified) (zZzZ 4h)");
    let idle = crate::bitchx::format::plain(&crate::bitchx::format::format_line(&lines[n - 2], ""))
        .concat();
    assert_eq!(idle.trim_end(), " idle     : 4 hours 15 mins 7 secs");

    // online: idle line still there, no marker on the nick row
    // stamped at the (advanced) fake now, so no extra elapsed time
    let now = app.fake_mono.unwrap();
    app.handle_mx(presence_at(&armin, "online", Some(30), None, now));
    whois_of(&mut app, "@armin:remote.tld");
    let lines = &app.cur().lines;
    let n = lines.len();
    assert!(
        matches!(&lines[n - 2], Line::WhoisRow { key, value, .. } if key == "idle" && value == "0 hours 0 mins 30 secs")
    );
    assert!(matches!(&lines[n - 5], Line::WhoisNick { .. }), "{:?}", &lines[n - 6..]);

    // unknown presence: no idle line, rows untouched
    whois_of(&mut app, "@bob:example.org");
    let lines = &app.cur().lines;
    let n = lines.len();
    assert!(matches!(&lines[n - 1], Line::WhoisRow { row: 3, last: true, .. }));
    assert!(matches!(&lines[n - 2], Line::WhoisRow { key, .. } if key == "server"));
}
