# BitchX-style frontend for iamb

## What this is

`iambitchx` is a fork of [iamb](https://iamb.chat) (Matrix terminal client, Vim
keybindings). This fork keeps **everything** about iamb — the Matrix backend,
E2EE, message rendering, notifications, config schema, the existing modal UI —
and adds an optional **BitchX-style frontend**.

BitchX is the classic curses IRC client. Its screen is:

```
row 0        [nick@server/channel +modes] topic
rows 1..h-2  messages
row h-1      input line
```

No tabs, no window borders, no `:` command bar, no Normal/Insert modal
switching — you just type, and `/` at the start of the input line is a command.

The BitchX frontend is a **view** over the same `ProgramStore` the normal iamb
UI uses. It renders the focused room's chat full-screen and interprets
slash commands typed in the message bar.

## Non-goals (explicit)

* Do **not** touch the Matrix protocol layer: `src/worker.rs`, `src/message/**`,
  `src/verifications.rs`, `src/oauth.rs`, `src/notifications.rs`, `src/preview.rs`
  stay byte-identical.
* Do **not** remove or rewrite iamb's modal UI, keybindings, `:commands`,
  windows, spaces, threads, etc. They stay reachable.
* Do **not** add new crates to `Cargo.toml` unless unavoidable — `Cargo.lock`
  must stay consistent (`cargo test --locked` is used by CI).

## Layout of the new code

```
src/bitchx/mod.rs        module root, re-exports, `Frontend` enum
src/bitchx/commands.rs   pure slash-command parser + unit tests
src/bitchx/status.rs     pure status-bar builder + unit tests
src/bitchx/screen.rs     ratatui widget rendering the BitchX layout + render test
```

## 1. `src/bitchx/commands.rs` (pure, no Matrix types)

```rust
pub enum BitchxCommand {
    Msg { target: String, text: String },
    Me { text: String },
    Action { text: String },
    Join { room: String, key: Option<String> },
    Part { room: Option<String>, reason: Option<String> },
    Leave,
    Quit { reason: Option<String> },
    Nick { nick: String },
    Topic { topic: String },
    Away { reason: Option<String> },
    Back,
    Clear,
    Close,
    Who { target: Option<String> },
    Names { target: Option<String> },
    List,
    Mode { target: String, modes: Option<String> },
    Kick { target: String, reason: Option<String> },
    Op { target: String },
    Deop { target: String },
    Invite { target: String, room: Option<String> },
    Query { target: String },
    Ctcp { target: String, text: String },
    Ignore { pattern: String },
    Unignore { pattern: String },
    Window { target: Option<String> },
    NextWindow,
    PrevWindow,
    Version,
    Help,
}

pub enum BitchxError {
    UnknownCommand(String),
    MissingArgument { command: &'static str, argument: &'static str },
    InvalidArgument { command: &'static str, argument: String },
}

pub fn parse(input: &str) -> Result<Option<BitchxCommand>, BitchxError>;
```

Rules:

* Leading `/` is stripped by the caller; `parse` takes the text *after* the `/`.
* Command names are case-insensitive (`/MSG`, `/Msg`, `/msg` are the same).
* Everything after the command word is the argument string; split on
  whitespace, but keep the remainder (after the first argument) intact so
  message bodies may contain spaces.
* Empty input, or input that is only whitespace → `Ok(None)` (send as a
  normal message).
* Input that does not start with a known command word → `Err(UnknownCommand)`.
* No-argument commands must have no argument → `Err(InvalidArgument)`.
* Commands requiring an argument with none → `Err(MissingArgument)`.
* `quit`/`exit` are aliases; `leave` is an alias of `part` with no room;
  `me`/`action` are aliases of each other.

## 2. `src/bitchx/status.rs` (pure)

BitchX's status bar looks like:

```
[nick@server/channel +modes] topic text
```

```rust
pub struct StatusBar {
    pub nick: String,
    pub host: String,
    pub channel: String,
    pub modes: Vec<String>,   // e.g. ["e"] for encrypted
    pub topic: String,
}

impl StatusBar {
    pub fn format(&self) -> String;              // "[nick@host/channel +modes] topic"
    pub fn render(&self, width: u16) -> Line<'_>; // truncated to `width` cells
}
```

* No topic → the `] topic` part is omitted entirely.
* No modes → the ` +modes` part is omitted entirely.
* Truncation must never split a grapheme (`unicode_width` is already a
  dependency) and must not panic on `width == 0`.

## 3. `src/bitchx/screen.rs` (rendering)

A `ratatui::widgets::StatefulWidget` (or plain `Widget`) that draws:

| region          | content                                              |
|-----------------|------------------------------------------------------|
| row 0           | the status bar                                        |
| rows 1..h-2     | the focused room's chat, via the existing `Chat` widget |
| row h-1         | the input line (the chat's own message bar)           |

Reuse `crate::windows::room::chat::Chat` for the message area so message
formatting, timestamps, image previews and the message bar stay identical to
iamb's normal UI — that is the "everything else stays the same" requirement.

The screen must not draw iamb's tab bar, window borders, or the `:` command
bar. iamb's `Screen` widget (modalkit-ratatui) draws all three, so the
BitchX screen is a separate widget used instead of `Screen` in
`Application::redraw()`.

## 4. Wiring

### 4.1 Config

Add to `TunableValues` (in `src/config/mod.rs`):

```rust
pub frontend: Frontend,   // enum { Iamb, Bitchx }, serde rename_all = "lowercase"
```

* `[tunables] frontend = "bitchx"` selects the BitchX frontend.
* Default stays `iamb` → **existing config files keep working unchanged**.
* `Frontend` derives `Deserialize`, `Serialize`, `Copy`, `Clone`, `Debug`,
  `PartialEq`, `Eq`, `Default` (default = `Iamb`).

### 4.2 Draw path

In `src/main.rs::Application::redraw()`, when the BitchX frontend is active
**and** the focused window is a joined room, render the BitchX screen instead
of modalkit's `Screen`. Otherwise fall back to the normal iamb screen (so
`:rooms`, `:members`, spaces, verification windows etc. still work — in those
cases the BitchX screen is not applicable).

