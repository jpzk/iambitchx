//! Tests for the splash logos and startup banner.

use super::*;

fn line1(app: &App) -> String {
    app.status().line1().spans.iter().map(|s| s.content.to_string()).collect()
}

#[test]
fn test_status_window_shows_its_name() {
    let mut app = app();
    app.handle_key(alt('1'));
    assert_eq!(app.prompt(), "[status] ");
    let l = line1(&app);
    assert!(l.contains("] [status]"), "{l}");
    assert!(!l.contains("[]") && !l.contains("[0]"), "{l}");
}

#[test]
fn test_query_status_shows_plus_e() {
    let mut app = app();
    app.handle_key(alt('4'));
    let mut dm = room("!c:example.org", "armin");
    dm.dm_peer = Some(owned_user_id!("@armin:remote.tld"));
    dm.encrypted = true;
    app.handle_mx(MxEvent::Room(dm));
    let l = line1(&app);
    assert!(l.contains("[Query: armin(+e)]"), "{l}");
    assert!(!l.contains("[]"), "{l}");
}

#[test]
fn test_utf8_detection() {
    // TERM names that carry utf/256/color, or modern terminals: yes.
    for t in [
        "xterm-256color",
        "screen",
        "tmux-256color",
        "kitty",
        "alacritty",
        "foot",
        "st-256color",
    ] {
        assert!(utf8_supported(t, ""), "{t} should be utf8");
    }
    // Legacy/dumb terminals: no, whatever the locale says.
    for t in ["dumb", "vt100", "vt220", "ansi", "vt100-ansi"] {
        assert!(!utf8_supported(t, "en_US.UTF-8"), "{t} should be ascii");
    }
    // No TERM: the locale decides.
    assert!(utf8_supported("", "en_US.UTF-8"));
    assert!(utf8_supported("", "C.UTF-8"));
    assert!(!utf8_supported("", "C"));
    assert!(!utf8_supported("", ""));
    // An unknown TERM falls back to the locale.
    assert!(utf8_supported("linux", "en_US.UTF-8"));
    assert!(!utf8_supported("linux", "C"));
}

#[test]
fn test_startup_logos() {
    let text = |ls: &[Line]| {
        ls.iter()
            .map(|l| match l {
                Line::Logo(t) | Line::LogoDim(t) | Line::Raw(t) => t.clone(),
                Line::LogoRuns(runs) => {
                    runs.iter().map(|r| r.text.clone()).collect::<Vec<_>>().join("")
                },
                _ => String::new(),
            })
            .collect::<Vec<_>>()
    };
    let orig = text(&startup_lines_for("carla", Logo::Original));
    let acid = text(&startup_lines_for("carla", Logo::Acid));
    let compact = text(&startup_lines_for("carla", Logo::Compact));
    let slant = text(&startup_lines_for("carla", Logo::Slant));

    // The original BitchX art, decoded to real Unicode, with the artist
    // credits kept and the version slot filled in.
    assert!(orig.iter().any(|l| l.contains('\u{2584}')), "original art has blocks");
    assert!(orig.iter().any(|l| l.contains("PhonyEye")), "{orig:?}");
    assert!(orig.iter().any(|l| l.contains("Awe/Cia")), "{orig:?}");
    assert!(!orig.iter().any(|l| l.contains("{ver}")), "version slot filled");
    assert!(orig.iter().any(|l| l.contains(env!("CARGO_PKG_VERSION"))), "{orig:?}");

    // The art keeps the original's own colours: the 3D shading is in the
    // background colours, the strokes in the foreground ones.
    let runs: Vec<Run> = startup_lines_for("carla", Logo::Original)
        .iter()
        .filter_map(|l| match l {
            Line::LogoRuns(runs) => Some(runs.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(runs.iter().any(|r| r.fg == Ink::Magenta), "magenta strokes");
    assert!(runs.iter().any(|r| r.fg == Ink::Blue), "blue strokes");
    assert!(runs.iter().any(|r| r.bg == Ink::Magenta), "magenta shading");
    assert!(runs.iter().any(|r| r.bg == Ink::Blue), "blue shading");
    assert!(runs.iter().any(|r| r.bold), "bold strokes");

    // The acidjazz art is pure ASCII.
    assert!(acid.iter().any(|l| l.contains("-acidjazz")), "{acid:?}");
    assert!(!acid.iter().any(|l| l.contains('\u{2588}')), "ascii art has no blocks");

    // Our compact wordmark: full blocks plus a dither fade.
    assert!(compact.iter().any(|l| l.contains('\u{2588}')), "compact logo has blocks");
    assert!(compact.iter().any(|l| l.contains('\u{2591}')), "compact logo has a dither fade");

    // The ASCII fallback.
    assert!(slant.iter().any(|l| l.contains("____  _ __")), "slant art");
    assert!(!slant.iter().any(|l| l.contains('\u{2588}')), "slant has no blocks");

    // Every variant carries the iamb credit line.
    for l in [&orig, &acid, &compact, &slant] {
        assert!(l.iter().any(|x| x.contains("iamb.chat")), "{l:?}");
    }
    // The header says which logo was chosen.
    assert!(orig[3].contains("original bitchx logo"), "{:?}", orig[3]);
    assert!(acid[3].contains("acidjazz logo"), "{:?}", acid[3]);
    assert!(compact[3].contains("block logo"), "{:?}", compact[3]);
    assert!(slant[3].contains("ascii logo"), "{:?}", slant[3]);
}

#[test]
fn test_logo_pick() {
    // Room for the original art: 78 columns and ~36 rows.
    assert_eq!(Logo::pick(true, 100, 50), Logo::Original);
    assert_eq!(Logo::pick(false, 100, 50), Logo::Acid);
    // Too narrow or too short: the compact/ASCII variants.
    assert_eq!(Logo::pick(true, 60, 50), Logo::Compact);
    assert_eq!(Logo::pick(true, 100, 30), Logo::Compact);
    assert_eq!(Logo::pick(false, 60, 50), Logo::Slant);
    assert_eq!(Logo::pick(false, 100, 30), Logo::Slant);
}
