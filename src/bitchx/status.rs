//! # BitchX status bar
//!
//! Pure builder for the `[nick@host/channel +modes] topic` line.
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The contents of the BitchX status bar.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatusBar {
    pub nick: String,
    pub host: String,
    pub channel: String,
    /// Single-letter mode flags, e.g. `["e"]` for an encrypted room.
    pub modes: Vec<String>,
    pub topic: String,
}

impl StatusBar {
    /// `"[nick@host/channel +modes] topic"`; the ` +modes` and ` topic`
    /// parts are omitted when empty.
    pub fn format(&self) -> String {
        let modes: String = self.modes.iter().map(|m| m.trim()).collect();
        let topic = self.topic.trim();

        let mut out = format!("[{}@{}/{}", self.nick, self.host, self.channel);

        if !modes.is_empty() {
            out.push_str(" +");
            out.push_str(&modes);
        }

        out.push(']');

        if !topic.is_empty() {
            out.push(' ');
            out.push_str(topic);
        }

        out
    }

    /// Render the status bar truncated to at most `width` terminal cells.
    ///
    /// Truncation happens on grapheme boundaries, so a wide character that
    /// would straddle the edge is dropped rather than split. Control
    /// characters (which could smuggle terminal escape sequences in from a
    /// room topic or display name) are replaced with spaces.
    pub fn render(&self, width: u16) -> Line<'static> {
        let text = truncate(&sanitize(&self.format()), width as usize);
        let style = Style::default().add_modifier(Modifier::REVERSED);

        Line::from(Span::styled(text, style))
    }
}

/// Replace control characters (newlines, ESC, ...) with spaces.
fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// Take whole graphemes from `s` while they fit into `width` cells.
fn truncate(s: &str, width: usize) -> String {
    let mut used = 0;
    let mut out = String::new();

    for g in s.graphemes(true) {
        let w = UnicodeWidthStr::width(g);

        if used + w > width {
            break;
        }

        used += w;
        out.push_str(g);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(modes: &[&str], topic: &str) -> StatusBar {
        StatusBar {
            nick: "alice".into(),
            host: "matrix.org".into(),
            channel: "#rust".into(),
            modes: modes.iter().map(|m| m.to_string()).collect(),
            topic: topic.into(),
        }
    }

    fn rendered(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn test_bitchx_status_format_full() {
        assert_eq!(bar(&["e"], "Rust talk").format(), "[alice@matrix.org/#rust +e] Rust talk");
        assert_eq!(bar(&["e", "p"], "t").format(), "[alice@matrix.org/#rust +ep] t");
    }

    #[test]
    fn test_bitchx_status_format_no_topic() {
        assert_eq!(bar(&["e"], "").format(), "[alice@matrix.org/#rust +e]");
        assert_eq!(bar(&["e"], "   ").format(), "[alice@matrix.org/#rust +e]");
    }

    #[test]
    fn test_bitchx_status_format_no_modes() {
        assert_eq!(bar(&[], "hi").format(), "[alice@matrix.org/#rust] hi");
        assert_eq!(bar(&[""], "hi").format(), "[alice@matrix.org/#rust] hi");
        assert_eq!(bar(&[], "").format(), "[alice@matrix.org/#rust]");
    }

    #[test]
    fn test_bitchx_status_render_fits() {
        let b = bar(&["e"], "Rust talk");
        assert_eq!(rendered(&b.render(200)), b.format());
        assert_eq!(rendered(&b.render(10)), "[alice@mat");
        assert_eq!(b.render(10).width(), 10);
    }

    #[test]
    fn test_bitchx_status_render_width_zero() {
        let b = bar(&["e"], "topic");
        let line = b.render(0);
        assert_eq!(rendered(&line), "");
        assert_eq!(line.width(), 0);

        let empty = StatusBar::default();
        assert_eq!(rendered(&empty.render(0)), "");
        assert_eq!(rendered(&empty.render(5)), "[@/]");
    }

    #[test]
    fn test_bitchx_status_render_wide_chars() {
        let b = StatusBar {
            nick: "日本".into(),
            host: "h".into(),
            channel: "c".into(),
            modes: vec![],
            topic: "🎉🎉".into(),
        };
        // "[日本@h/c] 🎉🎉" → widths: 1 + 4 + 4 + 1 + 1 + 4 = 15
        assert_eq!(rendered(&b.render(15)), "[日本@h/c] 🎉🎉");
        // Cutting in the middle of a wide char drops it rather than splitting.
        assert_eq!(rendered(&b.render(2)), "[");
        assert_eq!(rendered(&b.render(3)), "[日");
        assert_eq!(rendered(&b.render(14)), "[日本@h/c] 🎉");
        for w in 0..20 {
            assert!(b.render(w).width() <= w as usize, "width {w}");
        }
    }

    #[test]
    fn test_bitchx_status_render_graphemes() {
        // "e" + combining acute accent must stay together.
        let b = StatusBar {
            nick: "e\u{301}e\u{301}".into(),
            ..Default::default()
        };
        assert_eq!(rendered(&b.render(2)), "[e\u{301}");
        assert_eq!(rendered(&b.render(3)), "[e\u{301}e\u{301}");
    }

    #[test]
    fn test_bitchx_status_render_sanitizes_control_chars() {
        let b = bar(&[], "evil\u{1b}[2Jtopic\nline2");
        let out = rendered(&b.render(200));
        assert!(!out.chars().any(char::is_control), "{out:?}");
        assert!(out.ends_with("evil [2Jtopic line2"), "{out:?}");
    }
}
