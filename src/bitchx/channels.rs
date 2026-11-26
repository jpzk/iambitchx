//! # Channels: `/join` targets and the `/list` directory
//!
//! Pure helpers, no Matrix I/O: IRC-style `/join` target normalisation
//! (`#x` means `#x:<own homeserver>`, matrix.to links, `!ids`), `/list`
//! argument parsing and the BitchX-style `/list` lines.

/// How many public channels `/list` fetches at most.
pub const LIST_MAX: usize = 100;
/// Widest channel column in `/list` output.
const LIST_CHAN_MAX: usize = 24;

/// A normalised `/join` target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JoinTarget {
    /// `#alias:server` or `!roomid[:server]`.
    pub id: String,
    /// The server was filled in from our own homeserver (`/join #x`).
    pub implicit_server: bool,
    /// Servers to join through (`?via=` from a matrix.to link).
    pub via: Vec<String>,
}

/// Percent-decode `%23` and friends in a matrix.to fragment.
///
/// Works on bytes only: `%` followed by a multi-byte char must not slice the
/// `&str` off a char boundary (F32, that used to panic).
fn percent_decode(s: &str) -> String {
    fn hex(c: u8) -> Option<u8> {
        (c as char).to_digit(16).map(|d| d as u8)
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' &&
            i + 2 < b.len() &&
            let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Normalise what the user typed after `/join`.
///
/// - `#x` (or bare `x`) -> `#x:<own_server>`, flagged `implicit_server`
/// - `#x:server`, `!id`, `!id:server` -> as is
/// - `https://matrix.to/#/#x:server?via=a` (or `matrix.to/#/...`) -> `#x:server`, via `a`
pub fn normalize_join(target: &str, own_server: &str) -> Result<JoinTarget, String> {
    let t = target.trim();
    let mut via = vec![];
    let t: String = match t.split_once("matrix.to/#/") {
        Some((_, frag)) => {
            let (id, query) = frag.split_once('?').unwrap_or((frag, ""));
            for kv in query.split('&') {
                if let Some(v) = kv.strip_prefix("via=") &&
                    !v.is_empty()
                {
                    via.push(percent_decode(v));
                }
            }
            // matrix.to/#/!room:server/$event -> drop the event part.
            let id = percent_decode(id);
            id.split('/').next().unwrap_or("").to_string()
        },
        None => t.to_string(),
    };

    if t.is_empty() || t.chars().any(char::is_whitespace) {
        return Err(format!("Bad channel name: {target}"));
    }
    if t.starts_with('!') {
        return Ok(JoinTarget { id: t, implicit_server: false, via });
    }
    if t.starts_with('@') || t.starts_with('+') || t.starts_with('$') {
        return Err(format!("Not a channel: {target}"));
    }
    let chan = if t.starts_with('#') {
        t
    } else {
        format!("#{t}")
    };
    if chan.len() < 2 {
        return Err(format!("Bad channel name: {target}"));
    }
    match chan.split_once(':') {
        Some((name, server)) if name.len() > 1 && !server.is_empty() => {
            Ok(JoinTarget { id: chan, implicit_server: false, via })
        },
        Some(_) => Err(format!("Bad channel name: {target}")),
        None => {
            Ok(JoinTarget {
                id: format!("{chan}:{own_server}"),
                implicit_server: true,
                via,
            })
        },
    }
}

/// The window name a channel alias gets: `#x` on our own homeserver,
/// `#x:server` elsewhere (matches `matrix::channel_name`).
pub fn short_name(alias: &str, own_server: &str) -> String {
    match alias.strip_suffix(own_server).and_then(|a| a.strip_suffix(':')) {
        Some(short) if short.starts_with('#') => short.to_string(),
        _ => alias.to_string(),
    }
}

/// The hint printed when `/join #x` (no server) found nothing on our server.
pub fn not_found_hint(id: &str) -> String {
    let name = id.split(':').next().unwrap_or(id);
    format!("Cannot join channel {id} (no such channel) -- try /join {name}:server")
}

/// `/list [server] [-s text]`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ListArgs {
    pub server: Option<String>,
    pub search: Option<String>,
}

/// Parse the words after `/list`.
pub fn parse_list_args(rest: &str) -> Result<ListArgs, String> {
    let mut args = ListArgs::default();
    let mut rest = rest.trim();
    while !rest.is_empty() {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let (word, tail) = (&rest[..end], rest[end..].trim_start());
        if word == "-s" || word == "-search" {
            if tail.is_empty() {
                return Err("/list: -s needs a search text".into());
            }
            args.search = Some(tail.to_string());
            break;
        } else if word.starts_with('-') {
            return Err(format!("/list: unknown option {word}"));
        } else if args.server.is_some() {
            return Err(format!("/list: unexpected {word:?} (use /list [server] [-s text])"));
        } else {
            let server = word.trim_start_matches("https://").trim_end_matches('/');
            if server.is_empty() || server.contains('/') {
                return Err(format!("/list: bad server {word:?}"));
            }
            args.server = Some(server.to_string());
        }
        rest = tail;
    }
    Ok(args)
}

/// One public channel from the directory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ListEntry {
    /// Canonical alias, if any.
    pub alias: Option<String>,
    pub room_id: String,
    pub name: Option<String>,
    pub members: u64,
    pub topic: Option<String>,
    /// `room_type` is `m.space`.
    pub space: bool,
}

