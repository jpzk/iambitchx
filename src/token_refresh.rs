//! # Proactive token refresh
//!
//! The SDK's own auto-refresh is off (see `login::build`): it would rotate
//! the MAS refresh token outside `session.json.lock` and burn it when two
//! processes refresh at once. Without it, every request between access-token
//! expiry and the sync loop's 401 recovery fails (send, /away, nick, whois).
//!
//! This task closes that gap: it refreshes [`REFRESH_MARGIN`] before the
//! access token expires, through [`session_guard::proactive_locked`] (same
//! SessionLock and disk comparison as the 401 recovery: adopt `session.json`
//! if another process already rotated, else refresh and persist), then
//! reschedules from the new token's issue time. The 401 recovery in the sync
//! loop stays as the safety net. `--smoke` ([`Mode::ReadOnly`]) never starts
//! it.
//!
//! ## Where the expiry comes from
//!
//! matrix-sdk 0.19 drops `expires_in` from the token response
//! (`OAuth::refresh_access_token_inner` keeps only the two tokens) and
//! exposes no expiry. So the expiry is `issued + ACCESS_TOKEN_TTL`, where
//! `issued` is the mtime of `session.json` (rewritten on every refresh, by
//! us or by another process) and the TTL is the one measured from our MAS
//! homeserver: refresh -> next `M_UNKNOWN_TOKEN` was 307s and 309s with a 30s
//! sync long-poll, i.e. MAS's default 300s access-token TTL. If a server
//! issues shorter tokens, the 401 recovery still catches it.
use std::time::{Duration, SystemTime};

use matrix_sdk::Client;

use crate::config::ApplicationSettings;
use crate::login::{self, ClientOps};
use crate::session_guard::{self, Done, Mode};

/// Access-token lifetime assumed when the SDK hides `expires_in` (measured
/// from MAS: 300s; see module docs).
pub(crate) const ACCESS_TOKEN_TTL: Duration = Duration::from_secs(300);

/// Refresh this long before the access token expires.
pub(crate) const REFRESH_MARGIN: Duration = Duration::from_secs(60);

/// After a failed proactive refresh, try again this much later (the 401
/// recovery remains armed meanwhile).
pub(crate) const RETRY_AFTER_ERROR: Duration = Duration::from_secs(30);

/// A `session.json` younger than this at wake-up was rewritten after we
/// scheduled (e.g. by the 401 recovery): skip and reschedule. Slightly less
/// than `TTL - margin` so a normal, on-time wake-up always refreshes.
pub(crate) const fn fresh_within(ttl: Duration, margin: Duration) -> Duration {
    ttl.saturating_sub(margin).saturating_sub(Duration::from_secs(5))
}

/// Delay from `now` until the refresh is due: `issued + ttl - margin`
/// (i.e. expiry minus margin), zero if already past.
pub(crate) fn due_in(
    issued: SystemTime,
    now: SystemTime,
    ttl: Duration,
    margin: Duration,
) -> Duration {
    let due = issued + ttl.saturating_sub(margin);
    due.duration_since(now).unwrap_or(Duration::ZERO)
}

/// Whether a process in `mode` runs the proactive refresh at all.
pub(crate) fn enabled(mode: Mode) -> bool {
    mode.may_write()
}

/// Start the background refresh task for a live client. No-op (returns
/// `None`) in read-only mode.
pub(crate) fn spawn(
    client: Client,
    settings: ApplicationSettings,
    mode: Mode,
) -> Option<tokio::task::JoinHandle<()>> {
    if !enabled(mode) {
        return None;
    }
    Some(tokio::spawn(run(client, settings, mode)))
}

async fn run(client: Client, settings: ApplicationSettings, mode: Mode) {
    let fresh = fresh_within(ACCESS_TOKEN_TTL, REFRESH_MARGIN);
    let mut delay = next_delay(&settings);
    loop {
        tracing::info!(
            "token_refresh: next proactive refresh in {}s (ttl {}s, margin {}s)",
            delay.as_secs(),
            ACCESS_TOKEN_TTL.as_secs(),
            REFRESH_MARGIN.as_secs()
        );
        tokio::time::sleep(delay).await;

        let ops = ClientOps { client: &client, settings: &settings };
        let res = session_guard::proactive_locked(
            &ops,
            &settings.session_json,
            mode,
            fresh,
            session_guard::LOCK_TIMEOUT,
        )
        .await;
        delay = match res {
            Ok(Done::Refreshed) => {
                tracing::info!("token_refresh: proactive refresh done");
                next_delay(&settings)
            },
            Ok(Done::Reloaded) => {
                tracing::info!(
                    "token_refresh: session.json was rotated by another process; adopted it"
                );
                next_delay(&settings)
            },
            Ok(Done::Skipped) => {
                tracing::info!("token_refresh: session.json just refreshed; rescheduling");
                next_delay(&settings)
            },
            Ok(Done::Stopped) => {
                tracing::info!("token_refresh: no refresh token / read-only; task stopped");
                return;
            },
            Err(e) => {
                tracing::warn!(
                    "token_refresh: proactive refresh failed ({e}); retry in {}s, 401 recovery stays armed",
                    RETRY_AFTER_ERROR.as_secs()
                );
                RETRY_AFTER_ERROR
            },
        };
    }
}

/// Delay until the token currently in `session.json` is due. An unknown
/// issue time counts as "issued now" (one full interval).
fn next_delay(settings: &ApplicationSettings) -> Duration {
    let now = SystemTime::now();
    let issued = login::session_written_at(settings).unwrap_or(now);
    due_in(issued, now, ACCESS_TOKEN_TTL, REFRESH_MARGIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn test_schedule_is_expiry_minus_margin() {
        let issued = SystemTime::UNIX_EPOCH + S(1_000_000);
        let expiry = issued + ACCESS_TOKEN_TTL;
        // fresh token: due exactly at expiry - margin
        assert_eq!(due_in(issued, issued, ACCESS_TOKEN_TTL, REFRESH_MARGIN), S(240));
        assert_eq!(
            issued + due_in(issued, issued, ACCESS_TOKEN_TTL, REFRESH_MARGIN),
            expiry - REFRESH_MARGIN
        );
        // part-way through the lifetime (e.g. restart with an older session.json)
        assert_eq!(due_in(issued, issued + S(100), ACCESS_TOKEN_TTL, REFRESH_MARGIN), S(140));
        // already inside the margin or expired: refresh now
        assert_eq!(due_in(issued, issued + S(250), ACCESS_TOKEN_TTL, REFRESH_MARGIN), S(0));
        assert_eq!(due_in(issued, issued + S(9999), ACCESS_TOKEN_TTL, REFRESH_MARGIN), S(0));
        // other TTL / margin values
        assert_eq!(due_in(issued, issued, S(3600), S(120)), S(3480));
        // margin larger than ttl: refresh immediately rather than underflow
        assert_eq!(due_in(issued, issued, S(30), S(60)), S(0));
    }

    #[test]
    fn test_margin_constants_sane() {
        assert!(REFRESH_MARGIN < ACCESS_TOKEN_TTL);
        let fresh = fresh_within(ACCESS_TOKEN_TTL, REFRESH_MARGIN);
        // an on-time wake-up (age = ttl - margin) is never treated as "just refreshed"
        assert!(fresh < ACCESS_TOKEN_TTL - REFRESH_MARGIN);
        assert!(fresh > S(0));
        assert!(RETRY_AFTER_ERROR < REFRESH_MARGIN);
    }

    #[test]
    fn test_readonly_never_starts() {
        assert!(!enabled(Mode::ReadOnly));
        assert!(enabled(Mode::Live));
    }
}
