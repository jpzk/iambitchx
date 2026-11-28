//! `/lastlog` and `/set LOG`.
use std::path::{Path, PathBuf};

use chrono::{Local, TimeZone as _};

use super::*;

fn ts(h: u32, m: u32) -> i64 {
    Local.with_ymd_and_hms(2026, 11, 28, h, m, 0).unwrap().timestamp_millis()
}

fn say(app: &mut App, room: &str, id: &str, at: i64, nick: &str, text: &str) {
    let line = Line::Public { nick: nick.into(), own: false, text: text.into() };
    app.handle_mx(MxEvent::Lines {
        room_id: OwnedRoomId::try_from(room).unwrap(),
        lines: vec![(Some(id.into()), Some(at), line)],
        backfill: false,
    });
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("iambitchx-applog-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn test_lastlog_prints_matches_with_their_time() {
    let mut app = app();
    app.handle_key(alt('2'));
    say(&mut app, "!a:example.org", "$1", ts(9, 1), "armin", "the Needle is here");
    say(&mut app, "!a:example.org", "$2", ts(9, 2), "bob", "nothing");
    say(&mut app, "!a:example.org", "$3", ts(9, 3), "bob", "needle again");
    // case-sensitive by default
    typed(&mut app, "/lastlog Needle");
    let hits: Vec<i64> = app
        .cur()
        .lines
        .iter()
        .filter_map(|l| match l {
            Line::Lastlog { ts, .. } => Some(*ts),
            _ => None,
        })
        .collect();
    assert_eq!(hits, vec![ts(9, 1)]);
    // -i: any case; earlier output is not searched again
    typed(&mut app, "/lastlog -i needle");
    let out = rows(&mut app);
    let n = out.len();
    let stamp = app.vars.stamp_any(ts(9, 1)).unwrap();
    let stamp3 = app.vars.stamp_any(ts(9, 3)).unwrap();
    assert_eq!(
        out[n - 4..],
        [
            "Lastlog:".to_string(),
            format!("{stamp}<armin> the Needle is here"),
            format!("{stamp3}<bob> needle again"),
            "End of Lastlog".to_string(),
        ],
        "{out:#?}"
    );
    // TIMESTAMP is off: normal lines carry no stamp
    assert!(out.contains(&"<bob> nothing".to_string()));

    typed(&mut app, "/lastlog -clear");
    assert!(!app.cur().lines.iter().any(crate::bitchx::lastlog::is_output));
    assert_eq!(app.cur().lines.len(), app.cur().times.len());
    assert!(
        app.cur()
            .lines
            .iter()
            .any(|l| matches!(l, Line::Public { text, .. } if text == "nothing"))
    );
}

#[test]
fn test_lastlog_parse_errors() {
    use crate::bitchx::commands::{BitchxCommand, parse};
    assert_eq!(
        parse("lastlog -I foo  bar").unwrap(),
        Some(BitchxCommand::Lastlog { pattern: "foo  bar".into(), ignore_case: true })
    );
    assert_eq!(
        parse("lastlog -- -i").unwrap(),
        Some(BitchxCommand::Lastlog { pattern: "-i".into(), ignore_case: false })
    );
    assert_eq!(parse("lastlog -clear").unwrap(), Some(BitchxCommand::LastlogClear));
    assert!(parse("lastlog").is_err());
    assert!(parse("lastlog -i").is_err());
    assert!(parse("lastlog -clear x").is_err());
}

fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
#[test]
fn test_log_on_writes_window_files() {
    let base = tmp("on");
    let mut app = app();
    app.log_default_dir = Some(base.join("default"));
    // off by default: nothing is written, and the queue is dropped
    assert!(!app.vars.log);
    say(&mut app, "!a:example.org", "$0", ts(8, 0), "armin", "before");
    app.flush_log();
    assert!(!base.exists());

    let dir = base.join("logs");
    typed(&mut app, &format!("/set LOG_DIR {}", dir.display()));
    typed(&mut app, "/set LOG ON");
    say(&mut app, "!a:example.org", "$1", ts(10, 5), "armin", "hello #room");
    say(&mut app, "!c:example.org", "$2", ts(10, 6), "armin", "psst");
    app.windows[0].push(None, Line::Debug { cat: "http".into(), text: "GET /sync token".into() });
    app.flush_log();

    let chan = dir.join("example.org").join("#room.log");
    let text = std::fs::read_to_string(&chan).unwrap();
    assert!(text.starts_with("--- Log opened "), "{text}");
    assert!(text.contains("10:05 <armin> hello #room\n"), "{text}");
    assert!(!text.contains("before"), "{text}");
    let dm = std::fs::read_to_string(dir.join("example.org").join("armin.log")).unwrap();
    assert!(dm.contains("10:06 [armin(@armin:remote.tld)] psst\n"), "{dm}");
    let status = std::fs::read_to_string(dir.join("example.org").join("status.log")).unwrap();
    assert!(status.contains("Value of LOG set to ON"), "{status}");
    assert!(!status.contains("DBG") && !status.contains("GET /sync"), "{status}");
    assert_eq!(mode(&chan), 0o600);
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join("example.org")), 0o700);

    // /lastlog output is not logged again
    app.handle_key(alt('2'));
    typed(&mut app, "/lastlog hello");
    app.flush_log();
    let text = std::fs::read_to_string(&chan).unwrap();
    assert_eq!(text.matches("hello #room").count(), 1, "{text}");
    assert!(!text.contains("Lastlog"), "{text}");

    // OFF closes the files; later lines are not written
    typed(&mut app, "/set LOG OFF");
    app.flush_log();
    assert!(std::fs::read_to_string(&chan).unwrap().contains("--- Log closed "));
    say(&mut app, "!a:example.org", "$3", ts(10, 7), "armin", "after off");
    app.flush_log();
    assert!(!std::fs::read_to_string(&chan).unwrap().contains("after off"));
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn test_log_dir_default_and_validation() {
    let base = tmp("default");
    let mut app = app();
    app.log_default_dir = Some(base.clone());
    typed(&mut app, "/set LOG_DIR relative/dir");
    assert!(app.vars.log_dir.is_empty());
    assert!(matches!(last(&app), Line::Info(t) if t.contains("absolute")));
    typed(&mut app, "/set LOG ON");
    say(&mut app, "!b:example.org", "$1", ts(11, 0), "bob", "x");
    app.flush_log();
    assert!(base.join("example.org").join("#other.log").exists());
    app.close_logs();
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn test_log_without_dir_reports() {
    let mut app = app();
    app.log_default_dir = None;
    typed(&mut app, "/set LOG ON");
    assert!(
        app.cur()
            .lines
            .iter()
            .any(|l| matches!(l, Line::Error(t) if t.contains("LOG_DIR")))
    );
    app.flush_log();
}
