//! OAuth 2.0 Device Authorization Grant (RFC 8628) login.
//!
//! An alternative to the redirect-based flow in [`crate::oauth`]: the user
//! approves the login on any other device by visiting a URL and typing a
//! short code, so it also works over SSH or on headless machines.
//!
//! The SDK keeps its own device-grant helpers private, and its public
//! `OAuth::register_client()`/`restore_registered_client()` set the client's
//! authentication data, after which `OAuth::restore_session()` panics. To end
//! up with a regular, persistable [`OAuthSession`] we therefore talk to the
//! registration, device authorization and token endpoints directly (using the
//! SDK's own HTTP client) and then hand the tokens to `restore_session()`.
//!
//! Secrets (access token, refresh token, device code) are wrapped in
//! [`Secret`], whose `Debug` output is redacted and which has no `Display`,
//! so they can't end up in a `println!` or a tracing line by accident.
use std::time::Duration;

use anyhow::anyhow;
use matrix_sdk::Client;
use matrix_sdk::SessionMeta;
use matrix_sdk::SessionTokens;
use matrix_sdk::authentication::oauth::ClientId;
use matrix_sdk::authentication::oauth::OAuthSession;
use matrix_sdk::authentication::oauth::UserSession;
use matrix_sdk::reqwest;
use matrix_sdk::ruma::DeviceId;
use matrix_sdk::ruma::OwnedDeviceId;
use matrix_sdk::ruma::OwnedUserId;
use matrix_sdk::ruma::UserId;
use matrix_sdk::store::RoomLoadSettings;
use serde::Deserialize;
use url::{Url, form_urlencoded};

use crate::config::SavedDevice;
use crate::oauth::client_metadata;

/// Scope granting full access to the client-server API (MSC2967).
pub const SCOPE_API_FULL_ACCESS: &str = "urn:matrix:org.matrix.msc2967.client:api:*";

/// Prefix of the scope binding the access token to a device ID (MSC2967).
pub const SCOPE_DEVICE_PREFIX: &str = "urn:matrix:org.matrix.msc2967.client:device:";

/// The `grant_type` used when polling the token endpoint (RFC 8628 §3.4).
pub const DEVICE_CODE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// Polling interval when the server doesn't send one (RFC 8628 §3.2).
const DEFAULT_INTERVAL_SECS: u64 = 5;

/// Amount added to the interval on every `slow_down` (RFC 8628 §3.5).
const SLOW_DOWN_STEP_SECS: u64 = 5;

/// Upper bound for the polling interval, so a misbehaving server can't make
/// us sleep for hours.
const MAX_INTERVAL_SECS: u64 = 300;

/// Fallback lifetime of a device code when the server omits `expires_in`.
const DEFAULT_EXPIRES_IN_SECS: u64 = 600;

/// A secret string whose `Debug` output is redacted and which deliberately
/// does not implement `Display`.
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    fn expose(&self) -> &str {
        &self.0
    }

    fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

/// The device authorization response (RFC 8628 §3.2).
#[derive(Debug, Deserialize)]
struct DeviceAuthorizationResponse {
    device_code: Secret,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    interval: Option<u64>,
}

/// A successful token response (RFC 6749 §5.1).
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct DeviceTokens {
    access_token: Secret,
    #[serde(default)]
    refresh_token: Option<Secret>,
}

/// An error response from an OAuth 2.0 endpoint (RFC 6749 §5.2).
#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// What a single poll of the token endpoint told us.
#[derive(Debug, PartialEq, Eq)]
pub enum PollOutcome {
    /// The user approved; here are the tokens.
    Success(DeviceTokens),
    /// `authorization_pending`: keep polling at the current interval.
    Pending,
    /// `slow_down`: keep polling, but increase the interval by 5 seconds.
    SlowDown,
    /// `expired_token`: the device code expired, the user has to start over.
    Expired,
    /// `access_denied`: the user rejected the request.
    Denied,
    /// Anything else is fatal.
    Failed(String),
}

