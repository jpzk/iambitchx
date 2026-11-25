//! # BitchX message formats
//!
//! [`Line`]s rendered with the "default bx" FSETs of BitchX1.3
//! `include/color.h`; see `docs/bitchx-formats.md`.
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TLine, Span};
use unicode_width::UnicodeWidthChar;

use crate::bitchx::model::Line;

pub fn fg(c: Color) -> Style {
    Style::default().fg(c)
}

pub fn bold(c: Color) -> Style {
    fg(c).add_modifier(Modifier::BOLD)
}

/// `%n`: the terminal's default colours.
pub fn n() -> Style {
    Style::default()
}

/// `%K`: dark gray (bold black).
pub fn k() -> Style {
    bold(Color::DarkGray)
}

/// `%W`: bold bright white.
pub fn w() -> Style {
    bold(Color::White)
}

/// `%c`: cyan.
pub fn c() -> Style {
    fg(Color::Cyan)
}

/// Replace control characters (escape sequences, newlines) with spaces.
pub fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

pub fn sp(text: impl Into<String>, style: Style) -> Span<'static> {
    Span::styled(sanitize(&text.into()), style)
}

/// The leading glyph of every client/event line: a single (dark-grey) space,
/// as captured from BitchX (recon §3).
pub fn g() -> Vec<Span<'static>> {
    vec![Span::styled(" ", k())]
}

