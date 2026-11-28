//! # iambitchx
//!
//! A BitchX 1.x style Matrix client: [login] restores or creates a session,
//! [matrix] runs the sync loop and executes commands, and [bitchx] is the
//! pure UI state machine and renderer driven by [tui].
#![recursion_limit = "256"]
#![allow(clippy::needless_return)]

use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::Parser;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

use crate::bitchx::app::App;
use crate::bitchx::vars::Vars;
use crate::config::{ApplicationSettings, Cli};
use crate::session_guard::Mode;

mod bitchx;
mod config;
mod debug_tap;
#[allow(
    clippy::pedantic,
    reason = "backend module under a no-edit policy; pedantic findings listed in report.md"
)]
mod device_link;
mod e2ee;
#[allow(
    clippy::pedantic,
    reason = "backend module under a no-edit policy; pedantic findings listed in report.md"
)]
mod login;
#[allow(
    clippy::pedantic,
    reason = "backend module under a no-edit policy; pedantic findings listed in report.md"
)]
mod matrix;
#[allow(
    clippy::pedantic,
    reason = "backend module under a no-edit policy; pedantic findings listed in report.md"
)]
mod oauth;
#[allow(
    clippy::pedantic,
    reason = "backend module under a no-edit policy; pedantic findings listed in report.md"
)]
mod session_guard;
#[allow(
    clippy::pedantic,
    reason = "backend module under a no-edit policy; pedantic findings listed in report.md"
)]
mod smoke;
mod token_refresh;
mod tui;

fn setup_logging(settings: &ApplicationSettings) -> tracing_appender::non_blocking::WorkerGuard {
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(format!("iambitchx-log-{}", settings.profile_name))
        .max_log_files(7)
        .build(&settings.logs_dir)
        // Fail fast at startup: logs dir was just created by create_dirs().
        .expect("can build appending tracing logger");
    let (appender, guard) = tracing_appender::non_blocking(appender);

    let directives = std::env::var(EnvFilter::DEFAULT_ENV).unwrap_or(settings.log_level.clone());
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::OFF.into())
        .parse(directives)
        .unwrap_or_else(|e| {
            eprintln!("Unable to parse log level: {e}");
            process::exit(2);
        });

    let fmt = tracing_subscriber::fmt::layer().with_writer(appender).with_ansi(false);
    let subscriber = tracing_subscriber::registry()
        .with(fmt.with_filter(filter))
        .with(debug_tap::layer());
    // Fail fast: only fails if a global subscriber was already set (a bug).
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    guard
}

async fn run(settings: ApplicationSettings, smoke: bool) -> anyhow::Result<()> {
    let client = login::create_client(&settings).await?;

    if smoke {
        // read-only: never refreshes tokens, never writes session.json
        if !login::restore(&client, &settings, Mode::ReadOnly).await? {
            anyhow::bail!("--smoke needs a saved session ({})", settings.session_json.display());
        }
    } else {
        login::login(&client, &settings).await?;
    }

    let me = client.user_id().unwrap_or(&settings.user_id).to_owned();
    let mut app = App::new(me);
    app.vars = Vars::from_config(&settings.bitchx);
    app.config_toml = settings.config_toml.clone();
    app.log_default_dir =
        dirs::data_dir().map(|d| d.join(crate::config::APP_DIR_NAME).join("logs"));
    crate::bitchx::palette::set_depth(crate::bitchx::palette::ColorDepth::from_env());
    app.load_themes(Some(&settings.config_dir));

    if smoke {
        smoke::run(client, app).await
    } else {
        tui::run(client, app, settings).await
    }
}

fn main() {
    let cli = Cli::parse();

    let settings = ApplicationSettings::load(&cli).unwrap_or_else(|e| {
        eprintln!("{e:#}");
        process::exit(2);
    });

    // Tokens, keys etc. are only readable by the user.
    #[cfg(unix)]
    #[allow(
        unsafe_code,
        reason = "umask(2) has no safe std wrapper; it only sets the process file-mode mask"
    )]
    unsafe {
        libc::umask(0o077);
    };

    if let Err(e) = settings.create_dirs() {
        eprintln!("Cannot create data directories: {e}");
        process::exit(2);
    }
    let guard = setup_logging(&settings);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .thread_name_fn(|| {
            static ID: AtomicUsize = AtomicUsize::new(0);
            format!("iambitchx-worker-{}", ID.fetch_add(1, Ordering::SeqCst))
        })
        .build()
        // Fail fast: without a runtime nothing can run; main() returns ().
        .expect("tokio runtime");

    let res = rt.block_on(run(settings, cli.smoke));
    drop(guard);

    if let Err(e) = res {
        eprintln!("\n{e:#}\n");
        process::exit(2);
    }
}
