//! # BitchX status lines
//!
//! The two blue status lines BitchX draws above the input line
//! (`DOUBLE_STATUS_LINE`), built from the "default bx" `STATUS_FORMAT1` and
//! `STATUS_FORMAT2` in BitchX1.3 `include/color.h`, with the expandos of
//! `source/status.c`.
use ratatui::style::{Modifier as StyleModifier, Style};
use ratatui::text::{Line, Span};

use crate::bitchx::format::sp;
use crate::bitchx::model::PresenceMark;
use crate::bitchx::palette::pal;

/// The status bar background (`ESC[44m`), with the plain status text colour.
pub(crate) fn status_bg() -> Style {
    Style::default().bg(pal().status_bg).fg(pal().status_fg)
}

/// `ESC[0;44;36m`: cyan brackets on blue.
fn st_punct() -> Style {
    status_bg().fg(pal().status_punct)
}

/// `ESC[1;37m`: bold bright white on blue.
pub(crate) fn st_bright() -> Style {
    status_bg().fg(pal().status_bright).add_modifier(StyleModifier::BOLD)
}

/// `ESC[0;44;37m`: white on blue.
fn st_text() -> Style {
    status_bg().fg(pal().status_fg)
}

/// `ESC[1;32m`: bold green on blue (the `zZzZ` of `STATUS_AWAY`).
fn st_away() -> Style {
    status_bg().fg(pal().status_away).add_modifier(StyleModifier::BOLD)
}

/// A window with an unread mention in `Act:`: bold yellow on blue, as the
/// `%Y` nick of the AR formats.
pub(crate) fn st_hilite() -> Style {
    status_bg().fg(pal().status_hilite).add_modifier(StyleModifier::BOLD)
}

/// `[SYNC LOST]`: bold red on blue.
fn st_alert() -> Style {
    status_bg().fg(pal().status_alert).add_modifier(StyleModifier::BOLD)
}

/// `%C` is cut to this many columns (`#bxrecon-0122` shows as `#bxrecon-0`).
pub(crate) const CHANNEL_WIDTH: usize = 10;

/// The first `n` characters of `s`.
pub(crate) fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `%T`: BitchX's 12-hour clock, `09:24pm` (`CLOCK_24HOUR` off).
pub(crate) fn clock(t: &impl chrono::Timelike) -> String {
    let (pm, h) = t.hour12();
    format!("{h:02}:{:02}{}", t.minute(), if pm { "pm" } else { "am" })
}

/// Member counts for `%!` (`STATUS_USERS)`: `[O/n N/n I/n V/n F/n]`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct UserCounts {
    pub ops: usize,
    pub nonops: usize,
    pub voice: usize,
}

/// Everything shown on the two status lines.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct BxStatus {
    /// `%T`
    pub time: String,
    /// `%N`
    pub nick: String,
    /// `%#`, without the `+`.
    pub umodes: String,
    /// `%A`
    pub away: bool,
    /// MSGCOUNT: messages received while away, shown inside the `%A` marker.
    pub away_msgs: usize,
    /// `%@`
    pub chanop: bool,
    /// `%=`
    pub voice: bool,
    /// `%C`; `None` in the status window and in query (DM) windows.
    pub channel: Option<String>,
    /// The window name shown in `[%C]` when there is no channel: the status
    /// window reads `[status]`, not an empty `[]`.
    pub window: Option<String>,
    /// `%Q`: the query target of a DM window. Its bracket also carries the
    /// room modes (`[Query: x(+e)]` for an encrypted DM); a query window has
    /// no `[%C]`.
    pub query: Option<String>,
    /// The query target's presence marker: `zZzZ 4h` (unavailable) or
    /// `off 2d` (offline); `None` while online or currently active.
    pub query_away: Option<PresenceMark>,
    /// `%+`, without the `+`.
    pub modes: String,
    /// `%F`: refnums of other windows with unread activity.
    pub act: Vec<usize>,
    /// The subset of `act` with an unread mention (`AUTO_REPLY`), highlighted.
    pub hilite: Vec<usize>,
    /// `%!`; `None` outside of channels.
    pub users: Option<UserCounts>,
    /// `%L`: lag to the homeserver in seconds; `None` before the first sample.
    pub lag: Option<u64>,
    /// The sync loop died (F37): `[Lag ??]` is replaced by a persistent
    /// `[SYNC LOST]` until a restart, so a frozen client never looks live.
    pub sync_lost: bool,
    /// Nicks typing in the active window (`m.typing`): `[typing: a, b]`.
    pub typing: Vec<String>,
}

/// At most this many typing nicks are named; the rest read `+N`.
pub(crate) const TYPING_SHOWN: usize = 3;

/// The text inside the typing bracket: `typing: a, b` or `typing: a, b, c +2`;
/// `None` when nobody types.
pub(crate) fn typing_text(nicks: &[String]) -> Option<String> {
    if nicks.is_empty() {
        return None;
    }
    let shown: Vec<String> = nicks
        .iter()
        .take(TYPING_SHOWN)
        .map(|n| truncate(&crate::bitchx::text::one_line(n), CHANNEL_WIDTH * 2))
        .collect();
    let mut s = format!("typing: {}", shown.join(", "));
    if nicks.len() > TYPING_SHOWN {
        s = format!("{s} +{}", nicks.len() - TYPING_SHOWN);
    }
    Some(s)
}

