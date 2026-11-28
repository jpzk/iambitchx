//! # Logging windows to files, irssi style
//!
//! `/set LOG ON` appends every line printed in a window to
//! `LOG_DIR/<server>/<window>.log` (default `$XDG_DATA_HOME/iambitchx/logs`):
//! plain text, `HH:MM ` stamped, with irssi's `--- Log opened` /
//! `--- Day changed` / `--- Log closed` markers. The status window logs to
//! `status.log`, a query (DM) window to the peer's nick.
//!
//! Never logged: protocol debug output (`DBG`), the splash art, and
//! `/lastlog` views (copies of lines already logged). Secrets never reach a
//! window in the first place (`/recover` keys are masked before dispatch).
//!
//! Files are created `0600` (and re-chmodded if they already exist),
//! directories `0700`, and symlinked log files are refused (`O_NOFOLLOW`).
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate, TimeZone as _};

use crate::bitchx::format::{format_line, plain};
use crate::bitchx::model::Line;

/// Longest file name stem kept by [`sanitize_name`], in bytes.
pub(crate) const MAX_NAME: usize = 100;

/// A window or server name as a single safe path component: ASCII
/// letters/digits and `#&+-_.@!=,` survive (lowercased, so `#Foo` and `#foo`
/// share a file), everything else (`/`, `\`, `:`, spaces, controls, non-ASCII)
/// becomes `_`. A leading `.` becomes `_` (no `.`/`..`, no hidden files) and
/// the result is cut to [`MAX_NAME`] bytes; never empty.
pub(crate) fn sanitize_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "#&+-_.@!=,".contains(c) {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .take(MAX_NAME)
        .collect();
    if out.starts_with('.') {
        out.replace_range(..1, "_");
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

/// `dir/<server>/<window>.log`, both names sanitised.
pub(crate) fn log_path(dir: &Path, server: &str, window: &str) -> PathBuf {
    dir.join(sanitize_name(server))
        .join(format!("{}.log", sanitize_name(window)))
}

/// Whether a scrollback line belongs in the log file.
pub(crate) fn loggable(line: &Line) -> bool {
    !matches!(
        line,
        Line::Debug { .. }
            | Line::Logo(_)
            | Line::LogoRuns(_)
            | Line::Raw(_)
            | Line::Lastlog { .. }
            | Line::LastlogInfo(_)
    )
}

/// The plain text rows of a line as the window shows them (without colours
/// or stamp), control characters dropped.
pub(crate) fn line_rows(line: &Line, chan: &str) -> Vec<String> {
    plain(&format_line(line, chan))
        .into_iter()
        .map(|r| r.chars().filter(|c| !c.is_control()).collect::<String>())
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn local(ts_ms: i64) -> chrono::DateTime<Local> {
    Local.timestamp_millis_opt(ts_ms).single().unwrap_or_else(Local::now)
}

/// irssi's `--- Log opened Sat Nov 28 10:00:00 2026`.
fn marker(what: &str, now_ms: i64) -> String {
    format!("--- Log {what} {}\n", local(now_ms).format("%a %b %d %H:%M:%S %Y"))
}

struct Open {
    file: File,
    day: NaiveDate,
}

/// The open log files of this session.
#[derive(Default)]
pub(crate) struct Logger {
    files: HashMap<PathBuf, Open>,
    /// Paths that failed once: not retried (and not reported again) until
    /// the logger is reset by a `LOG`/`LOG_DIR` change.
    failed: HashSet<PathBuf>,
}

impl Logger {
    /// Append `rows` of a line that happened at `ts_ms` to `path`. An error
    /// is returned once per path; after that the path is skipped silently.
    pub(crate) fn write(
        &mut self,
        path: &Path,
        ts_ms: i64,
        now_ms: i64,
        rows: &[String],
    ) -> io::Result<()> {
        if self.failed.contains(path) {
            return Ok(());
        }
        let res = self.write_inner(path, ts_ms, now_ms, rows);
        if res.is_err() {
            self.files.remove(path);
            self.failed.insert(path.to_path_buf());
        }
        res
    }

    fn write_inner(
        &mut self,
        path: &Path,
        ts_ms: i64,
        now_ms: i64,
        rows: &[String],
    ) -> io::Result<()> {
        let mut buf = String::new();
        if !self.files.contains_key(path) {
            let file = open_log(path)?;
            buf.push_str(&marker("opened", now_ms));
            self.files
                .insert(path.to_path_buf(), Open { file, day: local(now_ms).date_naive() });
        }
        let open = self.files.get_mut(path).expect("just inserted");
        let at = local(ts_ms);
        if at.date_naive() != open.day {
            open.day = at.date_naive();
            let _ = writeln!(buf, "--- Day changed {}", at.format("%a %b %d %Y"));
        }
        let stamp = at.format("%H:%M");
        for row in rows {
            let _ = writeln!(buf, "{stamp} {row}");
        }
        open.file.write_all(buf.as_bytes())
    }

    /// Write `--- Log closed` to every open file and forget them (and the
    /// failures): `LOG OFF`, a new `LOG_DIR`, quitting.
    pub(crate) fn close_all(&mut self, now_ms: i64) {
        for (_, mut open) in self.files.drain() {
            let _ = open.file.write_all(marker("closed", now_ms).as_bytes());
        }
        self.failed.clear();
    }

    /// Number of open files (tests).
    #[cfg(test)]
    pub(crate) fn open_files(&self) -> usize {
        self.files.len()
    }
}

/// Create the directories (`0700`) and open `path` for appending (`0600`,
/// no symlinks).
fn open_log(path: &Path) -> io::Result<File> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("log path has no directory"))?;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(dir)?;

    let mut opts = std::fs::OpenOptions::new();
    opts.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = opts.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        // An existing file keeps its mode on open: tighten it.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("iambitchx-logtest-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("#Matrix"), "#matrix");
        assert_eq!(sanitize_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize_name(".."), "_.");
        assert_eq!(sanitize_name("."), "_");
        assert_eq!(sanitize_name(""), "_");
        assert_eq!(sanitize_name("a/b\\c"), "a_b_c");
        assert_eq!(sanitize_name("!room:server.org"), "!room_server.org");
        assert_eq!(sanitize_name("host:8448"), "host_8448");
        assert_eq!(sanitize_name("caf\u{e9} x\n\u{1b}[31m"), "caf__x___31m");
        assert_eq!(sanitize_name("\u{202e}gol.exe"), "_gol.exe");
        assert_eq!(sanitize_name(&"x".repeat(500)).len(), MAX_NAME);
        // multibyte input is cut by chars, but each becomes one ASCII byte
        assert_eq!(sanitize_name(&"\u{1F600}".repeat(300)).len(), MAX_NAME);
    }

    #[test]
    fn test_log_path_stays_inside_dir() {
        let dir = Path::new("/logs");
        assert_eq!(
            log_path(dir, "retarded.farm", "#Chan"),
            PathBuf::from("/logs/retarded.farm/#chan.log")
        );
        for evil in ["../x", "/etc/passwd", "..", "a/../../b", "\0"] {
            let p = log_path(dir, evil, evil);
            assert!(p.starts_with(dir), "{p:?}");
            assert_eq!(p.components().count(), 4, "{p:?}");
            assert!(!p.components().any(|c| c == std::path::Component::ParentDir), "{p:?}");
        }
    }

    #[test]
    fn test_loggable() {
        assert!(loggable(&Line::Info("x".into())));
        assert!(loggable(&Line::Public { nick: "a".into(), own: false, text: "b".into() }));
        assert!(!loggable(&Line::Debug { cat: "http".into(), text: "GET".into() }));
        assert!(!loggable(&Line::LastlogInfo("Lastlog:".into())));
        assert!(!loggable(&Line::Lastlog { ts: 0, line: Box::new(Line::Info("x".into())) }));
        assert!(!loggable(&Line::Logo("x".into())));
    }

    #[test]
    fn test_line_rows_plain() {
        let l = Line::Public {
            nick: "carla".into(),
            own: false,
            text: "hi there".into(),
        };
        assert_eq!(line_rows(&l, "#r"), vec!["<carla> hi there"]);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_creates_0600_file_in_0700_dirs() {
        use std::os::unix::fs::PermissionsExt as _;
        let base = tmp("mode");
        let path = log_path(&base.join("logs"), "srv", "#chan");
        let mut lg = Logger::default();
        let ts = Local.with_ymd_and_hms(2026, 11, 28, 10, 5, 0).unwrap().timestamp_millis();
        lg.write(&path, ts, ts, &["<a> hello".into()]).unwrap();
        lg.write(&path, ts, ts, &["<b> second".into()]).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&base.join("logs")), 0o700);
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("--- Log opened Sat Nov 28 10:05:00 2026"), "{text}");
        assert_eq!(lines[1], "10:05 <a> hello");
        assert_eq!(lines[2], "10:05 <b> second");
        lg.close_all(ts);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .ends_with("--- Log closed Sat Nov 28 10:05:00 2026\n")
        );
        assert_eq!(lg.open_files(), 0);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn test_existing_file_is_tightened_and_appended() {
        use std::os::unix::fs::PermissionsExt as _;
        let base = tmp("tighten");
        let path = log_path(&base, "srv", "status");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "old\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let mut lg = Logger::default();
        let ts = Local.with_ymd_and_hms(2026, 11, 28, 23, 59, 0).unwrap().timestamp_millis();
        let next = Local.with_ymd_and_hms(2026, 11, 29, 0, 1, 0).unwrap().timestamp_millis();
        lg.write(&path, ts, ts, &["a".into()]).unwrap();
        lg.write(&path, next, next, &["b".into(), "c".into()]).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "old");
        assert_eq!(lines[2], "23:59 a");
        assert_eq!(lines[3], "--- Day changed Sun Nov 29 2026");
        assert_eq!(&lines[4..], ["00:01 b", "00:01 c"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn test_symlinked_log_is_refused_once() {
        let base = tmp("symlink");
        let path = log_path(&base, "srv", "#x");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let target = base.join("target");
        std::fs::write(&target, "").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let mut lg = Logger::default();
        assert!(lg.write(&path, 0, 0, &["x".into()]).is_err());
        // reported once, then skipped quietly
        assert!(lg.write(&path, 0, 0, &["x".into()]).is_ok());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "");
        let _ = std::fs::remove_dir_all(&base);
    }
}
