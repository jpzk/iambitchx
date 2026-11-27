//! # BitchX `/et` variable
//!
//! The subset of BitchX1.3 `source/vars.c` that makes sense for Matrix:
//! `TIMESTAMP` (bool, default OFF) and `TIMESTAMP_STRING` (strftime,
//! default `"%I:%M%p "`, `include/config.h:436-438`). The stamp is the `%@`
//! of every FSET (`source/misc.c` `make_timestamp`), here prepended to each
//! scrollback line at render time, so toggling re-renders old lines too.
//!
//! `AUTO_AWAY` (bool, default ON, `include/config.h:455`) and
//! `AUTO_AWAY_TIME` (seconds, default 600, `include/config.h:407`), clamped
//! like `source/vars.c:1407-1418` `set_away_time`.
//!
//! `AWAY_NICK` (default `%n^afk`, IRC style, `%n` = the display name before going away,
//! empty = off), `AWAY_MSG` (default `away`, `/away` without text) and
//! `AUTO_AWAY_MSG` (default `Auto-Away after %t mins`, `%t` = minutes):
//! iamBitchX extras. While the away nick is on, the original display name is
//! kept in `away_nick_orig` (not a `/set` var) so a restart can restore it.
//!
//! `AUTO_REPLY` (comma list of extra highlight words, default empty; our
//! nick, mxid and display name always highlight) and `BEEP` (bool, default
//! ON, `source/vars.c` BEEP): see [`crate::bitchx::mention`].
//!
//! `DEBUG` (bool, default OFF), `DEBUG_FILTER` (comma list of
//! [`DEBUG_CATEGORIES`], default `all`) and `DEBUG_RAW` (bool, default OFF):
//! the protocol debug mode, see [`crate::debug_tap`].
//!
//! Values persist in `config.toml` under `[settings.bitchx]`, except `DEBUG`
//! and `DEBUG_RAW`: those are session-scoped and never written or read.
use std::fmt::Write as _;
use std::path::Path;

use anyhow::bail;
use chrono::{DateTime, Local, TimeZone};
use serde::Deserialize;

/// `DEFAULT_TIMESTAMP_STR`, trailing space included.
pub(crate) const DEFAULT_TIMESTAMP_STR: &str = "%I:%M%p ";

/// Every variable name, in `/set` listing order.
pub(crate) const NAMES: &[&str] = &[
    "AUTO_AWAY",
    "AUTO_AWAY_MSG",
    "AUTO_AWAY_TIME",
    "AUTO_REPLY",
    "AWAY_MSG",
    "AWAY_NICK",
    "BEEP",
    "DEBUG",
    "DEBUG_FILTER",
    "DEBUG_RAW",
    "TIMESTAMP",
    "TIMESTAMP_STRING",
];

/// `DEFAULT_AUTO_AWAY_TIME` (`include/config.h:407`), seconds.
pub(crate) const DEFAULT_AUTO_AWAY_TIME: u32 = 600;

/// `AWAY_NICK` default: `%n` is the display name before going away.
pub(crate) const DEFAULT_AWAY_NICK: &str = "%n^afk";
/// `AWAY_MSG` default, for `/away` without text.
pub(crate) const DEFAULT_AWAY_MSG: &str = "away";
/// `AUTO_AWAY_MSG` default: `%t` is `AUTO_AWAY_TIME` in minutes.
pub(crate) const DEFAULT_AUTO_AWAY_MSG: &str = "Auto-Away after %t mins";

/// `AWAY_NICK` with `%n` replaced by `name`; `None` when empty (off).
pub(crate) fn expand_away_nick(pattern: &str, name: &str) -> Option<String> {
    (!pattern.is_empty()).then(|| pattern.replace("%n", name))
}

/// `AUTO_AWAY_MSG` with `%t` replaced by `mins`.
pub(crate) fn expand_auto_away_msg(pattern: &str, mins: u32) -> String {
    pattern.replace("%t", &mins.to_string())
}

