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

/// What a proactive (before-expiry) refresh does, decided under the lock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Proactive {
    /// `session.json` holds different tokens (another process rotated):
    /// adopt them instead of spending our (now stale) refresh token.
    ReloadFromDisk,
    /// Disk agrees with us and the tokens are due: refresh, persist.
    Refresh,
    /// Disk agrees with us but was written within `fresh_within` (e.g. the
    /// sync loop's 401 recovery just refreshed): nothing to do, reschedule.
    Skip,
    /// Read-only mode, not logged in, or no refresh token: stop the task.
    Stop,
}

/// Proactive refresh policy. `disk_age` is how long ago `session.json` was
/// written (= when its access token was issued).
pub fn decide_proactive(
    mode: Mode,
    in_use: Option<&SessionTokens>,
    on_disk: Option<&SessionTokens>,
    disk_age: Option<Duration>,
    fresh_within: Duration,
) -> Proactive {
    if mode == Mode::ReadOnly {
        return Proactive::Stop;
    }
    let Some(mine) = in_use else {
        return Proactive::Stop;
    };
    match on_disk {
        Some(disk) if !same_tokens(mine, disk) => Proactive::ReloadFromDisk,
        _ if mine.refresh_token.is_none() => Proactive::Stop,
        Some(_) if disk_age.is_some_and(|age| age < fresh_within) => Proactive::Skip,
        _ => Proactive::Refresh,
    }
}

/// The token operations [`recover_locked`] / [`proactive_locked`] drive.
/// Implemented over the real client in `login.rs`, faked in tests.
#[allow(async_fn_in_trait)]
pub trait TokenOps {
    /// Tokens the client is using now.
    fn in_use(&self) -> Option<SessionTokens>;
    /// Tokens in `session.json`, if readable.
    fn on_disk(&self) -> Option<SessionTokens>;
    /// Age of `session.json` (time since its tokens were issued).
    fn disk_age(&self) -> Option<Duration>;
    /// Adopt the tokens from `session.json`. `Ok(true)`: adopted;
    /// `Ok(false)`: could not adopt but refreshed ourselves and persisted.
    async fn adopt_disk(&self, on_disk: Option<&SessionTokens>) -> anyhow::Result<bool>;
    /// Refresh with our refresh token and persist to `session.json` (0600).
    async fn refresh(&self) -> anyhow::Result<()>;
}

/// What a locked recovery / proactive refresh did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Done {
    /// Adopted tokens another process wrote to `session.json`.
    Reloaded,
    /// Refreshed with our refresh token and persisted.
    Refreshed,
    /// Nothing to do this time.
    Skipped,
    /// Never refresh again (read-only, no refresh token).
    Stopped,
}

/// How long to wait for another holder of `session.json.lock`.
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// Recover from `M_UNKNOWN_TOKEN` under the SessionLock (read, [`decide`],
/// act). `Err` is the relogin hint (or a lock timeout).
pub async fn recover_locked<O: TokenOps>(
    ops: &O,
    session_json: &Path,
    mode: Mode,
    attempts: u32,
    lock_timeout: Duration,
) -> anyhow::Result<Done> {
    let relogin = || anyhow::anyhow!(relogin_hint(session_json));
    if !mode.may_write() {
        anyhow::bail!(smoke_rejected_msg(session_json));
    }
    let _lock = SessionLock::acquire_async(session_json.to_path_buf(), lock_timeout).await?;
    let in_use = ops.in_use();
    let on_disk = ops.on_disk();
    match decide(mode, in_use.as_ref(), on_disk.as_ref(), attempts) {
        Recovery::ReloadFromDisk => match ops.adopt_disk(on_disk.as_ref()).await {
            Ok(true) => Ok(Done::Reloaded),
            Ok(false) => Ok(Done::Refreshed),
            Err(e) => {
                tracing::warn!("adopting session.json failed: {e}");
                Err(relogin())
            },
        },
        Recovery::Refresh => match ops.refresh().await {
            Ok(()) => Ok(Done::Refreshed),
            Err(e) => {
                tracing::warn!("token refresh failed: {e}");
                Err(relogin())
            },
        },
        Recovery::Relogin | Recovery::Fail => Err(relogin()),
    }
}

