//! # Token coordinator
//!
//! The SDK's own auto-refresh is off (see `login::build`): the only refresh
//! path is `login::recover`, under the `session.json` lock. OAuth access
//! tokens are short-lived (MAS: ~5 min), so every request can hit
//! `M_UNKNOWN_TOKEN`. This module keeps them fresh:
//!
//! * [`TokenCoordinator::refresh`] runs one refresh at a time and coalesces
//!   concurrent callers: whoever saw the same token epoch shares the result of
//!   a single refresh.
//! * [`with_retry`] wraps a one-shot request: on `M_UNKNOWN_TOKEN` / soft
//!   logout it refreshes through the coordinator and retries exactly once.
//! * [`refresh_delay`] / [`TokenCoordinator::proactive_tick`] refresh shortly
//!   before the token expires, so most requests never see a 401.
use std::future::{Future, IntoFuture};
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

/// Lifetime assumed when the token response's `expires_in` is unknown (the
/// SDK does not expose it): MAS issues 5-minute access tokens.
pub const ASSUMED_LIFETIME: Duration = Duration::from_secs(300);

/// Never schedule proactive refreshes closer together than this.
pub const MIN_DELAY: Duration = Duration::from_secs(15);

/// Something that can refresh the access token (live: `login::recover`).
pub trait Refresher: Send + Sync {
    /// Whether there is anything to refresh with (a refresh token).
    fn can_refresh(&self) -> bool;

    /// Refresh the tokens: `Ok(expires_in)` of the new access token if the
    /// token response told us (`None` if unknown); `Err` is for the user.
    fn refresh(&self) -> impl Future<Output = Result<Option<Duration>, String>> + Send;
}

/// Errors that may mean "the access token is no longer accepted".
pub trait TokenError {
    /// `M_UNKNOWN_TOKEN` (soft logout included).
    fn is_unknown_token(&self) -> bool;
}

impl TokenError for matrix_sdk::Error {
    fn is_unknown_token(&self) -> bool {
        crate::login::is_unknown_token(self)
    }
}

impl TokenError for matrix_sdk::HttpError {
    fn is_unknown_token(&self) -> bool {
        crate::login::is_unknown_token_http(self)
    }
}

impl TokenError for anyhow::Error {
    fn is_unknown_token(&self) -> bool {
        self.chain().any(|e| {
            e.downcast_ref::<matrix_sdk::Error>()
                .is_some_and(TokenError::is_unknown_token) ||
                e.downcast_ref::<matrix_sdk::HttpError>()
                    .is_some_and(TokenError::is_unknown_token)
        })
    }
}

/// When the next proactive refresh is due after a refresh: a margin of a
/// fifth of the lifetime (at least 30 s) before expiry, never sooner than
/// [`MIN_DELAY`]. Unknown lifetime → [`ASSUMED_LIFETIME`] (every 4 min).
pub fn refresh_delay(expires_in: Option<Duration>) -> Duration {
    let life = expires_in.unwrap_or(ASSUMED_LIFETIME);
    let margin = (life / 5).max(Duration::from_secs(30));
    life.saturating_sub(margin).max(MIN_DELAY)
}

/// How long to sleep from `now` until a refresh due `delay` after `last`
/// (zero if overdue).
pub fn time_until_due(last: Instant, now: Instant, delay: Duration) -> Duration {
    (last + delay).saturating_duration_since(now)
}

struct State {
    /// When the tokens in use were issued (last successful refresh).
    issued: Instant,
    /// Lifetime from the last token response, if known.
    expires_in: Option<Duration>,
    /// Outcome of the last refresh attempt, shared with coalesced callers.
    last_result: Result<(), String>,
}

/// The single owner of token refreshes in the live client.
pub struct TokenCoordinator<R> {
    refresher: R,
    /// Bumped after every refresh attempt (success or failure). A caller
    /// snapshots it before its request; if it moved by the time the caller
    /// wants a refresh, someone else already did one for it.
    epoch: AtomicU64,
    gate: Mutex<()>,
    state: StdMutex<State>,
}