/// The display name `name` came from if it already is an away nick made by
/// `pattern` (a single `%n` with fixed text around it), e.g. `carla` for
/// `carla afk` and `%n afk`. Used so suffixes never stack.
pub(crate) fn strip_away_nick(pattern: &str, name: &str) -> Option<String> {
    let (pre, post) = pattern.split_once("%n")?;
    if post.contains("%n") || (pre.is_empty() && post.is_empty()) {
        return None;
    }
    let base = name.strip_prefix(pre)?.strip_suffix(post)?;
    (!base.is_empty()).then(|| base.to_string())
}

/// `set_away_time` (`source/vars.c:1407-1418`): 0 disables, values under a
/// minute are minutes, anything else under ten minutes becomes ten minutes.
pub(crate) fn clamp_away_time(value: u32) -> u32 {
    if value == 0 {
        0
    } else if value / 60 == 0 {
        value * 60
    } else if value < 60 * 10 {
        60 * 10
    } else {
        value
    }
}

/// `[settings.bitchx]` as found in the config file.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub(crate) struct BitchxConfig {
    pub auto_away: Option<bool>,
    pub auto_away_time: Option<u32>,
    pub auto_away_msg: Option<String>,
    pub auto_reply: Option<String>,
    pub away_msg: Option<String>,
    pub away_nick: Option<String>,
    pub away_nick_orig: Option<String>,
    pub beep: Option<bool>,
    pub debug: Option<bool>,
    pub debug_filter: Option<String>,
    pub debug_raw: Option<bool>,
    pub timestamp: Option<bool>,
    pub timestamp_string: Option<String>,
}

impl BitchxConfig {
    pub(crate) fn merge(self, other: Self) -> Self {
        BitchxConfig {
            auto_away: self.auto_away.or(other.auto_away),
            auto_away_time: self.auto_away_time.or(other.auto_away_time),
            auto_away_msg: self.auto_away_msg.or(other.auto_away_msg),
            auto_reply: self.auto_reply.or(other.auto_reply),
            away_msg: self.away_msg.or(other.away_msg),
            away_nick: self.away_nick.or(other.away_nick),
            away_nick_orig: self.away_nick_orig.or(other.away_nick_orig),
            beep: self.beep.or(other.beep),
            debug: self.debug.or(other.debug),
            debug_filter: self.debug_filter.or(other.debug_filter),
            debug_raw: self.debug_raw.or(other.debug_raw),
            timestamp: self.timestamp.or(other.timestamp),
            timestamp_string: self.timestamp_string.or(other.timestamp_string),
        }
    }
}

/// The live values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Vars {
    pub auto_away: bool,
    /// Seconds; 0 disables.
    pub auto_away_time: u32,
    /// `%t` = minutes.
    pub auto_away_msg: String,
    /// Extra highlight words, comma separated (see [`crate::bitchx::mention`]).
    pub auto_reply: String,
    pub away_msg: String,
    /// `%n` = display name before going away; empty = off.
    pub away_nick: String,
    /// The display name to restore on `/back` while the away nick is on.
    pub away_nick_orig: Option<String>,
    /// Ring the terminal bell on a mention.
    pub beep: bool,
    /// Protocol debug mode (DBG lines).
    pub debug: bool,
    /// Normalized category list, `all` = every category.
    pub debug_filter: String,
    /// Also print the (redacted, truncated) raw event JSON.
    pub debug_raw: bool,
    pub timestamp: bool,
    pub timestamp_string: String,
}

impl Default for Vars {
    fn default() -> Self {
        Vars {
            auto_away: true,
            auto_away_time: DEFAULT_AUTO_AWAY_TIME,
            auto_away_msg: DEFAULT_AUTO_AWAY_MSG.to_string(),
            auto_reply: String::new(),
            away_msg: DEFAULT_AWAY_MSG.to_string(),
            away_nick: DEFAULT_AWAY_NICK.to_string(),
            away_nick_orig: None,
            beep: true,
            debug: false,
            debug_filter: DEBUG_ALL.to_string(),
            debug_raw: false,
            timestamp: false,
            timestamp_string: DEFAULT_TIMESTAMP_STR.to_string(),
        }
    }
}

