//! # The BitchX screen
//!
//! Scrollback from row 0 (row 1 under `/window split on`, which adds the
//! top bar), two blue status lines, the input line on the last row. No
//! borders, no box drawing.
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::text::{Line as TLine, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthChar;

use crate::bitchx::app::App;
use crate::bitchx::format::{format_line, n, sanitize, sp, wrap};
use crate::bitchx::status::status_bg;

/// `STATUS_FORMAT3`, the top bar shown with `/window split on` (capture 19).
pub const TOP_BAR: &str = "iamBitchX (BitchX by panasync)";

/// The scrollback rows of the current window that fit in `rows` x `width`,
/// honouring (and clamping) its scroll offset. Top-aligned when short.
pub fn scrollback(app: &mut App, width: usize, rows: usize) -> Vec<TLine<'static>> {
    let win = &app.windows[app.current];
    let chan = win.channel().unwrap_or("").to_string();
    let formatted: Vec<TLine<'static>> =
        win.lines.iter().flat_map(|l| format_line(l, &chan)).collect();
    let wrapped = wrap(formatted, width);

    let max_scroll = wrapped.len().saturating_sub(rows);
    let win = &mut app.windows[app.current];
    win.scroll = win.scroll.min(max_scroll);
    let end = wrapped.len() - win.scroll;
    let start = end.saturating_sub(rows);
    wrapped[start..end].to_vec()
}

