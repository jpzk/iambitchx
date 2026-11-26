//! # Login
//!
//! Builds the Matrix client (sqlite state + crypto store, refresh tokens),
//! restores a saved session or walks the user through an interactive login,
//! and keeps `session.json` up to date when tokens are refreshed.
use std::io::Write as _;
use std::time::Duration;

use anyhow::Context;
use gethostname::gethostname;
use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::config::RequestConfig;
use matrix_sdk::encryption::{BackupDownloadStrategy, EncryptionSettings};
use matrix_sdk::ruma::api::error::ErrorKind;
use matrix_sdk::{AuthApi, AuthSession, Client, ClientBuildError, SessionTokens, reqwest};
use url::Url;

use crate::config::ApplicationSettings;
use crate::device_link::device_link_login;
use crate::oauth::oauth_login;
use crate::session_guard::{self, Mode, Recovery, SessionLock};

const ENCRYPTION_SETTINGS: EncryptionSettings = EncryptionSettings {
    auto_enable_cross_signing: true,
    auto_enable_backups: true,
    backup_download_strategy: BackupDownloadStrategy::AfterDecryptionFailure,
};

pub const USER_AGENT: &str = "iambitchx";

fn initial_devname() -> String {
    format!("iambitchx on {}", gethostname().to_string_lossy())
}

async fn build(
    homeserver: Option<&Url>,
    settings: &ApplicationSettings,
) -> Result<Client, ClientBuildError> {
    // No `handle_refresh_tokens()`: the SDK would refresh on its own, outside
    // the session.json lock. Live mode refreshes in [`recover`] under the
    // lock; `--smoke` never refreshes at all.
    let timeout = Duration::from_secs(settings.request_timeout);
    let http = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(timeout)
        .pool_idle_timeout(Duration::from_secs(60))
        .tcp_keepalive(Duration::from_secs(10))
        .build()
        .map_err(matrix_sdk::HttpError::Reqwest)?;

    let builder = Client::builder()
        .http_client(http)
        .sqlite_store_with_cache_path(&settings.sqlite_dir, &settings.sqlite_cache_dir, None)
        .request_config(RequestConfig::new().timeout(timeout).retry_limit(8))
        .with_encryption_settings(ENCRYPTION_SETTINGS);

    let builder = match homeserver {
        Some(url) => builder.homeserver_url(url.as_str()),
        None => builder.server_name(settings.user_id.server_name()),
    };

    builder.build().await
}

/// Create the client for the active profile.
pub async fn create_client(settings: &ApplicationSettings) -> anyhow::Result<Client> {
    settings.create_dirs()?;

    let client = match build(settings.url.as_ref(), settings).await {
        Err(ClientBuildError::AutoDiscovery(_)) => {
            let url = Url::parse(&format!("https://{}/", settings.user_id.server_name()))?;
            build(Some(&url), settings).await?
        },
        res => res?,
    };

    Ok(client)
}

/// Whether an SDK error is `M_UNKNOWN_TOKEN` (soft logout included).
pub fn is_unknown_token_http(e: &matrix_sdk::HttpError) -> bool {
    matches!(e.client_api_error_kind(), Some(ErrorKind::UnknownToken(_)))
}

/// [`is_unknown_token_http`] for the SDK's top-level error (e.g. from sync).
pub fn is_unknown_token(e: &matrix_sdk::Error) -> bool {
    matches!(e.client_api_error_kind(), Some(ErrorKind::UnknownToken(_)))
}

fn saved_session_path(settings: &ApplicationSettings) -> Option<&std::path::Path> {
    if settings.session_json.is_file() {
        Some(&settings.session_json)
    } else if settings.session_json_old.is_file() {
        Some(&settings.session_json_old)
    } else {
        None
    }
}

/// The tokens currently in `session.json`, if readable.
fn disk_tokens(settings: &ApplicationSettings) -> Option<SessionTokens> {
    let path = saved_session_path(settings)?;
    settings.read_session(path).ok().map(|s| s.tokens())
}

