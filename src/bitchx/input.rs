//! # The input line
//!
//! A small readline: cursor movement, Ctrl-A/E/U/K/W, history and Tab nick
//! completion. Operates on `char`s; display width is the renderer's job.

use crate::bitchx::commands::{mask_secret, redact_secret};

/// State of an in-progress Tab completion, so repeated Tabs cycle.
#[derive(Clone, Debug, Default)]
struct Completion {
    start: usize,
    matches: Vec<String>,
    idx: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Input {
    buf: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    /// Index into `history` while browsing it.
    hist_pos: Option<usize>,
    /// The line being typed before history browsing began.
    stash: String,
    tab: Option<Completion>,
}

const HISTORY_MAX: usize = 100;

impl Input {
    pub fn text(&self) -> String {
        self.buf.iter().collect()
    }

    /// The chars to draw: secrets typed after `/recover ` are `*`.
    pub fn display_chars(&self) -> Vec<char> {
        mask_secret(&self.text()).chars().collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    #[cfg(test)]
    pub fn chars(&self) -> &[char] {
        &self.buf
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Any key other than Tab ends a completion cycle.
    pub fn reset_tab(&mut self) {
        self.tab = None;
    }

    pub fn set_text(&mut self, s: &str) {
        self.buf = s.chars().collect();
        self.cursor = self.buf.len();
    }

    pub fn insert(&mut self, c: char) {
        self.buf.insert(self.cursor, c);
        self.cursor += 1;
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.buf.len());
    }

    /// Home / Ctrl-A.
    pub fn home(&mut self) {
        self.cursor = 0;
    }

    /// End / Ctrl-E.
    pub fn end(&mut self) {
        self.cursor = self.buf.len();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.buf.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.buf.len() {
            self.buf.remove(self.cursor);
        }
    }

    /// Ctrl-U: erase from the start of the line to the cursor.
    pub fn kill_to_start(&mut self) {
        self.buf.drain(..self.cursor);
        self.cursor = 0;
    }

    /// Ctrl-K: erase from the cursor to the end of the line.
    pub fn kill_to_end(&mut self) {
        self.buf.truncate(self.cursor);
    }

    /// Ctrl-W: erase the word before the cursor (and the spaces after it).
    pub fn kill_word(&mut self) {
        let mut start = self.cursor;
        while start > 0 && self.buf[start - 1] == ' ' {
            start -= 1;
        }
        while start > 0 && self.buf[start - 1] != ' ' {
            start -= 1;
        }
        self.buf.drain(start..self.cursor);
        self.cursor = start;
    }

    /// Enter: take the line, remember it in history.
    pub fn submit(&mut self) -> String {
        let line = self.text();
        self.buf.clear();
        self.cursor = 0;
        self.hist_pos = None;
        self.tab = None;

        // Secrets (`/recover <key>`) are kept only in redacted form.
        let entry = redact_secret(&line);
        if !entry.trim().is_empty() && self.history.last() != Some(&entry) {
            self.history.push(entry);
            if self.history.len() > HISTORY_MAX {
                self.history.remove(0);
            }
        }

        line
    }

    /// Up: older history entry.
    pub fn history_prev(&mut self) {
        let pos = match self.hist_pos {
            None if self.history.is_empty() => return,
            None => {
                self.stash = self.text();
                self.history.len() - 1
            },
            Some(0) => return,
            Some(p) => p - 1,
        };
        self.hist_pos = Some(pos);
        let entry = self.history[pos].clone();
        self.set_text(&entry);
    }

    /// Down: newer history entry, then back to what was being typed.
    pub fn history_next(&mut self) {
        let Some(pos) = self.hist_pos else {
            return;
        };

        if pos + 1 < self.history.len() {
            self.hist_pos = Some(pos + 1);
            let entry = self.history[pos + 1].clone();
            self.set_text(&entry);
        } else {
            self.hist_pos = None;
            let stash = std::mem::take(&mut self.stash);
            self.set_text(&stash);
        }
    }

    /// Tab: complete the word before the cursor against `nicks`; repeated
    /// Tabs cycle through the matches. At the start of the line the nick is
    /// followed by `: `, elsewhere by a space.
    pub fn complete(&mut self, nicks: &[String]) {
        if self.tab.is_none() {
            let mut start = self.cursor;
            while start > 0 && self.buf[start - 1] != ' ' {
                start -= 1;
            }
            let prefix: String =
                self.buf[start..self.cursor].iter().collect::<String>().to_lowercase();
            if prefix.is_empty() {
                return;
            }
            let mut matches: Vec<String> = nicks
                .iter()
                .filter(|n| n.to_lowercase().starts_with(&prefix))
                .cloned()
                .collect();
            matches.sort_by_key(|n| n.to_lowercase());
            matches.dedup();
            if matches.is_empty() {
                return;
            }
            self.tab = Some(Completion { start, matches, idx: 0 });
        } else if let Some(tab) = &mut self.tab {
            tab.idx = (tab.idx + 1) % tab.matches.len();
        }

        let Some(tab) = &self.tab else {
            return;
        };
        let suffix = if tab.start == 0 { ": " } else { " " };
        let word: Vec<char> = format!("{}{suffix}", tab.matches[tab.idx]).chars().collect();
        let start = tab.start;
        self.buf.splice(start..self.cursor, word.iter().copied());
        self.cursor = start + word.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> Input {
        let mut i = Input::default();
        for c in s.chars() {
            i.insert(c);
        }
        i
    }

    #[test]
    fn test_insert_and_move() {
        let mut i = typed("hllo");
        i.home();
        i.right();
        i.insert('e');
        assert_eq!(i.text(), "hello");
        assert_eq!(i.cursor(), 2);
        i.end();
        assert_eq!(i.cursor(), 5);
        i.left();
        i.backspace();
        assert_eq!(i.text(), "helo");
        i.delete();
        assert_eq!(i.text(), "hel");
    }

    #[test]
    fn test_kill_to_start_and_end() {
        let mut i = typed("hello world");
        i.home();
        for _ in 0..5 {
            i.right();
        }
        i.kill_to_end();
        assert_eq!(i.text(), "hello");
        i.left();
        i.left();
        i.kill_to_start();
        assert_eq!(i.text(), "lo");
        assert_eq!(i.cursor(), 0);
    }

    #[test]
    fn test_kill_word() {
        let mut i = typed("one two  three ");
        i.kill_word();
        assert_eq!(i.text(), "one two  ");
        i.kill_word();
        assert_eq!(i.text(), "one ");
        i.kill_word();
        assert_eq!(i.text(), "");
        i.kill_word();
        assert_eq!(i.text(), "");
    }

    #[test]
    fn test_history() {
        let mut i = typed("first");
        assert_eq!(i.submit(), "first");
        for c in "second".chars() {
            i.insert(c);
        }
        i.submit();
        for c in "draft".chars() {
            i.insert(c);
        }

        i.history_prev();
        assert_eq!(i.text(), "second");
        i.history_prev();
        assert_eq!(i.text(), "first");
        i.history_prev();
        assert_eq!(i.text(), "first");
        i.history_next();
        assert_eq!(i.text(), "second");
        i.history_next();
        assert_eq!(i.text(), "draft");
        i.history_next();
        assert_eq!(i.text(), "draft");
    }

    #[test]
    fn test_history_skips_empty_and_duplicates() {
        let mut i = Input::default();
        i.submit();
        i.set_text("a");
        i.submit();
        i.set_text("a");
        i.submit();
        i.history_prev();
        assert_eq!(i.text(), "a");
        i.history_prev();
        assert_eq!(i.text(), "a");
        assert_eq!(i.history.len(), 1);
    }

    #[test]
    fn test_recover_masked_in_history_and_display() {
        let key = "EsTc 9xQ2 kLm7";
        let mut i = typed(&format!("/recover {key}"));
        let shown: String = i.display_chars().into_iter().collect();
        assert!(!shown.contains("EsTc"), "{shown}");
        assert_eq!(shown, format!("/recover {}", "*".repeat(key.chars().count())));
        assert_eq!(i.display_chars().len(), i.chars().len());
        // The submitted line still carries the key for the command itself...
        assert_eq!(i.submit(), format!("/recover {key}"));
        // ...but history only has the redacted form.
        assert!(i.history.iter().all(|h| !h.contains("EsTc") && !h.contains("kLm7")));
        i.history_prev();
        assert_eq!(i.text(), "/recover ********");
        // Ordinary lines are untouched.
        i.set_text("/msg bob hi");
        assert_eq!(i.display_chars().into_iter().collect::<String>(), "/msg bob hi");
    }

    #[test]
    fn test_tab_completion_cycles() {
        let nicks = vec!["armin".to_string(), "Arthur".to_string(), "bob".to_string()];
        let mut i = typed("ar");
        i.complete(&nicks);
        assert_eq!(i.text(), "armin: ");
        i.complete(&nicks);
        assert_eq!(i.text(), "Arthur: ");
        i.complete(&nicks);
        assert_eq!(i.text(), "armin: ");
        i.reset_tab();
        for c in "hi b".chars() {
            i.insert(c);
        }
        i.complete(&nicks);
        assert_eq!(i.text(), "armin: hi bob ");
    }

    #[test]
    fn test_tab_no_match() {
        let mut i = typed("zz");
        i.complete(&["armin".to_string()]);
        assert_eq!(i.text(), "zz");
    }
}
