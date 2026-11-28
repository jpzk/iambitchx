//! Keyboard input: one crossterm key event in, edits to the input line,
//! window switching, scrolling and submits out.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::bitchx::app::App;
use crate::bitchx::model::Effect;

impl App {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.kind == KeyEventKind::Release {
            return vec![];
        }
        let mut fx = self.activity();
        fx.extend(self.key(key));
        fx
    }

    pub(super) fn key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if self.picker.is_some() {
            self.picker_key(key);
            return vec![];
        }

        // `/bind`: a bound key runs its command instead of its default.
        if let Some(fx) = self.bound_key(&key) {
            return fx;
        }

        if key.code != KeyCode::Tab {
            self.input.reset_tab();
        }

        match key.code {
            KeyCode::Char(c @ '0'..='9') if alt => {
                let n = if c == '0' {
                    10
                } else {
                    c as usize - '0' as usize
                };
                return self.switch(n - 1);
            },
            KeyCode::Char('n') if ctrl => return self.next_window(),
            KeyCode::Char('p') if ctrl => return self.prev_window(),
            KeyCode::Char('a') if ctrl => self.input.home(),
            KeyCode::Char('e') if ctrl => self.input.end(),
            KeyCode::Char('u') if ctrl => self.input.kill_to_start(),
            KeyCode::Char('k') if ctrl => self.input.kill_to_end(),
            KeyCode::Char('w') if ctrl => self.input.kill_word(),
            KeyCode::Char('b') if ctrl => self.input.left(),
            KeyCode::Char('f') if ctrl => self.input.right(),
            KeyCode::Char('h') if ctrl => self.input.backspace(),
            KeyCode::Char('d') if ctrl => {
                if self.input.is_empty() {
                    self.quit = true;
                    return vec![Effect::Quit];
                }
                self.input.delete();
            },
            KeyCode::Char(_) if ctrl || alt => {},
            KeyCode::Char(c) => self.input.insert(c),
            KeyCode::Left => self.input.left(),
            KeyCode::Right => self.input.right(),
            KeyCode::Home => self.input.home(),
            KeyCode::End => self.input.end(),
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::Up => self.input.history_prev(),
            KeyCode::Down => self.input.history_next(),
            KeyCode::Tab => {
                // `/theme <Tab>` completes theme names, anything else nicks.
                let nicks = if self.input.text().to_lowercase().starts_with("/theme ") {
                    self.theme_names()
                } else {
                    self.nicks()
                };
                self.input.complete(&nicks);
            },
            KeyCode::PageUp => {
                let page = self.page.max(1);
                self.cur_mut().scroll += page;
            },
            KeyCode::PageDown => {
                let page = self.page.max(1);
                let win = self.cur_mut();
                win.scroll = win.scroll.saturating_sub(page);
            },
            KeyCode::Enter => {
                let line = self.input.submit();
                return self.submit(&line);
            },
            _ => {},
        }
        vec![]
    }
}