/// Classify a token endpoint response. Never includes the raw response body
/// in the error, since a body could carry tokens; the server-chosen
/// `error_description` is passed through the remote-text sanitiser (controls
/// neutralised, token-shaped strings masked) before it is quoted (AUTH-17).
pub fn classify_poll(status: u16, body: &[u8]) -> PollOutcome {
    if (200..300).contains(&status) {
        return match serde_json::from_slice::<DeviceTokens>(body) {
            Ok(tokens) => PollOutcome::Success(tokens),
            Err(_) => PollOutcome::Failed("malformed token response from server".into()),
        };
    }

    match serde_json::from_slice::<OAuthErrorResponse>(body) {
        Ok(err) => match err.error.as_str() {
            "authorization_pending" => PollOutcome::Pending,
            "slow_down" => PollOutcome::SlowDown,
            "expired_token" => PollOutcome::Expired,
            "access_denied" => PollOutcome::Denied,
            other => {
                let desc = err
                    .error_description
                    .map(|d| format!(": {}", crate::bitchx::text::terminal_text(&d)))
                    .unwrap_or_default();
                PollOutcome::Failed(format!(
                    "token endpoint error {}{desc}",
                    crate::bitchx::text::terminal_text(other)
                ))
            },
        },
        Err(_) => PollOutcome::Failed(format!("token endpoint returned HTTP {status}")),
    }
}

/// What to do after a poll.
#[derive(Debug, PartialEq, Eq)]
pub enum PollStep {
    /// Sleep for [`PollTimer::interval`] and poll again.
    Wait,
    /// Done: log in with these tokens.
    Done(DeviceTokens),
    /// Give up with this message.
    Fail(String),
}

/// Tracks the polling interval and the device code's lifetime.
#[derive(Debug)]
pub struct PollTimer {
    interval: Duration,
    remaining: Duration,
}

