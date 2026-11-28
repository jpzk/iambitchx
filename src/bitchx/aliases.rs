//! # `/alias` and `/bind`
//!
//! BitchX-style user aliases (`/alias NAME body`, `$1`..`$9` and `$*`
//! expanded from the words typed after `/NAME`) and key binds
//! (`/bind KEY /command` for `^X`, `M-x` and `F1`..`F12`). Both persist in
//! `config.toml` under `[settings.bitchx.aliases]` / `[settings.bitchx.binds]`.
//!
//! Nothing here touches the app: expansion is a pure string transform; the
//! caller runs the result and enforces [`MAX_DEPTH`].
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use anyhow::bail;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::bitchx::commands::COMMAND_NAMES;

/// How deep aliases may call aliases before the expansion is refused.
pub(crate) const MAX_DEPTH: usize = 8;

/// Command words that are built in but not in [`COMMAND_NAMES`] (hidden
/// spellings handled by the parser).
const HIDDEN_BUILTINS: &[&str] = &["rooms"];

/// `/alias add|del #x`: the room-alias subcommands, so never user alias names.
pub(crate) const ROOM_ALIAS_SUBS: &[&str] = &["add", "del", "delete", "rm", "remove"];

/// Whether `name` (any case) is a built-in command word.
pub(crate) fn is_builtin(name: &str) -> bool {
    let lower = name.to_lowercase();
    COMMAND_NAMES.contains(&lower.as_str()) || HIDDEN_BUILTINS.contains(&lower.as_str())
}

/// Check and normalise an alias name: lower case, `[a-z0-9_-]`, not a
/// built-in command and not a `/alias add|del` subcommand.
pub(crate) fn alias_name(name: &str) -> Result<String, String> {
    let lower = name.trim_start_matches('/').to_lowercase();
    let valid = !lower.is_empty()
        && !lower.starts_with('-')
        && lower.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !valid {
        return Err(format!("Invalid alias name {name:?}"));
    }
    if is_builtin(&lower) {
        return Err(format!("Cannot alias {}: it is a built-in command", lower.to_uppercase()));
    }
    if ROOM_ALIAS_SUBS.contains(&lower.as_str()) {
        return Err(format!("Cannot alias {}: reserved by /alias add|del", lower.to_uppercase()));
    }
    Ok(lower)
}

/// The user's aliases: name (lower case) -> body (a command line without
/// the leading `/`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Aliases(BTreeMap<String, String>);

impl Aliases {
    /// Load from `[settings.bitchx.aliases]`; bad entries are skipped and
    /// returned as warnings.
    pub(crate) fn from_config(map: Option<&BTreeMap<String, String>>) -> (Self, Vec<String>) {
        let mut out = Aliases::default();
        let mut warn = vec![];
        for (name, body) in map.into_iter().flatten() {
            if let Err(e) = out.set(name, body) {
                warn.push(format!("config.toml alias skipped: {e}"));
            }
        }
        (out, warn)
    }

    /// Define (or replace) an alias; returns the normalised name.
    pub(crate) fn set(&mut self, name: &str, body: &str) -> Result<String, String> {
        let name = alias_name(name)?;
        let body = body.trim();
        let body = body.strip_prefix('/').unwrap_or(body).trim_start();
        if body.is_empty() {
            return Err(format!("Alias {} has an empty body", name.to_uppercase()));
        }
        self.0.insert(name.clone(), body.to_string());
        Ok(name)
    }

    /// Remove an alias; the removed body, if it existed.
    pub(crate) fn remove(&mut self, name: &str) -> Option<String> {
        self.0.remove(&name.trim_start_matches('/').to_lowercase())
    }

    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.0.get(&name.to_lowercase()).map(String::as_str)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.0.iter()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn map(&self) -> &BTreeMap<String, String> {
        &self.0
    }

    /// If `line` (text after the `/`) starts with an alias, its expansion.
    pub(crate) fn expand_line(&self, line: &str) -> Option<String> {
        let line = line.trim_start();
        let (word, rest) = match line.find(char::is_whitespace) {
            Some(i) => (&line[..i], line[i..].trim_start()),
            None => (line, ""),
        };
        self.get(word).map(|body| expand(body, rest))
    }
}

