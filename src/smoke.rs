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
use crate::bitchx::channels::list_lines;
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

/// One line per room window: `N: name +e` (encrypted) or `N: name -e`.
pub fn encryption_report(app: &App) -> Vec<String> {
    let rows: Vec<String> = app
        .windows
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, w)| {
            let state = if w.room.encrypted {
                "+e encrypted (megolm)"
            } else {
                "-e not encrypted"
            };
            format!("{}: {} {state}", i + 1, w.room.name)
        })
        .collect();
    if rows.is_empty() {
        vec!["(no channels)".to_string()]
    } else {
        rows
    }
}

pub async fn run(client: Client, mut app: App) -> anyhow::Result<()> {
    // read-only: a rejected token is fatal here, never refreshed
    client.sync_once(SyncSettings::default()).await.map_err(|e| {
        if crate::login::is_unknown_token(&e) {
            anyhow::anyhow!(
                "--smoke: the homeserver rejected the access token (M_UNKNOWN_TOKEN); \
                 --smoke is read-only and does not refresh it"
            )
        } else {
            e.into()
        }
    })?;

    app.status_info(format!("Connected to {} as {}", client.homeserver(), app.me));
    crate::tui::seed(&mut app, &client).await;

    println!("=== encryption ===");
    for row in encryption_report(&app) {
        println!("{row}");
    }
    if let Some(s) = client.encryption().cross_signing_status().await {
        println!(
            "cross-signing: master={} self_signing={} user_signing={}",
            s.has_master, s.has_self_signing, s.has_user_signing
        );
    }

    // Tall enough to show all of window 1: banner and room list.
    let status_height = (app.windows[0].lines.len() + 3).clamp(HEIGHT as usize, 200) as u16;
    println!("=== window 1 (status) ===");
    for row in render_text(&mut app, WIDTH, status_height)? {
        println!("{row}");
    }

    // /list: the public channel directory of our homeserver.
    let entries = matrix::public_channels(&client, None, None).await?;
    println!("=== /list ===");
    let server = app.me.server_name().to_string();
    for row in list_lines(&server, None, &entries, WIDTH as usize - 2) {
        println!("{row}");
    }

    if app.windows.len() < 2 {
        println!("(no joined channels)");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitchx::app::tests::{app, room};

    #[test]
    fn test_smoke_encryption_report() {
        let mut app = app();
        let mut snap = room("!b:example.org", "#other");
        snap.encrypted = true;
        app.handle_mx(MxEvent::Room(snap));
        assert_eq!(encryption_report(&app), vec![
            "2: #room -e not encrypted".to_string(),
            "3: #other +e encrypted (megolm)".to_string(),
            "4: armin -e not encrypted".to_string(),
        ]);
        let empty = App::new(app.me.clone());
        assert_eq!(encryption_report(&empty), vec!["(no channels)".to_string()]);
    }
}
