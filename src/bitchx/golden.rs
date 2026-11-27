//! Golden snapshot of the default look: every [`Line`] variant formatted
//! (plain and as a mention) and whole screens rendered, each cell's text,
//! colours and modifiers written out. `testdata/golden-default.txt` was
//! generated from the code *before* colours moved into a palette, so this
//! test proves the default palette renders byte-identically.
//!
//! Regenerate (only when the default look is meant to change) with
//! `BX_GOLDEN_WRITE=1 cargo test golden`.
use std::fmt::Write as _;

use matrix_sdk::ruma::owned_room_id;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Style;
use ratatui::text::Line as TLine;

use crate::bitchx::app::tests::{alt, app};
use crate::bitchx::app::{App, Logo};
use crate::bitchx::format::format_line_hl;
use crate::bitchx::model::{Ink, Line, MxEvent, PresenceMark, Run};
use crate::bitchx::ui::draw;

fn s(v: &str) -> String {
    v.to_string()
}

/// One of every line kind.
fn every_line() -> Vec<Line> {
    let mark = |offline: bool| PresenceMark { offline, age: Some(s("4h")) };
    let inks = [
        Ink::Default,
        Ink::Black,
        Ink::Red,
        Ink::Blue,
        Ink::Magenta,
        Ink::Cyan,
        Ink::White,
    ];
    let runs = inks
        .iter()
        .enumerate()
        .map(|(i, ink)| Run {
            text: format!("r{i}"),
            fg: *ink,
            bg: inks[6 - i],
            bold: i % 2 == 0,
        })
        .collect();
    vec![
        Line::Public {
            nick: s("armin"),
            own: false,
            text: s("hello carla"),
        },
        Line::Public {
            nick: s("armin"),
            own: false,
            text: s("two\nlines"),
        },
        Line::Public { nick: s("carla"), own: true, text: s("hi") },
        Line::Action {
            nick: s("armin"),
            own: false,
            text: s("waves at carla"),
        },
        Line::Action { nick: s("carla"), own: true, text: s("waves") },
        Line::Private {
            nick: s("armin"),
            mxid: s("@armin:x"),
            own: false,
            text: s("psst carla"),
        },
        Line::Private {
            nick: s("armin"),
            mxid: s("@armin:x"),
            own: true,
            text: s("yes"),
        },
        Line::Notice {
            nick: s("bot"),
            mxid: s("@bot:x"),
            text: s("notice carla"),
        },
        Line::Join { nick: s("bob"), mxid: s("@bob:x") },
        Line::Part {
            nick: s("bob"),
            mxid: s("@bob:x"),
            reason: Some(s("bye")),
        },
        Line::Part { nick: s("bob"), mxid: s("@bob:x"), reason: None },
        Line::Kick {
            victim: s("bob"),
            kicker: s("armin"),
            reason: Some(s("spam")),
        },
        Line::Kick { victim: s("bob"), kicker: s("armin"), reason: None },
        Line::DisplayName { nick: s("bob"), name: Some(s("Bobby")) },
        Line::DisplayName { nick: s("bob"), name: None },
        Line::Topic { nick: Some(s("armin")), topic: s("new topic") },
        Line::Topic { nick: None, topic: s("old topic") },
        Line::Mode { modes: s("+o bob"), nick: s("armin") },
        Line::Invite { nick: s("armin"), target: s("bob") },
        Line::SetVar { name: s("BEEP"), value: Some(s("ON")) },
        Line::SetVar { name: s("AWAY_MSG"), value: None },
        Line::Info(s("info\nmore info")),
        Line::Error(s("broken")),
        Line::Undecryptable { nick: s("bob"), reason: s("no key") },
        Line::Sas { n: 1, emoji: s("*"), name: s("Dog") },
        Line::Raw(s("raw text")),
        Line::Logo(s("LOGO")),
        Line::LogoRuns(runs),
        Line::Users { chan: s("#room"), shown: 3, total: 4 },
        Line::Names {
            cells: vec![
                ('@', s("armin"), None),
                ('+', s("bob"), Some(mark(false))),
                (' ', s("carla"), Some(mark(true))),
                (' ', s("dave"), None),
            ],
            width: 6,
        },
        Line::WhoisRule,
        Line::WhoisNick { nick: s("bob"), mxid: s("@bob:x"), info: s("Bob") },
        Line::WhoisMark {
            nick: s("bob"),
            mxid: s("@bob:x"),
            info: s("Bob"),
            mark: mark(false),
        },
        Line::WhoisRow {
            row: 1,
            last: false,
            key: s("ircname"),
            value: s("Bob"),
        },
        Line::WhoisRow {
            row: 2,
            last: false,
            key: s("server"),
            value: s("x"),
        },
        Line::WhoisRow { row: 3, last: true, key: s("idle"), value: s("1m") },
        Line::Debug { cat: s("sync"), text: s("tick") },
        Line::Debug { cat: s(""), text: s("(suppressed 3)") },
    ]
}

fn style(st: Style) -> String {
    format!("{:?}/{:?}/{:?}/{:?}", st.fg, st.bg, st.add_modifier, st.sub_modifier)
}

