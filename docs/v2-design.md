# iambitchx v2 — a pure BitchX 1.x client on matrix-sdk

Status: design of record for branch `feat/bitchx-v2`.

The iamb (modalkit, vim) frontend is removed entirely. The client UI is
rebuilt from scratch as a BitchX 1.x clone: one full-screen window at a
time, numbered windows, two blue status lines, an IRC input line. No
modes, no tabs, no splits, no mouse, no external processes.

## Deleted

- `src/windows/**`, `src/keybindings.rs`, `src/commands.rs`,
  `src/completions.rs` — the modalkit window tree, vim keymaps, `:` command
  table, completer.
- `src/base.rs`, `src/message/**`, `src/worker.rs`, `src/notifications.rs`,
  `src/preview.rs`, `src/verifications.rs`, `src/util.rs`,
  `src/prelude.rs`, `src/tests.rs`, `src/config/{mod,theme}.rs` — the iamb
  store/actions/timeline model and everything only it used.
- `src/bitchx/{dispatch,screen,view}.rs` — glue onto modalkit actions and
  windows.
- Cargo deps: `modalkit`, `modalkit-ratatui`, and crates only the old UI
  used (image previews, html/markdown rendering, notifications, emoji,
  editor spawning, ...). Cargo.lock may only LOSE packages.

## Kept / adapted

- `src/bitchx/commands.rs` — the pure slash-command parser (+ `/whois`,
  `/rooms`).
- `src/bitchx/status.rs` — STATUS_FORMAT1/2 builders.
- `docs/bitchx-formats.md` — the BitchX1.3 default formats (source of truth
  for colours and line shapes).
- `src/oauth.rs` (redirect OAuth) and `src/device_link.rs` (RFC 8628 device
  link) — now return `anyhow::Result`.
- Session save/restore (`session.json`, `device.json`), sqlite state+crypto
  store, `iambitchx/` dir with legacy `iamb/` fallback, XDG_CONFIG_HOME /
  XDG_DATA_HOME honoured.

## Modules

    main.rs        CLI (clap): -P profile, -C config dir, --smoke.
                   logging, runtime, login, then tui::run or smoke::run.
    config.rs      TOML/JSON config: default_profile, profiles.{user_id,url,
                   password_file,dirs}, global dirs, settings.{log_level,
                   request_timeout}. Unknown (vim-era) keys are ignored.
                   Paths: <data>/profiles/<p>/{session.json,device.json,
                   sqlite}, <cache>/profiles/<p>/sqlite, <cache>/logs.
    login.rs       Client builder (sqlite store, refresh tokens), session
                   restore, interactive login ([p]assword [s]so [o]auth
                   [d]evice link), token persistence on refresh.
    matrix.rs      The Matrix side. sync loop (sync_with_callback), turns
                   SDK timeline events into `bitchx::model::Line`s, room
                   snapshots (name, alias, dm peer, members+PL, topic,
                   encryption), backfill via room.messages, and executes
                   `Effect`s (send text/emote, join, part, topic, nick,
                   invite, kick, op/deop=PL50, away/back, whois, query).
    bitchx/model.rs  Plain data: Line kinds, Member, RoomSnapshot, MxEvent.
    bitchx/input.rs  readline input line: cursor, Ctrl-A/E/U/K/W, Home/End,
                     history Up/Down, Tab nick completion.
    bitchx/app.rs    Pure state machine. Windows (1 = status window, every
                     joined room/DM a numbered window), current window,
                     Act list, scrollback offset. handle_key(KeyEvent) and
                     handle_mx(MxEvent) mutate state and return `Effect`s.
                     Slash-command dispatch lives here.
    bitchx/format.rs Line -> ratatui `Line` per docs/bitchx-formats.md.
    bitchx/ui.rs     draw(frame, &App): scrollback rows 0..h-3 (bottom
                     anchored, wrapped), status line 1 (h-3), status line 2
                     (h-2), input `[#room] text` (h-1). No borders.
    tui.rs         crossterm raw mode + alternate screen, EventStream +
                   mpsc from matrix.rs in one tokio::select! loop.
    smoke.rs       --smoke: restore session, sync once, build App from the
                   client's rooms, backfill the first room, render status
                   window and first room to TestBackend(80x24), print text.

## Data flow

    crossterm EventStream ─┐                 ┌─> Effect ─> matrix.rs task
                           ├─> App (pure) ───┤
    matrix.rs mpsc MxEvent ┘                 └─> ui::draw
    matrix.rs sync loop ──> MxEvent::{Room, Line, Left, Info}

The App never touches the SDK, so every behaviour (editing, window
switching, command dispatch, rendering) is unit-testable with a
`TestBackend`.

## Keys

Alt-1..9 window N, Ctrl-N/Ctrl-P next/prev window, PageUp/PageDown
scrollback, Enter submit, Left/Right, Home/End, Ctrl-A/E/U/K/W, Up/Down
history, Tab nick completion, Backspace/Delete, Ctrl-C/D (empty line) quit.

## Commands

/join /part /msg /query /me /topic /nick /names /who /whois /rooms
/window N|next|prev /clear /away /back /invite /kick /op /deop (PL 50)
/quit /help. Unknown commands print a `-:-` error line. Plain text goes
to the current room (encryption is automatic in encrypted rooms).