/// Expand `$1`..`$9` (the Nth word of `args`, empty when missing), `$*`
/// (all of `args`) and `$$` (a literal `$`). A body that uses none of them
/// gets the arguments appended, as in ircII.
pub(crate) fn expand(body: &str, args: &str) -> String {
    let args = args.trim();
    let words: Vec<&str> = args.split_whitespace().collect();
    let mut out = String::with_capacity(body.len() + args.len());
    let mut used = false;
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('*') => {
                chars.next();
                used = true;
                out.push_str(args);
            },
            Some(d @ '1'..='9') => {
                chars.next();
                used = true;
                let n = d as usize - '1' as usize;
                out.push_str(words.get(n).copied().unwrap_or(""));
            },
            Some('$') => {
                chars.next();
                out.push('$');
            },
            _ => out.push('$'),
        }
    }
    if !used && !args.is_empty() {
        out.push(' ');
        out.push_str(args);
    }
    out
}

/// A bindable key.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum BindKey {
    /// `^X`: Ctrl plus a letter (not `^I` `^J` `^M`: Tab and Enter).
    Ctrl(char),
    /// `M-x`: Alt/Meta plus a printable character.
    Meta(char),
    /// `F1`..`F12`.
    F(u8),
}

impl fmt::Display for BindKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BindKey::Ctrl(c) => write!(f, "^{}", c.to_ascii_uppercase()),
            BindKey::Meta(c) => write!(f, "M-{c}"),
            BindKey::F(n) => write!(f, "F{n}"),
        }
    }
}

impl BindKey {
    /// Parse `^X`, `C-x`, `ctrl-x`, `M-x`, `meta-x`, `alt-x`, `F1`..`F12`
    /// (any case; the letter is folded to lower case).
    pub(crate) fn parse(s: &str) -> Result<Self, String> {
        let bad = || format!("Invalid key {s:?} (use ^X, M-x or F1..F12)");
        let lower = s.to_lowercase();
        let one = |rest: &str| {
            let mut it = rest.chars();
            match (it.next(), it.next()) {
                (Some(c), None) => Some(c),
                _ => None,
            }
        };
        if let Some(n) = lower.strip_prefix('f').filter(|n| !n.is_empty()) {
            return match n.parse::<u8>() {
                Ok(n @ 1..=12) => Ok(BindKey::F(n)),
                _ => Err(bad()),
            };
        }
        let ctrl = lower
            .strip_prefix('^')
            .or_else(|| lower.strip_prefix("c-"))
            .or_else(|| lower.strip_prefix("ctrl-"));
        if let Some(rest) = ctrl {
            return match one(rest) {
                Some(c @ 'a'..='z') if !matches!(c, 'i' | 'j' | 'm') => Ok(BindKey::Ctrl(c)),
                _ => Err(bad()),
            };
        }
        let meta = lower
            .strip_prefix("m-")
            .or_else(|| lower.strip_prefix("meta-"))
            .or_else(|| lower.strip_prefix("alt-"));
        if let Some(rest) = meta {
            return match one(rest) {
                Some(c) if c.is_ascii_graphic() => Ok(BindKey::Meta(c)),
                _ => Err(bad()),
            };
        }
        Err(bad())
    }

    /// The bind key a key event stands for, if any.
    pub(crate) fn from_event(key: &KeyEvent) -> Option<Self> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::F(n @ 1..=12) if !ctrl && !alt => Some(BindKey::F(n)),
            KeyCode::Char(c) if ctrl && !alt => {
                let c = c.to_ascii_lowercase();
                c.is_ascii_lowercase().then_some(BindKey::Ctrl(c))
            },
            KeyCode::Char(c) if alt && !ctrl && c.is_ascii_graphic() => {
                Some(BindKey::Meta(c.to_ascii_lowercase()))
            },
            _ => None,
        }
    }
}

/// The user's key binds: key -> command line (with the leading `/`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Binds(BTreeMap<BindKey, String>);

impl Binds {
    /// Load from `[settings.bitchx.binds]`; bad entries are skipped and
    /// returned as warnings.
    pub(crate) fn from_config(map: Option<&BTreeMap<String, String>>) -> (Self, Vec<String>) {
        let mut out = Binds::default();
        let mut warn = vec![];
        for (key, cmd) in map.into_iter().flatten() {
            if let Err(e) = out.set(key, cmd) {
                warn.push(format!("config.toml bind skipped: {e}"));
            }
        }
        (out, warn)
    }

