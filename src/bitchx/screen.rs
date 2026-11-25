//! # BitchX screen
//!
//! Draws the BitchX layout: a blue status bar on the first row, the focused
//! room's chat (the classic UI's own [Chat](crate::windows::room::Chat) widget, so the
//! message formatting and the message bar stay identical) below it, with the
//! chat's message bar ending up on the last row. There is no tab bar, no
//! window border and no `:` command bar.
use crate::bitchx::status::StatusBar;
use crate::prelude::*;
use crate::windows::room::ChatState;

/// Style of the status bar: white, bold text on blue, like BitchX's default.
pub fn status_style() -> Style {
    Style::default()
        .fg(Color::White)
        .bg(Color::Blue)
        .add_modifier(StyleModifier::BOLD)
}

/// Style of the status bar's `[`, `]`, `@`, `/` and `+` punctuation.
pub fn status_punct_style() -> Style {
    status_style().fg(Color::Cyan)
}

/// BitchX's `-:-` notice prefix: bold blue dashes around a cyan colon.
pub fn notice_prefix() -> Vec<Span<'static>> {
    let dash = Style::default().fg(Color::LightBlue).add_modifier(StyleModifier::BOLD);
    let colon = Style::default().fg(Color::Cyan);

    vec![
        Span::styled("-", dash),
        Span::styled(":", colon),
        Span::styled("-", dash),
        Span::raw(" "),
    ]
}

/// Restyle a rendered [StatusBar] line in BitchX colours.
fn status_line(status: &StatusBar, width: u16) -> Line<'static> {
    let text: String = status.render(width).spans.iter().map(|s| s.content.as_ref()).collect();
    let body = status_style();
    let punct = status_punct_style();

    // Only the `[nick@host/channel +modes]` part gets coloured punctuation; the
    // topic after the closing bracket is shown verbatim.
    let mut spans = Vec::new();
    let mut run = String::new();
    let mut in_head = true;

    for c in text.chars() {
        if in_head && matches!(c, '[' | ']' | '@' | '/' | '+') {
            if !run.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut run), body));
            }
            spans.push(Span::styled(c.to_string(), punct));
            in_head = c != ']';
        } else {
            run.push(c);
        }
    }

    if !run.is_empty() {
        spans.push(Span::styled(run, body));
    }

    Line::from(spans).style(body)
}

/// The BitchX layout around a body widget (the classic `Chat` in the application).
pub struct BitchxScreen<W> {
    status: StatusBar,
    notice: Option<(String, Style)>,
    body: W,
}

impl<W> BitchxScreen<W> {
    pub fn new(status: StatusBar, body: W) -> Self {
        BitchxScreen { status, notice: None, body }
    }

    /// Show a `-:- message` notice (errors, info) under the status bar.
    pub fn notice(mut self, notice: Option<(String, Style)>) -> Self {
        self.notice = notice;
        self
    }
}

impl<W: StatefulWidget> StatefulWidget for BitchxScreen<W> {
    type State = W::State;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Row 0: status bar, filled to the full width in the status colours.
        let statusarea = Rect::new(area.x, area.y, area.width, 1);
        buf.set_style(statusarea, status_style());
        Paragraph::new(status_line(&self.status, area.width)).render(statusarea, buf);

        let mut body = Rect::new(area.x, area.y + 1, area.width, area.height - 1);

        // Optional notice row, only when it leaves room for the input line.
        if let Some((msg, style)) = self.notice &&
            body.height >= 2
        {
            let msg: String = msg.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
            let mut spans = notice_prefix();
            spans.push(Span::styled(msg, style));

            let noticearea = Rect::new(body.x, body.y, body.width, 1);
            Paragraph::new(Line::from(spans)).render(noticearea, buf);

            body.y += 1;
            body.height -= 1;
        }

        // Remaining rows: the chat, whose message bar sits on the last row.
        if body.height > 0 {
            self.body.render(body, buf, state);
        }
    }
}

/// Whether the BitchX screen applies to this window: only joined rooms.
pub fn bitchx_chat(win: &mut crate::windows::IambWindow) -> Option<&mut ChatState> {
    use crate::windows::room::RoomState;

    match win {
        crate::windows::IambWindow::Room(RoomState::Chat(chat)) => Some(chat.as_mut()),
        _ => None,
    }
}