### 4.3 Slash command dispatch

In `src/windows/room/chat.rs::ChatState::send_command()`, in the
`SendAction::Submit | SendAction::SubmitFromEditor` arm: if the BitchX
frontend is active and the message bar content starts with `/`, then
**before** sending:

1. `let text = self.tbox.get();`
2. `match bitchx::commands::parse(text.trim_start_matches('/'))`:
   * `Ok(None)` → fall through to normal send.
   * `Err(e)` → show the error in the message area (do **not** send).
   * `Ok(Some(cmd))` → dispatch, clear the message bar, and return without
     sending the text as a message.

Dispatch maps onto existing iamb machinery (do not reimplement Matrix calls):

| BitchX command      | iamb mechanism                                                        |
|---------------------|-----------------------------------------------------------------------|
| `msg`               | open/focus the DM room with the target, then send the message         |
| `me` / `action`     | send as a markdown emote (`*text*`)                                    |
| `join`              | reuse the `:join` command implementation (`iamb_join` in `src/commands.rs`) |
| `part` / `leave`    | `IambAction::Room(RoomAction::Leave(true))`                            |
| `quit`              | exit the application                                                   |
| `nick`              | set the display name of the logged-in user                             |
| `topic`             | set the room topic                                                     |
| `away` / `back`     | set presence to away / online                                          |
| `clear`             | clear the room's scrollback                                            |
| `close`             | close the focused room window                                          |
| `who` / `names`     | open the members window (`RoomAction::Members`)                        |
| `kick`              | room member update / ban via `RoomAction::MemberUpdate`                |
| `op` / `deop`       | room member update via `RoomAction::MemberUpdate`                      |
| `invite`            | `RoomAction::InviteSend`                                               |
| `list`              | open the rooms window (`iamb_rooms`)                                   |
| `query`             | open a DM room with the target                                         |
| `mode`              | `RoomAction::Show(RoomField::...)` / member update where applicable    |
| `ignore`/`unignore` | reuse `iamb_ignore`                                                    |
| `window`/`next`/`prev` | switch the focused room window                                      |
| `version`           | show the crate version in the message area                             |
| `help`              | show the list of supported commands in the message area                |

Where an exact iamb equivalent does not exist (e.g. `away`), implement the
smallest possible action in `src/base.rs` (`IambAction`/`RoomAction`/
`MessageAction`) and reuse the existing worker request helpers.

### 4.4 Key handling

In BitchX mode the input line behaves like BitchX:

* printable keys go straight into the message bar (no Normal/Insert switch),
* `Enter` submits (send or run the slash command),
* `Esc` clears/abandons the current input line,
* the existing iamb keybindings still work for navigation (e.g. `Ctrl-w`,
  `Ctrl-u`, `Ctrl-f`).

iamb's key handling lives in `src/keybindings.rs` +
`Application::action_run()` in `src/main.rs`. Implement "always insert" by
forcing the key manager into insert mode when the BitchX frontend is active
(the `keybindings` crate exposes mode switching; `KeyManager::reset_mode()`
is already used in `src/main.rs` for `normal_after_send`).

## 5. Tests (all runnable)

* `src/bitchx/commands.rs` — unit tests: every command parses; aliases work;
  case-insensitivity; missing/invalid arguments error; empty input is
  `Ok(None)`; unknown command errors; message bodies with spaces survive.
* `src/bitchx/status.rs` — unit tests: format with and without topic/modes;
  truncation at a given width; `width == 0` does not panic; wide characters
  are not split.
* `src/bitchx/screen.rs` — a `ratatui::backend::TestBackend` test that
  renders the BitchX screen and asserts the status bar text and the input
  line are present in the buffer.
* `src/config/mod.rs` — a test that `[tunables] frontend = "bitchx"` parses
  and that the default is `iamb`.

## 6. Style constraints (CI enforces these)

* `cargo +nightly fmt --all -- --check` must pass.
* `./.github/workflows/check-imports-format.sh` must pass for every file in
  `src/` — import order is: `std` imports, blank line, other imports, blank
  line, `crate` imports; and a `use` with braces may only pull from a single
  module (no `foo::{bar, baz::qux}`).
* `cargo clippy --all-targets -- -D warnings` must pass.
* `cargo test --locked` must pass.

## 7. Environment notes for the build box

* Toolchain: `export PATH=$HOME/.cargo/bin:$PATH` (rustup lives in
  `~/.cargo/bin`; the pinned toolchain is `1.96` per `rust-toolchain.toml`).
* C toolchain (`gcc`, `cmake`, `pkg-config`) is required by `aws-lc-rs` and
  the bundled SQLite.
* `cargo build` of this dependency tree takes several minutes — run it in the
  background, never block the turn on it.
