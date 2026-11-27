//! # BitchX message formats
//!
//! [`Line`]s rendered with the "default bx" FSETs of BitchX1.3
//! `include/color.h`; see `docs/bitchx-formats.md`.
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TLine, Span};
use unicode_width::UnicodeWidthChar;

use crate::bitchx::model::{Ink, Line, PresenceMark};
use crate::bitchx::palette::pal;

pub(crate) fn fg(c: Color) -> Style {
    Style::default().fg(c)
}

pub(crate) fn bold(c: Color) -> Style {
    fg(c).add_modifier(Modifier::BOLD)
}

/// A foreground colour, or the terminal default for `None`.
pub(crate) fn opt_fg(c: Option<Color>) -> Style {
    c.map_or_else(Style::default, fg)
}

/// `%n`: plain text, the terminal's default colours unless the palette sets one.
pub(crate) fn n() -> Style {
    opt_fg(pal().text)
}

/// `%K`: dark gray (bold black).
pub(crate) fn k() -> Style {
    bold(pal().punct)
}

/// `%W`: bold bright white.
pub(crate) fn w() -> Style {
    bold(pal().bright)
}

/// `%c`: cyan.
pub(crate) fn c() -> Style {
    fg(pal().accent)
}

/// Replace control characters (escape sequences, newlines) with spaces and
/// drop invisible format characters (bidi controls, zero-width, F30).
pub(crate) fn sanitize(s: &str) -> String {
    crate::bitchx::text::one_line(s)
}

pub(crate) fn sp(text: impl Into<String>, style: Style) -> Span<'static> {
    Span::styled(sanitize(&text.into()), style)
}

/// The terminal colour for an [`Ink`]; `Default` keeps the terminal's own.
pub(crate) fn ink_color(ink: Ink) -> Option<Color> {
    Some(match ink {
        Ink::Default => return None,
        Ink::Black => pal().logo_ink[0],
        Ink::Red => pal().logo_ink[1],
        Ink::Blue => pal().logo_ink[2],
        Ink::Magenta => pal().logo_ink[3],
        Ink::Cyan => pal().logo_ink[4],
        Ink::White => pal().logo_ink[5],
    })
}

/// The leading glyph of every client/event line: a single (dark-grey) space,
/// as captured from BitchX (recon §3).
pub(crate) fn g() -> Vec<Span<'static>> {
    vec![Span::styled(" ", k())]
}