/// `DEBUG_FILTER` categories, in display order (`ephemeral` = receipts and
/// typing, `todevice` = to-device events).
pub(crate) const DEBUG_CATEGORIES: &[&str] = &[
    "http",
    "sync",
    "event",
    "auth",
    "crypto",
    "presence",
    "ephemeral",
    "todevice",
];
/// `DEBUG_FILTER` value meaning every category.
pub(crate) const DEBUG_ALL: &str = "all";

/// Normalize a `DEBUG_FILTER` value: a comma (or space) separated list of
/// [`DEBUG_CATEGORIES`], any case, deduplicated and in canonical order.
/// Empty, `all` or `*` mean every category and normalize to `all`, as does a
/// list naming all of them.
pub(crate) fn parse_debug_filter(value: &str) -> Result<String, String> {
    let mut on = [false; 8];
    let mut any = false;
    for w in value.split([',', ' ']).map(str::trim).filter(|w| !w.is_empty()) {
        let w = w.to_lowercase();
        if w == DEBUG_ALL || w == "*" {
            return Ok(DEBUG_ALL.to_string());
        }
        let Some(i) = DEBUG_CATEGORIES.iter().position(|c| *c == w) else {
            return Err(format!(
                "Unknown debug category \"{w}\" (one of {}, all)",
                DEBUG_CATEGORIES.join(",")
            ));
        };
        on[i] = true;
        any = true;
    }
    if !any || on.iter().all(|b| *b) {
        return Ok(DEBUG_ALL.to_string());
    }
    let cats: Vec<&str> = DEBUG_CATEGORIES
        .iter()
        .zip(on)
        .filter(|(_, b)| *b)
        .map(|(c, _)| *c)
        .collect();
    Ok(cats.join(","))
}

/// `do_boolean`: ON, OFF or TOGGLE, any case.
fn boolean(value: &str, cur: bool) -> Option<bool> {
    match value.trim().to_uppercase().as_str() {
        "ON" => Some(true),
        "OFF" => Some(false),
        "TOGGLE" => Some(!cur),
        _ => None,
    }
}

/// `strftime` with chrono; `None` if the format is invalid.
pub(crate) fn strftime<Tz: TimeZone>(fmt: &str, dt: &DateTime<Tz>) -> Option<String>
where
    Tz::Offset: std::fmt::Display,
{
    let mut s = String::new();
    write!(s, "{}", dt.format(fmt)).ok()?;
    Some(s)
}

/// Resolve a (case-insensitive, possibly abbreviated) name like BitchX:
/// an exact match wins, else a unique prefix.
pub(crate) fn lookup(name: &str) -> Result<&'static str, String> {
    let up = name.to_uppercase();
    if let Some(n) = NAMES.iter().find(|n| **n == up) {
        return Ok(n);
    }
    let hits: Vec<_> = NAMES.iter().filter(|n| n.starts_with(&up)).collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("No such variable \"{up}\"")),
        _ => Err(format!("{up} is ambiguous")),
    }
}

impl Vars {
    pub(crate) fn from_config(c: &BitchxConfig) -> Self {
        let d = Vars::default();
        Vars {
            auto_away: c.auto_away.unwrap_or(d.auto_away),
            auto_away_time: c.auto_away_time.map_or(d.auto_away_time, clamp_away_time),
            auto_away_msg: c.auto_away_msg.clone().unwrap_or(d.auto_away_msg),
            auto_reply: c.auto_reply.clone().unwrap_or(d.auto_reply),
            away_msg: c.away_msg.clone().unwrap_or(d.away_msg),
            away_nick: c.away_nick.clone().unwrap_or(d.away_nick),
            away_nick_orig: c.away_nick_orig.clone().filter(|s| !s.is_empty()),
            beep: c.beep.unwrap_or(d.beep),
            // F-07: the debug switches are session-scoped: never armed from the file
            debug: d.debug,
            debug_filter: c
                .debug_filter
                .as_deref()
                .and_then(|s| parse_debug_filter(s).ok())
                .unwrap_or(d.debug_filter),
            debug_raw: d.debug_raw,
            timestamp: c.timestamp.unwrap_or(d.timestamp),
            timestamp_string: c.timestamp_string.clone().unwrap_or(d.timestamp_string),
        }
    }

