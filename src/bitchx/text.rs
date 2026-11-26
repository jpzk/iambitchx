//! # Remote text hygiene
//!
//! Everything a remote user controls (nicks, display names, bodies, topics,
//! room names, reasons) passes through here before it reaches the screen:
//!
//! - F30: bidi controls, zero-width characters and every other Unicode
//!   `Cf` (format) character are dropped, so `evil\u{202E}gnp.exe` or a
//!   zero-width-padded nick cannot pose as something else.
//! - F31: one-line fields get every control / line-separator character
//!   replaced by a space, so a newline in a nick, topic, room name or
//!   reason can never start a fresh, client-looking `-:-` line. Message
//!   bodies keep their newlines (multi-line messages are legitimate); the
//!   formatter indents their continuation rows under the text.

use crate::bitchx::model::{Line, RoomSnapshot, Run};

/// Unicode general category `Cf` (format characters, Unicode 15.1): bidi
/// embeddings/overrides/isolates and marks, zero-width space/joiners,
/// word joiner, invisible operators, BOM, soft hyphen, tags, ...
const CF: &[(u32, u32)] = &[
    (0x00AD, 0x00AD),
    (0x0600, 0x0605),
    (0x061C, 0x061C),
    (0x06DD, 0x06DD),
    (0x070F, 0x070F),
    (0x0890, 0x0891),
    (0x08E2, 0x08E2),
    (0x180E, 0x180E),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x2064),
    (0x2066, 0x206F),
    (0xFEFF, 0xFEFF),
    (0xFFF9, 0xFFFB),
    (0x110BD, 0x110BD),
    (0x110CD, 0x110CD),
    (0x13430, 0x1343F),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0001, 0xE0001),
    (0xE0020, 0xE007F),
];

/// A `Cf` (format) character: invisible, but it changes how text around it
/// is shown (bidi) or compared (zero-width).
pub fn is_format(c: char) -> bool {
    let c = c as u32;
    CF.iter().any(|&(lo, hi)| (lo..=hi).contains(&c))
}

/// Ends a line on some terminals or text layouts: C0/C1 controls (`\n`,
/// `\r`, NEL, ...) plus U+2028 LINE SEPARATOR / U+2029 PARAGRAPH SEPARATOR.
fn is_break(c: char) -> bool {
    c.is_control() || c == '\u{2028}' || c == '\u{2029}'
}

/// A remote one-line field (nick, display name, topic, room name, reason):
/// format characters dropped, controls and line breaks become spaces.
pub fn one_line(s: &str) -> String {
    s.chars()
        .filter(|c| !is_format(*c))
        .map(|c| if is_break(c) { ' ' } else { c })
        .collect()
}

/// A remote message body: format characters dropped, `\n` kept (U+2028 /
/// U+2029 / `\r\n` normalised to it), every other control a space.
pub fn body(s: &str) -> String {
    s.replace("\r\n", "\n")
        .chars()
        .filter(|c| !is_format(*c))
        .map(|c| {
            match c {
                '\n' => '\n',
                '\u{2028}' | '\u{2029}' => '\n',
                c if is_break(c) => ' ',
                c => c,
            }
        })
        .collect()
}

fn opt(s: Option<String>) -> Option<String> {
    s.map(|s| one_line(&s))
}

