//! # BitchX screen
//!
//! The full-screen BitchX layout: the window's scrollback from row 0, the two
//! blue status lines right above the last row, and the input line with its
//! `[#channel] ` prompt (BitchX's INPUT_PROMPT `[$C] `) on the last row. No
//! borders, no tab bar, no `:` command bar line.
use modalkit_ratatui::Window;
use modalkit_ratatui::screen::{Screen, ScreenState};
use modalkit_ratatui::textbox::TextBox;

use crate::bitchx::status::{BxStatus, status_bg};
use crate::bitchx::view::{n, notice_line, room_parts, sanitize, sp, status_parts, wrap};
use crate::prelude::*;
use crate::windows::IambWindow;
use crate::windows::room::{ChatState, RoomState};

/// The full-screen BitchX layout around an input line widget.
pub struct BxView<W> {
    lines: Vec<Line<'static>>,
    status: BxStatus,
    input: W,
}

impl<W> BxView<W> {
    pub fn new(lines: Vec<Line<'static>>, status: BxStatus, input: W) -> Self {
        BxView { lines, status, input }
    }

    /// The rows above the status lines, where the window's contents go.
    pub fn message_area(area: Rect) -> Rect {
        let nstatus = area.height.saturating_sub(1).min(2);
        let h = area.height.saturating_sub(1).saturating_sub(nstatus);
        Rect::new(area.x, area.y, area.width, h)
    }
}

impl<W: StatefulWidget> StatefulWidget for BxView<W> {
    type State = W::State;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let msgarea = Self::message_area(area);
        let nstatus = area.height - 1 - msgarea.height;

        // Scrollback: top-aligned, the newest lines at the bottom once it is full.
        let wrapped = wrap(&self.lines, area.width);
        let skip = wrapped.len().saturating_sub(msgarea.height as usize);

        for (i, line) in wrapped.into_iter().skip(skip).enumerate() {
            let row = Rect::new(area.x, area.y + i as u16, area.width, 1);
            Paragraph::new(line).render(row, buf);
        }

        // Status lines, blue across the full width.
        let s1 = Rect::new(area.x, msgarea.bottom(), area.width, 1);
        let s2 = Rect::new(area.x, msgarea.bottom() + 1, area.width, 1);

        if nstatus >= 1 {
            buf.set_style(s1, status_bg());
            Paragraph::new(self.status.line1()).render(s1, buf);
        }

        if nstatus >= 2 {
            buf.set_style(s2, status_bg());
            Paragraph::new(self.status.line2()).render(s2, buf);

            let right = self.status.line2_right();
            let rw = (right.width() as u16).min(area.width);
            let rarea = Rect::new(area.right() - rw, s2.y, rw, 1);
            let left_w = self.status.line2().width() as u16;
            if area.x + left_w < rarea.x {
                Paragraph::new(right).render(rarea, buf);
            }
        }

        // The input line.
        let input = Rect::new(area.x, area.bottom() - 1, area.width, 1);
        self.input.render(input, buf, state);
    }
}

/// An input line that only shows its prompt (nothing is being typed).
pub struct PromptOnly(pub String);

impl StatefulWidget for PromptOnly {
    type State = ();

    fn render(self, area: Rect, buf: &mut Buffer, _: &mut ()) {
        Paragraph::new(sp(self.0, n())).render(area, buf);
    }
}

/// INPUT_PROMPT `[$C] `.
pub fn prompt(channel: Option<&str>) -> String {
    match channel {
        Some(c) => format!("[{}] ", sanitize(c)),
        None => "[status] ".to_string(),
    }
}

/// The input line while the command bar is focused: the command bar's text
/// and cursor, but behind our `[#channel] ` prompt instead of `:`.
pub struct CmdbarInput<'a> {
    screen: Screen<'a, IambWindow, IambInfo>,
    prompt: String,
    focused: bool,
}

impl<'a> CmdbarInput<'a> {
    pub fn new(store: &'a mut ProgramStore, prompt: String, focused: bool) -> Self {
        CmdbarInput {
            screen: Screen::new(store).focus(true),
            prompt,
            focused,
        }
    }
}

