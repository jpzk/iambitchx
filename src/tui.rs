//! # The terminal loop
//!
//! Raw mode + alternate screen; crossterm's EventStream and the Matrix
//! channel are multiplexed in one `select!`.
use std::io::{Stdout, stdout};

use crossterm::event::{Event, EventStream, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen,
    LeaveAlternateScreen,
    disable_raw_mode,
    enable_raw_mode,
};
use futures::StreamExt;
use matrix_sdk::Client;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc::unbounded_channel;

use crate::bitchx::app::App;
use crate::bitchx::model::{Effect, MxEvent};
use crate::bitchx::ui::draw;
use crate::config::ApplicationSettings;
use crate::matrix;

fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
}

fn setup() -> std::io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        hook(info);
    }));

    Terminal::new(CrosstermBackend::new(stdout()))
}

/// Seed the app with the rooms the client already knows about.
pub async fn seed(app: &mut App, client: &Client) {
    let me = app.me.clone();
    app.channel_list_header(client.homeserver().as_str());
    let mut rooms = client.joined_rooms();
    rooms.sort_by_key(|r| r.room_id().to_owned());
    for room in rooms {
        app.handle_mx(MxEvent::Room(matrix::snapshot(&room, &me).await));
    }
}

pub async fn run(
    client: Client,
    mut app: App,
    settings: ApplicationSettings,
) -> anyhow::Result<()> {
    let (tx, mut rx) = unbounded_channel();

    seed(&mut app, &client).await;
    app.status_info(format!("Connected to {} as {}", client.homeserver(), app.me));
    if let Some(device) = client.device_id() {
        app.status_info(format!("Device {device}"));
    }
    crate::e2ee::register_verification_handlers(&client, tx.clone());
    matrix::spawn_sync(client.clone(), tx.clone(), settings);
    matrix::spawn_lag(client.clone(), tx.clone());

    // AUTO_AWAY runs only here, in the interactive client (never --smoke).
    app.auto_away_armed = true;
    app.last_input = app.now();
    let mut term = setup()?;
    let res = event_loop(&mut term, &client, &mut app, &mut rx, &tx).await;
    restore();
    res
}

async fn event_loop(
    term: &mut Terminal<CrosstermBackend<Stdout>>,
    client: &Client,
    app: &mut App,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<MxEvent>,
    tx: &matrix::Tx,
) -> anyhow::Result<()> {
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));

    loop {
        term.draw(|f| draw(f, app))?;

        let effects: Vec<Effect> = tokio::select! {
            ev = events.next() => match ev {
                Some(Ok(Event::Key(key))) => {
                    if key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL) {
                        term.clear()?;
                        app.activity()
                    } else {
                        app.handle_key(key)
                    }
                },
                Some(Ok(Event::Paste(text))) => {
                    let fx = app.activity();
                    for c in text.chars().filter(|c| !c.is_control()) {
                        app.input.insert(c);
                    }
                    fx
                },
                Some(Ok(_)) => vec![],
                Some(Err(e)) => return Err(e.into()),
                None => return Ok(()),
            },
            Some(mx) = rx.recv() => app.handle_mx(mx),
            _ = tick.tick() => app.tick(),
        };

        for effect in effects {
            if effect == Effect::Quit {
                return Ok(());
            }
            matrix::spawn_effect(client.clone(), effect, tx.clone());
        }

        if app.quit {
            return Ok(());
        }
    }
}
