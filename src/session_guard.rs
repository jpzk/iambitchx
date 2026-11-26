//! # Session guard
//!
//! Two iambitchx processes on the same profile share one `session.json`.
//! OAuth refresh tokens rotate: whoever refreshes first invalidates the
//! other's tokens. This module keeps them from stepping on each other:
//!
//! * [`SessionLock`] is an advisory lock file (`session.json.lock`) taken
//!   around every token refresh and every `session.json` write, so two
//!   processes never rotate concurrently.
//! * [`decide`] is the pure recovery policy when the homeserver says
//!   `M_UNKNOWN_TOKEN` (or soft logout): pick up tokens another process wrote
//!   to disk, refresh ourselves, or tell the user to log in again.
//! * `--smoke` is [`Mode::ReadOnly`]: it never refreshes and never writes.
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use matrix_sdk::SessionTokens;

/// How a process may treat the saved session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// The interactive client: may refresh tokens and rewrite `session.json`.
    Live,
    /// `--smoke`: never refresh, never write `session.json`.
    ReadOnly,
}

impl Mode {
    /// Whether the SDK may refresh tokens / we may persist them.
    pub fn may_write(self) -> bool {
        self == Mode::Live
    }
}

/// What to do after the homeserver rejected our access token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Recovery {
    /// `session.json` holds different tokens (another process rotated):
    /// adopt them and retry once.
    ReloadFromDisk,
    /// The disk agrees with us and we hold a refresh token: refresh (under
    /// the lock), persist, retry once.
    Refresh,
    /// Nothing left to try: tell the user to log in again.
    Relogin,
    /// Read-only mode: exit nonzero, touch nothing.
    Fail,
}

/// Recovery policy for `M_UNKNOWN_TOKEN` / soft logout.
///
/// `attempts` counts recoveries already tried for this incident (reset once
/// a request succeeds again): we retry exactly once.
pub fn decide(
    mode: Mode,
    in_use: Option<&SessionTokens>,
    on_disk: Option<&SessionTokens>,
    attempts: u32,
) -> Recovery {
    if mode == Mode::ReadOnly {
        return Recovery::Fail;
    }
    if attempts >= 1 {
        return Recovery::Relogin;
    }
    match (in_use, on_disk) {
        (Some(mine), Some(disk)) if !same_tokens(mine, disk) => Recovery::ReloadFromDisk,
        (None, Some(_)) => Recovery::ReloadFromDisk,
        (Some(mine), _) if mine.refresh_token.is_some() => Recovery::Refresh,
        _ => Recovery::Relogin,
    }
}

fn same_tokens(a: &SessionTokens, b: &SessionTokens) -> bool {
    a.access_token == b.access_token && a.refresh_token == b.refresh_token
}

/// The ` iamBitchX:` line printed after adopting tokens from disk.
pub const RELOADED_MSG: &str =
    "session.json was refreshed by another iambitchx; reloaded it from disk, retrying";

/// The ` iamBitchX:` line printed when nothing can save the session.
pub fn relogin_hint(session_json: &Path) -> String {
    format!(
        "the homeserver rejected this session (M_UNKNOWN_TOKEN). Quit, delete {} and start iambitchx to log in again",
        session_json.display()
    )
}

/// Message for `--smoke` when the access token is rejected.
pub fn smoke_rejected_msg(session_json: &Path) -> String {
    format!(
        "--smoke: the homeserver rejected the access token in {} (M_UNKNOWN_TOKEN). \
         --smoke is read-only and never refreshes tokens or rewrites session.json; \
         let the running client (or one interactive start) refresh it, then retry",
        session_json.display()
    )
}

/// `session.json` -> `session.json.lock`.
pub fn lock_path(session_json: &Path) -> PathBuf {
    let mut name = session_json.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    session_json.with_file_name(name)
}

/// An exclusive advisory lock on `session.json.lock` (`flock(2)` via std's
/// [`File::try_lock`]). The kernel drops it when the process dies, so a
/// crashed holder never leaves a stale lock. Released on drop.
#[derive(Debug)]
pub struct SessionLock {
    _file: File,
}

