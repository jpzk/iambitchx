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
                Line::LogoRuns(runs) => runs.iter().map(|r| r.text.clone()).collect::<String>(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
    };
    let orig = text(&startup_lines_for("carla", Logo::Original));
    let acid = text(&startup_lines_for("carla", Logo::Acid));
    let none = text(&startup_lines_for("carla", Logo::None));

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

    // No logo: no art lines at all, the credit line follows the iamb line.
    assert!(Logo::None.lines("0.0.0").is_empty());
    assert!(none[LOGO_START - 1].contains("iamb.chat"), "{none:?}");
    assert!(none[LOGO_START].starts_with("    [ iamBitchX-"), "{none:?}");

    // Every variant carries the iamb credit line and the version credit.
    for l in [&orig, &acid, &none] {
        assert!(l.iter().any(|x| x.contains("iamb.chat")), "{l:?}");
        assert!(
            l.iter()
                .any(|x| x.starts_with(&format!("    [ iamBitchX-{}]", env!("CARGO_PKG_VERSION")))),
            "{l:?}"
        );
    }
    // The header says which logo was chosen.
    assert!(orig[3].contains("original bitchx logo"), "{:?}", orig[3]);
    assert!(acid[3].contains("acidjazz logo"), "{:?}", acid[3]);
    assert!(none[3].contains("no logo"), "{:?}", none[3]);
}

#[test]
fn test_logo_pick() {
    // Room for the original art: 78 columns and ~36 rows.
    assert_eq!(Logo::pick(true, 100, 50), Logo::Original);
    assert_eq!(Logo::pick(false, 100, 50), Logo::Acid);
    assert_eq!(Logo::pick(true, 78, 36), Logo::Original);
    assert_eq!(Logo::pick(false, 78, 36), Logo::Acid);
    // Too narrow (<78) or too short (<36): no logo at all.
    for utf8 in [true, false] {
        assert_eq!(Logo::pick(utf8, 77, 50), Logo::None);
        assert_eq!(Logo::pick(utf8, 100, 35), Logo::None);
        assert_eq!(Logo::pick(utf8, 60, 30), Logo::None);
        assert_eq!(Logo::pick(utf8, 0, 0), Logo::None);
    }
}

/// The text of every line in the status window.
fn status_text(app: &App) -> Vec<String> {
    app.windows[0]
        .lines
        .iter()
        .map(|l| match l {
            Line::Logo(t) | Line::LogoDim(t) | Line::Raw(t) | Line::Error(t) => t.clone(),
            Line::LogoRuns(runs) => runs.iter().map(|r| r.text.clone()).collect::<String>(),
            _ => String::new(),
        })
        .collect()
}

/// Resizing big -> small drops the logo, small -> big brings the original
/// back; the iamb and version credit lines survive every swap, exactly once.
#[test]
fn test_fit_logo_resize() {
    let ver_credit = format!("    [ iamBitchX-{}]", env!("CARGO_PKG_VERSION"));
    let check = |app: &App, logo: Logo, what: &str| {
        let t = status_text(app);
        let art = logo.lines(env!("CARGO_PKG_VERSION")).len();
        assert_eq!(app.logo, logo, "{what}");
        assert!(t[3].contains(logo.label()), "{what}: {:?}", t[3]);
        assert!(t[LOGO_START - 1].contains("iamb.chat"), "{what}: {t:?}");
        assert!(t[LOGO_START + art].starts_with(&ver_credit), "{what}: {t:?}");
        assert_eq!(t.iter().filter(|l| l.starts_with(&ver_credit)).count(), 1, "{what}");
        assert_eq!(t.iter().filter(|l| l.contains("iamb.chat")).count(), 1, "{what}");
        assert_eq!(
            t.iter().filter(|l| l.contains("PhonyEye")).count(),
            usize::from(logo == Logo::Original),
            "{what}"
        );
        assert_eq!(
            t.iter().filter(|l| l.contains("-acidjazz")).count(),
            usize::from(logo == Logo::Acid),
            "{what}"
        );
        assert_eq!(app.windows[0].lines.len(), app.windows[0].times.len(), "{what}");
    };

    // Start the way the client does: size unknown, so no logo.
    let mut app = App::new_with_logo(owned_user_id!("@carla:example.org"), Logo::pick(true, 0, 0));
    check(&app, Logo::None, "startup");
    let base = app.windows[0].lines.len();
    app.fit_logo(true, 120, 50);
    check(&app, Logo::Original, "small -> big");
    app.fit_logo(true, 60, 50);
    check(&app, Logo::None, "big -> narrow");
    assert_eq!(app.windows[0].lines.len(), base);
    app.fit_logo(true, 120, 50);
    check(&app, Logo::Original, "narrow -> big");
    app.fit_logo(true, 120, 30);
    check(&app, Logo::None, "big -> short");
    app.fit_logo(false, 100, 40);
    check(&app, Logo::Acid, "short -> big ascii");
    app.fit_logo(false, 100, 20);
    check(&app, Logo::None, "big ascii -> short");

    // Starting big (an explicit original) and shrinking also drops it.
    let mut app = App::new_with_logo(owned_user_id!("@carla:example.org"), Logo::Original);
    check(&app, Logo::Original, "start big");
    app.fit_logo(true, 40, 20);
    check(&app, Logo::None, "start big -> small");
    app.fit_logo(true, 80, 40);
    check(&app, Logo::Original, "start big -> small -> big");
}