impl BxStatus {
    /// `STATUS_FORMAT1`: ` [%T][%*%@%=%N%#%A]%M [%C%+%W] %Q %H%B%F`.
    ///
    /// As captured (recon 22, 29):
    /// ` [09:24pm][@nick(+iw)] [#bxrecon-0(+nt)]  [Query: x]`.
    pub(crate) fn line1(&self) -> Line<'static> {
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
            // STATUS_AWAY ` ([zZzZ] %A)` with MSGCOUNT for %A
            // (`include/color.h:806`, `source/status.c:1243-1260`).
            s.push(sp(" (", st_punct()));
            s.push(sp("zZzZ", st_away()));
            s.push(sp(format!(" {}", self.away_msgs), st_bright()));
            s.push(sp(")", st_punct()));
        }

        s.push(sp("]", st_punct()));

        let modes = |s: &mut Vec<Span<'static>>| {
            if !self.modes.is_empty() {
                s.push(sp("(", st_bright()));
                s.push(sp("+", st_punct()));
                s.push(sp(self.modes.clone(), st_text()));
                s.push(sp(")", st_bright()));
            }
        };

        // `[%C%+]`, the channel truncated to 10 columns (`#bxrecon-0`).
        // BitchX prints `[]` when there is no channel (capture 02); we show
        // the window name there (`[status]`). A query window has no `[%C]`.
        if self.query.is_none() {
            s.push(sp(" [", st_punct()));
            if let Some(chan) = &self.channel {
                s.push(sp(truncate(chan, CHANNEL_WIDTH), st_text()));
                modes(&mut s);
            } else if let Some(w) = &self.window {
                s.push(sp(truncate(w, CHANNEL_WIDTH), st_text()));
            }
            s.push(sp("]", st_punct()));
        }

        // `%Q` (STATUS_QUERY ` [Query: %W%Q]`), with the room modes.
        if let Some(q) = &self.query {
            s.push(sp(" [", st_punct()));
            s.push(sp("Query: ", st_text()));
            s.push(sp(q.clone(), st_bright()));
            modes(&mut s);
            if let Some(mark) = &self.query_away {
                s.push(sp(" (", st_punct()));
                let word = if mark.offline { st_text() } else { st_away() };
                s.push(sp(mark.word(), word));
                if let Some(age) = &mark.age {
                    s.push(sp(format!(" {age}"), st_bright()));
                }
                s.push(sp(")", st_punct()));
            }
            s.push(sp("]", st_punct()));
        }

        if !self.act.is_empty() {
            s.push(sp(" [", st_punct()));
            s.push(sp("Act: ", st_text()));
            for (i, n) in self.act.iter().enumerate() {
                if i > 0 {
                    s.push(sp(",", st_bright()));
                }
                let style = if self.hilite.contains(n) {
                    st_hilite()
                } else {
                    st_bright()
                };
                s.push(sp(n.to_string(), style));
            }
            s.push(sp("]", st_punct()));
        }

        Line::from(s).style(status_bg())
    }

    /// `STATUS_FORMAT2`, left part: `%L %!` (`[Lag ??] [O/1 N/2 I/0 V/0 F/0]`).
    ///
    /// `%L` reads `??` until the first lag sample, then the seconds
    /// right-aligned to two columns (`[Lag  0]`, recon 2).
    pub(crate) fn line2(&self) -> Line<'static> {
        let lag = match self.lag {
            Some(n) => format!("{n:>2}"),
            None => "??".to_string(),
        };
        let mut s = vec![
            sp(" ", st_bright()),
            sp("[", st_punct()),
            sp("Lag ", st_text()),
            sp(lag, st_bright()),
            sp("]", st_punct()),
        ];
        if self.sync_lost {
            s.splice(2..4, [sp("SYNC LOST", st_alert())]);
        }

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

        if let Some(t) = typing_text(&self.typing) {
            s.push(sp(" [", st_punct()));
            s.push(sp(t, st_text()));
            s.push(sp("]", st_punct()));
        }

        Line::from(s).style(status_bg())
    }

    /// `STATUS_FORMAT2`, right-justified part (after `%>`):
    /// `[%u:%a:%f:%b:%h]`. BitchX shows `[U:a:S:b:h]` inside a channel and
    /// `[::::]` elsewhere, all cyan (recon 21, 22). The IRC flags have no
    /// Matrix meaning, so they are rendered exactly as captured.
    pub(crate) fn line2_right(&self) -> Line<'static> {
        let flags = if self.channel.is_some() {
            "[U:a:S:b:h]"
        } else {
            "[::::]"
        };
        Line::from(vec![sp(flags, st_punct()), sp(" ", st_punct())]).style(status_bg())
    }
}
