//! # Configuration
//!
//! The TOML (or JSON) config file: profiles with `user_id` and `url`, and the
//! directories the session, the sqlite state/crypto store and logs live in.
//! Settings that only made sense for the old vim UI are ignored.
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use clap::Parser;
use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::authentication::oauth::{ClientId, OAuthSession, UserSession};
use matrix_sdk::ruma::{DeviceId, OwnedDeviceId, OwnedUserId, UserId};
use matrix_sdk::{AuthSession, SessionMeta, SessionTokens};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::bitchx::vars::BitchxConfig;

/// Name of this application's directory under the config, data and cache dirs.
pub(crate) const APP_DIR_NAME: &str = "iambitchx";

/// Legacy directory name used by upstream iamb, which this fork falls back to.
pub(crate) const LEGACY_APP_DIR_NAME: &str = "iamb";

const DEFAULT_REQ_TIMEOUT: u64 = 120;

/// Pick the application directory inside `base`: `iambitchx/` normally, but the legacy
/// `iamb/` directory when only that one exists, so existing setups keep working.
pub(crate) fn app_dir(base: &Path) -> PathBuf {
    let dir = base.join(APP_DIR_NAME);
    let legacy = base.join(LEGACY_APP_DIR_NAME);

    if !dir.exists() && legacy.is_dir() {
        legacy
    } else {
        dir
    }
}

fn version() -> &'static str {
    match option_env!("VERGEN_GIT_SHA") {
        None => env!("CARGO_PKG_VERSION"),
        Some(sha) => Box::leak(format!("{} ({sha})", env!("CARGO_PKG_VERSION")).into_boxed_str()),
    }
}

/// Command line flags.
#[derive(Parser, Debug)]
#[clap(name = "iambitchx", version = version(), about, long_about = None)]
pub(crate) struct Cli {
    /// Profile to use (defaults to `default_profile`).
    #[clap(short = 'P', long, value_parser)]
    pub profile: Option<String>,

    /// Configuration directory (defaults to $`XDG_CONFIG_HOME/iambitchx`).
    #[clap(short = 'C', long, value_parser)]
    pub config_directory: Option<PathBuf>,

    /// Headless check: restore the saved session, sync once, print the status
    /// window and the first room as plain text, then exit.
    #[clap(long)]
    pub smoke: bool,
}

/// A saved login.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Session {
    access_token: String,
    refresh_token: Option<String>,
    user_id: OwnedUserId,
    device_id: OwnedDeviceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<ClientId>,
    /// The homeserver the tokens were issued for (CONF-2). Absent in sessions
    /// saved by older versions; recorded on the first successful restore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    homeserver: Option<Url>,
}

impl Session {
    /// The access + refresh token pair.
    pub fn tokens(&self) -> SessionTokens {
        SessionTokens {
            access_token: self.access_token.clone(),
            refresh_token: self.refresh_token.clone(),
        }
    }

    /// Whether this session was saved without its homeserver (older format).
    pub fn is_legacy(&self) -> bool {
        self.homeserver.is_none()
    }
}

impl From<Session> for AuthSession {
    fn from(session: Session) -> Self {
        let meta = SessionMeta {
            user_id: session.user_id,
            device_id: session.device_id,
        };
        let tokens = SessionTokens {
            access_token: session.access_token,
            refresh_token: session.refresh_token,
        };

        match session.client_id {
            Some(client_id) => AuthSession::OAuth(Box::new(OAuthSession {
                client_id,
                user: UserSession { meta, tokens },
            })),
            None => AuthSession::Matrix(MatrixSession { meta, tokens }),
        }
    }
}

impl From<MatrixSession> for Session {
    fn from(session: MatrixSession) -> Self {
        Session {
            access_token: session.tokens.access_token,
            refresh_token: session.tokens.refresh_token,
            user_id: session.meta.user_id,
            device_id: session.meta.device_id,
            client_id: None,
            homeserver: None,
        }
    }
}

impl From<OAuthSession> for Session {
    fn from(session: OAuthSession) -> Self {
        Session {
            access_token: session.user.tokens.access_token,
            refresh_token: session.user.tokens.refresh_token,
            user_id: session.user.meta.user_id,
            device_id: session.user.meta.device_id,
            client_id: Some(session.client_id),
            homeserver: None,
        }
    }
}

/// A device ID saved at logout so that the next login can reuse it (the SDK
/// store is keyed by user and device ID).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub(crate) enum SavedDevice {
    OAuth {
        user_id: OwnedUserId,
        device_id: OwnedDeviceId,
        client_id: ClientId,
    },
    MatrixAuth {
        user_id: OwnedUserId,
        device_id: OwnedDeviceId,
    },
}