/// Build the status bar for a joined room: `[nick@server/#room +e] topic`.
pub fn room_status(chat: &ChatState, store: &ProgramStore) -> StatusBar {
    let settings = &store.application.settings;
    let user = &settings.user_id;
    let room = chat.room();

    let channel = room
        .canonical_alias()
        .map(|a| a.to_string())
        .unwrap_or_else(|| store.application.get_room_title(chat.id()));
    let modes = if room.encryption_state().is_encrypted() {
        vec!["e".to_string()]
    } else {
        vec![]
    };

    StatusBar {
        nick: user.localpart().to_string(),
        host: user.server_name().to_string(),
        channel,
        modes,
        topic: room.topic().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    /// Stands in for the classic `Chat`: scrollback at the top, `> input` at the bottom.
    struct FakeChat;

    impl StatefulWidget for FakeChat {
        type State = String;

        fn render(self, area: Rect, buf: &mut Buffer, state: &mut String) {
            let top = Rect::new(area.x, area.y, area.width, 1);
            Paragraph::new("<bob> hi there").render(top, buf);

            let bottom = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
            Paragraph::new(format!("> {state}")).render(bottom, buf);
        }
    }

    fn status() -> StatusBar {
        StatusBar {
            nick: "alice".into(),
            host: "matrix.org".into(),
            channel: "#rust".into(),
            modes: vec!["e".into()],
            topic: "Rust talk".into(),
        }
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    fn draw(notice: Option<(String, Style)>, w: u16, h: u16) -> Buffer {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut input = String::from("hello world");

        term.draw(|f| {
            let screen = BitchxScreen::new(status(), FakeChat).notice(notice.clone());
            f.render_stateful_widget(screen, f.area(), &mut input);
        })
        .unwrap();

        term.backend().buffer().clone()
    }

    #[test]
    fn test_bitchx_screen_layout() {
        let buf = draw(None, 50, 8);

        let first = row(&buf, 0);
        assert!(first.starts_with("[alice@matrix.org/#rust +e] Rust talk"), "{first:?}");
        assert!(row(&buf, 1).starts_with("<bob> hi there"));
        assert!(row(&buf, 7).starts_with("> hello world"));

        // The whole status row is blue with bold white text; brackets are cyan.
        for x in 0..buf.area.width {
            assert_eq!(buf[(x, 0)].bg, Color::Blue);
            assert!(buf[(x, 0)].modifier.contains(StyleModifier::BOLD));
        }
        assert_eq!(buf[(0, 0)].fg, Color::Cyan);
        assert_eq!(buf[(1, 0)].fg, Color::White);
        assert_eq!(buf[(1, 1)].fg, Color::Reset);

        // No tab bar, window borders or command bar anywhere.
        let all: String = (0..buf.area.height).map(|y| row(&buf, y)).collect();
        for glyph in [
            '│', '─', '┌', '┐', '└', '┘', '╭', '╮', '╰', '╯', '┃', '━', '║', '═',
        ] {
            assert!(!all.contains(glyph), "found border glyph {glyph:?}");
        }
        assert!(!row(&buf, 7).starts_with(':'));
        assert!(!all.contains("-- INSERT --"));
    }

    #[test]
    fn test_bitchx_screen_notice() {
        let err = Style::default().fg(Color::Red);
        let buf = draw(Some(("No such room".into(), err)), 40, 6);

        assert!(row(&buf, 0).starts_with("[alice@matrix.org/#rust +e]"));
        assert!(row(&buf, 1).starts_with("-:- No such room"));
        assert_eq!(buf[(0, 1)].fg, Color::LightBlue);
        assert_eq!(buf[(1, 1)].fg, Color::Cyan);
        assert_eq!(buf[(4, 1)].fg, Color::Red);
        assert!(row(&buf, 2).starts_with("<bob> hi there"));
        assert!(row(&buf, 5).starts_with("> hello world"));
    }

    #[test]
    fn test_bitchx_screen_tiny() {
        // Must not panic on degenerate sizes.
        draw(Some(("x".into(), Style::default())), 10, 1);
        draw(None, 10, 2);
        draw(None, 1, 3);
        let mut term = Terminal::new(TestBackend::new(5, 5)).unwrap();
        term.draw(|f| {
            let screen = BitchxScreen::new(status(), FakeChat);
            f.render_stateful_widget(screen, Rect::new(0, 0, 0, 0), &mut String::new());
        })
        .unwrap();
    }
}