/// Draw the whole screen.
pub fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();
    if area.height < 4 || area.width < 10 {
        return;
    }

    // `/window split on`: row 0 is the `STATUS_FORMAT3` top bar (capture 19),
    // bold white on blue, and the output starts one row lower.
    let top = u16::from(app.split && area.height > 4);
    if top == 1 {
        let bar =
            TLine::from(sp(TOP_BAR, status_bg().fg(Color::White).add_modifier(Modifier::BOLD)));
        frame.render_widget(
            Paragraph::new(bar).style(status_bg()),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }
    let rows = (area.height - 3 - top) as usize;
    app.page = rows.saturating_sub(1).max(1);
    app.cols = area.width as usize;

    let lines = scrollback(app, area.width as usize, rows);
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(area.x, area.y + top, area.width, rows as u16),
    );

    // Status lines: blue background across the full width.
    let st = app.status();
    let y1 = area.y + area.height - 3;
    let y2 = y1 + 1;
    let row1 = Rect::new(area.x, y1, area.width, 1);
    let row2 = Rect::new(area.x, y2, area.width, 1);
    frame.render_widget(Paragraph::new(st.line1()).style(status_bg()), row1);
    frame.render_widget(Paragraph::new(st.line2()).style(status_bg()), row2);
    let right = st.line2_right();
    let rw = (right.width() as u16).min(area.width);
    frame.render_widget(
        Paragraph::new(right).style(status_bg()),
        Rect::new(area.x + area.width - rw, y2, rw, 1),
    );

    // Input line: `[#channel] text`, scrolled horizontally to keep the
    // cursor visible.
    let prompt = sanitize(&app.prompt());
    let pw: usize = prompt.chars().map(|c| c.width().unwrap_or(0)).sum();
    let avail = (area.width as usize).saturating_sub(pw + 1).max(1);
    let chars = app.input.display_chars();
    let cursor = app.input.cursor();
    let widths: Vec<usize> = chars.iter().map(|c| c.width().unwrap_or(0)).collect();
    let mut start = 0;
    while widths[start..cursor].iter().sum::<usize>() > avail {
        start += 1;
    }
    let visible: String = chars[start..].iter().collect();
    let cx = pw + widths[start..cursor].iter().sum::<usize>();
    let input = TLine::from(vec![sp(prompt, n()), Span::raw(sanitize(&visible))]);
    let y3 = area.y + area.height - 1;
    frame.render_widget(Paragraph::new(input), Rect::new(area.x, y3, area.width, 1));
    frame.set_cursor_position(Position::new(area.x + (cx as u16).min(area.width - 1), y3));
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::owned_room_id;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Modifier};

    use super::*;
    use crate::bitchx::app::tests::{alt, app};
    use crate::bitchx::model::{Line, MxEvent};

    fn render(app: &mut App, w: u16, h: u16) -> Buffer {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        term.backend().buffer().clone()
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>()
    }

    fn rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height).map(|y| row(buf, y)).collect()
    }

    fn assert_no_box_glyphs(buf: &Buffer) {
        for r in rows(buf) {
            for c in r.chars() {
                assert!(!('\u{2500}'..='\u{259f}').contains(&c), "box glyph {c:?} in {r:?}");
            }
        }
    }

    fn assert_blue_row(buf: &Buffer, y: u16) {
        for x in 0..buf.area.width {
            assert_eq!(buf[(x, y)].bg, Color::Blue, "row {y} col {x} not blue");
        }
    }

    fn room_app() -> App {
        let mut app = app();
        app.handle_key(alt('2'));
        let room_id = owned_room_id!("!a:example.org");
        let lines = vec![
            Line::Join {
                nick: "armin".into(),
                mxid: "@armin:remote.tld".into(),
            },
            Line::Public {
                nick: "armin".into(),
                own: false,
                text: "sup".into(),
            },
            Line::Public {
                nick: "carla".into(),
                own: true,
                text: "hello".into(),
            },
            Line::Action {
                nick: "armin".into(),
                own: false,
                text: "waves".into(),
            },
            Line::Part {
                nick: "bob".into(),
                mxid: "@bob:example.org".into(),
                reason: None,
            },
        ];
        app.handle_mx(MxEvent::Lines {
            room_id,
            lines: lines.into_iter().map(|l| (None, l)).collect(),
            backfill: false,
        });
        app
    }

    #[test]
    fn test_room_window_layout() {
        let mut app = room_app();
        let buf = render(&mut app, 60, 12);
        let r = rows(&buf);

        assert_no_box_glyphs(&buf);
        // Scrollback starts at row 0.
        assert!(r[0].starts_with(" armin [@armin:remote.tld] has joined #room"), "{r:?}");
        assert!(r[1].starts_with("<armin> sup"));
        assert!(r[2].starts_with("<carla> hello"));
        assert!(r[3].starts_with(" armin/#room waves"));
        assert!(r[4].starts_with(" bob [@bob:example.org] has left #room []"));
        // Two blue status rows directly above the last row.
        assert_blue_row(&buf, 9);
        assert_blue_row(&buf, 10);
        assert!(r[9].starts_with(" [09:24pm][@carla] [#room]"), "{:?}", r[9]);
        assert!(r[10].starts_with(" [Lag ??] [O/1 N/2 I/0 V/1 F/0]"), "{:?}", r[10]);
        assert!(r[10].trim_end().ends_with("[U:a:S:b:h]"), "{:?}", r[10]);
        // The prompt on the last row, not blue.
        assert!(r[11].starts_with("[#room] "), "{:?}", r[11]);
        assert_ne!(buf[(0, 11)].bg, Color::Blue);
        // Capture 23: others' brackets bold blue, our own bold magenta.
        assert_eq!(buf[(0, 1)].fg, Color::Blue);
        assert_eq!(buf[(0, 2)].fg, Color::Magenta);
        // Join line: no `-:-`, nick bold cyan from column 1.
        assert_eq!(buf[(1, 0)].fg, Color::Cyan);
    }

    #[test]
    fn test_status_window_layout() {
        let mut app = app();
        let buf = render(&mut app, 60, 30);
        let r = rows(&buf);

        assert_no_box_glyphs(&buf);
        // Recon 00/01: version header first, then the logo.
        assert!(
            r[0].starts_with("iamBitchX - Matrix client in the style of BitchX by panasync"),
            "{:?}",
            r[0]
        );
        assert!(r[1].starts_with("Version (iambitchx-"), "{:?}", r[1]);
        assert!(r[2].starts_with("Process ["), "{:?}", r[2]);
        assert!(r[3].starts_with("Using terminal type ["), "{:?}", r[3]);
        // Slanted iamBitchX logo, verbatim, full word including the X.
        for (i, l) in crate::bitchx::app::BANNER.iter().enumerate() {
            assert!(l.len() <= 70);
            assert!(r[5 + i].starts_with(&format!("    {l}")), "{:?}", r[5 + i]);
        }
        assert!(r[9].trim_end().ends_with("/_/|_|"), "X glyph: {:?}", r[9]);
        assert_eq!(buf[(8, 5)].fg, Color::Blue);
        assert!(buf[(8, 5)].modifier.contains(Modifier::ITALIC));
        assert!(r[10].starts_with("    [ iamBitchX-"), "{:?}", r[10]);
        assert!(
            r.iter()
                .any(|l| l.starts_with(" iamBitchX: Auto Response is set to - carla"))
        );
        // Room list numbered by window, server-list style.
        assert!(r.iter().any(|l| l.starts_with("      2) #room [")), "{r:?}");
        assert!(
            r.iter()
                .any(|l| l.starts_with("      4) armin [query with @armin:remote.tld]"))
        );
        assert_blue_row(&buf, 27);
        assert_blue_row(&buf, 28);
        assert!(r[27].starts_with(" [09:24pm][carla] []"), "{:?}", r[27]);
        assert!(r[28].starts_with(" [Lag ??]"), "{:?}", r[28]);
        assert!(r[28].trim_end().ends_with("[::::]"), "{:?}", r[28]);
        // Recon 02: the idle prompt is `[0]`.
        assert!(r[29].starts_with("[0] "), "{:?}", r[29]);
    }

    /// IRC vocabulary: Matrix rooms are channels everywhere the user can see.
    #[test]
    fn test_status_window_says_channel_never_room() {
        use crate::bitchx::app::tests::{room, typed};
        use crate::bitchx::channels::ListEntry;

        let mut app = App::new(matrix_sdk::ruma::owned_user_id!("@carla:example.org"));
        app.clock = Some("09:24pm".into());
        app.channel_list_header("https://matrix.example.org/");
        app.handle_mx(MxEvent::Room(room("!a:example.org", "#lobby")));
        app.handle_mx(MxEvent::Room(room("!b:example.org", "#rust:matrix.org")));
        let mut dm = room("!c:example.org", "armin");
        dm.dm_peer = Some(matrix_sdk::ruma::owned_user_id!("@armin:remote.tld"));
        app.handle_mx(MxEvent::Room(dm));
        typed(&mut app, "/channels");
        typed(&mut app, "/rooms");
        typed(&mut app, "/help");
        typed(&mut app, "/join @bob:x");
        typed(&mut app, "/part");
        typed(&mut app, "/list -s");
        let entries = vec![ListEntry {
            alias: Some("#welcome:example.org".into()),
            room_id: "!w:example.org".into(),
            members: 9,
            topic: Some("say hi".into()),
            ..Default::default()
        }];
        let server = "example.org".to_string();
        app.handle_mx(MxEvent::List { server, search: None, entries });
        assert_eq!(app.current, 0);

        let buf = render(&mut app, 100, 80);
        let text = rows(&buf).join("\n");
        assert!(text.contains("Channel list:"), "{text}");
        assert!(text.contains("Listing public channels on example.org"), "{text}");
        assert!(text.contains("#welcome:example.org     9  say hi"), "{text}");
        assert!(text.contains("End of /LIST"), "{text}");
        assert!(text.contains("/channels"), "{text}");
        assert!(!text.to_lowercase().contains("room"), "{text}");
    }

    /// `%T` is a 12-hour `hh:mmam`/`hh:mmpm` clock (recon 02, 22).
    fn is_bx_clock(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() == 7 &&
            b[..2].iter().all(u8::is_ascii_digit) &&
            b[2] == b':' &&
            b[3..5].iter().all(u8::is_ascii_digit) &&
            (s.ends_with("am") || s.ends_with("pm"))
    }

    #[test]
    fn test_status_clock_is_12h_am_pm() {
        let mut app = room_app();
        app.clock = None;
        let buf = render(&mut app, 60, 12);
        let r = row(&buf, 9);
        assert!(is_bx_clock(&r[2..9]), "{r:?}");
        assert_eq!(&r[..2], " [");
        assert_eq!(&r[9..11], "][");

        use chrono::NaiveTime;
        let t = |h, m| crate::bitchx::status::clock(NaiveTime::from_hms_opt(h, m, 0).unwrap());
        assert_eq!(t(21, 24), "09:24pm");
        assert_eq!(t(9, 5), "09:05am");
        assert_eq!(t(0, 0), "12:00am");
        assert_eq!(t(12, 30), "12:30pm");
    }

    #[test]
    fn test_status_field_order() {
        // Line 1: ` [time][@nick] [#chan(+modes)]`, line 2: ` [Lag n] [O/ N/ I/ V/ F/]`.
        let mut app = room_app();
        let mut snap = crate::bitchx::app::tests::room("!a:example.org", "#bxrecon-0122");
        snap.invite_only = true;
        app.handle_mx(MxEvent::Room(snap));
        let buf = render(&mut app, 80, 12);
        let l1 = row(&buf, 9);
        let l2 = row(&buf, 10);
        assert!(l1.starts_with(" [09:24pm][@carla] [#bxrecon-0(+i)]"), "{l1:?}");
        let pos = |s: &str, n: &str| s.find(n).unwrap_or_else(|| panic!("{n} not in {s:?}"));
        assert!(pos(&l1, "pm]") < pos(&l1, "carla") && pos(&l1, "carla") < pos(&l1, "#bxrecon"));
        assert!(pos(&l2, "[Lag") < pos(&l2, "[O/"), "{l2:?}");
        // Per-field colours from 22-channel-join.ansi.
        let st = |x: u16| buf[(x, 9)].clone();
        assert_eq!(st(1).fg, Color::Cyan); // [
        assert_eq!(st(2).fg, Color::White); // clock, bold
        assert!(st(2).modifier.contains(ratatui::style::Modifier::BOLD));
        assert_eq!(st(11).fg, Color::White); // @ bold
        assert_eq!(st(12).fg, Color::Gray); // nick, plain white
        assert!(!st(12).modifier.contains(ratatui::style::Modifier::BOLD));

        // A DM window: `[]` for the channel, then `[Query: nick]`.
        app.handle_key(alt('4'));
        let buf = render(&mut app, 80, 12);
        let l1 = row(&buf, 9);
        assert!(l1.starts_with(" [09:24pm][@carla] []  [Query: armin]"), "{l1:?}");
    }

    #[test]
    fn test_window_split_top_bar() {
        // Capture 19: `/window split on` puts TOP_BAR (bold white
        // on blue) on row 0 and pushes the output down one row.
        let mut app = room_app();
        let before = render(&mut app, 80, 12);
        assert!(!row(&before, 0).starts_with(TOP_BAR));
        let first = row(&before, 0);

        app.submit("/window split on");
        assert!(app.split);
        let buf = render(&mut app, 80, 12);
        assert_eq!(row(&buf, 0).trim_end(), TOP_BAR);
        assert_eq!(buf[(0, 0)].bg, Color::Blue);
        assert_eq!(buf[(79, 0)].bg, Color::Blue);
        assert_eq!(buf[(0, 0)].fg, Color::White);
        assert!(buf[(0, 0)].modifier.contains(ratatui::style::Modifier::BOLD));
        assert_eq!(row(&buf, 1), first);
        // The status lines and input stay at the bottom.
        assert!(row(&buf, 9).starts_with(" [09:24pm]"), "{:?}", row(&buf, 9));
        assert!(row(&buf, 10).starts_with(" [Lag ??]"));
        assert!(rows(&buf).iter().any(|r| r.contains("Window SPLIT is ON")));

        app.submit("/window split toggle");
        assert!(!app.split);
        assert!(!row(&render(&mut app, 80, 12), 0).starts_with(TOP_BAR));
    }

    #[test]
    fn test_status_line2_lag_and_flags() {
        // Recon 21/22: `[Lag ??]` until the first sample, then `[Lag  n]`;
        // right-aligned cyan `[U:a:S:b:h]` in a channel, `[::::]` elsewhere.
        let mut app = room_app();
        let buf = render(&mut app, 80, 12);
        let l2 = row(&buf, 10);
        assert!(l2.starts_with(" [Lag ??] [O/"), "{l2:?}");
        assert!(l2.ends_with("[U:a:S:b:h] "), "{l2:?}");
        let x = l2.find("[U:a").unwrap() as u16;
        for dx in 0..11 {
            assert_eq!(buf[(x + dx, 10)].fg, Color::Cyan);
            assert_eq!(buf[(x + dx, 10)].bg, Color::Blue);
        }
        // `Lag` white, value bold white.
        assert_eq!(buf[(2, 10)].fg, Color::Gray);
        assert!(buf[(6, 10)].modifier.contains(ratatui::style::Modifier::BOLD));

        app.handle_mx(MxEvent::Lag(0));
        let buf = render(&mut app, 80, 12);
        assert!(row(&buf, 10).starts_with(" [Lag  0] [O/"), "{:?}", row(&buf, 10));
        app.handle_mx(MxEvent::Lag(12));
        let buf = render(&mut app, 80, 12);
        assert!(row(&buf, 10).starts_with(" [Lag 12] [O/"));

        // Status window: no counters, `[::::]`.
        app.handle_key(alt('1'));
        let buf = render(&mut app, 80, 12);
        let l2 = row(&buf, 10);
        assert!(!l2.contains("[O/"), "{l2:?}");
        assert!(l2.trim_end().ends_with("[::::]"), "{l2:?}");
    }

    #[test]
    fn test_act_in_status_line() {
        let mut app = app();
        app.handle_mx(MxEvent::Lines {
            room_id: owned_room_id!("!b:example.org"),
            lines: vec![(None, Line::Public { nick: "x".into(), own: false, text: "y".into() })],
            backfill: false,
        });
        let buf = render(&mut app, 70, 8);
        assert!(row(&buf, 5).contains("[Act: 3]"), "{:?}", row(&buf, 5));
    }

    #[test]
    fn test_scrollback_paging_and_input_scroll() {
        let mut app = room_app();
        for i in 0..30 {
            app.handle_mx(MxEvent::Lines {
                room_id: owned_room_id!("!a:example.org"),
                lines: vec![(None, Line::Info(format!("line {i}")))],
                backfill: false,
            });
        }
        let buf = render(&mut app, 40, 8);
        assert!(row(&buf, 4).starts_with(" line 29"));
        app.handle_key(crate::bitchx::app::tests::key(crossterm::event::KeyCode::PageUp));
        let buf = render(&mut app, 40, 8);
        assert!(row(&buf, 4).starts_with(" line 25"), "{:?}", row(&buf, 4));
        // Scrolling far past the top clamps.
        app.cur_scroll_for_test(1000);
        let buf = render(&mut app, 40, 8);
        assert!(row(&buf, 0).starts_with(" armin"), "{:?}", row(&buf, 0));

        // A long input line keeps the cursor visible.
        app.input.set_text(&"x".repeat(100));
        let buf = render(&mut app, 40, 8);
        assert!(row(&buf, 7).starts_with("[#room] xxx"));
    }

    #[test]
    fn test_recover_key_never_on_screen() {
        use crossterm::event::KeyCode;

        use crate::bitchx::app::tests::key;

        let mut app = room_app();
        let secret = "EsTc 9xQ2 kLm7";
        for c in format!("/recover {secret}").chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        let screen = rows(&render(&mut app, 80, 24)).join("\n");
        assert!(screen.contains("/recover **************"), "{screen}");
        assert!(!screen.contains("EsTc") && !screen.contains("kLm7"));
        app.handle_key(key(KeyCode::Enter));
        app.handle_key(key(KeyCode::Up));
        let screen = rows(&render(&mut app, 80, 24)).join("\n");
        assert!(screen.contains("/recover ********"), "{screen}");
        assert!(!screen.contains("EsTc") && !screen.contains("kLm7"), "{screen}");
    }

    #[test]
    fn test_status_bar_shows_plus_e() {
        let mut app = room_app();
        let mut snap = crate::bitchx::app::tests::room("!a:example.org", "#room");
        snap.encrypted = true;
        app.handle_mx(MxEvent::Room(snap));
        let screen = rows(&render(&mut app, 80, 24));
        assert!(screen[21].contains("[#room(+e)]"), "{}", screen[21]);
    }
}