impl<R: Refresher> TokenCoordinator<R> {
    /// `issued`: when the current tokens were issued (e.g. the mtime of
    /// `session.json`), so the first proactive refresh is not late.
    pub fn new(refresher: R, issued: Instant) -> Self {
        TokenCoordinator {
            refresher,
            epoch: AtomicU64::new(0),
            gate: Mutex::new(()),
            state: StdMutex::new(State { issued, expires_in: None, last_result: Ok(()) }),
        }
    }

    /// The current token epoch; pass it to [`Self::refresh`].
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// When the tokens in use were issued.
    pub fn issued(&self) -> Instant {
        self.state().issued
    }

    /// Delay between issue and the next proactive refresh.
    pub fn delay(&self) -> Duration {
        refresh_delay(self.state().expires_in)
    }

    /// Refresh the tokens unless another caller already did since `seen`
    /// (then share its outcome). `Ok(true)`: this call refreshed.
    pub async fn refresh(&self, seen: u64) -> Result<bool, String> {
        let _gate = self.gate.lock().await;
        if self.epoch() != seen {
            return self.state().last_result.clone().map(|()| false);
        }
        let res = self.refresher.refresh().await;
        {
            let mut st = self.state();
            if let Ok(expires_in) = res {
                st.issued = Instant::now();
                st.expires_in = expires_in;
            }
            st.last_result = res.clone().map(|_| ());
        }
        self.epoch.fetch_add(1, Ordering::SeqCst);
        res.map(|_| true)
    }

    /// One step of the proactive loop at `now`: refresh if due. `Ok(None)`
    /// if not due (or nothing to refresh with), otherwise the refresh result.
    pub async fn proactive_tick(&self, now: Instant) -> Option<Result<bool, String>> {
        if !self.refresher.can_refresh() {
            return None;
        }
        let seen = self.epoch();
        if !time_until_due(self.issued(), now, self.delay()).is_zero() {
            return None;
        }
        Some(self.refresh(seen).await)
    }

    /// Whether the refresher has a refresh token to work with.
    pub fn can_refresh(&self) -> bool {
        self.refresher.can_refresh()
    }

    /// How long until the next proactive refresh is due.
    pub fn until_due(&self, now: Instant) -> Duration {
        time_until_due(self.issued(), now, self.delay())
    }
}