impl SavedDevice {
    pub(crate) fn device_id(&self) -> &DeviceId {
        match self {
            SavedDevice::OAuth { device_id, .. } | SavedDevice::MatrixAuth { device_id, .. } => {
                device_id
            },
        }
    }

    pub(crate) fn is_user(&self, user: &UserId) -> bool {
        match self {
            SavedDevice::OAuth { user_id, .. } | SavedDevice::MatrixAuth { user_id, .. } => {
                user == user_id
            },
        }
    }
}

/// `dirs` table (global or per profile).
#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Directories {
    pub cache: Option<String>,
    pub data: Option<String>,
    pub logs: Option<String>,
}

impl Directories {
    fn merge(self, other: Self) -> Self {
        Directories {
            cache: self.cache.or(other.cache),
            data: self.data.or(other.data),
            logs: self.logs.or(other.logs),
        }
    }
}

fn expand(dir: &str) -> anyhow::Result<PathBuf> {
    let dir = shellexpand::full(dir).map_err(|e| anyhow!("{e}"))?;
    Ok(PathBuf::from(dir.as_ref()))
}

/// `settings` table; everything else in it (vim-era tunables) is ignored.
#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Settings {
    pub log_level: Option<String>,
    pub request_timeout: Option<u64>,
    /// `[settings.bitchx]`: `/set` variables.
    pub bitchx: Option<BitchxConfig>,
}