    /// Bind `key` to `cmd` (a leading `/` is added when missing).
    pub(crate) fn set(&mut self, key: &str, cmd: &str) -> Result<BindKey, String> {
        let key = BindKey::parse(key)?;
        let cmd = cmd.trim();
        let cmd = cmd.strip_prefix('/').unwrap_or(cmd).trim_start();
        if cmd.is_empty() {
            return Err(format!("Bind {key} has no command"));
        }
        self.0.insert(key, format!("/{cmd}"));
        Ok(key)
    }

    pub(crate) fn remove(&mut self, key: &str) -> Result<Option<(BindKey, String)>, String> {
        let key = BindKey::parse(key)?;
        Ok(self.0.remove(&key).map(|c| (key, c)))
    }

    pub(crate) fn get(&self, key: BindKey) -> Option<&str> {
        self.0.get(&key).map(String::as_str)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&BindKey, &String)> {
        self.0.iter()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// As stored in `config.toml`: canonical key name -> command.
    pub(crate) fn to_config(&self) -> BTreeMap<String, String> {
        self.0.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }
}

/// Write `map` as the `[settings.bitchx.<table>]` table of the TOML file at
/// `path` (created when missing), keeping every other key. An empty map
/// removes the table.
pub(crate) fn persist_table(
    path: &Path,
    table: &str,
    map: &BTreeMap<String, String>,
) -> anyhow::Result<()> {
    use toml::{Table, Value};

    let mut doc: Table = match std::fs::read_to_string(path) {
        Ok(s) => s.parse()?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Table::new(),
        Err(e) => return Err(e.into()),
    };
    let Value::Table(settings) =
        doc.entry("settings").or_insert_with(|| Value::Table(Table::new()))
    else {
        bail!("settings is not a table");
    };
    let Value::Table(bx) = settings.entry("bitchx").or_insert_with(|| Value::Table(Table::new()))
    else {
        bail!("settings.bitchx is not a table");
    };
    if map.is_empty() {
        bx.remove(table);
    } else {
        let t: Table = map.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect();
        bx.insert(table.to_string(), Value::Table(t));
    }

    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, toml::to_string(&doc)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_positional_and_star() {
        assert_eq!(expand("msg $1 hi $2", "bob alice"), "msg bob hi alice");
        assert_eq!(expand("me says $*", "hello  there"), "me says hello  there");
        assert_eq!(expand("join $1", ""), "join ");
        assert_eq!(expand("msg $1 [$9]", "bob"), "msg bob []");
        assert_eq!(expand("me costs $$5 $", "x"), "me costs $5 $ x");
        assert_eq!(expand("msg $1 $*", "a b c"), "msg a a b c");
    }

    #[test]
    fn expand_appends_unused_args() {
        assert_eq!(expand("join", "#rust"), "join #rust");
        assert_eq!(expand("away gone", ""), "away gone");
        // $0 and $10 are not arguments: $0 stays, $10 is $1 then 0
        assert_eq!(expand("me $0", "x"), "me $0 x");
        assert_eq!(expand("me $10", "x"), "me x0");
    }

    #[test]
    fn alias_names_refuse_builtins() {
        let mut a = Aliases::default();
        for name in [
            "join", "JOIN", "/msg", "rooms", "j", "exit", "alias", "bind",
        ] {
            assert!(a.set(name, "me hi").is_err(), "{name}");
        }
        for name in ["add", "del", "remove"] {
            assert!(a.set(name, "me hi").is_err(), "{name}");
        }
        assert!(a.set("bad name", "me").is_err());
        assert!(a.set("-x", "me").is_err());
        assert!(a.set("ok", "  ").is_err());
        assert_eq!(a.set("Hi", "/me waves $*"), Ok("hi".into()));
        assert_eq!(a.get("HI"), Some("me waves $*"));
        assert!(!a.is_empty());
        assert_eq!(a.remove("hi"), Some("me waves $*".into()));
        assert!(a.is_empty());
    }

    #[test]
    fn expand_line_only_for_aliases() {
        let mut a = Aliases::default();
        a.set("hi", "msg $1 hello").unwrap();
        assert_eq!(a.expand_line("hi bob"), Some("msg bob hello".into()));
        assert_eq!(a.expand_line("HI   bob"), Some("msg bob hello".into()));
        assert_eq!(a.expand_line("nope bob"), None);
    }

    #[test]
    fn config_skips_bad_entries() {
        let map: BTreeMap<String, String> = [
            ("join".into(), "me x".into()),
            ("hi".into(), "me hi".into()),
        ]
        .into();
        let (a, warn) = Aliases::from_config(Some(&map));
        assert_eq!(a.map().len(), 1);
        assert_eq!(warn.len(), 1);
        assert!(warn[0].contains("built-in"), "{warn:?}");
    }

    #[test]
    fn bind_keys_parse() {
        assert_eq!(BindKey::parse("^X"), Ok(BindKey::Ctrl('x')));
        assert_eq!(BindKey::parse("C-x"), Ok(BindKey::Ctrl('x')));
        assert_eq!(BindKey::parse("ctrl-T"), Ok(BindKey::Ctrl('t')));
        assert_eq!(BindKey::parse("M-a"), Ok(BindKey::Meta('a')));
        assert_eq!(BindKey::parse("meta-1"), Ok(BindKey::Meta('1')));
        assert_eq!(BindKey::parse("alt-z"), Ok(BindKey::Meta('z')));
        assert_eq!(BindKey::parse("F5"), Ok(BindKey::F(5)));
        assert_eq!(BindKey::parse("f12"), Ok(BindKey::F(12)));
        for bad in [
            "F0", "F13", "f", "^I", "^M", "^J", "^1", "x", "M-", "M-ab", "M- ", "",
        ] {
            assert!(BindKey::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(BindKey::Ctrl('x').to_string(), "^X");
        assert_eq!(BindKey::Meta('a').to_string(), "M-a");
        assert_eq!(BindKey::F(3).to_string(), "F3");
        // display round-trips
        for k in [BindKey::Ctrl('t'), BindKey::Meta('q'), BindKey::F(9)] {
            assert_eq!(BindKey::parse(&k.to_string()), Ok(k));
        }
    }

    #[test]
    fn bind_keys_from_events() {
        let ev = |code, m| KeyEvent::new(code, m);
        assert_eq!(
            BindKey::from_event(&ev(KeyCode::Char('T'), KeyModifiers::CONTROL)),
            Some(BindKey::Ctrl('t'))
        );
        assert_eq!(
            BindKey::from_event(&ev(KeyCode::Char('q'), KeyModifiers::ALT)),
            Some(BindKey::Meta('q'))
        );
        assert_eq!(
            BindKey::from_event(&ev(KeyCode::F(2), KeyModifiers::NONE)),
            Some(BindKey::F(2))
        );
        assert_eq!(BindKey::from_event(&ev(KeyCode::Char('q'), KeyModifiers::NONE)), None);
        assert_eq!(BindKey::from_event(&ev(KeyCode::Enter, KeyModifiers::NONE)), None);
    }

    #[test]
    fn binds_store_commands_with_slash() {
        let mut b = Binds::default();
        assert_eq!(b.set("F1", "join #help"), Ok(BindKey::F(1)));
        assert_eq!(b.get(BindKey::F(1)), Some("/join #help"));
        assert!(b.set("F1", "  ").is_err());
        assert_eq!(b.to_config().get("F1").map(String::as_str), Some("/join #help"));
        assert_eq!(b.remove("f1"), Ok(Some((BindKey::F(1), "/join #help".into()))));
        assert_eq!(b.remove("F1"), Ok(None));
        assert!(b.remove("nope").is_err());
    }

    #[test]
    fn persist_tables_round_trip() {
        let dir = std::env::temp_dir().join(format!("bx-aliases-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[settings.bitchx]\nbeep = true\n").unwrap();
        let map: BTreeMap<String, String> = [("hi".into(), "me hi $*".into())].into();
        persist_table(&path, "aliases", &map).unwrap();
        let binds: BTreeMap<String, String> = [("^T".into(), "/hi".into())].into();
        persist_table(&path, "binds", &binds).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let cfg = crate::config::Config::parse_toml(&text).unwrap();
        let bx = cfg.settings.unwrap().bitchx.unwrap();
        assert_eq!(bx.beep, Some(true));
        assert_eq!(bx.aliases, Some(map));
        assert_eq!(bx.binds, Some(binds));
        persist_table(&path, "aliases", &BTreeMap::new()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("aliases"), "{text}");
    }
}