/// A line that came from the Matrix side: every remote field cleaned.
/// Bodies keep newlines; `Info`/`Error` lines are one line each (a remote
/// room name or server error must not add lines of its own).
pub fn clean_line(line: Line) -> Line {
    let o = |s: String| one_line(&s);
    match line {
        Line::Public { nick, own, text } => Line::Public { nick: o(nick), own, text: body(&text) },
        Line::Action { nick, own, text } => Line::Action { nick: o(nick), own, text: body(&text) },
        Line::Private { nick, mxid, own, text } => {
            Line::Private {
                nick: o(nick),
                mxid: o(mxid),
                own,
                text: body(&text),
            }
        },
        Line::Notice { nick, mxid, text } => {
            Line::Notice { nick: o(nick), mxid: o(mxid), text: body(&text) }
        },
        Line::Join { nick, mxid } => Line::Join { nick: o(nick), mxid: o(mxid) },
        Line::Part { nick, mxid, reason } => {
            Line::Part { nick: o(nick), mxid: o(mxid), reason: opt(reason) }
        },
        Line::Kick { victim, kicker, reason } => {
            Line::Kick {
                victim: o(victim),
                kicker: o(kicker),
                reason: opt(reason),
            }
        },
        Line::DisplayName { nick, name } => Line::DisplayName { nick: o(nick), name: opt(name) },
        Line::Topic { nick, topic } => Line::Topic { nick: opt(nick), topic: o(topic) },
        Line::Mode { modes, nick } => Line::Mode { modes: o(modes), nick: o(nick) },
        Line::Invite { nick, target } => Line::Invite { nick: o(nick), target: o(target) },
        Line::Info(t) => Line::Info(o(t)),
        Line::Error(t) => Line::Error(o(t)),
        Line::Undecryptable { nick, reason } => {
            Line::Undecryptable { nick: o(nick), reason: o(reason) }
        },
        Line::Sas { n, emoji, name } => Line::Sas { n, emoji: o(emoji), name: o(name) },
        Line::Users { chan, shown, total } => Line::Users { chan: o(chan), shown, total },
        Line::Names { cells, width } => {
            Line::Names {
                cells: cells.into_iter().map(|(f, n, p)| (f, o(n), p)).collect(),
                width,
            }
        },
        Line::WhoisNick { nick, mxid, info } => {
            Line::WhoisNick { nick: o(nick), mxid: o(mxid), info: o(info) }
        },
        Line::WhoisMark { nick, mxid, info, mark } => {
            Line::WhoisMark { nick: o(nick), mxid: o(mxid), info: o(info), mark }
        },
        Line::WhoisRow { row, last, key, value } => {
            Line::WhoisRow { row, last, key: o(key), value: o(value) }
        },
        Line::SetVar { name, value } => Line::SetVar { name: o(name), value: opt(value) },
        Line::Raw(t) => Line::Raw(o(t)),
        Line::Logo(t) => Line::Logo(o(t)),
        Line::LogoDim(t) => Line::LogoDim(o(t)),
        Line::LogoRuns(runs) => {
            Line::LogoRuns(
                runs.iter().map(|r| Run { text: o(r.text.clone()), ..r.clone() }).collect(),
            )
        },
        Line::Debug { cat, text } => Line::Debug { cat: o(cat), text: o(text) },
        l @ Line::WhoisRule => l,
    }
}

/// A room snapshot from the Matrix side: name, topic and member nicks
/// cleaned.
pub fn clean_snapshot(mut snap: RoomSnapshot) -> RoomSnapshot {
    snap.name = one_line(&snap.name);
    snap.topic = opt(snap.topic);
    for m in &mut snap.members {
        m.nick = one_line(&m.nick);
    }
    snap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bidi_and_zero_width_stripped() {
        // F30: RLO "exe.gnp", isolates, ZWSP/ZWJ/ZWNJ, word joiner, BOM,
        // soft hyphen, tag characters: all gone.
        assert_eq!(one_line("evil\u{202E}gnp.exe"), "evilgnp.exe");
        assert_eq!(one_line("\u{2067}admin\u{2069}"), "admin");
        assert_eq!(one_line("b\u{200B}o\u{200C}b\u{200D}\u{2060}\u{FEFF}"), "bob");
        assert_eq!(one_line("ad\u{00AD}min\u{E0041}"), "admin");
        assert_eq!(body("hi \u{200E}there\u{200F}\nx\u{061C}"), "hi there\nx");
        // Ordinary non-ASCII text is untouched.
        assert_eq!(one_line("grüße 🐶 日本"), "grüße 🐶 日本");
        assert!(!is_format('a') && is_format('\u{202D}') && is_format('\u{E007F}'));
    }

    #[test]
    fn test_one_line_has_no_breaks() {
        // F31: nothing in a one-line field may end the line.
        for s in [
            "a\nb",
            "a\rb",
            "a\u{85}b",
            "a\u{2028}b",
            "a\u{2029}b",
            "a\x1b[2Jb",
            "a\tb",
        ] {
            let o = one_line(s);
            assert!(!o.chars().any(is_break), "{s:?} -> {o:?}");
        }
        assert_eq!(body("a\r\nb\u{2028}c\x07"), "a\nb\nc ");
    }

    #[test]
    fn test_clean_line_fields() {
        let l = clean_line(Line::Topic {
            nick: Some("ev\nil".into()),
            topic: "x\n -:- bob is now verified".into(),
        });
        assert_eq!(l, Line::Topic {
            nick: Some("ev il".into()),
            topic: "x  -:- bob is now verified".into()
        });
        let l =
            clean_line(Line::Info("eve changed the channel name to x\n bob is verified".into()));
        assert_eq!(l, Line::Info("eve changed the channel name to x  bob is verified".into()));
        let l = clean_line(Line::Public {
            nick: "b\u{200B}ob".into(),
            own: false,
            text: "a\nb".into(),
        });
        assert_eq!(l, Line::Public {
            nick: "bob".into(),
            own: false,
            text: "a\nb".into()
        });
    }
}
