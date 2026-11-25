//! # BitchX status lines
//!
//! The two blue status lines BitchX draws above the input line
//! (`DOUBLE_STATUS_LINE`), built from the "default bx" STATUS_FORMAT1 and
//! STATUS_FORMAT2 in BitchX1.3 `include/color.h`, with the expandos of
//! `source/status.c`.
use ratatui::style::{Color, Modifier as StyleModifier, Style};
use ratatui::text::Line;

use crate::bitchx::format::sp;

/// The status bar background (`ESC[44m`).
pub fn status_bg() -> Style {
    Style::default().bg(Color::Blue).fg(Color::Gray)
}

/// `ESC[0;44;36m`: cyan brackets on blue.
fn st_punct() -> Style {
    status_bg().fg(Color::Cyan)
}

/// `ESC[1;37m`: bold bright white on blue.
fn st_bright() -> Style {
    status_bg().fg(Color::White).add_modifier(StyleModifier::BOLD)
}

/// `ESC[0;44;37m`: white on blue.
fn st_text() -> Style {
    status_bg().fg(Color::Gray)
}

/// `%C` is cut to this many columns (`#bxrecon-0122` shows as `#bxrecon-0`).
pub const CHANNEL_WIDTH: usize = 10;

/// The first `n` characters of `s`.
pub fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `%T`: BitchX's 12-hour clock, `09:24pm` (CLOCK_24HOUR off).
pub fn clock(t: impl chrono::Timelike) -> String {
    let (pm, h) = t.hour12();
    format!("{h:02}:{:02}{}", t.minute(), if pm { "pm" } else { "am" })
}

/// Member counts for `%!` (STATUS_USERS): `[O/n N/n I/n V/n F/n]`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UserCounts {
    pub ops: usize,
    pub nonops: usize,
    pub voice: usize,
}

/// Everything shown on the two status lines.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BxStatus {
    /// `%T`
    pub time: String,
    /// `%N`
    pub nick: String,
    /// `%#`, without the `+`.
    pub umodes: String,
    /// `%A`
    pub away: bool,
    /// `%@`
    pub chanop: bool,
    /// `%=`
    pub voice: bool,
    /// `%C`; `None` in the status window and in query (DM) windows.
    pub channel: Option<String>,
    /// `%Q`: the query target of a DM window.
    pub query: Option<String>,
    /// `%+`, without the `+`.
    pub modes: String,
    /// `%F`: refnums of other windows with unread activity.
    pub act: Vec<usize>,
    /// `%R`: this window's refnum.
    pub window: usize,
    /// `%!`; `None` outside of channels.
    pub users: Option<UserCounts>,
    /// `%L`: lag to the homeserver in seconds.
    pub lag: u64,
}

impl BxStatus {
    /// STATUS_FORMAT1: ` [%T][%*%@%=%N%#%A]%M [%C%+%W] %Q %H%B%F`.
    ///
    /// As captured (recon 22, 29):
    /// ` [09:24pm][@nick(+iw)] [#bxrecon-0(+nt)]  [Query: x]`.
    pub fn line1(&self) -> Line<'static> {
        let mut s = vec![
            sp(" ", st_bright()),
            sp("[", st_punct()),
            sp(self.time.clone(), st_bright()),
            sp("][", st_punct()),
        ];

        if self.chanop {
            s.push(sp("@", st_bright()));
        } else if self.voice {
            s.push(sp("+", st_bright()));
        }

        s.push(sp(self.nick.clone(), st_text()));

        if !self.umodes.is_empty() {
            s.push(sp("(", st_bright()));
            s.push(sp("+", st_punct()));
            s.push(sp(self.umodes.clone(), st_text()));
            s.push(sp(")", st_bright()));
        }

        if self.away {
            s.push(sp(" (zZzZ)", st_text()));
        }

        s.push(sp("]", st_punct()));

        // `[%C%+]`: BitchX prints `[]` when there is no channel (capture 02)
        // and truncates the channel to 10 columns (`#bxrecon-0`).
        s.push(sp(" [", st_punct()));
        if let Some(chan) = &self.channel {
            s.push(sp(truncate(chan, CHANNEL_WIDTH), st_text()));

            if !self.modes.is_empty() {
                s.push(sp("(", st_bright()));
                s.push(sp("+", st_punct()));
                s.push(sp(self.modes.clone(), st_text()));
                s.push(sp(")", st_bright()));
            }
        }
        s.push(sp("]", st_punct()));

        // `%Q` (STATUS_QUERY ` [Query: %W%Q]`), after a double space.
        if let Some(q) = &self.query {
            s.push(sp("  [", st_punct()));
            s.push(sp("Query: ", st_text()));
            s.push(sp(q.clone(), st_bright()));
            s.push(sp("]", st_punct()));
        }

        if !self.act.is_empty() {
            let act: Vec<String> = self.act.iter().map(usize::to_string).collect();
            s.push(sp(" [", st_punct()));
            s.push(sp("Act: ", st_text()));
            s.push(sp(act.join(","), st_bright()));
            s.push(sp("]", st_punct()));
        }

        Line::from(s).style(status_bg())
    }

    /// STATUS_FORMAT2, left part: ` %L %!` (`[Lag 0] [O/1 N/2 I/0 V/0 F/0]`).
    pub fn line2(&self) -> Line<'static> {
        let mut s = vec![
            sp(" ", st_text()),
            sp("[", st_punct()),
            sp("Lag ", st_text()),
            sp(self.lag.to_string(), st_bright()),
            sp("]", st_punct()),
        ];

        if let Some(u) = self.users {
            s.push(sp(" [", st_punct()));

            for (i, (label, count)) in [
                ("O", u.ops),
                ("N", u.nonops),
                ("I", 0),
                ("V", u.voice),
                ("F", 0),
            ]
            .into_iter()
            .enumerate()
            {
                if i > 0 {
                    s.push(sp(" ", st_text()));
                }
                s.push(sp(label, st_text()));
                s.push(sp("/", st_punct()));
                s.push(sp(count.to_string(), st_bright()));
            }

            s.push(sp("]", st_punct()));
        }

        Line::from(s).style(status_bg())
    }

    /// STATUS_FORMAT2, right-justified part (after `%>`): the window refnum.
    pub fn line2_right(&self) -> Line<'static> {
        Line::from(vec![
            sp("[", st_punct()),
            sp("Win ", st_text()),
            sp(self.window.to_string(), st_bright()),
            sp("] ", st_punct()),
        ])
        .style(status_bg())
    }
}
