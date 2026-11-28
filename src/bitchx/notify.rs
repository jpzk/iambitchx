//! # Desktop notifications (`/set NOTIFY ON`)
//!
//! A live mention can raise a desktop notification. No notification crate
//! is in `Cargo.lock` (upstream iamb's `notify-rust` is not a dependency of
//! this fork), so the notification is a terminal escape the terminal
//! emulator turns into a desktop notification itself:
//!
//! - OSC 777 `ESC ] 777 ; notify ; title ; body BEL` (urxvt, foot, VTE
//!   terminals, Ghostty, `WezTerm`) when `TERM` / `VTE_VERSION` /
//!   `TERM_PROGRAM` name one of those;
//! - OSC 9 `ESC ] 9 ; text BEL` (iTerm2, kitty, `WezTerm`, Ghostty, ...)
//!   otherwise.
//!
//! Inside tmux (`TMUX` set) the sequence is wrapped in a DCS passthrough
//! (`ESC P tmux; ... ESC \`, inner ESCs doubled); tmux only forwards it
//! with `set -g allow-passthrough on`.
//!
//! Every remote field goes through [`terminal_text`] (controls, ESC, BEL,
//! bidi and zero-width characters are gone, tokens masked), `;` becomes
//! `,` so a field cannot add OSC 777 parameters, and each field is cut to
//! [`MAX_FIELD`] characters. The text always starts with `iamBitchX`, so an
//! OSC 9 payload can never look like a `ConEmu` `9;N` sub-command.
use crate::bitchx::model::Line;
use crate::bitchx::text::terminal_text;

/// Longest nick / room / message part, in characters.
pub(crate) const MAX_FIELD: usize = 120;

/// The notification escape flavour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Osc {
    /// OSC 777 `notify;title;body`.
    Osc777,
    /// OSC 9 `text`.
    Osc9,
}

/// Pick the flavour from `TERM`, `TERM_PROGRAM` and whether `VTE_VERSION`
/// is set.
pub(crate) fn flavour(term: Option<&str>, term_program: Option<&str>, vte: bool) -> Osc {
    let term = term.unwrap_or("").to_lowercase();
    let prog = term_program.unwrap_or("").to_lowercase();
    let t777 = ["rxvt", "foot", "xterm-ghostty", "wezterm"];
    let p777 = ["ghostty", "wezterm"];
    if vte || t777.iter().any(|t| term.starts_with(t)) || p777.contains(&prog.as_str()) {
        Osc::Osc777
    } else {
        Osc::Osc9
    }
}

/// The flavour for this process's environment.
pub(crate) fn env_flavour() -> Osc {
    let var = |k| std::env::var(k).ok();
    flavour(
        var("TERM").as_deref(),
        var("TERM_PROGRAM").as_deref(),
        std::env::var_os("VTE_VERSION").is_some(),
    )
}

/// A remote field made inert for an OSC payload.
pub(crate) fn clean(s: &str) -> String {
    let s: String = terminal_text(s).replace(';', ",");
    let s = s.trim();
    let mut out: String = s.chars().take(MAX_FIELD).collect();
    if s.chars().count() > MAX_FIELD {
        out.push_str("...");
    }
    out
}

/// What to say about a mention line in window `win`: `(title, body)`, or
/// `None` for a line that cannot be a mention.
pub(crate) fn summary(line: &Line, win: &str) -> Option<(String, String)> {
    let (nick, text, action) = match line {
        Line::Public { nick, text, .. }
        | Line::Notice { nick, text, .. }
        | Line::Private { nick, text, .. } => (nick, text, false),
        Line::Action { nick, text, .. } => (nick, text, true),
        _ => return None,
    };
    let title = format!("iamBitchX: {} in {}", clean(nick), clean(win));
    let body = if action {
        format!("* {} {}", clean(nick), clean(text))
    } else {
        clean(text)
    };
    Some((title, body))
}

/// The escape sequence for `(title, body)` (already [`clean`]ed), wrapped
/// for tmux when `tmux`.
pub(crate) fn sequence(osc: Osc, title: &str, body: &str, tmux: bool) -> String {
    let seq = match osc {
        Osc::Osc777 => format!("\x1b]777;notify;{title};{body}\x07"),
        Osc::Osc9 => format!("\x1b]9;{title}: {body}\x07"),
    };
    if tmux {
        format!("\x1bPtmux;{}\x1b\\", seq.replace('\x1b', "\x1b\x1b"))
    } else {
        seq
    }
}

/// Write the notification for `(title, body)` to the terminal.
pub(crate) fn emit(title: &str, body: &str) {
    use std::io::Write as _;
    let tmux = std::env::var_os("TMUX").is_some();
    let seq = sequence(env_flavour(), title, body, tmux);
    let mut out = std::io::stdout();
    let _ = out.write_all(seq.as_bytes()).and_then(|()| out.flush());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitchx::text::{HOSTILE, assert_inert};

    #[test]
    fn test_flavour() {
        assert_eq!(flavour(Some("rxvt-unicode-256color"), None, false), Osc::Osc777);
        assert_eq!(flavour(Some("foot"), None, false), Osc::Osc777);
        assert_eq!(flavour(Some("xterm-256color"), None, true), Osc::Osc777, "VTE");
        assert_eq!(flavour(Some("xterm-256color"), Some("WezTerm"), false), Osc::Osc777);
        assert_eq!(flavour(Some("xterm-256color"), Some("iTerm.app"), false), Osc::Osc9);
        assert_eq!(flavour(Some("xterm-kitty"), None, false), Osc::Osc9);
        assert_eq!(flavour(None, None, false), Osc::Osc9);
    }

    #[test]
    fn test_sequences() {
        assert_eq!(sequence(Osc::Osc777, "t", "b", false), "\x1b]777;notify;t;b\x07");
        assert_eq!(sequence(Osc::Osc9, "t", "b", false), "\x1b]9;t: b\x07");
        assert_eq!(sequence(Osc::Osc9, "t", "b", true), "\x1bPtmux;\x1b\x1b]9;t: b\x07\x1b\\");
    }

    #[test]
    fn test_hostile_fields_are_inert() {
        for h in HOSTILE {
            let c = clean(h);
            assert_inert(&c);
            assert!(!c.contains(';'), "{c:?}");
            let line = Line::Public { nick: (*h).into(), own: false, text: (*h).into() };
            let (t, b) = summary(&line, h).unwrap();
            assert_inert(&t);
            assert_inert(&b);
            // The only controls in the sequence are the frame's own.
            let seq = sequence(Osc::Osc777, &t, &b, false);
            assert_eq!(seq.matches('\x1b').count(), 1);
            assert_eq!(seq.matches('\x07').count(), 1);
            assert_eq!(seq.matches(';').count(), 3, "{seq:?}");
        }
    }

    #[test]
    fn test_summary_and_truncation() {
        let line = Line::Action {
            nick: "bob".into(),
            own: false,
            text: "pokes carla".into(),
        };
        assert_eq!(
            summary(&line, "#r"),
            Some(("iamBitchX: bob in #r".into(), "* bob pokes carla".into()))
        );
        assert_eq!(summary(&Line::Info("carla".into()), "#r"), None);
        let long = "x".repeat(500);
        assert_eq!(clean(&long).chars().count(), MAX_FIELD + 3);
    }
}