    /// The value as `/set` shows it; `None` for an empty string ("has no value").
    pub(crate) fn get(&self, name: &str) -> Option<String> {
        match name {
            "AUTO_AWAY" => Some(if self.auto_away { "ON" } else { "OFF" }.to_string()),
            "AUTO_AWAY_TIME" => Some(self.auto_away_time.to_string()),
            "AUTO_AWAY_MSG" => Some(self.auto_away_msg.clone()).filter(|s| !s.is_empty()),
            "AUTO_REPLY" => Some(self.auto_reply.clone()).filter(|s| !s.is_empty()),
            "AWAY_MSG" => Some(self.away_msg.clone()).filter(|s| !s.is_empty()),
            "AWAY_NICK" => Some(self.away_nick.clone()).filter(|s| !s.is_empty()),
            "BEEP" => Some(if self.beep { "ON" } else { "OFF" }.to_string()),
            "DEBUG" => Some(if self.debug { "ON" } else { "OFF" }.to_string()),
            "DEBUG_FILTER" => Some(self.debug_filter.clone()),
            "DEBUG_RAW" => Some(if self.debug_raw { "ON" } else { "OFF" }.to_string()),
            "TIMESTAMP" => Some(if self.timestamp { "ON" } else { "OFF" }.to_string()),
            "TIMESTAMP_STRING" => Some(self.timestamp_string.clone()).filter(|s| !s.is_empty()),
            _ => None,
        }
    }