impl PollTimer {
    pub fn new(interval: Option<u64>, expires_in: Option<u64>) -> Self {
        let interval = interval.unwrap_or(DEFAULT_INTERVAL_SECS).clamp(1, MAX_INTERVAL_SECS);
        let remaining = expires_in.unwrap_or(DEFAULT_EXPIRES_IN_SECS);

        PollTimer {
            interval: Duration::from_secs(interval),
            remaining: Duration::from_secs(remaining),
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Account for one sleep of [`Self::interval`]. Returns `false` once the
    /// device code would have expired before the next poll.
    pub fn tick(&mut self) -> bool {
        match self.remaining.checked_sub(self.interval) {
            Some(rest) => {
                self.remaining = rest;
                true
            },
            None => false,
        }
    }

    fn slow_down(&mut self) {
        let next = self.interval + Duration::from_secs(SLOW_DOWN_STEP_SECS);
        self.interval = next.min(Duration::from_secs(MAX_INTERVAL_SECS));
    }

    /// Advance the state machine with the outcome of one poll.
    pub fn step(&mut self, outcome: PollOutcome) -> PollStep {
        match outcome {
            PollOutcome::Success(tokens) => PollStep::Done(tokens),
            PollOutcome::Pending => PollStep::Wait,
            PollOutcome::SlowDown => {
                self.slow_down();
                PollStep::Wait
            },
            PollOutcome::Expired => {
                PollStep::Fail("The device code expired before it was approved".into())
            },
            PollOutcome::Denied => PollStep::Fail("The login request was denied".into()),
            PollOutcome::Failed(msg) => PollStep::Fail(msg),
        }
    }
}

/// The space-separated scope string: full API access plus the device ID.
pub fn build_scope(device_id: &DeviceId) -> String {
    format!("{SCOPE_API_FULL_ACCESS} {SCOPE_DEVICE_PREFIX}{device_id}")
}

/// Reject anything that isn't `https`.
pub fn require_https(url: &Url, what: &str) -> Result<(), String> {
    if url.scheme() == "https" {
        Ok(())
    } else {
        Err(format!("Refusing non-https {what}: {url}"))
    }
}

/// Anti-phishing check: the verification URI must be https and live on the
/// same host (and port) as the OAuth 2.0 issuer.
pub fn check_verification_uri(uri: &str, issuer: &Url) -> Result<Url, String> {
    let url = Url::parse(uri).map_err(|e| format!("Invalid verification URI {uri:?}: {e}"))?;
    require_https(&url, "verification URI")?;

    let same_host = url.host_str().is_some() && url.host_str() == issuer.host_str();
    let same_port = url.port_or_known_default() == issuer.port_or_known_default();

    if same_host && same_port {
        Ok(url)
    } else {
        Err(format!(
            "Refusing verification URI {url}: its host does not match the issuer {issuer} \
            (possible phishing)"
        ))
    }
}

/// `oauth.rs`'s client metadata, extended with the device code grant type.
fn device_client_metadata() -> Result<serde_json::Value, String> {
    let mut metadata: serde_json::Value = client_metadata()
        .deserialize_as()
        .map_err(|e| format!("Failed to build client metadata: {e}"))?;

    let grants = metadata
        .get_mut("grant_types")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| "Client metadata has no grant_types".to_string())?;

    let device = serde_json::Value::from(DEVICE_CODE_GRANT_TYPE);
    if !grants.contains(&device) {
        grants.push(device);
    }

    Ok(metadata)
}

fn fail(msg: impl Into<String>) -> anyhow::Error {
    anyhow!(msg.into())
}

fn form_body(fields: &[(&str, &str)]) -> String {
    let mut ser = form_urlencoded::Serializer::new(String::new());
    for (k, v) in fields {
        ser.append_pair(k, v);
    }
    ser.finish()
}

async fn read_response(resp: reqwest::Response) -> Result<(u16, Vec<u8>), String> {
    let status = resp.status().as_u16();
    let body = resp.bytes().await.map_err(|e| format!("Failed to read response: {e}"))?;
    Ok((status, body.to_vec()))
}

async fn post_form(
    http: &reqwest::Client,
    url: &Url,
    fields: &[(&str, &str)],
) -> Result<(u16, Vec<u8>), String> {
    let resp = http
        .post(url.as_str())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(form_body(fields))
        .send()
        .await
        .map_err(|e| format!("Request to {url} failed: {e}"))?;
    read_response(resp).await
}

fn endpoint_error(what: &str, status: u16, body: &[u8]) -> String {
    match serde_json::from_slice::<OAuthErrorResponse>(body) {
        Ok(err) => {
            let desc = err
                .error_description
                .map(|d| format!(": {}", crate::bitchx::text::terminal_text(&d)))
                .unwrap_or_default();
            format!(
                "{what} failed (HTTP {status}): {}{desc}",
                crate::bitchx::text::terminal_text(&err.error)
            )
        },
        Err(_) => format!("{what} failed (HTTP {status})"),
    }
}

/// Dynamic client registration (RFC 7591) with the device code grant.
async fn register(http: &reqwest::Client, endpoint: &Url) -> Result<ClientId, String> {
    #[derive(Deserialize)]
    struct Registered {
        client_id: String,
    }

    require_https(endpoint, "registration endpoint")?;
    let metadata = device_client_metadata()?;
    let resp = http
        .post(endpoint.as_str())
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(metadata.to_string())
        .send()
        .await
        .map_err(|e| format!("Client registration request failed: {e}"))?;
    let (status, body) = read_response(resp).await?;

    if !(200..300).contains(&status) {
        return Err(endpoint_error("Client registration", status, &body));
    }

    let reg: Registered = serde_json::from_slice(&body)
        .map_err(|_| "Malformed client registration response".to_string())?;
    Ok(ClientId::new(reg.client_id))
}

/// Ask the homeserver who the token belongs to, before activating anything.
async fn whoami_raw(
    http: &reqwest::Client,
    homeserver: &Url,
    access_token: &Secret,
) -> Result<(OwnedUserId, Option<OwnedDeviceId>), String> {
    #[derive(Deserialize)]
    struct WhoAmI {
        user_id: OwnedUserId,
        #[serde(default)]
        device_id: Option<OwnedDeviceId>,
    }

    let url = homeserver
        .join("_matrix/client/v3/account/whoami")
        .map_err(|e| format!("Invalid homeserver URL: {e}"))?;
    let resp = http
        .get(url.as_str())
        .bearer_auth(access_token.expose())
        .send()
        .await
        .map_err(|e| format!("whoami request failed: {e}"))?;
    let (status, body) = read_response(resp).await?;

    if !(200..300).contains(&status) {
        return Err(format!("whoami failed (HTTP {status})"));
    }

    let who: WhoAmI =
        serde_json::from_slice(&body).map_err(|_| "Malformed whoami response".to_string())?;
    Ok((who.user_id, who.device_id))
}

/// Best-effort revocation of a token we refuse to use.
async fn logout_raw(http: &reqwest::Client, homeserver: &Url, access_token: &Secret) {
    if let Ok(url) = homeserver.join("_matrix/client/v3/logout") {
        let _ = http
            .post(url.as_str())
            .bearer_auth(access_token.expose())
            .header("Content-Type", "application/json")
            .body("{}")
            .send()
            .await;
    }
}

fn print_instructions(auth: &DeviceAuthorizationResponse, uri: &Url, complete: Option<&Url>) {
    print!("{}", instructions(auth, uri, complete));
}

/// The device-link instructions. The URIs and the user code are chosen by
/// the homeserver, so they go through the remote-text sanitiser: no control
/// sequence can clear, overwrite or displace the anti-phishing warning that
/// follows the code (AUTH-16).
fn instructions(auth: &DeviceAuthorizationResponse, uri: &Url, complete: Option<&Url>) -> String {
    let t = crate::bitchx::text::terminal_text;
    let minutes = auth.expires_in.unwrap_or(DEFAULT_EXPIRES_IN_SECS) / 60;

    let mut out = vec![
        String::new(),
        "==================== DEVICE LINK LOGIN ====================".into(),
        String::new(),
        "  On any device with a browser, open:".into(),
        String::new(),
        format!("      {}", t(uri.as_str())),
    ];
    if let Some(complete) = complete {
        out.push(String::new());
        out.push("  or open this link, which already contains the code:".into());
        out.push(String::new());
        out.push(format!("      {}", t(complete.as_str())));
    }
    out.extend([
        String::new(),
        "  and enter the code:".into(),
        String::new(),
        format!("      >>>   {}   <<<", t(&auth.user_code)),
        String::new(),
        "  WARNING: only approve a code that THIS terminal shows you.".into(),
        "  Never enter a code that someone sent you: approving it".into(),
        "  hands them access to your account.".into(),
        String::new(),
        format!("  The code expires in about {minutes} minute(s). Waiting for approval..."),
        "============================================================".into(),
        String::new(),
    ]);
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Log in with the OAuth 2.0 device authorization grant.
///
/// On success the client is logged in and the returned [`OAuthSession`] is
/// what the caller should persist (exactly like the redirect OAuth path).
pub async fn device_link_login(
    client: Client,
    user_hint: &UserId,
    saved_device: Option<SavedDevice>,
) -> anyhow::Result<OAuthSession> {
    let oauth = client.oauth();

    let metadata = oauth.server_metadata().await.map_err(|error| {
        if error.is_not_supported() {
            tracing::warn!("Homeserver doesn't advertise OAuth 2.0 metadata");
        } else {
            tracing::warn!("Error fetching OAuth 2.0 metadata: {error:?}");
        }
        fail(
            "Error fetching homeserver OAuth 2.0 metadata. \
            Homeserver needs to support native OAuth 2.0 for device link login to work",
        )
    })?;

    let issuer = metadata.issuer.clone();
    require_https(&issuer, "OAuth 2.0 issuer").map_err(fail)?;

    let device_endpoint = metadata.device_authorization_endpoint.clone().ok_or_else(|| {
        fail(
            "The homeserver's OAuth 2.0 server does not advertise a \
            device_authorization_endpoint, so device link login is not supported. \
            Try [o]auth instead.",
        )
    })?;
    require_https(&device_endpoint, "device authorization endpoint").map_err(fail)?;

    let token_endpoint = metadata.token_endpoint.clone();
    require_https(&token_endpoint, "token endpoint").map_err(fail)?;

    let http = client.http_client().raw().clone();

    let (client_id, device_id) =
        if let Some(SavedDevice::OAuth { device_id, client_id, .. }) = saved_device {
            (client_id, device_id)
        } else {
            let endpoint = metadata.registration_endpoint.clone().ok_or_else(|| {
                fail("The OAuth 2.0 server does not support dynamic client registration")
            })?;
            (register(&http, &endpoint).await.map_err(fail)?, DeviceId::new())
        };

    tracing::info!("Starting device link login with issuer {issuer}");

    let scope = build_scope(&device_id);
    let (status, body) = post_form(
        &http,
        &device_endpoint,
        &[("client_id", client_id.as_str()), ("scope", scope.as_str())],
    )
    .await
    .map_err(fail)?;

    if !(200..300).contains(&status) {
        let mut msg = endpoint_error("Device authorization request", status, &body);
        if status == 400 || status == 401 {
            msg.push_str(
                " (if this profile was previously logged in with [o]auth, its saved client \
                registration may not allow the device code grant)",
            );
        }
        return Err(fail(msg));
    }

    let auth: DeviceAuthorizationResponse = serde_json::from_slice(&body)
        .map_err(|_| fail("Malformed device authorization response"))?;

    let uri = check_verification_uri(&auth.verification_uri, &issuer).map_err(fail)?;
    let complete = auth
        .verification_uri_complete
        .as_deref()
        .map(|c| check_verification_uri(c, &issuer))
        .transpose()
        .map_err(fail)?;

    print_instructions(&auth, &uri, complete.as_ref());

    let mut timer = PollTimer::new(auth.interval, auth.expires_in);
    let tokens = loop {
        if !timer.tick() {
            return Err(fail("The device code expired before it was approved"));
        }
        tokio::time::sleep(timer.interval()).await;

        let outcome = match post_form(
            &http,
            &token_endpoint,
            &[
                ("grant_type", DEVICE_CODE_GRANT_TYPE),
                ("device_code", auth.device_code.expose()),
                ("client_id", client_id.as_str()),
            ],
        )
        .await
        {
            Ok((status, body)) => classify_poll(status, &body),
            Err(msg) => PollOutcome::Failed(msg),
        };

        match timer.step(outcome) {
            PollStep::Wait => continue,
            PollStep::Done(tokens) => break tokens,
            PollStep::Fail(msg) => return Err(fail(msg)),
        }
    };

    let homeserver = client.homeserver();
    let (user_id, server_device_id) =
        whoami_raw(&http, &homeserver, &tokens.access_token).await.map_err(fail)?;

    if user_id != user_hint {
        logout_raw(&http, &homeserver, &tokens.access_token).await;
        return Err(fail(format!(
            "The login was approved for {user_id}, but this profile is for {user_hint}. \
            The new session has been logged out."
        )));
    }

    let device_id = server_device_id.unwrap_or(device_id);
    let session = OAuthSession {
        client_id,
        user: UserSession {
            meta: SessionMeta { user_id, device_id },
            tokens: SessionTokens {
                access_token: tokens.access_token.into_inner(),
                refresh_token: tokens.refresh_token.map(Secret::into_inner),
            },
        },
    };

    oauth.restore_session(session, RoomLoadSettings::default()).await?;
    client.whoami().await?;

    println!("Device link approved, logged in as {user_hint}.");

    oauth
        .full_session()
        .ok_or_else(|| fail("Logged in client has no OAuth 2.0 session"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issuer() -> Url {
        Url::parse("https://account.example.org/").unwrap()
    }

    fn error_body(error: &str) -> Vec<u8> {
        format!(r#"{{"error":"{error}"}}"#).into_bytes()
    }

    #[test]
    fn test_poll_pending() {
        assert_eq!(classify_poll(400, &error_body("authorization_pending")), PollOutcome::Pending);
        let mut timer = PollTimer::new(Some(5), Some(600));
        assert_eq!(timer.step(PollOutcome::Pending), PollStep::Wait);
        assert_eq!(timer.interval(), Duration::from_secs(5));
    }

    #[test]
    fn test_poll_slow_down() {
        assert_eq!(classify_poll(400, &error_body("slow_down")), PollOutcome::SlowDown);
        let mut timer = PollTimer::new(Some(5), Some(600));
        assert_eq!(timer.step(PollOutcome::SlowDown), PollStep::Wait);
        assert_eq!(timer.interval(), Duration::from_secs(10));
        assert_eq!(timer.step(PollOutcome::SlowDown), PollStep::Wait);
        assert_eq!(timer.interval(), Duration::from_secs(15));
        // Pending afterwards keeps the increased interval.
        assert_eq!(timer.step(PollOutcome::Pending), PollStep::Wait);
        assert_eq!(timer.interval(), Duration::from_secs(15));
    }

    #[test]
    fn test_poll_expired() {
        assert_eq!(classify_poll(400, &error_body("expired_token")), PollOutcome::Expired);
        let mut timer = PollTimer::new(None, None);
        assert!(matches!(timer.step(PollOutcome::Expired), PollStep::Fail(_)));
    }

    #[test]
    fn test_poll_denied() {
        assert_eq!(classify_poll(400, &error_body("access_denied")), PollOutcome::Denied);
        let mut timer = PollTimer::new(None, None);
        assert!(matches!(timer.step(PollOutcome::Denied), PollStep::Fail(_)));
    }

    #[test]
    fn test_poll_success() {
        let body = br#"{"access_token":"at-123","refresh_token":"rt-456","token_type":"Bearer","expires_in":300}"#;
        let outcome = classify_poll(200, body);
        let PollOutcome::Success(tokens) = outcome else {
            panic!("expected success, got {outcome:?}");
        };
        assert_eq!(tokens.access_token.expose(), "at-123");
        assert_eq!(tokens.refresh_token.as_ref().map(Secret::expose), Some("rt-456"));

        let mut timer = PollTimer::new(None, None);
        assert!(matches!(timer.step(PollOutcome::Success(tokens)), PollStep::Done(_)));

        // Refresh token is optional.
        assert!(matches!(
            classify_poll(200, br#"{"access_token":"x","token_type":"Bearer"}"#),
            PollOutcome::Success(DeviceTokens { refresh_token: None, .. })
        ));
    }

    #[test]
    fn test_poll_failures() {
        assert!(matches!(classify_poll(400, &error_body("invalid_grant")), PollOutcome::Failed(_)));
        assert!(matches!(classify_poll(500, b"<html>oops</html>"), PollOutcome::Failed(_)));
        assert!(matches!(classify_poll(200, b"not json"), PollOutcome::Failed(_)));
        let mut timer = PollTimer::new(None, None);
        assert!(matches!(timer.step(PollOutcome::Failed("x".into())), PollStep::Fail(_)));
    }

    #[test]
    fn test_secrets_are_redacted() {
        let outcome =
            classify_poll(200, br#"{"access_token":"at-SECRET","refresh_token":"rt-SECRET"}"#);
        let dbg = format!("{outcome:?}");
        assert!(!dbg.contains("SECRET"), "{dbg}");

        let auth: DeviceAuthorizationResponse = serde_json::from_slice(
            br#"{"device_code":"dc-SECRET","user_code":"ABCD-EFGH",
                "verification_uri":"https://account.example.org/link","expires_in":600}"#,
        )
        .unwrap();
        let dbg = format!("{auth:?}");
        assert!(!dbg.contains("SECRET"), "{dbg}");
        assert!(dbg.contains("ABCD-EFGH"));
    }

    #[test]
    fn test_interval_backoff() {
        // Default interval is 5s.
        assert_eq!(PollTimer::new(None, None).interval(), Duration::from_secs(5));
        // Server-provided interval is honoured.
        assert_eq!(PollTimer::new(Some(2), None).interval(), Duration::from_secs(2));
        // A zero interval would hammer the server.
        assert_eq!(PollTimer::new(Some(0), None).interval(), Duration::from_secs(1));
        // Capped, also under repeated slow_down.
        let mut timer = PollTimer::new(Some(10_000), None);
        assert_eq!(timer.interval(), Duration::from_secs(MAX_INTERVAL_SECS));
        timer.step(PollOutcome::SlowDown);
        assert_eq!(timer.interval(), Duration::from_secs(MAX_INTERVAL_SECS));
    }

    #[test]
    fn test_expires_in() {
        let mut timer = PollTimer::new(Some(5), Some(12));
        assert!(timer.tick()); // 7s left
        assert!(timer.tick()); // 2s left
        assert!(!timer.tick()); // next poll would be after expiry

        // slow_down eats into the remaining lifetime faster.
        let mut timer = PollTimer::new(Some(5), Some(20));
        assert!(timer.tick()); // 15s left
        timer.step(PollOutcome::SlowDown); // 10s interval
        assert!(timer.tick()); // 5s left
        assert!(!timer.tick());
    }

    #[test]
    fn test_scope_builder() {
        let device_id: OwnedDeviceId = "ABCDEFGHIJ".into();
        assert_eq!(
            build_scope(&device_id),
            "urn:matrix:org.matrix.msc2967.client:api:* \
            urn:matrix:org.matrix.msc2967.client:device:ABCDEFGHIJ"
        );

        let generated = DeviceId::new();
        let scope = build_scope(&generated);
        let parts: Vec<&str> = scope.split(' ').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], SCOPE_API_FULL_ACCESS);
        assert_eq!(parts[1], format!("{SCOPE_DEVICE_PREFIX}{generated}"));
    }

    #[test]
    fn test_verification_uri_host_check() {
        let iss = issuer();

        let ok = check_verification_uri("https://account.example.org/link", &iss).unwrap();
        assert_eq!(ok.path(), "/link");
        assert!(check_verification_uri("https://ACCOUNT.example.org/link?code=X", &iss).is_ok());

        // Different host: phishing.
        assert!(check_verification_uri("https://evil.example.com/link", &iss).is_err());
        // Look-alike / subdomain tricks.
        assert!(check_verification_uri("https://account.example.org.evil.com/", &iss).is_err());
        assert!(check_verification_uri("https://evil.account.example.org/", &iss).is_err());
        assert!(check_verification_uri("https://account.example.org@evil.com/link", &iss).is_err());
        // Different port.
        assert!(check_verification_uri("https://account.example.org:8443/link", &iss).is_err());
        // https only.
        assert!(check_verification_uri("http://account.example.org/link", &iss).is_err());
        // Garbage.
        assert!(check_verification_uri("not a url", &iss).is_err());
        assert!(check_verification_uri("javascript:alert(1)", &iss).is_err());
    }

    #[test]
    fn test_require_https() {
        assert!(require_https(&Url::parse("https://a.example/").unwrap(), "x").is_ok());
        assert!(require_https(&Url::parse("http://a.example/").unwrap(), "x").is_err());
    }

    #[test]
    fn test_device_client_metadata() {
        let metadata = device_client_metadata().unwrap();
        let grants = metadata["grant_types"].as_array().unwrap();
        assert!(grants.contains(&serde_json::Value::from(DEVICE_CODE_GRANT_TYPE)));
        assert!(grants.contains(&serde_json::Value::from("authorization_code")));
    }

    #[test]
    fn test_form_body() {
        assert_eq!(
            form_body(&[("client_id", "a b"), ("scope", "x:* y&z")]),
            "client_id=a+b&scope=x%3A*+y%26z"
        );
    }

    /// AUTH-17: the server-chosen `error` and `error_description` are
    /// sanitised before they are quoted in the error text.
    #[test]
    fn test_auth17_error_description_is_sanitised() {
        use crate::bitchx::text::{HOSTILE, assert_inert};
        for p in HOSTILE {
            let body = serde_json::json!({"error": format!("bad{p}"), "error_description": p})
                .to_string()
                .into_bytes();
            let PollOutcome::Failed(msg) = classify_poll(400, &body) else {
                panic!("expected failure");
            };
            assert_inert(&msg);
            assert_inert(&endpoint_error("Device authorization", 400, &body));
        }
        let body = br#"{"error":"invalid_grant","error_description":"mat_S3CRETtoken"}"#;
        let PollOutcome::Failed(msg) = classify_poll(400, body) else {
            panic!()
        };
        assert!(!msg.contains("S3CRET"), "{msg}");
    }

    /// AUTH-16: a hostile user code or URI cannot inject control sequences or
    /// extra rows, and the anti-phishing warning still follows the code.
    #[test]
    fn test_auth16_instructions_are_sanitised() {
        use crate::bitchx::text::{HOSTILE, assert_inert};
        let uri = Url::parse("https://account.example.org/link").unwrap();
        let complete = Url::parse("https://account.example.org/link?code=X").unwrap();
        let auth = |code: &str| DeviceAuthorizationResponse {
            device_code: Secret("dc".into()),
            user_code: code.into(),
            verification_uri: uri.to_string(),
            verification_uri_complete: Some(complete.to_string()),
            expires_in: Some(600),
            interval: None,
        };
        let benign = instructions(&auth("ABCD-EFGH"), &uri, Some(&complete));
        for p in HOSTILE {
            let text = instructions(&auth(p), &uri, Some(&complete));
            let rows: Vec<&str> = text.lines().collect();
            assert_eq!(rows.len(), benign.lines().count(), "{text}");
            for r in &rows {
                assert_inert(r);
            }
            let code = rows.iter().position(|r| r.contains(">>>")).unwrap();
            assert!(rows[code + 2].starts_with("  WARNING: only approve"), "{text}");
        }
    }
}