fn g_line(rest: Vec<Span<'static>>) -> TLine<'static> {
    let mut spans = g();
    spans.extend(rest);
    TLine::from(spans)
}

/// Lines of a multi-line body, the continuation lines indented under the text.
fn body_lines(prefix: Vec<Span<'static>>, text: &str, style: Style) -> Vec<TLine<'static>> {
    let indent = " ".repeat(TLine::from(prefix.clone()).width());
    text.split('\n')
        .enumerate()
        .map(|(i, part)| {
            let mut spans = if i == 0 {
                prefix.clone()
            } else {
                vec![Span::raw(indent.clone())]
            };
            spans.push(sp(part, style));
            TLine::from(spans)
        })
        .collect()
}

/// Format one line for the window whose channel name is `chan`.
/// Formats and colours follow the BitchX recon captures 22-32.
pub fn format_line(line: &Line, chan: &str) -> Vec<TLine<'static>> {
    match line {
        // `<nick> text`: only the brackets are coloured, bold magenta for
        // our own lines and bold blue for everyone else.
        Line::Public { nick, own, text } => {
            let bracket = if *own {
                bold(Color::Magenta)
            } else {
                bold(Color::Blue)
            };
            let prefix = vec![
                sp("<", bracket),
                sp(nick.clone(), n()),
                sp(">", bracket),
                sp(" ", n()),
            ];
            body_lines(prefix, text, n())
        },
        // Own: ` nick text` (nick bold white). Others: ` nick/#chan text`
        // (nick and channel bold yellow).
        Line::Action { nick, own, text } => {
            let who = if *own {
                sp(format!("{nick} "), w())
            } else {
                sp(format!("{nick}/{chan} "), bold(Color::Yellow))
            };
            body_lines(vec![sp(" ", k()), who], text, n())
        },
        // `-nick(mxid)- text`: punctuation dark grey, nick and host magenta.
        Line::Notice { nick, mxid, text } => {
            let prefix = vec![
                sp("-", k()),
                sp(nick.clone(), fg(Color::Magenta)),
                sp("(", k()),
                sp(mxid.clone(), fg(Color::Magenta)),
                sp(")-", k()),
                sp(" ", n()),
            ];
            body_lines(prefix, text, n())
        },
        // ` nick [mxid] has joined #chan`: nick bold cyan, host cyan.
        Line::Join { nick, mxid } => {
            vec![g_line(vec![
                sp(format!("{nick} "), bold(Color::Cyan)),
                sp("[", k()),
                sp(mxid.clone(), c()),
                sp("]", k()),
                sp(format!(" has joined {chan}"), n()),
            ])]
        },
        // ` nick [mxid] has left #chan [reason]`: the reason brackets are
        // always printed, `[]` when there is none.
        Line::Part { nick, mxid, reason } => {
            let mut s = vec![
                sp(format!("{nick} "), n()),
                sp("[", k()),
                sp(mxid.clone(), n()),
                sp("]", k()),
                sp(format!(" has left {chan} "), n()),
            ];
            match reason {
                Some(r) if !r.is_empty() => {
                    s.extend([sp("[", k()), sp(r.clone(), n()), sp("]", k())])
                },
                _ => s.push(sp("[]", k())),
            }
            vec![g_line(s)]
        },
        // ` victim was kicked off #chan by kicker (reason)`: kicker cyan.
        Line::Kick { victim, kicker, reason } => {
            let mut s = vec![
                sp(format!("{victim} was kicked off {chan} by "), n()),
                sp(kicker.clone(), c()),
            ];
            if let Some(r) = reason {
                s.extend([sp(" (", k()), sp(r.clone(), n()), sp(")", k())]);
            }
            vec![g_line(s)]
        },
        // ` old is now known as new`: old bold white, new cyan.
        Line::Nick { old, new } => {
            vec![g_line(vec![
                sp(format!("{old} "), w()),
                sp("is now known as ", n()),
                sp(new.clone(), c()),
            ])]
        },
        // ` Topic (#chan): changed by nick: topic`
        Line::Topic { nick, topic } => {
            let mut s = vec![
                sp("Topic ", n()),
                sp("(", k()),
                sp(chan, c()),
                sp("):", k()),
            ];
            if let Some(nick) = nick {
                s.extend([sp(" changed by ", n()), sp(nick.clone(), c()), sp(":", k())]);
            }
            s.push(sp(format!(" {topic}"), n()));
            vec![g_line(s)]
        },
        // ` mode/#chan [modes] by nick`
        Line::Mode { modes, nick } => {
            vec![g_line(vec![
                sp("mode", n()),
                sp("/", k()),
                sp(format!("{chan} "), c()),
                sp("[", k()),
                sp(modes.clone(), w()),
                sp("]", k()),
                sp(" by ", n()),
                sp(nick.clone(), w()),
            ])]
        },
        Line::Invite { nick, target } => {
            vec![g_line(vec![
                sp(nick.clone(), c()),
                sp(format!(" invited {target} to {chan}"), n()),
            ])]
        },
        Line::Info(text) => text.split('\n').map(|t| g_line(vec![sp(t, n())])).collect(),
        // ` BitchX: text` ("BitchX" bold), as for client errors.
        Line::Error(text) => {
            text.split('\n')
                .map(|t| {
                    g_line(vec![
                        sp("BitchX", n().add_modifier(Modifier::BOLD)),
                        sp(format!(": {t}"), n()),
                    ])
                })
                .collect()
        },
        Line::Undecryptable { nick, reason } => {
            vec![g_line(vec![
                sp("[", k()),
                sp(format!("unable to decrypt: {reason}"), fg(Color::LightRed)),
                sp("]", k()),
                sp(" from ", n()),
                sp(nick.clone(), c()),
            ])]
        },
        Line::Sas { n: i, emoji, name } => {
            vec![g_line(vec![
                sp(format!("SAS {i}/7  "), k()),
                sp(format!("{emoji}  "), n()),
                sp(name.clone(), w()),
            ])]
        },
        Line::Raw(text) => vec![TLine::from(sp(text.clone(), n()))],
        Line::Logo(text) => vec![TLine::from(sp(text.clone(), bold(Color::Blue)))],
        // ` [Users(#chan:n/m)]` (capture 25).
        Line::Users { chan, shown, total } => {
            let gr = fg(Color::Green);
            vec![g_line(vec![
                sp("[", k()),
                sp("Users", bold(Color::Green)),
                sp("(", k()),
                sp(chan.clone(), gr),
                sp(":", k()),
                sp(shown.to_string(), gr),
                sp("/", k()),
                sp(total.to_string(), gr),
                sp(")]", k()),
            ])]
        },
        // `[@op ] [ nick ]`: no leading glyph, ops bold white, others bold blue.
        Line::Names { cells, width } => {
            let mut spans = vec![];
            for (i, (flag, nick)) in cells.iter().enumerate() {
                if i > 0 {
                    spans.push(sp(" ", n()));
                }
                let (fs, ns) = match flag {
                    '@' => (bold(Color::Cyan), w()),
                    '+' => (bold(Color::Green), bold(Color::Blue)),
                    _ => (k(), bold(Color::Blue)),
                };
                spans.push(sp("[", k()));
                spans.push(sp(flag.to_string(), fs));
                spans.push(sp(format!("{nick:<width$} "), ns));
                spans.push(sp("]", k()));
            }
            vec![TLine::from(spans)]
        },
        // The fading green/grey ruler that opens a WHOIS (capture 26).
        Line::WhoisRule => {
            vec![TLine::from(vec![
                sp("------", bold(Color::Green)),
                sp("------", fg(Color::Green)),
                sp("--- --  -", k()),
            ])]
        },
        Line::WhoisNick { nick, mxid, info } => {
            vec![TLine::from(vec![
                sp("| ", bold(Color::Green)),
                sp(nick.clone(), w()),
                sp(" (", k()),
                sp(mxid.clone(), n()),
                sp(") (", k()),
                sp(info.clone(), w()),
                sp(")", k()),
            ])]
        },
        Line::WhoisRow { row, last, key, value } => {
            let lead = match (last, row % 2) {
                (true, _) => ": ",
                (false, 1) => " ",
                _ => "| ",
            };
            let ls = match row {
                0 | 1 => bold(Color::Green),
                2 => fg(Color::Green),
                _ => k(),
            };
            let mut chars = key.chars();
            let first = chars.next().map(String::from).unwrap_or_default();
            let rest: String = chars.collect();
            let pad = 8usize.saturating_sub(key.chars().count());
            vec![TLine::from(vec![
                sp(lead, ls),
                sp(first, w()),
                sp(format!("{rest}{} : {value}", " ".repeat(pad)), n()),
            ])]
        },
    }
}

/// Split lines so that none is wider than `width` cells.
pub fn wrap(lines: Vec<TLine<'static>>, width: usize) -> Vec<TLine<'static>> {
    let mut out = vec![];

    if width == 0 {
        return out;
    }

    for line in lines {
        let mut cur: Vec<Span<'static>> = vec![];
        let mut used = 0;

        for span in line.spans {
            let mut buf = String::new();

            for ch in span.content.chars() {
                let cw = ch.width().unwrap_or(0);

                if used + cw > width && used > 0 {
                    if !buf.is_empty() {
                        cur.push(Span::styled(std::mem::take(&mut buf), span.style));
                    }
                    out.push(TLine::from(std::mem::take(&mut cur)));
                    used = 0;
                }

                buf.push(ch);
                used += cw;
            }

            if !buf.is_empty() {
                cur.push(Span::styled(buf, span.style));
            }
        }

        out.push(TLine::from(cur));
    }

    out
}

/// The plain text of rendered lines.
#[cfg(test)]
pub fn plain(lines: &[TLine<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line) -> String {
        plain(&format_line(line, "#room")).join("\n")
    }

    /// The style of the span containing `needle` in the first rendered line.
    fn style_of(lines: &[TLine<'_>], needle: &str) -> Style {
        lines[0]
            .spans
            .iter()
            .find(|s| s.content.contains(needle))
            .unwrap_or_else(|| panic!("no span {needle:?}"))
            .style
    }

    #[test]
    fn test_public_and_action() {
        let l = Line::Public {
            nick: "armin".into(),
            own: false,
            text: "hi".into(),
        };
        assert_eq!(text(&l), "<armin> hi");
        // Capture 24: own action ` nick text`, other's ` nick/#chan text`.
        let own = Line::Action {
            nick: "carla".into(),
            own: true,
            text: "waves".into(),
        };
        assert_eq!(text(&own), " carla waves");
        assert_eq!(style_of(&format_line(&own, "#room"), "carla"), w());
        let other = Line::Action {
            nick: "armin".into(),
            own: false,
            text: "waves back".into(),
        };
        assert_eq!(text(&other), " armin/#room waves back");
        assert_eq!(style_of(&format_line(&other, "#room"), "armin/#room"), bold(Color::Yellow));
    }

    #[test]
    fn test_public_bracket_colors() {
        // Capture 23: own `<`/`>` bold magenta, others bold blue, nick plain.
        let own =
            format_line(&Line::Public { nick: "me".into(), own: true, text: "x".into() }, "#r");
        let other =
            format_line(&Line::Public { nick: "me".into(), own: false, text: "x".into() }, "#r");
        assert_eq!(own[0].spans[0].style, bold(Color::Magenta));
        assert_eq!(own[0].spans[2].style, bold(Color::Magenta));
        assert_eq!(other[0].spans[0].style, bold(Color::Blue));
        assert_eq!(own[0].spans[1].style, n());
        assert_eq!(other[0].spans[1].style, n());
    }

    #[test]
    fn test_join_line_format() {
        // Capture 22: ` bxrecon23 [~node@host] has joined #chan`, nick bold
        // cyan, brackets dark grey, host cyan, the rest plain.
        let j = Line::Join {
            nick: "armin".into(),
            mxid: "@armin:remote.tld".into(),
        };
        assert_eq!(text(&j), " armin [@armin:remote.tld] has joined #room");
        let l = format_line(&j, "#room");
        assert_eq!(style_of(&l, "armin "), bold(Color::Cyan));
        assert_eq!(style_of(&l, "["), k());
        assert_eq!(style_of(&l, "@armin:remote.tld"), c());
        assert_eq!(style_of(&l, "]"), k());
        assert_eq!(style_of(&l, "has joined"), n());
        assert!(!text(&j).contains("-:-"));
    }

    #[test]
    fn test_membership_lines() {
        let p = Line::Part {
            nick: "a".into(),
            mxid: "@a:b".into(),
            reason: Some("bye".into()),
        };
        assert_eq!(text(&p), " a [@a:b] has left #room [bye]");
        // Capture 32: the reason brackets are printed even when empty.
        let p = Line::Part {
            nick: "a".into(),
            mxid: "@a:b".into(),
            reason: None,
        };
        assert_eq!(text(&p), " a [@a:b] has left #room []");
        let k = Line::Kick {
            victim: "a".into(),
            kicker: "op".into(),
            reason: Some("recon kick test".into()),
        };
        assert_eq!(text(&k), " a was kicked off #room by op (recon kick test)");
        let n = Line::Nick { old: "a".into(), new: "b".into() };
        assert_eq!(text(&n), " a is now known as b");
        let t = Line::Topic { nick: Some("op".into()), topic: "hello".into() };
        assert_eq!(text(&t), " Topic (#room): changed by op: hello");
        let m = Line::Mode { modes: "+o b".into(), nick: "op".into() };
        assert_eq!(text(&m), " mode/#room [+o b] by op");
    }

    #[test]
    fn test_notice_and_error() {
        // Capture 27: `-nick(host)- text`.
        let l = Line::Notice {
            nick: "bob".into(),
            mxid: "@bob:x.org".into(),
            text: "test notice".into(),
        };
        assert_eq!(text(&l), "-bob(@bob:x.org)- test notice");
        assert_eq!(style_of(&format_line(&l, "#r"), "bob"), fg(Color::Magenta));
        // Capture 30: ` BitchX: Unknown command: FOO`, "BitchX" bold.
        let e = Line::Error("Unknown command: FOO".into());
        assert_eq!(text(&e), " BitchX: Unknown command: FOO");
        assert!(
            style_of(&format_line(&e, "#r"), "BitchX")
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn test_multiline_body_indents() {
        let l = Line::Public {
            nick: "a".into(),
            own: false,
            text: "one\ntwo".into(),
        };
        assert_eq!(text(&l), "<a> one\n    two");
    }

    #[test]
    fn test_wrap() {
        let l = format_line(&Line::Info("abcdefghij".into()), "#r");
        let w = plain(&wrap(l, 8));
        assert_eq!(w, vec![" abcdefg", "hij"]);
    }

    #[test]
    fn test_sanitize_strips_escapes() {
        let l = Line::Public {
            nick: "a".into(),
            own: false,
            text: "x\u{1b}[2Jy".into(),
        };
        assert_eq!(text(&l), "<a> x [2Jy");
    }

    #[test]
    fn test_names_layout() {
        let h = Line::Users { chan: "#r".into(), shown: 2, total: 2 };
        assert_eq!(text(&h), " [Users(#r:2/2)]");
        let h = format_line(&h, "#r");
        assert_eq!(style_of(&h, "Users"), bold(Color::Green));
        assert_eq!(style_of(&h, ")]"), k());
        let row = Line::Names {
            cells: vec![('@', "op".into()), (' ', "armin".into())],
            width: 5,
        };
        assert_eq!(text(&row), "[@op    ] [ armin ]");
        let r = format_line(&row, "#r");
        assert_eq!(style_of(&r, "@"), bold(Color::Cyan));
        assert_eq!(style_of(&r, "op    "), w());
        assert_eq!(style_of(&r, "armin "), bold(Color::Blue));
    }

    #[test]
    fn test_whois_layout() {
        assert_eq!(text(&Line::WhoisRule), "--------------- --  -");
        let l = Line::WhoisNick {
            nick: "armin".into(),
            mxid: "@armin:x".into(),
            info: "unknown".into(),
        };
        assert_eq!(text(&l), "| armin (@armin:x) (unknown)");
        let row = |row, last, key: &str| {
            text(&Line::WhoisRow { row, last, key: key.into(), value: "v".into() })
        };
        assert_eq!(row(1, false, "ircname"), " ircname  : v");
        assert_eq!(row(2, false, "channels"), "| channels : v");
        assert_eq!(row(3, false, "server"), " server   : v");
        assert_eq!(row(4, true, "devices"), ": devices  : v");
        let r = format_line(
            &Line::WhoisRow {
                row: 1,
                last: false,
                key: "ircname".into(),
                value: "v".into(),
            },
            "#r",
        );
        assert_eq!(r[0].spans[1].content, "i");
        assert_eq!(r[0].spans[1].style, w());
    }
}