/// Proactive refresh under the same SessionLock and disk comparison as
/// [`recover_locked`]: adopt `session.json` if another process already
/// rotated, skip if it was just refreshed, else refresh. Read-only mode
/// returns [`Done::Stopped`] without touching the lock or the tokens.
pub async fn proactive_locked<O: TokenOps>(
    ops: &O,
    session_json: &Path,
    mode: Mode,
    fresh_within: Duration,
    lock_timeout: Duration,
) -> anyhow::Result<Done> {
    if !mode.may_write() {
        return Ok(Done::Stopped);
    }
    let _lock = SessionLock::acquire_async(session_json.to_path_buf(), lock_timeout).await?;
    let in_use = ops.in_use();
    let on_disk = ops.on_disk();
    let decision =
        decide_proactive(mode, in_use.as_ref(), on_disk.as_ref(), ops.disk_age(), fresh_within);
    match decision {
        Proactive::ReloadFromDisk => ops.adopt_disk(on_disk.as_ref()).await.map(|adopted| {
            if adopted {
                Done::Reloaded
            } else {
                Done::Refreshed
            }
        }),
        Proactive::Refresh => ops.refresh().await.map(|()| Done::Refreshed),
        Proactive::Skip => Ok(Done::Skipped),
        Proactive::Stop => Ok(Done::Stopped),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// Fake client: counts adopt/refresh calls and records whether the
    /// SessionLock was held while they ran.
    struct FakeOps {
        path: PathBuf,
        mine: Mutex<Option<SessionTokens>>,
        disk: Option<SessionTokens>,
        age: Option<Duration>,
        adopts: AtomicU32,
        refreshes: AtomicU32,
        unlocked_calls: AtomicU32,
    }

    impl FakeOps {
        fn new(path: &Path, mine: SessionTokens, disk: SessionTokens, age: u64) -> Self {
            FakeOps {
                path: path.to_path_buf(),
                mine: Mutex::new(Some(mine)),
                disk: Some(disk),
                age: Some(Duration::from_secs(age)),
                adopts: AtomicU32::new(0),
                refreshes: AtomicU32::new(0),
                unlocked_calls: AtomicU32::new(0),
            }
        }
        fn check_locked(&self) {
            if SessionLock::try_acquire(&self.path).unwrap().is_some() {
                self.unlocked_calls.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    impl TokenOps for FakeOps {
        fn in_use(&self) -> Option<SessionTokens> {
            self.mine.lock().unwrap().clone()
        }
        fn on_disk(&self) -> Option<SessionTokens> {
            self.disk.clone()
        }
        fn disk_age(&self) -> Option<Duration> {
            self.age
        }
        async fn adopt_disk(&self, on_disk: Option<&SessionTokens>) -> anyhow::Result<bool> {
            self.check_locked();
            self.adopts.fetch_add(1, Ordering::SeqCst);
            *self.mine.lock().unwrap() = on_disk.cloned();
            Ok(true)
        }
        async fn refresh(&self) -> anyhow::Result<()> {
            self.check_locked();
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            *self.mine.lock().unwrap() = Some(toks("new", Some("newr")));
            Ok(())
        }
    }

    const FRESH: Duration = Duration::from_secs(235);

    #[test]
    fn test_decide_proactive() {
        let mine = toks("a", Some("r"));
        let other = toks("b", Some("s"));
        let age = |s| Some(Duration::from_secs(s));
        let d = |m, i, o, a| decide_proactive(m, i, o, a, FRESH);
        // read-only never refreshes or reloads
        for (i, o) in [
            (Some(&mine), Some(&mine)),
            (Some(&mine), Some(&other)),
            (None, None),
        ] {
            assert_eq!(d(Mode::ReadOnly, i, o, age(999)), Proactive::Stop);
        }
        // disk newer (another process rotated): reload, never spend our refresh token
        assert_eq!(d(Mode::Live, Some(&mine), Some(&other), age(1)), Proactive::ReloadFromDisk);
        assert_eq!(d(Mode::Live, Some(&mine), Some(&other), age(999)), Proactive::ReloadFromDisk);
        // same tokens, due: refresh; just written: skip
        assert_eq!(d(Mode::Live, Some(&mine), Some(&mine), age(240)), Proactive::Refresh);
        assert_eq!(d(Mode::Live, Some(&mine), Some(&mine), age(10)), Proactive::Skip);
        assert_eq!(d(Mode::Live, Some(&mine), None, None), Proactive::Refresh);
        // nothing to refresh with
        let no_rt = toks("a", None);
        assert_eq!(d(Mode::Live, Some(&no_rt), Some(&no_rt), age(999)), Proactive::Stop);
        assert_eq!(d(Mode::Live, None, Some(&mine), age(999)), Proactive::Stop);
    }

    #[tokio::test]
    async fn test_proactive_readonly_never_refreshes() {
        let d = tmpdir("ro");
        let s = d.join("session.json");
        let ops = FakeOps::new(&s, toks("a", Some("r")), toks("a", Some("r")), 999);
        let done = proactive_locked(&ops, &s, Mode::ReadOnly, FRESH, LOCK_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(done, Done::Stopped);
        assert_eq!(ops.refreshes.load(Ordering::SeqCst), 0);
        assert_eq!(ops.adopts.load(Ordering::SeqCst), 0);
        // read-only does not even create the lock file
        assert!(!lock_path(&s).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn test_proactive_refresh_waits_for_lock() {
        let d = tmpdir("plock");
        let s = d.join("session.json");
        let ops = FakeOps::new(&s, toks("a", Some("r")), toks("a", Some("r")), 240);
        // another process holds the lock: no refresh happens, timeout error
        let held = SessionLock::acquire(&s, Duration::from_millis(10)).unwrap();
        let err = proactive_locked(&ops, &s, Mode::Live, FRESH, Duration::from_millis(100)).await;
        assert!(err.is_err());
        assert_eq!(ops.refreshes.load(Ordering::SeqCst), 0);
        drop(held);
        // free: refreshes, and the lock was held during the refresh
        let done = proactive_locked(&ops, &s, Mode::Live, FRESH, LOCK_TIMEOUT).await.unwrap();
        assert_eq!(done, Done::Refreshed);
        assert_eq!(ops.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(ops.adopts.load(Ordering::SeqCst), 0);
        assert_eq!(ops.unlocked_calls.load(Ordering::SeqCst), 0);
        // lock released afterwards
        assert!(SessionLock::try_acquire(&s).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn test_proactive_reloads_when_disk_newer() {
        let d = tmpdir("preload");
        let s = d.join("session.json");
        let disk = toks("b", Some("s"));
        let ops = FakeOps::new(&s, toks("a", Some("r")), disk.clone(), 5);
        let done = proactive_locked(&ops, &s, Mode::Live, FRESH, LOCK_TIMEOUT).await.unwrap();
        assert_eq!(done, Done::Reloaded);
        assert_eq!(
            ops.refreshes.load(Ordering::SeqCst),
            0,
            "must not spend the stale refresh token"
        );
        assert_eq!(ops.adopts.load(Ordering::SeqCst), 1);
        assert_eq!(ops.unlocked_calls.load(Ordering::SeqCst), 0);
        assert_eq!(ops.in_use(), Some(disk));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn test_proactive_skips_when_just_refreshed() {
        let d = tmpdir("pskip");
        let s = d.join("session.json");
        let ops = FakeOps::new(&s, toks("a", Some("r")), toks("a", Some("r")), 3);
        let done = proactive_locked(&ops, &s, Mode::Live, FRESH, LOCK_TIMEOUT).await.unwrap();
        assert_eq!(done, Done::Skipped);
        assert_eq!(ops.refreshes.load(Ordering::SeqCst) + ops.adopts.load(Ordering::SeqCst), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn test_recover_locked_paths() {
        let d = tmpdir("rec");
        let s = d.join("session.json");
        // disk differs: reload under the lock
        let ops = FakeOps::new(&s, toks("a", Some("r")), toks("b", Some("s")), 1);
        assert_eq!(
            recover_locked(&ops, &s, Mode::Live, 0, LOCK_TIMEOUT).await.unwrap(),
            Done::Reloaded
        );
        assert_eq!(ops.refreshes.load(Ordering::SeqCst), 0);
        // disk same: refresh under the lock
        let ops = FakeOps::new(&s, toks("a", Some("r")), toks("a", Some("r")), 1);
        assert_eq!(
            recover_locked(&ops, &s, Mode::Live, 0, LOCK_TIMEOUT).await.unwrap(),
            Done::Refreshed
        );
        assert_eq!(ops.unlocked_calls.load(Ordering::SeqCst), 0);
        // second rejection: relogin; read-only: smoke message, nothing touched
        let err = recover_locked(&ops, &s, Mode::Live, 1, LOCK_TIMEOUT).await.unwrap_err();
        assert!(err.to_string().contains("log in again"));
        let err = recover_locked(&ops, &s, Mode::ReadOnly, 0, LOCK_TIMEOUT)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("read-only"));
        assert_eq!(ops.refreshes.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

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