/// Restore the saved session. Returns `false` if there is none.
///
/// [`Mode::ReadOnly`] (`--smoke`) only reads `session.json`: no refresh, no
/// write; a rejected access token is an error. [`Mode::Live`] installs the
/// locked save/reload callbacks and recovers a rejected token via [`recover`].
pub async fn restore(
    client: &Client,
    settings: &ApplicationSettings,
    mode: Mode,
) -> anyhow::Result<bool> {
    let Some(path) = saved_session_path(settings) else {
        return Ok(false);
    };

    let session: AuthSession = settings.read_session(path)?.into();
    if mode.may_write() {
        install_session_callbacks(client, settings, &session).await?;
    }
    client.restore_session(session).await.context("restoring session")?;

    let mut attempts = 0;
    loop {
        match client.whoami().await {
            Ok(_) => return Ok(true),
            Err(e) if is_unknown_token_http(&e) => {
                if !mode.may_write() {
                    anyhow::bail!(session_guard::smoke_rejected_msg(&settings.session_json));
                }
                if let Some(msg) = recover(client, settings, mode, attempts).await? {
                    println!("iamBitchX: {msg}");
                }
                attempts += 1;
            },
            Err(e) => {
                return Err(e).context("checking the saved session with the homeserver");
            },
        }
    }
}

/// Live mode: let the SDK reload tokens from `session.json` and persist
/// refreshed ones. For OAuth sessions this also enables the SDK's
/// cross-process refresh guard, whose session-hash check is what lets a
/// running client adopt tokens another process rotated.
async fn install_session_callbacks(
    client: &Client,
    settings: &ApplicationSettings,
    session: &AuthSession,
) -> anyhow::Result<()> {
    set_callbacks(client, settings)?;
    if matches!(session, AuthSession::OAuth(_)) {
        client
            .oauth()
            .enable_cross_process_refresh_lock(format!("iambitchx-{}", std::process::id()))
            .await?;
    }
    Ok(())
}

fn set_callbacks(client: &Client, settings: &ApplicationSettings) -> anyhow::Result<()> {
    let reload = settings.clone();
    let save = settings.clone();
    client.set_session_callbacks(
        Box::new(move |_| {
            // Called by the SDK inside `refresh_access_token`, i.e. from
            // `recover` which already holds the SessionLock; writes are atomic
            // renames, so a plain read is consistent.
            disk_tokens(&reload).ok_or_else(|| "session.json unreadable".into())
        }),
        Box::new(move |client| {
            // Same: only reached from `recover` (the sole refresh path, the
            // SDK's own auto-refresh is off) which holds the SessionLock.
            write_current_tokens(&client, &save).map_err(|e| e.to_string().into())
        }),
    )?;
    Ok(())
}

/// Recover from `M_UNKNOWN_TOKEN` / soft logout. Holds the SessionLock for
/// the whole read-decide-refresh-write sequence so two processes never rotate
/// concurrently. `Ok(Some(line))` is a ` iamBitchX:` line for the user;
/// `Err` carries the relogin hint.
pub async fn recover(
    client: &Client,
    settings: &ApplicationSettings,
    mode: Mode,
    attempts: u32,
) -> anyhow::Result<Option<String>> {
    if !mode.may_write() {
        anyhow::bail!(session_guard::smoke_rejected_msg(&settings.session_json));
    }
    let _lock =
        SessionLock::acquire_async(settings.session_json.clone(), Duration::from_secs(30)).await?;

    let in_use = client.session_tokens();
    let on_disk = disk_tokens(settings);
    let relogin = || anyhow::anyhow!(session_guard::relogin_hint(&settings.session_json));

    match session_guard::decide(mode, in_use.as_ref(), on_disk.as_ref(), attempts) {
        Recovery::ReloadFromDisk => {
            // With the cross-process guard, the SDK sees the session hash
            // changed and adopts the tokens from our reload callback
            // (session.json) instead of spending our stale refresh token.
            let res = client.refresh_access_token().await;
            if client.session_tokens() == on_disk {
                Ok(Some(session_guard::RELOADED_MSG.to_string()))
            } else if res.is_ok() {
                // no hash change seen, but our own refresh worked: keep those
                write_current_tokens(client, settings).map(|()| None)
            } else {
                Err(relogin())
            }
        },
        Recovery::Refresh => {
            match client.refresh_access_token().await {
                // persist under our lock (the save callback may have already)
                Ok(()) => write_current_tokens(client, settings).map(|()| None),
                Err(e) => {
                    tracing::warn!("token refresh failed: {e}");
                    Err(relogin())
                },
            }
        },
        Recovery::Relogin | Recovery::Fail => Err(relogin()),
    }
}

