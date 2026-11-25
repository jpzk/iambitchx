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

/// `$G` (SHOW_NUMERICS_STR, ASCII build): bold red `-`, bold white `:`, bold red `-`.
pub fn g() -> Vec<Span<'static>> {
    vec![
        Span::styled("-", bold(Color::LightRed)),
        Span::styled(":", w()),
        Span::styled("-", bold(Color::LightRed)),
        Span::raw(" "),
    ]
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
pub fn format_line(line: &Line, chan: &str) -> Vec<TLine<'static>> {
    match line {
        // PUBLIC `<nick> text` (bold cyan brackets); SEND_PUBLIC highlights
        // our own nick: bold white nick between magenta brackets.
        Line::Public { nick, own, text } => {
            let (bracket, nick_style) = if *own {
                (bold(Color::LightMagenta), w())
            } else {
                (bold(Color::Cyan), n())
            };
            let prefix = vec![
                sp("<", bracket),
                sp(nick.clone(), nick_style),
                sp(">", bracket),
                sp(" ", n()),
            ];
            body_lines(prefix, text, n())
        },
        // ACTION `%K* %W$nick %n$text`
        Line::Action { nick, text, .. } => {
            body_lines(vec![sp("* ", k()), sp(nick.clone(), w()), sp(" ", n())], text, n())
        },
        // PUBLIC_NOTICE `%K-%P$nick%K:%p$chan%K-%n text`
        Line::Notice { nick, text } => {
            let prefix = vec![
                sp("-", k()),
                sp(nick.clone(), fg(Color::LightMagenta)),
                sp(":", k()),
                sp(chan, fg(Color::Magenta)),
                sp("-", k()),
                sp(" ", n()),
            ];
            body_lines(prefix, text, n())
        },
        // JOIN `$G %C$nick %K[%c$userhost%K]%n has joined $chan`
        Line::Join { nick, mxid } => {
            vec![g_line(vec![
                sp(nick.clone(), bold(Color::LightCyan)),
                sp(" [", k()),
                sp(mxid.clone(), c()),
                sp("]", k()),
                sp(format!(" has joined {chan}"), n()),
            ])]
        },
        // LEAVE `$G $nick %K[%w$userhost%K]%n has left $chan %K[%W$reason%K]`
        Line::Part { nick, mxid, reason } => {
            let mut s = vec![
                sp(nick.clone(), n()),
                sp(" [", k()),
                sp(mxid.clone(), fg(Color::Gray)),
                sp("]", k()),
                sp(format!(" has left {chan}"), n()),
            ];
            if let Some(r) = reason {
                s.extend([sp(" [", k()), sp(r.clone(), w()), sp("]", k())]);
            }
            vec![g_line(s)]
        },
        // KICK `$G %n$victim was kicked off $chan by %c$kicker %K(%n$reason%K)`
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
        // NICKNAME `$G %W$old %nis now known as %c$new`
        Line::Nick { old, new } => {
            vec![g_line(vec![
                sp(old.clone(), w()),
                sp(" is now known as ", n()),
                sp(new.clone(), c()),
            ])]
        },
        // TOPIC / TOPIC_CHANGE
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
        // MODE `$G %nmode%K/%c$chan %K[%W$modes%K]%n by %W$nick`
        Line::Mode { modes, nick } => {
            vec![g_line(vec![
                sp("mode", n()),
                sp("/", k()),
                sp(chan, c()),
                sp(" [", k()),
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
        Line::Error(text) => {
            text.split('\n')
                .map(|t| g_line(vec![sp(t, fg(Color::LightRed))]))
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
        Line::Raw(text) => vec![TLine::from(sp(text.clone(), bold(Color::LightBlue)))],
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

    #[test]
    fn test_public_and_action() {
        let l = Line::Public {
            nick: "armin".into(),
            own: false,
            text: "hi".into(),
        };
        assert_eq!(text(&l), "<armin> hi");
        let l = Line::Action {
            nick: "carla".into(),
            own: true,
            text: "waves".into(),
        };
        assert_eq!(text(&l), "* carla waves");
    }

    #[test]
    fn test_own_nick_highlighted() {
        let own =
            format_line(&Line::Public { nick: "me".into(), own: true, text: "x".into() }, "#r");
        let other =
            format_line(&Line::Public { nick: "me".into(), own: false, text: "x".into() }, "#r");
        assert_ne!(own[0].spans[1].style, other[0].spans[1].style);
        assert!(own[0].spans[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_membership_lines() {
        let j = Line::Join {
            nick: "armin".into(),
            mxid: "@armin:remote.tld".into(),
        };
        assert_eq!(text(&j), "-:- armin [@armin:remote.tld] has joined #room");
        let p = Line::Part {
            nick: "a".into(),
            mxid: "@a:b".into(),
            reason: Some("bye".into()),
        };
        assert_eq!(text(&p), "-:- a [@a:b] has left #room [bye]");
        let k = Line::Kick {
            victim: "a".into(),
            kicker: "op".into(),
            reason: None,
        };
        assert_eq!(text(&k), "-:- a was kicked off #room by op");
        let n = Line::Nick { old: "a".into(), new: "b".into() };
        assert_eq!(text(&n), "-:- a is now known as b");
        let t = Line::Topic { nick: Some("op".into()), topic: "hello".into() };
        assert_eq!(text(&t), "-:- Topic (#room): changed by op: hello");
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
        assert_eq!(w, vec!["-:- abcd", "efghij"]);
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
}