    /// Set a canonical `name` (see [`lookup`]).
    pub(crate) fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        match name {
            "AUTO_AWAY" => {
                self.auto_away = boolean(value, self.auto_away)
                    .ok_or("Value must be either ON, OFF, or TOGGLE")?;
            },
            "AUTO_AWAY_TIME" => {
                // INT_TYPE_VAR, `source/vars.c:892-905`.
                let v: i64 = value
                    .trim()
                    .parse()
                    .map_err(|_| format!("Value of {name} must be numeric!"))?;
                if v < 0 {
                    return Err(format!("Value of {name} must be greater than 0"));
                }
                self.auto_away_time = clamp_away_time(u32::try_from(v).unwrap_or(u32::MAX));
            },
            "AUTO_AWAY_MSG" => self.auto_away_msg = value.to_string(),
            "AUTO_REPLY" => {
                self.auto_reply = crate::bitchx::mention::split_words(value).join(",");
            },
            "BEEP" => {
                self.beep =
                    boolean(value, self.beep).ok_or("Value must be either ON, OFF, or TOGGLE")?;
            },
            "DEBUG" => {
                self.debug =
                    boolean(value, self.debug).ok_or("Value must be either ON, OFF, or TOGGLE")?;
            },
            "DEBUG_FILTER" => self.debug_filter = parse_debug_filter(value)?,
            "DEBUG_RAW" => {
                self.debug_raw = boolean(value, self.debug_raw)
                    .ok_or("Value must be either ON, OFF, or TOGGLE")?;
            },
            "AWAY_MSG" => self.away_msg = value.to_string(),
            "AWAY_NICK" => {
                if value.matches("%n").count() > 1 {
                    return Err("AWAY_NICK may contain %n only once".into());
                }
                self.away_nick = value.to_string();
            },
            "TIMESTAMP" => {
                self.timestamp = boolean(value, self.timestamp)
                    .ok_or("Value must be either ON, OFF, or TOGGLE")?;
            },
            "TIMESTAMP_STRING" => {
                if strftime(value, &Local::now()).is_none() {
                    return Err(format!("Invalid strftime format: {value:?}"));
                }
                self.timestamp_string = value.to_string();
            },
            _ => return Err(format!("No such variable \"{name}\"")),
        }
        Ok(())
    }

    /// The `%@` stamp for an event at `ts_ms` (unix millis), in local time;
    /// `None` when `TIMESTAMP` is off.
    pub(crate) fn stamp(&self, ts_ms: i64) -> Option<String> {
        if !self.timestamp {
            return None;
        }
        let dt = Local.timestamp_millis_opt(ts_ms).single()?;
        strftime(&self.timestamp_string, &dt)
    }

    /// Write the values into `[settings.bitchx]` of the TOML file at `path`,
    /// creating file and tables when missing and keeping every other key.
    /// (Comments are not preserved: the file is re-serialized.)
    pub(crate) fn persist(&self, path: &Path) -> anyhow::Result<()> {
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
        let Value::Table(bx) =
            settings.entry("bitchx").or_insert_with(|| Value::Table(Table::new()))
        else {
            bail!("settings.bitchx is not a table");
        };
        bx.insert("auto_away".into(), Value::Boolean(self.auto_away));
        bx.insert("auto_away_time".into(), Value::Integer(i64::from(self.auto_away_time)));
        bx.insert("timestamp".into(), Value::Boolean(self.timestamp));
        bx.insert("timestamp_string".into(), Value::String(self.timestamp_string.clone()));
        bx.insert("auto_away_msg".into(), Value::String(self.auto_away_msg.clone()));
        bx.insert("away_msg".into(), Value::String(self.away_msg.clone()));
        bx.insert("away_nick".into(), Value::String(self.away_nick.clone()));
        bx.insert("auto_reply".into(), Value::String(self.auto_reply.clone()));
        bx.insert("beep".into(), Value::Boolean(self.beep));
        // F-07: never persist the debug switches; drop stale ones
        bx.remove("debug");
        bx.insert("debug_filter".into(), Value::String(self.debug_filter.clone()));
        bx.remove("debug_raw");
        match &self.away_nick_orig {
            Some(orig) => bx.insert("away_nick_orig".into(), Value::String(orig.clone())),
            None => bx.remove("away_nick_orig"),
        };

        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string(&doc)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;

    #[test]
    fn test_default_is_bitchx() {
        let v = Vars::default();
        assert!(!v.timestamp);
        assert_eq!(v.timestamp_string, "%I:%M%p ");
        assert_eq!(v.stamp(0), None);
    }

    #[test]
    fn test_format_i_m_p() {
        let tz = FixedOffset::east_opt(0).unwrap();
        let dt = tz.with_ymd_and_hms(2026, 11, 26, 21, 24, 5).unwrap();
        assert_eq!(strftime(DEFAULT_TIMESTAMP_STR, &dt).unwrap(), "09:24PM ");
        let dt = tz.with_ymd_and_hms(2026, 11, 26, 9, 5, 0).unwrap();
        assert_eq!(strftime(DEFAULT_TIMESTAMP_STR, &dt).unwrap(), "09:05AM ");
        assert_eq!(strftime("[%H:%M] ", &dt).unwrap(), "[09:05] ");
        assert_eq!(strftime("%Q", &dt), None);
    }

    #[test]
    fn test_stamp_uses_local_time_of_ts() {
        let mut v = Vars::default();
        v.set("TIMESTAMP", "on").unwrap();
        let ts = 1_764_000_000_123;
        let want = Local.timestamp_millis_opt(ts).unwrap().format("%I:%M%p ").to_string();
        assert_eq!(v.stamp(ts), Some(want));
    }

    #[test]
    fn test_lookup_and_set() {
        assert_eq!(lookup("timestamp"), Ok("TIMESTAMP"));
        assert_eq!(lookup("timestamp_s"), Ok("TIMESTAMP_STRING"));
        assert_eq!(lookup("time"), Err("TIME is ambiguous".into()));
        assert_eq!(lookup("nope"), Err("No such variable \"NOPE\"".into()));
        let mut v = Vars::default();
        v.set("TIMESTAMP", "toggle").unwrap();
        assert!(v.timestamp);
        assert!(v.set("TIMESTAMP", "maybe").is_err());
        assert!(v.set("TIMESTAMP_STRING", "%Q").is_err());
        v.set("TIMESTAMP_STRING", "%H:%M ").unwrap();
        assert_eq!(v.get("TIMESTAMP_STRING").as_deref(), Some("%H:%M "));
        assert_eq!(v.get("TIMESTAMP").as_deref(), Some("ON"));
    }

    #[test]
    fn test_auto_away_vars() {
        let v = Vars::default();
        assert!(v.auto_away);
        assert_eq!(v.auto_away_time, 600);
        assert_eq!(lookup("auto_away"), Ok("AUTO_AWAY"));
        assert_eq!(lookup("auto_away_t"), Ok("AUTO_AWAY_TIME"));
        assert_eq!(lookup("auto"), Err("AUTO is ambiguous".into()));
        let mut v = Vars::default();
        v.set("AUTO_AWAY", "off").unwrap();
        assert_eq!(v.get("AUTO_AWAY").as_deref(), Some("OFF"));
        // set_away_time clamping.
        for (input, want) in [("0", 0), ("5", 300), ("120", 600), ("900", 900)] {
            v.set("AUTO_AWAY_TIME", input).unwrap();
            assert_eq!(v.auto_away_time, want, "{input}");
        }
        assert_eq!(v.get("AUTO_AWAY_TIME").as_deref(), Some("900"));
        assert_eq!(
            v.set("AUTO_AWAY_TIME", "x"),
            Err("Value of AUTO_AWAY_TIME must be numeric!".into())
        );
        assert_eq!(
            v.set("AUTO_AWAY_TIME", "-1"),
            Err("Value of AUTO_AWAY_TIME must be greater than 0".into())
        );
        let c = BitchxConfig { auto_away_time: Some(7), ..Default::default() };
        assert_eq!(Vars::from_config(&c).auto_away_time, 420);
    }

    #[test]
    fn test_persist_round_trip_keeps_other_keys() {
        let dir = std::env::temp_dir().join(format!("bx-vars-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "default_profile = \"x\"\n[settings]\nlog_level = \"warn\"\n\
             [profiles.x]\nuser_id = \"@a:b\"\n",
        )
        .unwrap();
        let v = Vars {
            auto_away: false,
            auto_away_time: 1200,
            timestamp: true,
            timestamp_string: "[%H:%M] ".into(),
            auto_away_msg: "brb %t".into(),
            away_msg: "lunch".into(),
            away_nick: "%n|zz".into(),
            away_nick_orig: Some("Carla".into()),
            auto_reply: "creed,bx".into(),
            beep: false,
            debug: true,
            debug_filter: "http,sync".into(),
            debug_raw: true,
        };
        v.persist(&path).unwrap();

        let cfg =
            crate::config::Config::parse_toml(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(cfg.default_profile.as_deref(), Some("x"));
        let settings = cfg.settings.unwrap();
        assert_eq!(settings.log_level.as_deref(), Some("warn"));
        // debug switches are session-scoped (F-07)
        let want = Vars { debug: false, debug_raw: false, ..v.clone() };
        assert_eq!(Vars::from_config(&settings.bitchx.unwrap()), want);
        assert!(cfg.profiles.contains_key("x"));

        // Missing file: created with just the table.
        let fresh = dir.join("new.toml");
        let _ = std::fs::remove_file(&fresh);
        Vars::default().persist(&fresh).unwrap();
        let cfg =
            crate::config::Config::parse_toml(&std::fs::read_to_string(&fresh).unwrap()).unwrap();
        assert_eq!(Vars::from_config(&cfg.settings.unwrap().bitchx.unwrap()), Vars::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_away_nick_expansion() {
        assert_eq!(expand_away_nick("%n afk", "carla").as_deref(), Some("carla afk"));
        assert_eq!(expand_away_nick("[%n]zz", "Carla").as_deref(), Some("[Carla]zz"));
        assert_eq!(expand_away_nick("gone", "carla").as_deref(), Some("gone"));
        assert_eq!(expand_away_nick("", "carla"), None, "empty AWAY_NICK = off");
        assert_eq!(expand_auto_away_msg("Auto-Away after %t mins", 10), "Auto-Away after 10 mins");
        assert_eq!(expand_auto_away_msg("idle %t/%t", 15), "idle 15/15");
        assert_eq!(expand_auto_away_msg("zZz", 15), "zZz");
    }

    #[test]
    fn test_strip_away_nick() {
        assert_eq!(strip_away_nick("%n afk", "carla afk").as_deref(), Some("carla"));
        assert_eq!(strip_away_nick("%n afk", "carla afk afk").as_deref(), Some("carla afk"));
        assert_eq!(strip_away_nick("%n afk", "carla"), None);
        assert_eq!(strip_away_nick("%n afk", " afk"), None);
        assert_eq!(strip_away_nick("[%n]", "[x]").as_deref(), Some("x"));
        assert_eq!(strip_away_nick("gone", "gone"), None);
        assert_eq!(strip_away_nick("%n", "x"), None);
    }

    #[test]
    fn test_auto_reply_and_beep_vars() {
        let v = Vars::default();
        assert!(v.beep, "BEEP defaults to ON");
        assert_eq!(v.get("AUTO_REPLY"), None, "AUTO_REPLY defaults to empty");
        assert_eq!(lookup("auto_r"), Ok("AUTO_REPLY"));
        assert_eq!(lookup("beep"), Ok("BEEP"));
        let mut v = Vars::default();
        v.set("BEEP", "off").unwrap();
        assert_eq!(v.get("BEEP").as_deref(), Some("OFF"));
        assert!(v.set("BEEP", "loud").is_err());
        v.set("AUTO_REPLY", " creed , ,bx ").unwrap();
        assert_eq!(v.get("AUTO_REPLY").as_deref(), Some("creed,bx"));
        v.set("AUTO_REPLY", "").unwrap();
        assert_eq!(v.get("AUTO_REPLY"), None);
        let c = BitchxConfig {
            auto_reply: Some("x".into()),
            beep: Some(false),
            ..Default::default()
        };
        let v = Vars::from_config(&c);
        assert_eq!((v.auto_reply.as_str(), v.beep), ("x", false));
    }

    #[test]
    fn test_away_vars() {
        let v = Vars::default();
        assert_eq!(v.get("AWAY_NICK").as_deref(), Some("%n^afk"));
        assert_eq!(v.get("AWAY_MSG").as_deref(), Some("away"));
        assert_eq!(v.get("AUTO_AWAY_MSG").as_deref(), Some("Auto-Away after %t mins"));
        assert_eq!(lookup("away_n"), Ok("AWAY_NICK"));
        assert_eq!(lookup("away_m"), Ok("AWAY_MSG"));
        assert_eq!(lookup("auto_away_m"), Ok("AUTO_AWAY_MSG"));
        assert_eq!(lookup("away"), Err("AWAY is ambiguous".into()));
        let mut v = Vars::default();
        v.set("AWAY_NICK", "").unwrap();
        assert_eq!(v.get("AWAY_NICK"), None);
        assert!(v.set("AWAY_NICK", "%n %n").is_err());
        v.set("AWAY_MSG", "brb").unwrap();
        v.set("AUTO_AWAY_MSG", "idle %t").unwrap();
        assert_eq!((v.away_msg.as_str(), v.auto_away_msg.as_str()), ("brb", "idle %t"));
        // An empty orig in the file is no orig.
        let c = BitchxConfig {
            away_nick_orig: Some(String::new()),
            ..Default::default()
        };
        assert_eq!(Vars::from_config(&c).away_nick_orig, None);
    }
    #[test]
    fn test_debug_filter_parsing() {
        assert_eq!(parse_debug_filter(""), Ok("all".into()));
        assert_eq!(parse_debug_filter("ALL"), Ok("all".into()));
        assert_eq!(parse_debug_filter("*"), Ok("all".into()));
        assert_eq!(parse_debug_filter("http"), Ok("http".into()));
        assert_eq!(parse_debug_filter(" Sync , http,http "), Ok("http,sync".into()));
        assert_eq!(parse_debug_filter("todevice ephemeral"), Ok("ephemeral,todevice".into()));
        assert_eq!(
            parse_debug_filter("http,sync,event,auth,crypto,presence,ephemeral,todevice"),
            Ok("all".into())
        );
        assert!(parse_debug_filter("http,bogus").unwrap_err().contains("bogus"));
    }

    #[test]
    fn test_debug_vars() {
        let v = Vars::default();
        assert!(!v.debug && !v.debug_raw, "debug mode defaults to OFF");
        assert_eq!(v.get("DEBUG_FILTER").as_deref(), Some("all"));
        assert_eq!(lookup("debug"), Ok("DEBUG"), "exact match beats the prefix");
        assert_eq!(lookup("debug_f"), Ok("DEBUG_FILTER"));
        assert_eq!(lookup("debug_r"), Ok("DEBUG_RAW"));
        let mut v = Vars::default();
        v.set("DEBUG", "on").unwrap();
        v.set("DEBUG_RAW", "toggle").unwrap();
        v.set("DEBUG_FILTER", "sync,HTTP").unwrap();
        assert_eq!(v.get("DEBUG").as_deref(), Some("ON"));
        assert_eq!(v.get("DEBUG_RAW").as_deref(), Some("ON"));
        assert_eq!(v.get("DEBUG_FILTER").as_deref(), Some("http,sync"));
        assert!(v.set("DEBUG", "loud").is_err());
        assert!(v.set("DEBUG_FILTER", "nope").is_err());
        assert_eq!(v.debug_filter, "http,sync", "a bad filter keeps the old one");
        // A bad value in the file falls back to the default.
        let c = BitchxConfig {
            debug: Some(true),
            debug_filter: Some("garbage".into()),
            ..Default::default()
        };
        let v = Vars::from_config(&c);
        assert!(!v.debug, "never armed from the file");
        assert_eq!(v.debug_filter, "all");
    }

    #[test]
    fn test_debug_switches_never_persisted() {
        // F-07: DEBUG / DEBUG_RAW were written to config.toml and re-armed
        // the tap on every start; the filter is harmless and still persists
        let dir = std::env::temp_dir().join(format!("bx-dbgvars-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        // a stale file from an older version that armed the tap
        std::fs::write(&path, "[settings.bitchx]\ndebug = true\ndebug_raw = true\n").unwrap();
        let read = |p: &std::path::Path| {
            let text = std::fs::read_to_string(p).unwrap();
            let cfg = crate::config::Config::parse_toml(&text).unwrap();
            (text, Vars::from_config(&cfg.settings.unwrap().bitchx.unwrap()))
        };
        let (_, stale) = read(&path);
        assert!(!stale.debug && !stale.debug_raw, "file must not arm the tap");

        let mut v = Vars::default();
        v.set("DEBUG", "on").unwrap();
        v.set("DEBUG_RAW", "on").unwrap();
        v.set("DEBUG_FILTER", "http,presence").unwrap();
        v.persist(&path).unwrap();
        let (text, back) = read(&path);
        assert!(!text.contains("debug ="), "{text}");
        assert!(!text.contains("debug_raw"), "{text}");
        assert!(!back.debug && !back.debug_raw);
        assert_eq!(back.debug_filter, "http,presence");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
