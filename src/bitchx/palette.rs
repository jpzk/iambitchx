//! # Colour palette
//!
//! Every colour the BitchX screen draws comes from a [`Palette`] of semantic
//! roles. [`Palette::bitchx`] is the "default bx" look of BitchX1.3
//! (`include/color.h`); themes (see [`crate::bitchx::theme`]) supply others.
//!
//! The palette in use lives in a thread-local: the renderer runs on one
//! thread, formats every line at draw time, and so picks up a new palette
//! (a `/theme` preview) on the very next frame. Tests run one per thread and
//! therefore always start from the default.
use std::cell::Cell;

use ratatui::style::Color;

/// The semantic colour roles of the BitchX screen. `None` means the
/// terminal's own default colour (`%n`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Palette {
    /// `%n`: plain text (message bodies, info lines, part lines, topics).
    pub text: Option<Color>,
    /// Background behind the scrollback and the input line.
    pub background: Option<Color>,
    /// `%K`: punctuation, brackets, the leading glyph.
    pub punct: Color,
    /// `%W`: emphasis (mention text, own action nick, modes, whois values).
    pub bright: Color,
    /// `%c`: hosts, channel names, kickers, renamed display names.
    pub accent: Color,
    /// `<>` brackets of other people's messages; nicks in `/names`.
    pub nick: Color,
    /// `<>` brackets of our own messages.
    pub own_nick: Color,
    /// Mentions (the AR nick) and other people's `/me` nick.
    pub highlight: Color,
    /// Sent queries: `[msg(peer)]`.
    pub query_out: Color,
    /// Received queries and notices: `[nick(mxid)]`, `-nick(mxid)-`.
    pub query_in: Color,
    /// The nick of a join line.
    pub join: Color,
    /// The `@` flag of an op in `/names`.
    pub op: Color,
    /// Green: `[Users(...)]`, the whois ruler, the `+` flag, `zZzZ`.
    pub good: Color,
    /// `/set` values.
    pub value: Color,
    /// Errors: undecryptable events.
    pub error: Color,
    /// `DBG` lines: tag and category.
    pub debug: Color,
    /// `DBG` lines: the text.
    pub debug_text: Color,
    /// The `%@` timestamp.
    pub timestamp: Option<Color>,
    /// The input line (prompt and text).
    pub input: Option<Color>,
    /// The fallback logo (bold italic) and its shadow rows.
    pub logo: Color,
    /// The splash art inks, indexed like [`crate::bitchx::model::Ink`]:
    /// black, red, blue, magenta, cyan, white.
    pub logo_ink: [Color; 6],
    /// Status bars: background.
    pub status_bg: Color,
    /// Status bars: plain text.
    pub status_fg: Color,
    /// Status bars: brackets and flags.
    pub status_punct: Color,
    /// Status bars: bold values (clock, nick, channel).
    pub status_bright: Color,
    /// Status bars: `zZzZ`.
    pub status_away: Color,
    /// Status bars: a window with an unread mention in `Act:`.
    pub status_hilite: Color,
    /// Status bars: `[SYNC LOST]`.
    pub status_alert: Color,
}

impl Palette {
    /// The colours iamBitchX always had: "default bx".
    pub(crate) const fn bitchx() -> Self {
        Palette {
            text: None,
            background: None,
            punct: Color::DarkGray,
            bright: Color::White,
            accent: Color::Cyan,
            nick: Color::Blue,
            own_nick: Color::Magenta,
            highlight: Color::Yellow,
            query_out: Color::Red,
            query_in: Color::Magenta,
            join: Color::Cyan,
            op: Color::Cyan,
            good: Color::Green,
            value: Color::Gray,
            error: Color::LightRed,
            debug: Color::Magenta,
            debug_text: Color::DarkGray,
            timestamp: None,
            input: None,
            logo: Color::Blue,
            logo_ink: [
                Color::Black,
                Color::Red,
                Color::Blue,
                Color::Magenta,
                Color::Cyan,
                Color::White,
            ],
            status_bg: Color::Blue,
            status_fg: Color::Gray,
            status_punct: Color::Cyan,
            status_bright: Color::White,
            status_away: Color::Green,
            status_hilite: Color::Yellow,
            status_alert: Color::LightRed,
        }
    }