fn ask(question: &str) -> String {
    print!("{question}");
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    let _ = std::io::stdin().read_line(&mut input);
    input.trim().to_string()
}

async fn password_login(
    client: &Client,
    settings: &ApplicationSettings,
    password: &str,
) -> anyhow::Result<()> {
    let mut login = client
        .matrix_auth()
        .login_username(&settings.user_id, password)
        .initial_device_display_name(&initial_devname());
    let saved = settings.read_saved_device();
    if let Some(device) = &saved {
        login = login.device_id(device.device_id().as_str());
    }
    let resp = login.send().await?;
    persist_login(settings, MatrixSession::from(&resp)).await
}

async fn sso_login(client: &Client, settings: &ApplicationSettings) -> anyhow::Result<()> {
    let resp = client
        .matrix_auth()
        .login_sso(|url| {
            async move {
                println!("Open this URL in your browser:\n    {url}");
                tokio::task::spawn_blocking(move || open::that(url));
                Ok(())
            }
        })
        .initial_device_display_name(&initial_devname())
        .send()
        .await?;
    persist_login(settings, MatrixSession::from(&resp)).await
}

/// Restore the session or log in interactively on the terminal.
pub async fn login(client: &Client, settings: &ApplicationSettings) -> anyhow::Result<()> {
    if restore(client, settings, Mode::Live).await? {
        return Ok(());
    }

    if let Some(file) = &settings.password_file {
        match std::fs::read_to_string(file) {
            Ok(pw) => {
                match password_login(client, settings, pw.trim_end_matches('\n')).await {
                    Ok(()) => return Ok(()),
                    Err(e) => println!("Password file login failed: {e}"),
                }
            },
            Err(e) => println!("Cannot read password file {file:?}: {e}"),
        }
    }

    println!("* Logging in as {}", settings.user_id);

    loop {
        let choice = ask("Login type: [p]assword / [s]ingle sign on / [o]auth / [d]evice link: ");
        let res = match choice.chars().next().map(|c| c.to_ascii_lowercase()) {
            None | Some('p') => {
                let pw = rpassword::prompt_password("Password: ")?;
                password_login(client, settings, &pw).await
            },
            Some('s') => sso_login(client, settings).await,
            Some('o') => {
                match oauth_login(client.clone(), &settings.user_id, settings.read_saved_device())
                    .await
                {
                    Ok(()) => {
                        match client.oauth().full_session() {
                            Some(s) => persist_login(settings, s).await,
                            None => Err(anyhow::anyhow!("no OAuth session after login")),
                        }
                    },
                    Err(e) => Err(e),
                }
            },
            Some('d') => {
                match device_link_login(
                    client.clone(),
                    &settings.user_id,
                    settings.read_saved_device(),
                )
                .await
                {
                    Ok(s) => persist_login(settings, s).await,
                    Err(e) => Err(e),
                }
            },
            Some(_) => {
                println!("Please enter 'p', 's', 'o' or 'd'");
                continue;
            },
        };

        match res {
            Ok(()) => {
                set_callbacks(client, settings)?;
                return Ok(());
            },
            Err(e) => println!("Login failed: {e}"),
        }
    }
}

/// Write the client's current tokens to `session.json`. The caller holds the
/// SessionLock; `--smoke` never gets here.
fn write_current_tokens(client: &Client, settings: &ApplicationSettings) -> anyhow::Result<()> {
    match client.auth_api() {
        Some(AuthApi::OAuth(oauth)) => {
            settings.write_session(oauth.full_session().context("no OAuth session")?)
        },
        Some(AuthApi::Matrix(matrix)) => {
            settings.write_session(matrix.session().context("no Matrix session")?)
        },
        _ => anyhow::bail!("no session to save"),
    }
}

/// Write a fresh login to `session.json` under the SessionLock.
async fn persist_login(
    settings: &ApplicationSettings,
    session: impl Into<crate::config::Session>,
) -> anyhow::Result<()> {
    let _lock =
        SessionLock::acquire_async(settings.session_json.clone(), Duration::from_secs(30)).await?;
    settings.write_session(session)
}
