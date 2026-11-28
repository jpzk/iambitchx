//! # `/lastlog`: search the current window
//!
//! BitchX/ircII `lastlog`: `/lastlog [-i] TEXT` prints every scrollback line
//! of the current window whose text contains `TEXT` (case-sensitive; `-i`
//! ignores case) between ` Lastlog:` and ` End of Lastlog`, each with the
//! time it happened. `/lastlog -clear` removes that output again.
//!
//! The text searched is what the window shows (`<nick> text`, ` nick has
//! joined`, ...) without the stamp; earlier `/lastlog` output and the splash
//! art are never searched.
use crate::bitchx::logfile::line_rows;
use crate::bitchx::model::Line;

/// Header line of the output.
pub(crate) const HEADER: &str = "Lastlog:";
/// Footer line of the output.
pub(crate) const FOOTER: &str = "End of Lastlog";

/// Whether a line takes part in a search.
pub(crate) fn searchable(line: &Line) -> bool {
    !matches!(
        line,
        Line::Lastlog { .. }
            | Line::LastlogInfo(_)
            | Line::Logo(_)
            | Line::LogoRuns(_)
            | Line::Raw(_)
    )
}

/// Whether `line` (in the window of channel `chan`) contains `pattern`.
pub(crate) fn matches(line: &Line, chan: &str, pattern: &str, ignore_case: bool) -> bool {
    if !searchable(line) || pattern.is_empty() {
        return false;
    }
    let text = line_rows(line, chan).join(" ");
    if ignore_case {
        text.to_lowercase().contains(&pattern.to_lowercase())
    } else {
        text.contains(pattern)
    }
}

/// The `/lastlog` output for `lines`/`times`: header, the hits as
/// [`Line::Lastlog`] (oldest first), footer.
pub(crate) fn search(
    lines: &[Line],
    times: &[i64],
    chan: &str,
    pattern: &str,
    ignore_case: bool,
) -> Vec<Line> {
    let mut out = vec![Line::LastlogInfo(HEADER.into())];
    out.extend(
        lines
            .iter()
            .zip(times)
            .filter(|(l, _)| matches(l, chan, pattern, ignore_case))
            .map(|(l, ts)| Line::Lastlog { ts: *ts, line: Box::new(l.clone()) }),
    );
    out.push(Line::LastlogInfo(FOOTER.into()));
    out
}

/// Whether a line is `/lastlog` output (removed by `-clear`).
pub(crate) fn is_output(line: &Line) -> bool {
    matches!(line, Line::Lastlog { .. } | Line::LastlogInfo(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(nick: &str, text: &str) -> Line {
        Line::Public { nick: nick.into(), own: false, text: text.into() }
    }

    #[test]
    fn test_matches_case() {
        let l = public("carla", "Hello World");
        assert!(matches(&l, "#r", "World", false));
        assert!(!matches(&l, "#r", "world", false));
        assert!(matches(&l, "#r", "world", true));
        assert!(matches(&l, "#r", "HELLO", true));
        // the nick is part of the shown text
        assert!(matches(&l, "#r", "<carla>", false));
        assert!(!matches(&l, "#r", "", true));
        assert!(!matches(&l, "#r", "nope", true));
    }

    #[test]
    fn test_matches_other_kinds_and_skips_own_output() {
        let j = Line::Join { nick: "bob".into(), mxid: "@bob:x".into() };
        assert!(matches(&j, "#r", "has joined", false));
        assert!(matches(&Line::Info("server notice".into()), "", "notice", false));
        let hit = Line::Lastlog { ts: 1, line: Box::new(public("a", "needle")) };
        assert!(!matches(&hit, "#r", "needle", false));
        assert!(!matches(&Line::LastlogInfo(HEADER.into()), "#r", "Lastlog", false));
        assert!(!matches(&Line::Logo("needle".into()), "#r", "needle", false));
    }

    #[test]
    fn test_search_frames_hits_with_their_times() {
        let lines = vec![
            public("a", "one foo"),
            public("b", "two"),
            public("c", "FOO three"),
        ];
        let out = search(&lines, &[10, 20, 30], "#r", "foo", true);
        assert_eq!(
            out,
            vec![
                Line::LastlogInfo(HEADER.into()),
                Line::Lastlog { ts: 10, line: Box::new(lines[0].clone()) },
                Line::Lastlog { ts: 30, line: Box::new(lines[2].clone()) },
                Line::LastlogInfo(FOOTER.into()),
            ]
        );
        let out = search(&lines, &[10, 20, 30], "#r", "foo", false);
        assert_eq!(out.len(), 3);
        // searching again does not find the previous output
        let mut again = lines.clone();
        again.extend(out);
        let times = vec![0; again.len()];
        assert_eq!(search(&again, &times, "#r", "foo", false).len(), 3);
        assert!(again.iter().filter(|l| is_output(l)).count() == 3);
    }
}