fn g_line(rest: Vec<Span<'static>>) -> TLine<'static> {
    let mut spans = g();
    spans.extend(rest);
    TLine::from(spans)
}

/// Lines of a multi-line body, the continuation lines indented under the text.
fn body_lines(prefix: &[Span<'static>], text: &str, style: Style) -> Vec<TLine<'static>> {
    let indent = " ".repeat(TLine::from(prefix.to_vec()).width());
    text.split('\n')
        .enumerate()
        .map(|(i, part)| {
            let mut spans = if i == 0 {
                prefix.to_vec()
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
#[cfg(test)]
pub(crate) fn format_line(line: &Line, chan: &str) -> Vec<TLine<'static>> {
    format_line_hl(line, chan, false)
}

/// The whole-line highlight of a mention: the text bold bright white.
pub(crate) fn hl_text() -> Style {
    w()
}

/// The AR (auto-reply, i.e. mention) FSETs of BitchX1.3 `include/color.h`
/// (`FORMAT_PUBLIC_AR` `%b<%Y$1%b>%n $3-`, `FORMAT_ACTION_AR`
/// `%K* %Y$1 %n$4-`, `FORMAT_MSG_AR` `%K[%Y$1%K(%Y$2%K)]%n $3-`), the nick
/// bold yellow; on top of BitchX the text is bold bright white so the line
/// stands out in a busy channel. Lines without an AR form get the text
/// highlight only.
fn format_ar(line: &Line) -> Option<Vec<TLine<'static>>> {
    let y = bold(pal().highlight);
    Some(match line {
        Line::Public { nick, own: false, text } => {
            let prefix = vec![
                sp("<", fg(pal().nick)),
                sp(nick.clone(), y),
                sp(">", fg(pal().nick)),
                sp(" ", n()),
            ];
            body_lines(&prefix, text, hl_text())
        },
        Line::Action { nick, own: false, text } => {
            body_lines(&[sp("* ", k()), sp(format!("{nick} "), y)], text, hl_text())
        },
        Line::Private { nick, mxid, own: false, text } => {
            let prefix = vec![
                sp("[", k()),
                sp(nick.clone(), y),
                sp("(", k()),
                sp(mxid.clone(), y),
                sp(")]", k()),
                sp(" ", n()),
            ];
            body_lines(&prefix, text, hl_text())
        },
        Line::Notice { nick, mxid, text } => {
            let prefix = vec![
                sp("-", k()),
                sp(nick.clone(), y),
                sp("(", k()),
                sp(mxid.clone(), fg(pal().query_in)),
                sp(")-", k()),
                sp(" ", n()),
            ];
            body_lines(&prefix, text, hl_text())
        },
        _ => return None,
    })
}

/// `format_line` (test-only), with `hl` (a mention, see [`crate::bitchx::mention`])
/// rendering the AR form.
pub(crate) fn format_line_hl(line: &Line, chan: &str, hl: bool) -> Vec<TLine<'static>> {
    if hl && let Some(out) = format_ar(line) {
        return out;
    }
    match line {
        // `<nick> text`: only the brackets are coloured, bold magenta for
        // our own lines and bold blue for everyone else.
        Line::Public { nick, own, text } => {
            let bracket = if *own {
                bold(pal().own_nick)
            } else {
                bold(pal().nick)
            };
            let prefix = vec![
                sp("<", bracket),
                sp(nick.clone(), n()),
                sp(">", bracket),
                sp(" ", n()),
            ];
            body_lines(&prefix, text, n())
        },
        // Own: ` nick text` (nick bold white). Others: ` nick/#chan text`
        // (nick and channel bold yellow).
        Line::Action { nick, own, text } => {
            let who = if *own {
                sp(format!("{nick} "), w())
            } else {
                sp(format!("{nick}/{chan} "), bold(pal().highlight))
            };
            body_lines(&[sp(" ", k()), who], text, n())
        },
        // Query lines (capture 29). Sent: `[msg(peer)] text`, `msg` red and
        // the peer bold red. Received: `[nick(mxid)] text`, nick bold
        // magenta, host magenta. Punctuation dark grey.
        Line::Private { nick, mxid, own, text } => {
            let prefix = if *own {
                vec![
                    sp("[", k()),
                    sp("msg", fg(pal().query_out)),
                    sp("(", k()),
                    sp(nick.clone(), bold(pal().query_out)),
                    sp(")]", k()),
                    sp(" ", n()),
                ]
            } else {
                vec![
                    sp("[", k()),
                    sp(nick.clone(), bold(pal().query_in)),
                    sp("(", k()),
                    sp(mxid.clone(), fg(pal().query_in)),
                    sp(")]", k()),
                    sp(" ", n()),
                ]
            };
            body_lines(&prefix, text, n())
        },
        // `-nick(mxid)- text`: punctuation dark grey, nick and host magenta.
        Line::Notice { nick, mxid, text } => {
            let prefix = vec![
                sp("-", k()),
                sp(nick.clone(), fg(pal().query_in)),
                sp("(", k()),
                sp(mxid.clone(), fg(pal().query_in)),
                sp(")-", k()),
                sp(" ", n()),
            ];
            body_lines(&prefix, text, n())
        },
        // ` nick [mxid] has joined #chan`: nick bold cyan, host cyan.
        Line::Join { nick, mxid } => {
            vec![g_line(vec![
                sp(format!("{nick} "), bold(pal().join)),
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
                    s.extend([sp("[", k()), sp(r.clone(), n()), sp("]", k())]);
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
        // ` nick changed display name to name`: nick bold white, name cyan.
        Line::DisplayName { nick, name } => {
            let mut s = vec![sp(format!("{nick} "), w())];
            match name {
                Some(name) => {
                    s.push(sp("changed display name to ", n()));
                    s.push(sp(name.clone(), c()));
                },
                None => s.push(sp("removed their display name", n())),
            }
            vec![g_line(s)]
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
        // FORMAT_SET (default bx): `$G    %K$tolower($[27]0) %w$1-`;
        // FORMAT_SET_NOVALUE: `$G    %K$tolower($0) has no value`.
        Line::SetVar { name, value } => {
            let name = name.to_lowercase();
            vec![g_line(match value {
                Some(v) => {
                    vec![
                        sp(format!("    {name:<27} "), k()),
                        sp(v.clone(), fg(pal().value)),
                    ]
                },
                None => vec![sp(format!("    {name} has no value"), k())],
            })]
        },
        // One entry, one row (F-FMT-1): an embedded LF is never a row break,
        // `sp()` turns it into a space, so text that skipped the ingest
        // cleaner still cannot forge a client-looking row.
        Line::Info(text) => vec![g_line(vec![sp(text.clone(), n())])],
        // ` iamBitchX: text` ("iamBitchX" bold), as for client errors.
        Line::Error(text) => vec![g_line(vec![
            sp("iamBitchX", n().add_modifier(Modifier::BOLD)),
            sp(format!(": {text}"), n()),
        ])],
        Line::Debug { cat, text } => {
            let mut spans = vec![sp("DBG", fg(pal().debug).add_modifier(Modifier::BOLD))];
            if !cat.is_empty() {
                spans.push(sp(format!(" {cat}"), fg(pal().debug)));
            }
            spans.push(sp(format!(" {text}"), fg(pal().debug_text)));
            vec![g_line(spans)]
        },
        Line::Undecryptable { nick, reason } => {
            vec![g_line(vec![
                sp("[", k()),
                sp(format!("unable to decrypt: {reason}"), fg(pal().error)),
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
        Line::Logo(text) => {
            vec![TLine::from(sp(
                text.clone(),
                bold(pal().logo).add_modifier(Modifier::ITALIC),
            ))]
        },
        // The logo's shadow/fade rows: blue, dimmed, no italic.
        Line::LogoDim(text) => {
            vec![TLine::from(sp(
                text.clone(),
                fg(pal().logo).add_modifier(Modifier::DIM),
            ))]
        },
        // The splash art in the original's own colours: one span per run.
        Line::LogoRuns(runs) => {
            let spans: Vec<Span<'static>> = runs
                .iter()
                .map(|r| {
                    let mut st = Style::default();
                    if let Some(c) = ink_color(r.fg) {
                        st = st.fg(c);
                    }
                    if let Some(c) = ink_color(r.bg) {
                        st = st.bg(c);
                    }
                    if r.bold {
                        st = st.add_modifier(Modifier::BOLD);
                    }
                    sp(r.text.clone(), st)
                })
                .collect();
            vec![TLine::from(spans)]
        },
        // ` [Users(#chan:n/m)]` (capture 25).
        Line::Users { chan, shown, total } => {
            let gr = fg(pal().good);
            vec![g_line(vec![
                sp("[", k()),
                sp("Users", bold(pal().good)),
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
        // Away/offline peers carry their marker: `[ armin (zZzZ 4h) ]`.
        Line::Names { cells, width } => {
            let mut spans = vec![];
            for (i, (flag, nick, mark)) in cells.iter().enumerate() {
                if i > 0 {
                    spans.push(sp(" ", n()));
                }
                let (fs, ns) = match flag {
                    '@' => (bold(pal().op), w()),
                    '+' => (bold(pal().good), bold(pal().nick)),
                    _ => (k(), bold(pal().nick)),
                };
                spans.push(sp("[", k()));
                spans.push(sp(flag.to_string(), fs));
                match mark {
                    None => spans.push(sp(format!("{nick:<width$} "), ns)),
                    Some(m) => {
                        spans.push(sp(nick.clone(), ns));
                        spans.push(sp(" (", k()));
                        spans.extend(mark_spans(m));
                        spans.push(sp(")", k()));
                        let pad = width.saturating_sub(names_cell_width(nick, Some(m)));
                        spans.push(sp(" ".repeat(pad + 1), ns));
                    },
                }
                spans.push(sp("]", k()));
            }
            vec![TLine::from(spans)]
        },
        // The fading green/grey ruler that opens a WHOIS (capture 26).
        Line::WhoisRule => {
            vec![TLine::from(vec![
                sp("------", bold(pal().good)),
                sp("------", fg(pal().good)),
                sp("--- --  -", k()),
            ])]
        },
        Line::WhoisNick { nick, mxid, info } => {
            vec![TLine::from(vec![
                sp("| ", bold(pal().good)),
                sp(nick.clone(), w()),
                sp(" (", k()),
                sp(mxid.clone(), n()),
                sp(") (", k()),
                sp(info.clone(), w()),
                sp(")", k()),
            ])]
        },
        Line::WhoisMark { nick, mxid, info, mark } => {
            let mut l = format_line_hl(
                &Line::WhoisNick {
                    nick: nick.clone(),
                    mxid: mxid.clone(),
                    info: info.clone(),
                },
                chan,
                false,
            );
            l[0].spans.push(sp(" (", k()));
            l[0].spans.extend(mark_spans(mark));
            l[0].spans.push(sp(")", k()));
            l
        },
        Line::WhoisRow { row, last, key, value } => {
            let lead = match (last, row % 2) {
                (true, _) => ": ",
                (false, 1) => " ",
                _ => "| ",
            };
            let ls = match row {
                0 | 1 => bold(pal().good),
                2 => fg(pal().good),
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

/// A presence marker `zZzZ 4h` / `off 2d`: green zZzZ like `STATUS_AWAY`,
/// grey `off`, the age in the default colour.
pub(crate) fn mark_spans(mark: &PresenceMark) -> Vec<Span<'static>> {
    let word = if mark.offline { k() } else { fg(pal().good) };
    let mut out = vec![sp(mark.word(), word)];
    if let Some(age) = &mark.age {
        out.push(sp(format!(" {age}"), n()));
    }
    out
}

/// Columns of a NAMES cell: the nick plus ` (marker)`.
pub(crate) fn names_cell_width(nick: &str, mark: Option<&PresenceMark>) -> usize {
    let nick = nick.chars().count();
    mark.map_or(nick, |m| nick + m.text().chars().count() + 3)
}

/// Split lines so that none is wider than `width` cells.
pub(crate) fn wrap(lines: Vec<TLine<'static>>, width: usize) -> Vec<TLine<'static>> {
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
pub(crate) fn plain(lines: &[TLine<'_>]) -> Vec<String> {
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
    fn test_mention_renders_ar_and_highlights_line() {
        let l = Line::Public {
            nick: "armin".into(),
            own: false,
            text: "yo carla".into(),
        };
        let r = format_line_hl(&l, "#r", true);
        assert_eq!(plain(&r), vec!["<armin> yo carla"]);
        // FORMAT_PUBLIC_AR `%b<%Y$1%b>%n $3-`, plus the whole text bold white.
        assert_eq!(style_of(&r, "<"), fg(Color::Blue));
        assert_eq!(style_of(&r, "armin"), bold(Color::Yellow));
        assert_eq!(style_of(&r, "yo carla"), hl_text());
        assert_eq!(hl_text(), bold(Color::White));
        // Not a mention: unchanged default bx.
        assert_eq!(format_line_hl(&l, "#r", false), format_line(&l, "#r"));
        assert_eq!(style_of(&format_line(&l, "#r"), "yo carla"), n());
        // Own lines and event lines never take the AR form.
        let own = Line::Public {
            nick: "carla".into(),
            own: true,
            text: "carla".into(),
        };
        assert_eq!(format_line_hl(&own, "#r", true), format_line(&own, "#r"));
        // FORMAT_ACTION_AR `%K* %Y$1 %n$4-` and FORMAT_MSG_AR.
        let act = Line::Action {
            nick: "armin".into(),
            own: false,
            text: "pokes carla".into(),
        };
        let r = format_line_hl(&act, "#r", true);
        assert_eq!(plain(&r), vec!["* armin pokes carla"]);
        assert_eq!(style_of(&r, "armin"), bold(Color::Yellow));
        assert_eq!(style_of(&r, "pokes"), hl_text());
        let q = Line::Private {
            nick: "armin".into(),
            mxid: "@armin:x".into(),
            own: false,
            text: "carla?\nline2".into(),
        };
        let r = format_line_hl(&q, "#r", true);
        assert_eq!(plain(&r), vec!["[armin(@armin:x)] carla?", "                  line2"]);
        assert_eq!(style_of(&r, "@armin:x"), bold(Color::Yellow));
        assert_eq!(r[1].spans[1].style, hl_text(), "continuation lines highlighted too");
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
    #[allow(
        clippy::many_single_char_names,
        reason = "short fixture names: one letter per membership event kind"
    )]
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
        let n = Line::DisplayName { nick: "carla".into(), name: Some("Carla".into()) };
        assert_eq!(text(&n), " carla changed display name to Carla");
        let n = Line::DisplayName { nick: "carla".into(), name: None };
        assert_eq!(text(&n), " carla removed their display name");
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
        assert_eq!(text(&e), " iamBitchX: Unknown command: FOO");
        assert!(
            style_of(&format_line(&e, "#r"), "iamBitchX")
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn test_query_message_formats() {
        // Capture 29: `[msg(bxrecon78)] private hello`, `msg` red, nick bold red.
        let sent = Line::Private {
            nick: "armin".into(),
            mxid: "@armin:remote.tld".into(),
            own: true,
            text: "private hello".into(),
        };
        assert_eq!(text(&sent), "[msg(armin)] private hello");
        let f = format_line(&sent, "armin");
        assert_eq!(style_of(&f, "msg"), fg(Color::Red));
        assert_eq!(style_of(&f, "armin"), bold(Color::Red));
        assert_eq!(style_of(&f, "["), k());
        // `[bxrecon78(~node@host)] private reply`: nick bold magenta, host magenta.
        let recv = Line::Private {
            nick: "armin".into(),
            mxid: "@armin:remote.tld".into(),
            own: false,
            text: "private reply".into(),
        };
        assert_eq!(text(&recv), "[armin(@armin:remote.tld)] private reply");
        let f = format_line(&recv, "armin");
        assert_eq!(style_of(&f, "armin"), bold(Color::Magenta));
        assert_eq!(style_of(&f, "@armin:remote.tld"), fg(Color::Magenta));
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
            cells: vec![('@', "op".into(), None), (' ', "armin".into(), None)],
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

    /// F-FMT-1: an embedded LF in an Info/Error line is not a row break,
    /// even when the text never passed the ingest cleaner.
    #[test]
    fn test_ffmt1_info_error_embedded_lf_is_one_row() {
        for p in crate::bitchx::text::HOSTILE {
            for line in [
                Line::Info(format!("note {p}")),
                Line::Error(format!("sync: {p}")),
            ] {
                let rows = plain(&format_line(&line, "#room"));
                assert_eq!(rows.len(), 1, "{rows:?}");
                crate::bitchx::text::assert_inert(&rows[0]);
            }
        }
    }
}
