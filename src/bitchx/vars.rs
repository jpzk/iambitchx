//! # BitchX `/set` variables
//!
//! The subset of BitchX1.3 `source/vars.c` that makes sense for Matrix:
//! `TIMESTAMP` (bool, default OFF) and `TIMESTAMP_STRING` (strftime,
//! default `"%I:%M%p "`, `include/config.h:436-438`). The stamp is the `%@`
//! of every FSET (`source/misc.c` `make_timestamp`), here prepended to each
//! scrollback line at render time, so toggling re-renders old lines too.
//!
//! Values persist in `config.toml` under `[settings.bitchx]`.
use std::fmt::Write as _;
use std::path::Path;

use anyhow::bail;
use chrono::{DateTime, Local, TimeZone};
use serde::Deserialize;

/// `DEFAULT_TIMESTAMP_STR`, trailing space included.
pub const DEFAULT_TIMESTAMP_STR: &str = "%I:%M%p ";

/// Every variable name, in `/set` listing order.
pub const NAMES: &[&str] = &["TIMESTAMP", "TIMESTAMP_STRING"];

/// `[settings.bitchx]` as found in the config file.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct BitchxConfig {
    pub timestamp: Option<bool>,
    pub timestamp_string: Option<String>,
}

impl BitchxConfig {
    pub fn merge(self, other: Self) -> Self {
        BitchxConfig {
            timestamp: self.timestamp.or(other.timestamp),
            timestamp_string: self.timestamp_string.or(other.timestamp_string),
        }
    }
}

/// The live values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Vars {
    pub timestamp: bool,
    pub timestamp_string: String,
}

impl Default for Vars {
    fn default() -> Self {
        Vars {
            timestamp: false,
            timestamp_string: DEFAULT_TIMESTAMP_STR.to_string(),
        }
    }
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
pub fn strftime<Tz: TimeZone>(fmt: &str, dt: &DateTime<Tz>) -> Option<String>
where
    Tz::Offset: std::fmt::Display,
{
    let mut s = String::new();
    write!(s, "{}", dt.format(fmt)).ok()?;
    Some(s)
}

/// Resolve a (case-insensitive, possibly abbreviated) name like BitchX:
/// an exact match wins, else a unique prefix.
pub fn lookup(name: &str) -> Result<&'static str, String> {
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
    pub fn from_config(c: &BitchxConfig) -> Self {
        let d = Vars::default();
        Vars {
            timestamp: c.timestamp.unwrap_or(d.timestamp),
            timestamp_string: c.timestamp_string.clone().unwrap_or(d.timestamp_string),
        }
    }

    /// The value as `/set` shows it; `None` for an empty string ("has no value").
    pub fn get(&self, name: &str) -> Option<String> {
        match name {
            "TIMESTAMP" => Some(if self.timestamp { "ON" } else { "OFF" }.to_string()),
            "TIMESTAMP_STRING" => Some(self.timestamp_string.clone()).filter(|s| !s.is_empty()),
            _ => None,
        }
    }

    /// Set a canonical `name` (see [`lookup`]).
    pub fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        match name {
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
    pub fn stamp(&self, ts_ms: i64) -> Option<String> {
        if !self.timestamp {
            return None;
        }
        let dt = Local.timestamp_millis_opt(ts_ms).single()?;
        strftime(&self.timestamp_string, &dt)
    }

    /// Write the values into `[settings.bitchx]` of the TOML file at `path`,
    /// creating file and tables when missing and keeping every other key.
    /// (Comments are not preserved: the file is re-serialized.)
    pub fn persist(&self, path: &Path) -> anyhow::Result<()> {
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
        bx.insert("timestamp".into(), Value::Boolean(self.timestamp));
        bx.insert("timestamp_string".into(), Value::String(self.timestamp_string.clone()));

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
            timestamp: true,
            timestamp_string: "[%H:%M] ".into(),
        };
        v.persist(&path).unwrap();

        let cfg =
            crate::config::Config::parse_toml(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(cfg.default_profile.as_deref(), Some("x"));
        let settings = cfg.settings.unwrap();
        assert_eq!(settings.log_level.as_deref(), Some("warn"));
        assert_eq!(Vars::from_config(&settings.bitchx.unwrap()), v);
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
}
