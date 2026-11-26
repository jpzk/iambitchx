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

/// Name of this application's directory under the config, data and cache dirs.
pub const APP_DIR_NAME: &str = "iambitchx";

/// Legacy directory name used by upstream iamb, which this fork falls back to.
pub const LEGACY_APP_DIR_NAME: &str = "iamb";

const DEFAULT_REQ_TIMEOUT: u64 = 120;

/// Pick the application directory inside `base`: `iambitchx/` normally, but the legacy
/// `iamb/` directory when only that one exists, so existing setups keep working.
pub fn app_dir(base: &Path) -> PathBuf {
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
pub struct Cli {
    /// Profile to use (defaults to `default_profile`).
    #[clap(short = 'P', long, value_parser)]
    pub profile: Option<String>,

    /// Configuration directory (defaults to $XDG_CONFIG_HOME/iambitchx).
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
}

impl Session {
    /// The access + refresh token pair.
    pub fn tokens(&self) -> SessionTokens {
        SessionTokens {
            access_token: self.access_token.clone(),
            refresh_token: self.refresh_token.clone(),
        }
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
            Some(client_id) => {
                AuthSession::OAuth(Box::new(OAuthSession {
                    client_id,
                    user: UserSession { meta, tokens },
                }))
            },
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
        }
    }
}

/// A device ID saved at logout so that the next login can reuse it (the SDK
/// store is keyed by user and device ID).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SavedDevice {
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
    pub fn device_id(&self) -> &DeviceId {
        match self {
            SavedDevice::OAuth { device_id, .. } => device_id,
            SavedDevice::MatrixAuth { device_id, .. } => device_id,
        }
    }

    pub fn is_user(&self, user: &UserId) -> bool {
        match self {
            SavedDevice::OAuth { user_id, .. } => user == user_id,
            SavedDevice::MatrixAuth { user_id, .. } => user == user_id,
        }
    }
}

/// `dirs` table (global or per profile).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Directories {
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
pub struct Settings {
    pub log_level: Option<String>,
    pub request_timeout: Option<u64>,
}

impl Settings {
    fn merge(self, other: Self) -> Self {
        Settings {
            log_level: self.log_level.or(other.log_level),
            request_timeout: self.request_timeout.or(other.request_timeout),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProfileConfig {
    pub user_id: Option<OwnedUserId>,
    pub url: Option<Url>,
    pub password_file: Option<PathBuf>,
    pub settings: Option<Settings>,
    pub dirs: Option<Directories>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileConfig>,
    pub default_profile: Option<String>,
    pub settings: Option<Settings>,
    pub dirs: Option<Directories>,
}

impl Config {
    pub fn parse_toml(s: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(s)?)
    }

    pub fn load(dir: &Path) -> anyhow::Result<(Self, PathBuf)> {
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

fn valid_profile_name(name: &str) -> bool {
    let mut chars = name.chars();

    chars.next().is_some_and(|c| c.is_ascii_alphanumeric()) &&
        chars.all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// Everything resolved for the active profile.
#[derive(Clone, Debug)]
pub struct ApplicationSettings {
    pub profile_name: String,
    pub user_id: OwnedUserId,
    pub url: Option<Url>,
    pub password_file: Option<PathBuf>,
    pub log_level: String,
    pub request_timeout: u64,
    pub session_json: PathBuf,
    pub session_json_old: PathBuf,
    pub device_json: PathBuf,
    pub sqlite_dir: PathBuf,
    pub sqlite_cache_dir: PathBuf,
    pub logs_dir: PathBuf,
}

impl ApplicationSettings {
    pub fn load(cli: &Cli) -> anyhow::Result<Self> {
        let base = cli
            .config_directory
            .clone()
            .or_else(|| env::var("XDG_CONFIG_HOME").ok().map(PathBuf::from))
            .or_else(dirs::config_dir)
            .ok_or_else(|| anyhow!("No configuration directory found; use -C"))?;
        let config_dir = app_dir(&base);
        let (config, _) = Config::load(&config_dir)?;

        Self::resolve(config, &config_dir, cli.profile.clone(), dirs::data_dir(), dirs::cache_dir())
    }

    /// Pick the profile and lay out its paths. `data_base`/`cache_base` are the
    /// XDG base dirs (without the app dir).
    pub fn resolve(
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
            Some(name) => {
                profiles
                    .remove_entry(&name)
                    .ok_or_else(|| anyhow!("No configured profile with the name {name:?}"))?
            },
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
            session_json: profile_data.join("session.json"),
            session_json_old: config_dir.join("profiles").join(&profile_name).join("session.json"),
            device_json: profile_data.join("device.json"),
            sqlite_dir: profile_data.join("sqlite"),
            sqlite_cache_dir: profile_cache.join("sqlite"),
            logs_dir,
            profile_name,
        })
    }

    pub fn create_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.sqlite_dir)?;
        std::fs::create_dir_all(&self.sqlite_cache_dir)?;
        std::fs::create_dir_all(&self.logs_dir)?;
        Ok(())
    }

    pub fn read_session(&self, path: &Path) -> anyhow::Result<Session> {
        let reader = BufReader::new(File::open(path)?);
        Ok(serde_json::from_reader(reader)?)
    }

    /// Write `session.json` atomically (temp file + rename), so a concurrent
    /// reader never sees a half-written file. Callers hold the
    /// [`crate::session_guard::SessionLock`].
    pub fn write_session(&self, session: impl Into<Session>) -> anyhow::Result<()> {
        let tmp = self.session_json.with_extension("json.tmp");
        {
            let mut writer = BufWriter::new(File::create(&tmp)?);
            serde_json::to_writer(&mut writer, &session.into())?;
            writer.flush()?;
            writer.get_ref().sync_all()?;
        }
        std::fs::rename(&tmp, &self.session_json)?;
        Ok(())
    }

    /// The device ID saved at logout, if it belongs to this profile's user.
    pub fn read_saved_device(&self) -> Option<SavedDevice> {
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

    #[test]
    fn test_profile_names() {
        assert!(valid_profile_name("carla"));
        assert!(valid_profile_name("a.b-c"));
        assert!(!valid_profile_name("-x"));
        assert!(!valid_profile_name(""));
    }
}
