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
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use crate::bitchx::app::App;
use crate::config::{ApplicationSettings, Cli};

mod bitchx;
mod config;
mod device_link;
mod login;
mod matrix;
mod oauth;
mod smoke;
mod tui;

fn setup_logging(settings: &ApplicationSettings) -> tracing_appender::non_blocking::WorkerGuard {
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(format!("iambitchx-log-{}", settings.profile_name))
        .max_log_files(7)
        .build(&settings.logs_dir)
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

    let subscriber = FmtSubscriber::builder()
        .with_writer(appender)
        .with_env_filter(filter)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    guard
}

async fn run(settings: ApplicationSettings, smoke: bool) -> anyhow::Result<()> {
    let client = login::create_client(&settings).await?;

    if smoke {
        if !login::restore(&client, &settings).await? {
            anyhow::bail!("--smoke needs a saved session ({})", settings.session_json.display());
        }
    } else {
        login::login(&client, &settings).await?;
    }
    login::save_tokens(&client, &settings);
    login::spawn_token_saver(client.clone(), settings.clone());

    let me = client.user_id().unwrap_or(&settings.user_id).to_owned();
    let app = App::new(me);

    if smoke {
        smoke::run(client, app).await
    } else {
        tui::run(client, app).await
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
        .expect("tokio runtime");

    let res = rt.block_on(run(settings, cli.smoke));
    drop(guard);

    if let Err(e) = res {
        eprintln!("\n{e:#}\n");
        process::exit(2);
    }
}