/// Run a one-shot request; on `M_UNKNOWN_TOKEN` refresh through `tokens`
/// (coalesced with concurrent callers) and retry exactly once. A failed
/// refresh or a second rejection returns the request's error.
pub async fn with_retry<R, T, E, F, Fut>(tokens: &TokenCoordinator<R>, mut op: F) -> Result<T, E>
where
    R: Refresher,
    E: TokenError,
    F: FnMut() -> Fut,
    Fut: IntoFuture<Output = Result<T, E>>,
{
    let seen = tokens.epoch();
    match op().await {
        Err(e) if e.is_unknown_token() => {
            if let Err(msg) = tokens.refresh(seen).await {
                tracing::warn!("token refresh failed: {msg}");
                return Err(e);
            }
            op().await
        },
        res => res,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;

    use super::*;

    /// A fake homeserver: tokens have a version; requests made with an older
    /// version than the server's current one are rejected.
    #[derive(Default)]
    struct Fake {
        expires_in: Option<Duration>,
        refreshes: AtomicU32,
        fail: bool,
        no_refresh_token: bool,
        server_version: AtomicU32,
    }

    #[derive(Debug, PartialEq)]
    enum FakeErr {
        UnknownToken,
        Other,
    }

    impl TokenError for FakeErr {
        fn is_unknown_token(&self) -> bool {
            *self == FakeErr::UnknownToken
        }
    }

    impl Refresher for Arc<Fake> {
        fn can_refresh(&self) -> bool {
            !self.no_refresh_token
        }

        async fn refresh(&self) -> Result<Option<Duration>, String> {
            // let concurrent callers pile up behind the gate
            tokio::time::sleep(Duration::from_millis(20)).await;
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err("refresh rejected".into());
            }
            self.server_version.fetch_add(1, Ordering::SeqCst);
            Ok(self.expires_in)
        }
    }

    fn coord(fake: &Arc<Fake>) -> TokenCoordinator<Arc<Fake>> {
        TokenCoordinator::new(fake.clone(), Instant::now())
    }

    #[tokio::test]
    async fn test_401_then_success_retries_once() {
        let fake = Arc::new(Fake::default());
        let tokens = coord(&fake);
        let calls = AtomicU32::new(0);
        let res: Result<&str, FakeErr> = with_retry(&tokens, || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    Err(FakeErr::UnknownToken)
                } else {
                    Ok("sent")
                }
            }
        })
        .await;
        assert_eq!(res, Ok("sent"));
        assert_eq!(calls.load(Ordering::SeqCst), 2, "request retried exactly once");
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_second_401_surfaces_error() {
        let fake = Arc::new(Fake::default());
        let tokens = coord(&fake);
        let calls = AtomicU32::new(0);
        let res: Result<(), FakeErr> = with_retry(&tokens, || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(FakeErr::UnknownToken) }
        })
        .await;
        assert_eq!(res, Err(FakeErr::UnknownToken));
        assert_eq!(calls.load(Ordering::SeqCst), 2, "one retry, no more");
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_failed_refresh_surfaces_error_without_retry() {
        let fake = Arc::new(Fake { fail: true, ..Fake::default() });
        let tokens = coord(&fake);
        let calls = AtomicU32::new(0);
        let res: Result<(), FakeErr> = with_retry(&tokens, || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(FakeErr::UnknownToken) }
        })
        .await;
        assert_eq!(res, Err(FakeErr::UnknownToken));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_other_errors_are_not_retried() {
        let fake = Arc::new(Fake::default());
        let tokens = coord(&fake);
        let calls = AtomicU32::new(0);
        let res: Result<(), FakeErr> = with_retry(&tokens, || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(FakeErr::Other) }
        })
        .await;
        assert_eq!(res, Err(FakeErr::Other));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 0);
    }

    /// A request made with token version `used`: version 0 is the expired
    /// startup token, anything a refresh issued is accepted.
    async fn request(used: u32) -> Result<u32, FakeErr> {
        tokio::task::yield_now().await;
        if used == 0 {
            Err(FakeErr::UnknownToken)
        } else {
            Ok(used)
        }
    }

    #[tokio::test]
    async fn test_concurrent_401s_trigger_one_refresh() {
        let fake = Arc::new(Fake::default());
        let tokens = coord(&fake);
        let one = || {
            with_retry(&tokens, || {
                // the token in use is whatever the server last issued
                let used = fake.server_version.load(Ordering::SeqCst);
                request(used)
            })
        };
        let (a, b, c, d, e) = tokio::join!(one(), one(), one(), one(), one());
        for r in [a, b, c, d, e] {
            assert_eq!(r, Ok(1), "every caller succeeds on its retry");
        }
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1, "exactly one refresh");
        assert_eq!(tokens.epoch(), 1);
    }

    #[tokio::test]
    async fn test_concurrent_failed_refresh_is_shared() {
        let fake = Arc::new(Fake { fail: true, ..Fake::default() });
        let tokens = coord(&fake);
        let seen = tokens.epoch();
        let (a, b, c) =
            tokio::join!(tokens.refresh(seen), tokens.refresh(seen), tokens.refresh(seen));
        for r in [a, b, c] {
            assert_eq!(r, Err("refresh rejected".to_string()));
        }
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1, "failure not retried per caller");
        // a later incident (new epoch) may try again
        assert!(tokens.refresh(tokens.epoch()).await.is_err());
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_refresh_delay() {
        // unknown lifetime: MAS 5 min, refresh after 4
        assert_eq!(refresh_delay(None), Duration::from_secs(240));
        assert_eq!(refresh_delay(Some(Duration::from_secs(300))), Duration::from_secs(240));
        // long tokens: a fifth before expiry
        assert_eq!(refresh_delay(Some(Duration::from_secs(3600))), Duration::from_secs(2880));
        // short tokens: at least 30 s margin, never below MIN_DELAY
        assert_eq!(refresh_delay(Some(Duration::from_secs(60))), Duration::from_secs(30));
        assert_eq!(refresh_delay(Some(Duration::from_secs(20))), MIN_DELAY);
    }

    #[test]
    fn test_time_until_due() {
        let t0 = Instant::now();
        let d = Duration::from_secs(240);
        assert_eq!(time_until_due(t0, t0, d), d);
        assert_eq!(time_until_due(t0, t0 + Duration::from_secs(200), d), Duration::from_secs(40));
        assert_eq!(time_until_due(t0, t0 + Duration::from_secs(240), d), Duration::ZERO);
        assert_eq!(time_until_due(t0, t0 + Duration::from_secs(999), d), Duration::ZERO);
    }

    #[tokio::test]
    async fn test_proactive_tick_schedule() {
        let fake = Arc::new(Fake::default());
        let t0 = Instant::now();
        let tokens = TokenCoordinator::new(fake.clone(), t0);
        assert_eq!(tokens.until_due(t0), Duration::from_secs(240));

        // not due yet: no refresh
        assert_eq!(tokens.proactive_tick(t0 + Duration::from_secs(239)).await, None);
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 0);

        // due: one refresh, and the clock restarts from it
        assert_eq!(tokens.proactive_tick(t0 + Duration::from_secs(240)).await, Some(Ok(true)));
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1);
        let issued = tokens.issued();
        assert!(issued > t0);
        assert_eq!(tokens.proactive_tick(issued + Duration::from_secs(239)).await, None);
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_proactive_tick_tracks_expires_in() {
        // the token response said 1 h: next refresh 48 min after it
        let fake = Arc::new(Fake {
            expires_in: Some(Duration::from_secs(3600)),
            ..Fake::default()
        });
        let t0 = Instant::now();
        let tokens = TokenCoordinator::new(fake.clone(), t0);
        assert_eq!(tokens.delay(), Duration::from_secs(240), "unknown until the first response");
        assert_eq!(tokens.proactive_tick(t0 + Duration::from_secs(240)).await, Some(Ok(true)));
        assert_eq!(tokens.delay(), Duration::from_secs(2880));
        let issued = tokens.issued();
        assert_eq!(tokens.proactive_tick(issued + Duration::from_secs(2879)).await, None);
        assert_eq!(tokens.proactive_tick(issued + Duration::from_secs(2880)).await, Some(Ok(true)));
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_proactive_tick_old_session_refreshes_at_once() {
        // tokens issued 10 min ago (session.json mtime): overdue on startup
        let fake = Arc::new(Fake::default());
        let now = Instant::now();
        let issued = now.checked_sub(Duration::from_secs(600)).unwrap_or(now);
        let tokens = TokenCoordinator::new(fake.clone(), issued);
        if issued < now {
            assert_eq!(tokens.until_due(now), Duration::ZERO);
            assert_eq!(tokens.proactive_tick(now).await, Some(Ok(true)));
        }
    }

    #[tokio::test]
    async fn test_proactive_tick_needs_refresh_token() {
        let fake = Arc::new(Fake { no_refresh_token: true, ..Fake::default() });
        let t0 = Instant::now();
        let tokens = TokenCoordinator::new(fake.clone(), t0);
        assert_eq!(tokens.proactive_tick(t0 + Duration::from_secs(9999)).await, None);
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 0);
    }
}