impl Settings {
    fn merge(self, other: Self) -> Self {
        Settings {
            log_level: self.log_level.or(other.log_level),
            request_timeout: self.request_timeout.or(other.request_timeout),
            bitchx: match (self.bitchx, other.bitchx) {
                (Some(a), Some(b)) => Some(a.merge(b)),
                (a, b) => a.or(b),
            },
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct ProfileConfig {
    pub user_id: Option<OwnedUserId>,
    pub url: Option<Url>,
    pub password_file: Option<PathBuf>,
    pub settings: Option<Settings>,
    pub dirs: Option<Directories>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Config {
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileConfig>,
    pub default_profile: Option<String>,
    pub settings: Option<Settings>,
    pub dirs: Option<Directories>,
}

impl Config {
    pub(crate) fn parse_toml(s: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(s)?)
    }

    pub(crate) fn load(dir: &Path) -> anyhow::Result<(Self, PathBuf)> {
        let toml_path = dir.join("config.toml");
        let json_path = dir.join("config.json");

        if toml_path.is_file() {
            let s = std::fs::read_to_string(&toml_path)?;
            let cfg = Self::parse_toml(&s).with_context(|| format!("{}", toml_path.display()))?;
            Ok((cfg, toml_path))
        } else if json_path.is_file() {
            let s = std::fs::read_to_string(&json_path)?;
            let cfg =
                serde_json::from_str(&s).with_context(|| format!("{}", json_path.display()))?;
            Ok((cfg, json_path))
        } else {
            bail!("Please create a configuration file at {}", toml_path.display())
        }
    }
}

/// The config base dir: `-C`, else `$XDG_CONFIG_HOME`, else `fallback`
/// (`dirs::config_dir`). Per the XDG spec an empty or relative
/// `XDG_CONFIG_HOME` is invalid and ignored, so the config is never read
/// relative to the current working directory (CONF-1).
fn config_base(
    cli_dir: Option<PathBuf>,
    xdg_config_home: Option<std::ffi::OsString>,
    fallback: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    cli_dir
        .or_else(|| xdg_config_home.map(PathBuf::from).filter(|p| p.is_absolute()))
        .or_else(fallback)
}

fn valid_profile_name(name: &str) -> bool {
    let mut chars = name.chars();

    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// Everything resolved for the active profile.
#[derive(Clone, Debug)]
pub(crate) struct ApplicationSettings {
    pub profile_name: String,
    pub user_id: OwnedUserId,
    pub url: Option<Url>,
    pub password_file: Option<PathBuf>,
    pub log_level: String,
    pub request_timeout: u64,
    /// `/set` variables from `[settings.bitchx]`.
    pub bitchx: BitchxConfig,
    /// Where `/set` persists: the `config.toml`, or `None` if the config is JSON.
    pub config_toml: Option<PathBuf>,
    /// The configuration directory (`/theme` also looks in its `themes/`).
    pub config_dir: PathBuf,
    pub session_json: PathBuf,
    pub session_json_old: PathBuf,
    pub device_json: PathBuf,
    pub sqlite_dir: PathBuf,
    pub sqlite_cache_dir: PathBuf,
    pub logs_dir: PathBuf,
}

impl ApplicationSettings {
    pub(crate) fn load(cli: &Cli) -> anyhow::Result<Self> {
        let base = config_base(
            cli.config_directory.clone(),
            env::var_os("XDG_CONFIG_HOME"),
            dirs::config_dir,
        )
        .ok_or_else(|| anyhow!("No configuration directory found; use -C"))?;
        let config_dir = app_dir(&base);
        let (config, path) = Config::load(&config_dir)?;

        let mut s = Self::resolve(
            config,
            &config_dir,
            cli.profile.clone(),
            dirs::data_dir(),
            dirs::cache_dir(),
        )?;
        s.config_toml = path.extension().is_some_and(|e| e == "toml").then_some(path);
        Ok(s)
    }

    /// Pick the profile and lay out its paths. `data_base`/`cache_base` are the
    /// XDG base dirs (without the app dir).
    pub(crate) fn resolve(
        config: Config,
        config_dir: &Path,
        profile: Option<String>,
        data_base: Option<PathBuf>,
        cache_base: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let Config { mut profiles, default_profile, settings, dirs } = config;

        if let Some(bad) = profiles.keys().find(|n| !valid_profile_name(n)) {
            bail!("{bad:?} is not a valid profile name");
        }

        let (profile_name, profile) = match profile.or(default_profile) {
            Some(name) => profiles
                .remove_entry(&name)
                .ok_or_else(|| anyhow!("No configured profile with the name {name:?}"))?,
            // len() == 1 was just checked, so next() is always Some.
            None if profiles.len() == 1 => profiles.into_iter().next().unwrap(),
            None if profiles.is_empty() => bail!("No profiles configured"),
            None => {
                print!("Profiles: ");
                for name in profiles.keys() {
                    print!("{name} ");
                }
                println!();
                print!("Select a profile: ");
                let _ = std::io::stdout().flush();
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                let name = input.trim().to_string();
                profiles
                    .remove_entry(&name)
                    .ok_or_else(|| anyhow!("No configured profile with the name {name:?}"))?
            },
        };

        let user_id = profile
            .user_id
            .clone()
            .ok_or_else(|| anyhow!("Profile {profile_name:?} has no user_id"))?;

        let settings = profile.settings.unwrap_or_default().merge(settings.unwrap_or_default());
        let dirs = profile.dirs.unwrap_or_default().merge(dirs.unwrap_or_default());

        let data = match dirs.data {
            Some(d) => expand(&d)?,
            None => app_dir(&data_base.ok_or_else(|| anyhow!("no data dir; set dirs.data"))?),
        };
        let cache = match dirs.cache {
            Some(d) => expand(&d)?,
            None => app_dir(&cache_base.ok_or_else(|| anyhow!("no cache dir; set dirs.cache"))?),
        };
        let logs_dir = match dirs.logs {
            Some(d) => expand(&d)?,
            None => cache.join("logs"),
        };

        let profile_data = data.join("profiles").join(&profile_name);
        let profile_cache = cache.join("profiles").join(&profile_name);

        Ok(ApplicationSettings {
            user_id,
            url: profile.url,
            password_file: profile.password_file,
            log_level: settings.log_level.unwrap_or_else(|| "off".into()),
            request_timeout: settings.request_timeout.unwrap_or(DEFAULT_REQ_TIMEOUT),
            bitchx: settings.bitchx.unwrap_or_default(),
            config_toml: None,
            config_dir: config_dir.to_path_buf(),
            session_json: profile_data.join("session.json"),
            session_json_old: config_dir.join("profiles").join(&profile_name).join("session.json"),
            device_json: profile_data.join("device.json"),
            sqlite_dir: profile_data.join("sqlite"),
            sqlite_cache_dir: profile_cache.join("sqlite"),
            logs_dir,
            profile_name,
        })
    }

    pub(crate) fn create_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.sqlite_dir)?;
        std::fs::create_dir_all(&self.sqlite_cache_dir)?;
        std::fs::create_dir_all(&self.logs_dir)?;
        Ok(())
    }

    /// The homeserver this profile is configured for: `url`, or the user's
    /// server name (the same fallback [`crate::login::create_client`] uses).
    pub(crate) fn configured_homeserver(&self) -> anyhow::Result<Url> {
        match &self.url {
            Some(url) => Ok(url.clone()),
            None => Ok(Url::parse(&format!("https://{}/", self.user_id.server_name()))?),
        }
    }

    /// Read a saved session. Refuses (before any token is used) a session
    /// that was saved for a different homeserver than the configured one
    /// (CONF-2).
    pub(crate) fn read_session(&self, path: &Path) -> anyhow::Result<Session> {
        let reader = BufReader::new(File::open(path)?);
        let session: Session = serde_json::from_reader(reader)?;
        if let Some(saved) = &session.homeserver {
            let configured = self.configured_homeserver()?;
            if *saved != configured {
                anyhow::bail!(
                    "{} was saved for homeserver {saved}, but the profile is configured for \
                     {configured}; refusing to send the saved token there. Fix `url` in the \
                     config, or log in again after removing the session file.",
                    path.display()
                );
            }
        }
        Ok(session)
    }

    /// Record the configured homeserver in a session saved by an older
    /// version, after it was verified against that server. Callers hold the
    /// [`crate::session_guard::SessionLock`].
    ///
    /// Reads `session.json` (or `old`, the legacy location, if there is no
    /// `session.json` yet) and writes `session.json`.
    pub(crate) fn backfill_homeserver(&self, old: &Path) -> anyhow::Result<()> {
        let path = if self.session_json.is_file() {
            &self.session_json
        } else {
            old
        };
        let session = self.read_session(path)?;
        if session.is_legacy() {
            self.write_session(session)?;
        }
        Ok(())
    }

    /// Write `session.json` atomically (temp file + rename), so a concurrent
    /// reader never sees a half-written file. Callers hold the
    /// [`crate::session_guard::SessionLock`].
    pub(crate) fn write_session(&self, session: impl Into<Session>) -> anyhow::Result<()> {
        let mut session: Session = session.into();
        session.homeserver = Some(self.configured_homeserver()?);
        let tmp = self.session_json.with_extension("json.tmp");
        {
            let mut writer = BufWriter::new(File::create(&tmp)?);
            serde_json::to_writer(&mut writer, &session)?;
            writer.flush()?;
            writer.get_ref().sync_all()?;
        }
        std::fs::rename(&tmp, &self.session_json)?;
        Ok(())
    }

    /// The device ID saved at logout, if it belongs to this profile's user.
    pub(crate) fn read_saved_device(&self) -> Option<SavedDevice> {
        let reader = BufReader::new(File::open(&self.device_json).ok()?);
        let saved: SavedDevice = serde_json::from_reader(reader).ok()?;
        saved.is_user(&self.user_id).then_some(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("iambitchx-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_app_dir_falls_back_to_legacy() {
        let base = scratch_dir("legacy");
        std::fs::create_dir(base.join(LEGACY_APP_DIR_NAME)).unwrap();
        assert_eq!(app_dir(&base), base.join("iamb"));

        std::fs::create_dir(base.join(APP_DIR_NAME)).unwrap();
        assert_eq!(app_dir(&base), base.join("iambitchx"));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_config_base_ignores_empty_or_relative_xdg() {
        let home = || Some(PathBuf::from("/home/carla/.config"));
        // CONF-1: empty / relative XDG_CONFIG_HOME must not resolve against the CWD.
        assert_eq!(config_base(None, Some("".into()), home), home());
        assert_eq!(config_base(None, Some("cfg".into()), home), home());
        assert_eq!(config_base(None, Some("./cfg".into()), home), home());
        // A valid absolute value and an explicit -C still win (temp_dir is
        // absolute on every OS; "/xdg" is not absolute on Windows).
        let xdg = std::env::temp_dir().join("xdg");
        assert_eq!(config_base(None, Some(xdg.clone().into()), home), Some(xdg.clone()));
        assert_eq!(
            config_base(Some(PathBuf::from("/cli")), Some(xdg.into()), home),
            Some(PathBuf::from("/cli"))
        );
        assert_eq!(config_base(None, None, home), home());
    }

    #[test]
    fn test_app_dir_defaults_to_new_name() {
        let base = scratch_dir("fresh");
        assert_eq!(app_dir(&base), base.join("iambitchx"));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_old_config_still_parses() {
        let cfg = Config::parse_toml(
            "default_profile = \"carla\"\n\
             [profiles.carla]\n\
             user_id = \"@carla:home-srv.test\"\n\
             url = \"https://matrix.home-srv.test/\"\n\
             [settings]\n\
             [settings.tunables]\n\
             frontend = \"bitchx\"\n\
             [settings.image_preview]\n",
        )
        .unwrap();

        let base = scratch_dir("resolve");
        std::fs::create_dir(base.join("iamb")).unwrap();
        let s = ApplicationSettings::resolve(
            cfg,
            &base.join("cfg"),
            None,
            Some(base.clone()),
            Some(base.join("cache")),
        )
        .unwrap();

        assert_eq!(s.profile_name, "carla");
        assert_eq!(s.user_id.as_str(), "@carla:home-srv.test");
        assert_eq!(s.session_json, base.join("iamb/profiles/carla/session.json"));
        assert_eq!(s.sqlite_dir, base.join("iamb/profiles/carla/sqlite"));
        std::fs::remove_dir_all(&base).unwrap();
    }

    const LEGACY_SESSION: &str = r#"{"access_token":"tok","refresh_token":null,"user_id":"@carla:home-srv.test","device_id":"DEV"}"#;

    fn settings_with_url(base: &Path, url: Option<&str>) -> ApplicationSettings {
        let url = url.map(|u| format!("url = \"{u}\"\n")).unwrap_or_default();
        let cfg = Config::parse_toml(&format!(
            "default_profile = \"carla\"\n\
             [profiles.carla]\n\
             user_id = \"@carla:home-srv.test\"\n\
             {url}\
             [settings]\n\
             [settings.tunables]\n\
             frontend = \"bitchx\"\n\
             [settings.image_preview]\n",
        ))
        .unwrap();
        let s = ApplicationSettings::resolve(
            cfg,
            &base.join("cfg"),
            None,
            Some(base.to_path_buf()),
            Some(base.join("cache")),
        )
        .unwrap();
        std::fs::create_dir_all(s.session_json.parent().unwrap()).unwrap();
        s
    }

    #[test]
    fn test_session_without_homeserver_still_parses() {
        // CONF-2: the new field is optional; old session.json files still load.
        let s: Session = serde_json::from_str(LEGACY_SESSION).unwrap();
        assert!(s.is_legacy());
        assert!(!serde_json::to_string(&s).unwrap().contains("homeserver"));
    }

    #[test]
    fn test_write_session_records_homeserver() {
        let base = scratch_dir("hs-write");
        let s = settings_with_url(&base, Some("https://matrix.home-srv.test/"));
        let legacy: Session = serde_json::from_str(LEGACY_SESSION).unwrap();
        s.write_session(legacy).unwrap();
        let saved = s.read_session(&s.session_json).unwrap();
        assert_eq!(
            saved.homeserver.as_ref().map(Url::as_str),
            Some("https://matrix.home-srv.test/")
        );

        // Without `url`, the user's server name is recorded.
        let s = settings_with_url(&base, None);
        let legacy: Session = serde_json::from_str(LEGACY_SESSION).unwrap();
        s.write_session(legacy).unwrap();
        let saved = s.read_session(&s.session_json).unwrap();
        assert_eq!(saved.homeserver.as_ref().map(Url::as_str), Some("https://home-srv.test/"));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_read_session_refuses_other_homeserver() {
        // CONF-2: a config pointing `url` elsewhere must not receive the token.
        let base = scratch_dir("hs-refuse");
        let good = settings_with_url(&base, Some("https://matrix.home-srv.test/"));
        let legacy: Session = serde_json::from_str(LEGACY_SESSION).unwrap();
        good.write_session(legacy).unwrap();

        let evil = settings_with_url(&base, Some("http://evil.test/"));
        let err = evil.read_session(&evil.session_json).unwrap_err().to_string();
        assert!(err.contains("refusing"), "{err}");
        assert!(settings_with_url(&base, None).read_session(&good.session_json).is_err());
        assert!(good.read_session(&good.session_json).is_ok());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_backfill_homeserver_pins_legacy_session() {
        let base = scratch_dir("hs-backfill");
        let s = settings_with_url(&base, Some("https://matrix.home-srv.test/"));
        std::fs::write(&s.session_json, LEGACY_SESSION).unwrap();
        // A legacy session has nothing to compare against yet.
        let evil = settings_with_url(&base, Some("http://evil.test/"));
        assert!(evil.read_session(&evil.session_json).unwrap().is_legacy());

        s.backfill_homeserver(&s.session_json_old).unwrap();
        let saved = s.read_session(&s.session_json).unwrap();
        assert!(!saved.is_legacy());
        assert_eq!(saved.tokens().access_token, "tok");
        // Once pinned, a different homeserver is refused.
        assert!(evil.read_session(&evil.session_json).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_profile_names() {
        assert!(valid_profile_name("carla"));
        assert!(valid_profile_name("a.b-c"));
        assert!(!valid_profile_name("-x"));
        assert!(!valid_profile_name(""));
    }
}