    /// The `terminal` theme: overrides nothing. Every role is
    /// [`Color::Reset`] (or `None`), so text, status bars, the input line,
    /// the picker and the splash all use the terminal's own default
    /// foreground and background.
    ///
    /// The palette carries no modifiers, so the structural, colour-free
    /// emphasis drawn by the formatter stays: bold (own/others' nicks,
    /// mentions, status values, `%W` text), italic (the fallback logo) and
    /// the picker's reversed selection row, so the cursor stays visible.
    pub(crate) const fn terminal() -> Self {
        let r = Color::Reset;
        Palette {
            text: None,
            background: None,
            punct: r,
            bright: r,
            accent: r,
            nick: r,
            own_nick: r,
            highlight: r,
            query_out: r,
            query_in: r,
            join: r,
            op: r,
            good: r,
            value: r,
            error: r,
            debug: r,
            debug_text: r,
            timestamp: None,
            input: None,
            logo: r,
            logo_ink: [r; 6],
            status_bg: r,
            status_fg: r,
            status_punct: r,
            status_bright: r,
            status_away: r,
            status_hilite: r,
            status_alert: r,
        }
    }

    /// Every colour mapped to what a terminal of `depth` can show.
    pub(crate) fn degrade(self, depth: ColorDepth) -> Self {
        let c = |c: Color| depth.fit(c);
        let o = |c: Option<Color>| c.map(|c| depth.fit(c));
        Palette {
            text: o(self.text),
            background: o(self.background),
            punct: c(self.punct),
            bright: c(self.bright),
            accent: c(self.accent),
            nick: c(self.nick),
            own_nick: c(self.own_nick),
            highlight: c(self.highlight),
            query_out: c(self.query_out),
            query_in: c(self.query_in),
            join: c(self.join),
            op: c(self.op),
            good: c(self.good),
            value: c(self.value),
            error: c(self.error),
            debug: c(self.debug),
            debug_text: c(self.debug_text),
            timestamp: o(self.timestamp),
            input: o(self.input),
            logo: c(self.logo),
            logo_ink: self.logo_ink.map(c),
            status_bg: c(self.status_bg),
            status_fg: c(self.status_fg),
            status_punct: c(self.status_punct),
            status_bright: c(self.status_bright),
            status_away: c(self.status_away),
            status_hilite: c(self.status_hilite),
            status_alert: c(self.status_alert),
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Palette::bitchx()
    }
}

/// How many colours the terminal can show.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ColorDepth {
    /// 24-bit RGB.
    TrueColor,
    /// The xterm 256-colour palette.
    Indexed256,
    /// The 16 ANSI colours.
    Ansi16,
}

/// The xterm RGB values of the 16 ANSI colours.
const ANSI16: [(Color, (u8, u8, u8)); 16] = [
    (Color::Black, (0, 0, 0)),
    (Color::Red, (205, 0, 0)),
    (Color::Green, (0, 205, 0)),
    (Color::Yellow, (205, 205, 0)),
    (Color::Blue, (0, 0, 238)),
    (Color::Magenta, (205, 0, 205)),
    (Color::Cyan, (0, 205, 205)),
    (Color::Gray, (229, 229, 229)),
    (Color::DarkGray, (127, 127, 127)),
    (Color::LightRed, (255, 0, 0)),
    (Color::LightGreen, (0, 255, 0)),
    (Color::LightYellow, (255, 255, 0)),
    (Color::LightBlue, (92, 92, 255)),
    (Color::LightMagenta, (255, 0, 255)),
    (Color::LightCyan, (0, 255, 255)),
    (Color::White, (255, 255, 255)),
];

/// The six levels of the xterm 6x6x6 colour cube.
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

fn dist((r1, g1, b1): (u8, u8, u8), (r2, g2, b2): (u8, u8, u8)) -> u32 {
    let d = |a: u8, b: u8| u32::from(a.abs_diff(b)).pow(2);
    d(r1, r2) + d(g1, g2) + d(b1, b2)
}

/// The RGB of an xterm palette index.
fn index_rgb(i: u8) -> (u8, u8, u8) {
    match i {
        0..=15 => ANSI16[usize::from(i)].1,
        16..=231 => {
            let i = i - 16;
            (CUBE[usize::from(i / 36)], CUBE[usize::from(i / 6 % 6)], CUBE[usize::from(i % 6)])
        },
        _ => {
            let v = 8 + (i - 232) * 10;
            (v, v, v)
        },
    }
}

/// The nearest of the 16 ANSI colours.
fn nearest16(rgb: (u8, u8, u8)) -> Color {
    ANSI16
        .iter()
        .min_by_key(|(_, c)| dist(*c, rgb))
        .map_or(Color::Reset, |(c, _)| *c)
}

/// The nearest xterm 256-palette index (cube or grey ramp, 16..=255).
fn nearest256(rgb: (u8, u8, u8)) -> u8 {
    (16..=255u8).min_by_key(|i| dist(index_rgb(*i), rgb)).unwrap_or(16)
}