fn dump_lines(out: &mut String, lines: &[TLine<'_>]) {
    for l in lines {
        let _ = write!(out, "  line {}:", style(l.style));
        for sp in &l.spans {
            let _ = write!(out, " [{} {:?}]", style(sp.style), sp.content);
        }
        out.push('\n');
    }
}

fn dump_screen(out: &mut String, title: &str, app: &mut App, w: u16, h: u16) {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| draw(f, app)).unwrap();
    let buf = term.backend().buffer().clone();
    let _ = writeln!(out, "== screen {title} {w}x{h}");
    for y in 0..h {
        let mut run_style = None;
        let mut text = String::new();
        let _ = write!(out, "{y:3}:");
        for x in 0..w {
            let c = &buf[(x, y)];
            let st = format!("{:?}/{:?}/{:?}", c.fg, c.bg, c.modifier);
            if run_style.as_ref() != Some(&st) {
                if let Some(prev) = run_style.take() {
                    let _ = write!(out, " <{prev}>{text:?}");
                }
                text.clear();
                run_style = Some(st);
            }
            text.push_str(c.symbol());
        }
        if let Some(prev) = run_style {
            let _ = write!(out, " <{prev}>{text:?}");
        }
        out.push('\n');
    }
}

/// The whole default look as text.
pub(crate) fn snapshot() -> String {
    let mut out = String::new();
    for line in every_line() {
        for hl in [false, true] {
            let _ = writeln!(out, "== {line:?} hl={hl}");
            dump_lines(&mut out, &format_line_hl(&line, "#room", hl));
        }
    }

    // A channel window: every line kind, a mention, timestamps, the top bar.
    let mut a = app();
    a.handle_key(alt('2'));
    a.vars.timestamp = true;
    a.vars.timestamp_string = "[ts] ".into();
    a.split = true;
    a.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!a:example.org"),
        lines: every_line().into_iter().map(|l| (None, Some(0), l)).collect(),
        backfill: false,
    });
    for c in "typed text".chars() {
        a.handle_key(crate::bitchx::app::tests::key(crossterm::event::KeyCode::Char(c)));
    }
    dump_screen(&mut out, "channel", &mut a, 100, 60);
    a.away = Some(s("gone"));
    a.sync_lost = true;
    a.handle_key(alt('3'));
    dump_screen(&mut out, "query-away", &mut a, 80, 12);

    // Each splash logo (the status window itself holds the pid and $TERM).
    for logo in [Logo::Original, Logo::Acid, Logo::None] {
        let _ = writeln!(out, "== logo {logo:?}");
        for line in logo.lines("0.0.0") {
            dump_lines(&mut out, &format_line_hl(&line, "", false));
        }
    }
    out
}

#[test]
fn test_default_palette_renders_like_before() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/bitchx/testdata/golden-default.txt");
    let now = snapshot();
    if std::env::var_os("BX_GOLDEN_WRITE").is_some() {
        std::fs::write(&path, &now).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap();
    for (i, (a, b)) in want.lines().zip(now.lines()).enumerate() {
        assert_eq!(a, b, "golden line {} differs", i + 1);
    }
    assert_eq!(want.lines().count(), now.lines().count());
}

/// `terminal` overrides no colour: every span and every screen cell keeps
/// the terminal's default foreground and background.
#[test]
fn test_terminal_theme_has_no_colours() {
    use crossterm::event::KeyCode;
    use ratatui::style::Color;

    use crate::bitchx::app::tests::{key, typed};
    use crate::bitchx::palette::{Palette, pal, set_palette};
    use crate::bitchx::theme::TERMINAL_THEME;

    let plain = |c: Option<Color>| matches!(c, None | Some(Color::Reset));
    let check_lines = |what: &str, lines: &[TLine<'_>]| {
        for l in lines {
            assert!(plain(l.style.fg) && plain(l.style.bg), "{what}: line {:?}", l.style);
            for sp in &l.spans {
                assert!(plain(sp.style.fg) && plain(sp.style.bg), "{what}: {sp:?}");
            }
        }
    };
    let check_screen = |what: &str, app: &mut App, w: u16, h: u16| {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        for y in 0..h {
            for x in 0..w {
                let c = &buf[(x, y)];
                assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset), "{what} ({x},{y})");
            }
        }
    };

    let mut a = app();
    // The picker previews `terminal` like any other theme (second entry).
    typed(&mut a, "/theme");
    a.handle_key(key(KeyCode::Down));
    assert_eq!(a.themes.list[a.picker.as_ref().unwrap().sel].name, TERMINAL_THEME);
    assert_eq!(pal(), Palette::terminal());

    for line in every_line() {
        for hl in [false, true] {
            check_lines(&format!("{line:?}"), &format_line_hl(&line, "#room", hl));
        }
    }
    for logo in [Logo::Original, Logo::Acid, Logo::None] {
        for line in logo.lines("0.0.0") {
            check_lines(&format!("{logo:?}"), &format_line_hl(&line, "", false));
        }
    }
    // Status window (splash), picker open, split top bar, timestamps.
    check_screen("status+picker", &mut a, 100, 40);
    a.handle_key(key(KeyCode::Enter));
    assert!(a.picker.is_none());
    assert_eq!(a.vars.theme, TERMINAL_THEME);
    a.handle_key(alt('2'));
    a.vars.timestamp = true;
    a.split = true;
    a.handle_mx(MxEvent::Lines {
        room_id: owned_room_id!("!a:example.org"),
        lines: every_line().into_iter().map(|l| (None, Some(0), l)).collect(),
        backfill: false,
    });
    for c in "typed".chars() {
        a.handle_key(key(KeyCode::Char(c)));
    }
    check_screen("channel", &mut a, 100, 60);
    a.away = Some(s("gone"));
    a.sync_lost = true;
    a.handle_key(alt('3'));
    check_screen("query-away", &mut a, 80, 12);
    set_palette(Palette::bitchx());
}
