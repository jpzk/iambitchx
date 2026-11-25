//! # `--smoke`
//!
//! Headless end-to-end check: the saved session is restored, one sync is
//! run, and the status window and first room window are rendered to a
//! ratatui TestBackend and printed as plain text.
use matrix_sdk::Client;
use matrix_sdk::config::SyncSettings;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::bitchx::app::App;
use crate::bitchx::model::MxEvent;
use crate::bitchx::ui::draw;
use crate::matrix;

const WIDTH: u16 = 100;
const HEIGHT: u16 = 24;

/// Render the current window as plain text, one string per row.
pub fn render_text(app: &mut App, width: u16, height: u16) -> anyhow::Result<Vec<String>> {
    let mut term = Terminal::new(TestBackend::new(width, height))?;
    term.draw(|f| draw(f, app))?;
    let buf = term.backend().buffer();
    Ok((0..height)
        .map(|y| {
            let row: String = (0..width).map(|x| buf[(x, y)].symbol()).collect();
            row.trim_end().to_string()
        })
        .collect())
}

pub async fn run(client: Client, mut app: App) -> anyhow::Result<()> {
    client.sync_once(SyncSettings::default()).await?;

    app.status_info(format!("Connected to {} as {}", client.homeserver(), app.me));
    crate::tui::seed(&mut app, &client).await;

    println!("=== window 1 (status) ===");
    for row in render_text(&mut app, WIDTH, HEIGHT)? {
        println!("{row}");
    }

    if app.windows.len() < 2 {
        println!("(no joined rooms)");
        return Ok(());
    }

    app.switch(1);
    let room_id = app.windows[1].room_id().cloned().expect("room window");
    if let Some(room) = client.get_room(&room_id) {
        let lines = matrix::backfill(&room, &app.me).await?;
        app.handle_mx(MxEvent::Lines { room_id, lines, backfill: true });
    }

    println!("=== window 2 ({}) ===", app.windows[1].room.name);
    for row in render_text(&mut app, WIDTH, HEIGHT)? {
        println!("{row}");
    }

    Ok(())
}