impl ColorDepth {
    /// Guess from `COLORTERM` and `TERM`.
    pub(crate) fn detect(colorterm: &str, term: &str) -> Self {
        let ct = colorterm.to_lowercase();
        if ct.contains("truecolor") || ct.contains("24bit") {
            ColorDepth::TrueColor
        } else if term.contains("256") {
            ColorDepth::Indexed256
        } else {
            ColorDepth::Ansi16
        }
    }

    /// The depth of the terminal we run in.
    pub(crate) fn from_env() -> Self {
        let get = |k| std::env::var(k).unwrap_or_default();
        ColorDepth::detect(&get("COLORTERM"), &get("TERM"))
    }

    /// `c` as this terminal can show it: RGB and indexed colours are moved
    /// to the nearest one available; the named ANSI colours always fit.
    pub(crate) fn fit(self, c: Color) -> Color {
        match (self, c) {
            (ColorDepth::TrueColor, c) => c,
            (ColorDepth::Indexed256, Color::Rgb(r, g, b)) => Color::Indexed(nearest256((r, g, b))),
            (ColorDepth::Ansi16, Color::Rgb(r, g, b)) => nearest16((r, g, b)),
            (ColorDepth::Ansi16, Color::Indexed(i)) => nearest16(index_rgb(i)),
            (_, c) => c,
        }
    }
}

thread_local! {
    static CURRENT: Cell<Palette> = const { Cell::new(Palette::bitchx()) };
    static DEPTH: Cell<ColorDepth> = const { Cell::new(ColorDepth::TrueColor) };
}

/// The palette in use.
pub(crate) fn pal() -> Palette {
    CURRENT.with(Cell::get)
}

/// Use `p` from the next frame on, fitted to the terminal's colour depth.
pub(crate) fn set_palette(p: Palette) {
    let depth = DEPTH.with(Cell::get);
    CURRENT.with(|c| c.set(p.degrade(depth)));
}

/// Set the terminal's colour depth (applies to later [`set_palette`] calls).
pub(crate) fn set_depth(depth: ColorDepth) {
    DEPTH.with(|d| d.set(depth));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_palette_is_bitchx() {
        assert_eq!(pal(), Palette::bitchx());
        assert_eq!(Palette::default(), Palette::bitchx());
        // The named colours are left alone at every depth.
        for d in [
            ColorDepth::TrueColor,
            ColorDepth::Indexed256,
            ColorDepth::Ansi16,
        ] {
            assert_eq!(Palette::bitchx().degrade(d), Palette::bitchx());
        }
    }

    #[test]
    fn test_degrade_to_nearest() {
        let a16 = ColorDepth::Ansi16;
        assert_eq!(a16.fit(Color::Rgb(250, 10, 10)), Color::LightRed);
        assert_eq!(a16.fit(Color::Rgb(0, 0, 230)), Color::Blue);
        assert_eq!(a16.fit(Color::Rgb(30, 30, 30)), Color::Black);
        assert_eq!(a16.fit(Color::Indexed(196)), Color::LightRed);
        let a256 = ColorDepth::Indexed256;
        assert_eq!(a256.fit(Color::Rgb(255, 0, 0)), Color::Indexed(196));
        assert_eq!(a256.fit(Color::Rgb(0x1c, 0x1c, 0x1c)), Color::Indexed(234));
        assert_eq!(a256.fit(Color::Cyan), Color::Cyan);
        assert_eq!(ColorDepth::TrueColor.fit(Color::Rgb(1, 2, 3)), Color::Rgb(1, 2, 3));
        // Every index and many RGB values map without panicking.
        for i in 0..=255u8 {
            let _ = a16.fit(Color::Indexed(i));
            let _ = a256.fit(Color::Rgb(i, 255 - i, i / 2));
        }
    }

    #[test]
    fn test_detect_depth() {
        assert_eq!(ColorDepth::detect("truecolor", "xterm"), ColorDepth::TrueColor);
        assert_eq!(ColorDepth::detect("24bit", "screen"), ColorDepth::TrueColor);
        assert_eq!(ColorDepth::detect("", "xterm-256color"), ColorDepth::Indexed256);
        assert_eq!(ColorDepth::detect("", "linux"), ColorDepth::Ansi16);
        assert_eq!(ColorDepth::detect("", ""), ColorDepth::Ansi16);
    }

    #[test]
    fn test_set_palette_degrades() {
        let mut p = Palette::bitchx();
        p.status_bg = Color::Rgb(0, 0, 230);
        set_depth(ColorDepth::Ansi16);
        set_palette(p);
        assert_eq!(pal().status_bg, Color::Blue);
        set_depth(ColorDepth::TrueColor);
        set_palette(Palette::bitchx());
        assert_eq!(pal(), Palette::bitchx());
    }
}