impl StatefulWidget for CmdbarInput<'_> {
    type State = ScreenState<IambWindow, IambInfo>;

    fn render(self, row: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let pw = (UnicodeWidthStr::width(self.prompt.as_str()) as u16).min(row.width);

        if self.focused && pw >= 1 && pw < row.width && row.bottom() > buf.area.y {
            // Let the classic screen lay itself out off-screen so that its
            // command bar lands on our input row with its `:` right under the
            // last prompt cell, then copy just the text over. The cursor
            // position it records is then correct for the real screen.
            let full = buf.area;
            let sarea =
                Rect::new(row.x + pw - 1, full.y, row.width - (pw - 1), row.bottom() - full.y);
            let mut scratch = Buffer::empty(full);
            self.screen.render(sarea, &mut scratch, state);

            for x in (row.x + pw)..row.right() {
                buf[(x, row.y)] = scratch[(x, row.y)].clone();
            }
        }

        Paragraph::new(sp(self.prompt, n())).render(Rect::new(row.x, row.y, pw, 1), buf);
    }
}

/// Draw the whole BitchX screen for whatever window is focused.
pub fn draw(
    area: Rect,
    buf: &mut Buffer,
    sstate: &mut ScreenState<IambWindow, IambInfo>,
    store: &mut ProgramStore,
    cmdbar_focused: bool,
    notice: Option<(String, Style)>,
    dialog: &[Cow<'_, str>],
) {
    let msgheight = BxView::<PromptOnly>::message_area(area).height;

    match sstate.current_window_mut().ok() {
        Some(IambWindow::Room(RoomState::Chat(chat))) => {
            let (lines, status) = room_parts(chat, store, msgheight, notice);
            let prompt = prompt(status.channel.as_deref());

            if cmdbar_focused {
                let input = CmdbarInput::new(store, prompt, true);
                BxView::new(lines, status, input).render(area, buf, sstate);
            } else {
                let input = TextBox::new().prompt(Line::from(prompt));
                BxView::new(lines, status, input).render(area, buf, chat.tbox_mut());
            }
        },
        Some(IambWindow::Welcome(_)) | None => {
            let (lines, status) = status_parts(store, notice);
            let input = CmdbarInput::new(store, prompt(None), cmdbar_focused);
            BxView::new(lines, status, input).render(area, buf, sstate);
        },
        Some(_) => {
            // Room lists, member lists, ...: drawn as the window's contents,
            // without borders, above the status lines.
            let (_, mut status) = status_parts(store, None);
            let mut lines = vec![];
            if let Some((msg, style)) = notice {
                lines.push(notice_line(&msg, style));
            }
            let title = sstate
                .current_window_mut()
                .map(|w| {
                    let t = w.get_win_title(store);
                    t.spans.iter().map(|s| s.content.as_ref()).collect::<String>()
                })
                .unwrap_or_default();
            status.channel = Some(title.clone());
            status.window = 0;

            let input = CmdbarInput::new(store, prompt(Some(&title)), cmdbar_focused);
            BxView::new(vec![], status, input).render(area, buf, sstate);

            let msgarea = BxView::<PromptOnly>::message_area(area);
            let notice_h = (lines.len() as u16).min(msgarea.height);
            let winarea = Rect::new(
                msgarea.x,
                msgarea.y + notice_h,
                msgarea.width,
                msgarea.height - notice_h,
            );
            for (i, line) in lines.into_iter().take(notice_h as usize).enumerate() {
                let row = Rect::new(msgarea.x, msgarea.y + i as u16, msgarea.width, 1);
                Paragraph::new(line).render(row, buf);
            }
            if let Ok(win) = sstate.current_window_mut() {
                win.draw(winarea, buf, true, store);
            }
        },
    }

    // Dialogs (confirmations, the pager prompt) cover the bottom rows.
    if !dialog.is_empty() {
        let h = (dialog.len() as u16).min(area.height);
        for (i, line) in dialog.iter().take(h as usize).enumerate() {
            let y = area.bottom() - h + i as u16;
            let row = Rect::new(area.x, y, area.width, 1);
            buf.set_style(row, Style::default());
            for x in row.left()..row.right() {
                buf[(x, y)].set_symbol(" ");
            }
            buf.set_span(area.x, y, &Span::raw(line.as_ref()), area.width);
        }
    }
}

/// The joined room's chat in this window, if it is one.
pub fn bitchx_chat(win: &mut IambWindow) -> Option<&mut ChatState> {
    match win {
        IambWindow::Room(RoomState::Chat(chat)) => Some(chat.as_mut()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
    use matrix_sdk::ruma::owned_room_id;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use serde_json::json;

    use super::*;
    use crate::bitchx::status::UserCounts;
    use crate::bitchx::view::{BxWindow, FmtCtx, format_message, status_window_lines};
    use crate::tests::*;

    const BORDERS: [char; 14] = [
        '│', '─', '┌', '┐', '└', '┘', '╭', '╮', '╰', '╯', '┃', '━', '║', '═',
    ];

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    fn all(buf: &Buffer) -> String {
        (0..buf.area.height).map(|y| row(buf, y) + "\n").collect()
    }

    /// No classic chrome anywhere on screen.
    fn assert_no_iamb_chrome(buf: &Buffer) {
        let text = all(buf);
        for glyph in BORDERS {
            assert!(!text.contains(glyph), "border glyph {glyph:?} in\n{text}");
        }
        assert!(!text.contains("Welcome to iambitchx"), "{text}");
        assert!(!text.contains("Useful Keybindings"), "{text}");
        assert!(!text.contains("-- INSERT --"), "{text}");
        assert!(!row(buf, buf.area.height - 1).starts_with(':'));
    }

    /// The two rows right above the last one are the blue status bar; the
    /// last row (the input line) is not.
    fn assert_status_at_bottom(buf: &Buffer) {
        let h = buf.area.height;
        for y in [h - 3, h - 2] {
            for x in 0..buf.area.width {
                assert_eq!(buf[(x, y)].bg, Color::Blue, "({x}, {y}) in\n{}", all(buf));
            }
        }
        for y in (0..h - 3).chain([h - 1]) {
            assert_ne!(buf[(0, y)].bg, Color::Blue, "row {y} in\n{}", all(buf));
        }
    }

    fn join_event(user: &UserId) -> Message {
        let ev: AnySyncStateEvent = serde_json::from_value(json!({
            "type": "m.room.member",
            "state_key": user,
            "sender": user,
            "event_id": "$join:example.com",
            "origin_server_ts": 1,
            "content": { "membership": "join" },
        }))
        .unwrap();

        Message::new(MessageEvent::State(Box::new(ev)), user.to_owned(), MSG1_KEY.ts)
    }

    fn room_lines(info: &RoomInfo, me: &UserId) -> Vec<Line<'static>> {
        let own = mock_room1_message(
            RoomMessageEventContent::text_plain("hi all"),
            me.to_owned(),
            MSG2_KEY.clone(),
        );
        let ctx = FmtCtx { me, info, channel: "#room1" };

        [join_event(&TEST_USER3), mock_message1(), own]
            .iter()
            .flat_map(|m| format_message(m, &ctx))
            .collect()
    }

    fn room_status() -> BxStatus {
        BxStatus {
            time: "20:41".into(),
            nick: "user".into(),
            channel: Some("#room1".into()),
            modes: "e".into(),
            act: vec![3, 5],
            window: 2,
            users: Some(UserCounts { ops: 1, nonops: 2, voice: 0 }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_bitchx_room_screen() {
        let mut store = mock_store().await;
        let me = store.application.settings.user_id.clone();
        let info = mock_room();
        let lines = room_lines(&info, &me);

        let id = IambBufferId::Room(
            TEST_ROOM1_ID.clone(),
            None::<OwnedEventId>.into(),
            RoomFocus::MessageBar,
        );
        let ebuf = store.buffers.load_str(id, "hello world");
        let mut tbox = modalkit_ratatui::textbox::TextBoxState::<IambInfo>::new(ebuf);

        let mut term = Terminal::new(TestBackend::new(72, 12)).unwrap();
        term.draw(|f| {
            let input = TextBox::new().prompt(Line::from(prompt(Some("#room1"))));
            let view = BxView::new(lines.clone(), room_status(), input);
            f.render_stateful_widget(view, f.area(), &mut tbox);
        })
        .unwrap();
        let buf = term.backend().buffer().clone();
        let text = all(&buf);

        assert_no_iamb_chrome(&buf);
        assert_status_at_bottom(&buf);

        // The scrollback starts at row 0.
        assert!(
            row(&buf, 0).starts_with("-:- user3 [@user3:example.com] has joined #room1"),
            "{text}"
        );
        assert!(row(&buf, 1).starts_with("<user1> writhe"), "{text}");
        assert!(row(&buf, 2).starts_with("<user> hi all"), "{text}");

        // PUBLIC: bright blue brackets; SEND_PUBLIC (own nick): bright magenta.
        assert_eq!(buf[(0, 1)].fg, Color::LightBlue);
        assert_eq!(buf[(0, 2)].fg, Color::LightMagenta);
        // $G: bold red dashes around a bold white colon.
        assert_eq!(buf[(0, 0)].fg, Color::LightRed);
        assert_eq!(buf[(1, 0)].fg, Color::White);

        // Status lines (FORMAT1 / FORMAT2) and the input line with its prompt.
        let s1 = row(&buf, 9);
        assert!(s1.starts_with(" [20:41][user] [#room1(+e)] [Act: 3,5]"), "{s1:?}");
        let s2 = row(&buf, 10);
        assert!(s2.starts_with(" [Lag 0] [O/1 N/2 I/0 V/0 F/0]"), "{s2:?}");
        assert!(s2.trim_end().ends_with("[Win 2]"), "{s2:?}");
        assert!(row(&buf, 11).starts_with("[#room1] hello world"), "{text}");
    }

    #[test]
    fn test_bitchx_status_window_screen() {
        let wins = vec![
            BxWindow {
                refnum: 2,
                room_id: TEST_ROOM1_ID.clone(),
                title: "#room1".into(),
                unread: false,
            },
            BxWindow {
                refnum: 3,
                room_id: owned_room_id!("!other:example.com"),
                title: "#rust".into(),
                unread: true,
            },
        ];
        let lines = status_window_lines("user", &wins);
        let status = BxStatus {
            time: "20:41".into(),
            nick: "user".into(),
            act: vec![3],
            window: 1,
            ..Default::default()
        };

        let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
        term.draw(|f| {
            let view = BxView::new(lines.clone(), status.clone(), PromptOnly(prompt(None)));
            f.render_stateful_widget(view, f.area(), &mut ());
        })
        .unwrap();
        let buf = term.backend().buffer().clone();
        let text = all(&buf);

        assert_no_iamb_chrome(&buf);
        assert_status_at_bottom(&buf);

        // Banner at the top, then -:- lines with the numbered windows and hints.
        assert!(row(&buf, 0).contains("____"), "{text}");
        assert!(text.contains("-:- [2] #room1"), "{text}");
        assert!(text.contains("-:- [3] #rust (Act)"), "{text}");
        for hint in ["/join", "/window", "/rooms", "/help"] {
            assert!(text.contains(hint), "{hint} missing in\n{text}");
        }

        assert!(row(&buf, 17).starts_with(" [20:41][user]  * type /help for help [Act: 3]"));
        assert!(row(&buf, 18).trim_end().ends_with("[Win 1]"));
        assert!(row(&buf, 19).starts_with("[status] "), "{text}");
    }

    #[test]
    fn test_bitchx_screen_scrolls_and_wraps() {
        let lines: Vec<Line<'static>> = (0..20)
            .map(|i| Line::from(format!("<bob> line {i} {}", "x".repeat(30))))
            .collect();

        let mut term = Terminal::new(TestBackend::new(30, 8)).unwrap();
        term.draw(|f| {
            let view = BxView::new(lines.clone(), BxStatus::default(), PromptOnly("[#a] ".into()));
            f.render_stateful_widget(view, f.area(), &mut ());
        })
        .unwrap();
        let buf = term.backend().buffer().clone();

        // 5 message rows; the newest line, wrapped over two rows, ends at the bottom.
        assert!(row(&buf, 3).starts_with("<bob> line 19"), "{}", all(&buf));
        assert!(row(&buf, 4).starts_with("xxxx"), "{}", all(&buf));
        assert!(row(&buf, 7).starts_with("[#a] "));
    }

    #[test]
    fn test_bitchx_screen_tiny() {
        for (w, h) in [(10, 1), (10, 2), (1, 3), (0, 0), (5, 3)] {
            let mut term = Terminal::new(TestBackend::new(w.max(1), h.max(1))).unwrap();
            term.draw(|f| {
                let lines = vec![Line::from("<bob> hi")];
                let view = BxView::new(lines, BxStatus::default(), PromptOnly("[#a] ".into()));
                f.render_stateful_widget(view, Rect::new(0, 0, w, h), &mut ());
            })
            .unwrap();
        }
    }
}