impl SessionLock {
    /// Take the lock now, or `Ok(None)` if another holder has it.
    pub fn try_acquire(session_json: &Path) -> std::io::Result<Option<SessionLock>> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path(session_json))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(SessionLock { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }

    /// Wait up to `timeout` for the lock (blocking; call from
    /// `spawn_blocking` in async code).
    pub fn acquire(session_json: &Path, timeout: Duration) -> anyhow::Result<SessionLock> {
        let start = Instant::now();
        loop {
            if let Some(lock) = Self::try_acquire(session_json)? {
                return Ok(lock);
            }
            if start.elapsed() >= timeout {
                anyhow::bail!(
                    "{} is held by another iambitchx for more than {}s",
                    lock_path(session_json).display(),
                    timeout.as_secs()
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Async wrapper around [`SessionLock::acquire`].
    pub async fn acquire_async(session_json: PathBuf, timeout: Duration) -> anyhow::Result<Self> {
        tokio::task::spawn_blocking(move || Self::acquire(&session_json, timeout)).await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(at: &str, rt: Option<&str>) -> SessionTokens {
        SessionTokens {
            access_token: at.into(),
            refresh_token: rt.map(Into::into),
        }
    }

    #[test]
    fn test_readonly_never_recovers() {
        let mine = toks("a", Some("r"));
        let disk = toks("b", Some("s"));
        for attempts in 0..3 {
            assert_eq!(decide(Mode::ReadOnly, Some(&mine), Some(&disk), attempts), Recovery::Fail);
            assert_eq!(decide(Mode::ReadOnly, Some(&mine), Some(&mine), attempts), Recovery::Fail);
        }
        assert!(!Mode::ReadOnly.may_write());
        assert!(Mode::Live.may_write());
    }

    #[test]
    fn test_reload_when_disk_differs() {
        let mine = toks("a", Some("r"));
        assert_eq!(
            decide(Mode::Live, Some(&mine), Some(&toks("b", Some("s"))), 0),
            Recovery::ReloadFromDisk
        );
        // only the refresh token changed
        assert_eq!(
            decide(Mode::Live, Some(&mine), Some(&toks("a", Some("s"))), 0),
            Recovery::ReloadFromDisk
        );
        assert_eq!(decide(Mode::Live, None, Some(&mine), 0), Recovery::ReloadFromDisk);
    }

    #[test]
    fn test_refresh_when_disk_same() {
        let mine = toks("a", Some("r"));
        assert_eq!(decide(Mode::Live, Some(&mine), Some(&mine.clone()), 0), Recovery::Refresh);
        assert_eq!(decide(Mode::Live, Some(&mine), None, 0), Recovery::Refresh);
    }

    #[test]
    fn test_relogin_cases() {
        let no_rt = toks("a", None);
        assert_eq!(decide(Mode::Live, Some(&no_rt), Some(&no_rt), 0), Recovery::Relogin);
        assert_eq!(decide(Mode::Live, None, None, 0), Recovery::Relogin);
        // retry exactly once: the second rejection gives up even if disk differs
        let mine = toks("a", Some("r"));
        let disk = toks("b", Some("s"));
        assert_eq!(decide(Mode::Live, Some(&mine), Some(&disk), 1), Recovery::Relogin);
        assert_eq!(decide(Mode::Live, Some(&mine), Some(&mine), 1), Recovery::Relogin);
    }

    #[test]
    fn test_messages() {
        let p = Path::new("/x/session.json");
        assert!(relogin_hint(p).contains("/x/session.json"));
        assert!(relogin_hint(p).contains("log in again"));
        assert!(smoke_rejected_msg(p).contains("read-only"));
        assert!(RELOADED_MSG.contains("reloaded"));
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bx-lock-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn test_lock_path_next_to_session() {
        assert_eq!(
            lock_path(Path::new("/p/data/session.json")),
            PathBuf::from("/p/data/session.json.lock")
        );
    }

    #[test]
    fn test_lock_is_exclusive_and_released_on_drop() {
        let d = tmpdir("excl");
        let s = d.join("session.json");
        let first = SessionLock::try_acquire(&s).unwrap().expect("free lock");
        assert!(lock_path(&s).exists());
        // a second holder (separate open file, as another process would be) is refused
        assert!(SessionLock::try_acquire(&s).unwrap().is_none());
        drop(first);
        let again = SessionLock::try_acquire(&s).unwrap();
        assert!(again.is_some());
        drop(again);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn test_lock_acquire_times_out_then_succeeds() {
        let d = tmpdir("timeout");
        let s = d.join("session.json");
        let held = SessionLock::acquire(&s, Duration::from_millis(10)).unwrap();
        let start = Instant::now();
        let err = SessionLock::acquire(&s, Duration::from_millis(150)).unwrap_err();
        assert!(start.elapsed() >= Duration::from_millis(150));
        assert!(err.to_string().contains("held by another iambitchx"));
        // waiter gets it once the holder lets go
        let s2 = s.clone();
        let waiter =
            std::thread::spawn(move || SessionLock::acquire(&s2, Duration::from_secs(5)).is_ok());
        std::thread::sleep(Duration::from_millis(100));
        drop(held);
        assert!(waiter.join().unwrap());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn test_stale_lock_file_is_not_a_held_lock() {
        // a leftover lock file from a crashed process does not block
        let d = tmpdir("stale");
        let s = d.join("session.json");
        std::fs::write(lock_path(&s), b"").unwrap();
        assert!(SessionLock::try_acquire(&s).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&d);
    }
}
