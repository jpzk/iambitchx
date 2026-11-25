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
use matrix_sdk::{AuthApi, AuthSession, Client, ClientBuildError, reqwest};
use url::Url;

use crate::config::ApplicationSettings;
use crate::device_link::device_link_login;
use crate::oauth::oauth_login;

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
        .handle_refresh_tokens()
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

/// Restore the saved session. Returns `false` if there is none.
pub async fn restore(client: &Client, settings: &ApplicationSettings) -> anyhow::Result<bool> {
    let path = if settings.session_json.is_file() {
        &settings.session_json
    } else if settings.session_json_old.is_file() {
        &settings.session_json_old
    } else {
        return Ok(false);
    };

    let session: AuthSession = settings.read_session(path)?.into();
    client.restore_session(session).await.context("restoring session")?;
    client.whoami().await.context(
        "the homeserver rejected the saved session; delete session.json to log in again",
    )?;

    Ok(true)
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
    settings.write_session(MatrixSession::from(&resp))
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
    settings.write_session(MatrixSession::from(&resp))
}

/// Restore the session or log in interactively on the terminal.
pub async fn login(client: &Client, settings: &ApplicationSettings) -> anyhow::Result<()> {
    if restore(client, settings).await? {
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
                            Some(s) => settings.write_session(s),
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
                    Ok(s) => settings.write_session(s),
                    Err(e) => Err(e),
                }
            },
            Some(_) => {
                println!("Please enter 'p', 's', 'o' or 'd'");
                continue;
            },
        };

        match res {
            Ok(()) => return Ok(()),
            Err(e) => println!("Login failed: {e}"),
        }
    }
}

/// Write the current tokens to `session.json`.
pub fn save_tokens(client: &Client, settings: &ApplicationSettings) {
    let res = match client.auth_api() {
        Some(AuthApi::OAuth(oauth)) => oauth.full_session().map(|s| settings.write_session(s)),
        Some(AuthApi::Matrix(matrix)) => matrix.session().map(|s| settings.write_session(s)),
        _ => None,
    };

    if let Some(Err(e)) = res {
        tracing::warn!("failed to persist session: {e}");
    }
}

/// Persist refreshed tokens for as long as the client lives.
pub fn spawn_token_saver(client: Client, settings: ApplicationSettings) {
    tokio::spawn(async move {
        let mut changes = client.subscribe_to_session_changes();

        while let Ok(change) = changes.recv().await {
            match change {
                matrix_sdk::SessionChange::TokensRefreshed => save_tokens(&client, &settings),
                matrix_sdk::SessionChange::UnknownToken(t) => {
                    tracing::warn!("unknown token (soft logout = {})", t.soft_logout);
                },
            }
        }
    });
}