impl ListEntry {
    fn chan(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.room_id)
    }
}

/// Truncate to `width` columns (chars), marking the cut with `>`.
fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut t: String = s.chars().take(width - 1).collect();
    t.push('>');
    t
}

/// The BitchX `/list` output for `entries`, sorted by member count,
/// biggest first, fitted to `width` columns (the `-:-`-style glyph prefix
/// is not counted; `width` is the text width). Spaces are tagged `[space]`.
pub fn list_lines(
    server: &str,
    search: Option<&str>,
    entries: &[ListEntry],
    width: usize,
) -> Vec<String> {
    let mut sorted: Vec<&ListEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| b.members.cmp(&a.members).then_with(|| a.chan().cmp(b.chan())));

    let col = sorted.iter().map(|e| e.chan().chars().count()).max().unwrap_or(7);
    let col = col.clamp("Channel".len(), LIST_CHAN_MAX);

    let mut out = vec![];
    out.push(match search {
        Some(s) => format!("Searching public channels on {server} for \"{s}\""),
        None => format!("Listing public channels on {server}"),
    });
    out.push(format!("{:<col$} {:>5}  Topic", "Channel", "Users"));
    for e in sorted {
        // Never cut the alias (it is what you /join); long ones push the row.
        let chan = e.chan();
        let used = chan.chars().count().max(col);
        let mut topic = e.topic.clone().or_else(|| e.name.clone()).unwrap_or_default();
        topic = topic.split_whitespace().collect::<Vec<_>>().join(" ");
        if e.space {
            topic = if topic.is_empty() {
                "[space]".into()
            } else {
                format!("[space] {topic}")
            };
        }
        let rest = width.saturating_sub(used + 1 + 5 + 2);
        let line = format!("{chan:<col$} {:>5}  {}", e.members, truncate(&topic, rest));
        out.push(line.trim_end().to_string());
    }
    out.push(format!("End of /LIST ({} channels)", entries.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(t: &str) -> JoinTarget {
        normalize_join(t, "own.server").unwrap()
    }

    #[test]
    fn test_join_alias_normalisation() {
        assert_eq!(j("#x"), JoinTarget {
            id: "#x:own.server".into(),
            implicit_server: true,
            via: vec![]
        });
        assert_eq!(j("x").id, "#x:own.server");
        assert!(j("x").implicit_server);
        assert_eq!(j("#x:matrix.org"), JoinTarget {
            id: "#x:matrix.org".into(),
            implicit_server: false,
            via: vec![]
        });
        assert_eq!(j("!abc:matrix.org").id, "!abc:matrix.org");
        assert_eq!(j("!abc").id, "!abc");
        assert!(!j("!abc:matrix.org").implicit_server);
    }

    #[test]
    fn test_percent_decode_non_char_boundary() {
        // F32: '%' followed by multi-byte chars must not panic.
        assert_eq!(percent_decode("%é"), "%é");
        assert_eq!(percent_decode("a%€x"), "a%€x");
        assert_eq!(percent_decode("%2é"), "%2é");
        assert_eq!(percent_decode("%23x%3a"), "#x:");
        assert_eq!(percent_decode("%"), "%");
        assert!(normalize_join("https://matrix.to/#/#a%é:b?via=%€", "own").is_ok());
    }

    #[test]
    fn test_join_matrix_to() {
        assert_eq!(j("https://matrix.to/#/#rust:matrix.org").id, "#rust:matrix.org");
        assert_eq!(j("https://matrix.to/#/%23rust%3Amatrix.org").id, "#rust:matrix.org");
        let t = j("https://matrix.to/#/!abc:x.org/$ev?via=matrix.org&via=x.org");
        assert_eq!(t.id, "!abc:x.org");
        assert_eq!(t.via, vec!["matrix.org".to_string(), "x.org".into()]);
        assert_eq!(j("matrix.to/#/#a:b").id, "#a:b");
    }

    #[test]
    fn test_join_bad() {
        assert!(normalize_join("#:x", "o").is_err());
        assert!(normalize_join("#x:", "o").is_err());
        assert!(normalize_join("#", "o").is_err());
        assert!(normalize_join("@bob:x", "o").is_err());
        let e = normalize_join("@bob:x", "o").unwrap_err();
        assert!(!e.to_lowercase().contains("room"), "{e}");
    }

    #[test]
    fn test_short_name_and_hint() {
        assert_eq!(short_name("#x:own.server", "own.server"), "#x");
        assert_eq!(short_name("#x:matrix.org", "own.server"), "#x:matrix.org");
        assert_eq!(short_name("!id:own.server", "own.server"), "!id:own.server");
        let h = not_found_hint("#x:own.server");
        assert!(h.contains("#x:own.server") && h.contains("/join #x:server"), "{h}");
        assert!(!h.to_lowercase().contains("room"));
    }

    #[test]
    fn test_list_args() {
        let p = |s: &str| parse_list_args(s).unwrap();
        assert_eq!(p(""), ListArgs::default());
        assert_eq!(p("matrix.org"), ListArgs { server: Some("matrix.org".into()), search: None });
        assert_eq!(p("-s rust lang"), ListArgs { server: None, search: Some("rust lang".into()) });
        assert_eq!(p("matrix.org -s rust"), ListArgs {
            server: Some("matrix.org".into()),
            search: Some("rust".into()),
        });
        assert_eq!(p("https://matrix.org/").server.as_deref(), Some("matrix.org"));
        assert!(parse_list_args("-s").is_err());
        assert!(parse_list_args("-x").is_err());
        assert!(parse_list_args("a b").is_err());
    }

    fn e(alias: Option<&str>, id: &str, members: u64, topic: Option<&str>) -> ListEntry {
        ListEntry {
            alias: alias.map(Into::into),
            room_id: id.into(),
            members,
            topic: topic.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn test_list_lines() {
        let entries = vec![
            e(Some("#small:x"), "!a:x", 3, Some("tiny")),
            e(None, "!b:x", 40, Some("no alias here")),
            e(
                Some("#welcome:x"),
                "!c:x",
                12,
                Some("a very long topic that will not fit in the width"),
            ),
            ListEntry { space: true, ..e(Some("#hub:x"), "!d:x", 7, None) },
        ];
        let l = list_lines("x", None, &entries, 40);
        assert_eq!(l[0], "Listing public channels on x");
        assert_eq!(l[1], "Channel    Users  Topic");
        assert_eq!(l[2], "!b:x          40  no alias here");
        assert!(l[3].starts_with("#welcome:x    12  a very long"), "{}", l[3]);
        assert!(l[3].ends_with('>') && l[3].chars().count() == 40, "{:?}", l[3]);
        assert_eq!(l[4], "#hub:x         7  [space]");
        assert_eq!(l[5], "#small:x       3  tiny");
        assert_eq!(l[6], "End of /LIST (4 channels)");
        let s = list_lines("x", Some("rust"), &[], 40);
        let long = e(Some("#home-srv-test:home-srv.test"), "!r:x", 1, Some("topic"));
        let l2 = list_lines("x", None, &[long], 60);
        assert_eq!(l2[1], "Channel                  Users  Topic");
        assert_eq!(l2[2], "#home-srv-test:home-srv.test     1  topic");
        assert_eq!(s[0], "Searching public channels on x for \"rust\"");
        assert_eq!(s.last().unwrap(), "End of /LIST (0 channels)");
        for line in l.iter().chain(&s) {
            assert!(!line.to_lowercase().contains("room"), "{line}");
        }
    }
}
