# AGENTS.md — iamBitchX code index

An index, not a tutorial. What this repo is, where each piece lives, and
the handful of rules that turn CI red when ignored.

## What this is

- A **BitchX 1.x-style Matrix client** for the terminal: `/join #chan`,
  `/msg`, `/whois`, `/away`, `/set`, `/names`, `/mode` — IRC vocabulary over
  Matrix, E2EE underneath.
- **Fork of [iamb](https://github.com/ulyssa/iamb)** (Apache-2.0, by Ulyssa).
  The Matrix backend (sync, E2EE, store, `matrix-rust-sdk` 0.20) is iamb's.
  The Vim frontend is **gone**; the BitchX frontend is new. Login/token
  handling was reworked (OAuth + device link + proactive refresh).
- Rust 2024 edition, toolchain pinned to **1.96** (`rust-toolchain.toml`).
  Binary `iambitchx`. ~200 unit tests, all inline `#[cfg(test)]` modules —
  there is no `tests/` directory.
- origin: `https://github.com/jpzk/iambitchx.git`, base upstream commit
  `fbc4700` (upstream history is intact; only our own commits are ours).
- Work happens on **`main`**. Every `feat/*` branch is merged into it
  (design of record for the frontend: `docs/v2-design.md`). The old
  `feat/bitchx-v2` tip is kept as a remote branch; `main` is the trunk.

## Tree map

    src/            the binary (25 .rs files, ~15k lines)
    src/bitchx/     the frontend: pure state machine + renderer
    docs/           design docs, man pages, supply-chain notes (see map below)
    themes/         62 packaged TOML themes
    site/           GitHub Pages landing page (site/index.html)
    .github/workflows/  CI: ci.yml, release.yml, binaries.yml, codeql.yml, pages.yml
    flake.nix       nix build
    Cargo.toml      deps, features (bundled sqlite), deb/rpm/tar packaging metadata

Sibling dirs in `/workspace` are **not** part of this repo:

    recon/          BitchX 1.3 reference material + captures (read-only)
    ref/            upstream sources fetched for reference (read-only)
    goal/           autopilot goal ledger dirs (history, not source)
    bin/, scripts/  helper scripts (CI gate, dashboard) — outside the repo
    *-wt/           git worktrees of this repo (see branches below)

## Module index

### Frontend — `src/bitchx/` (pure: no SDK, no terminal, no I/O)

- `mod.rs` — module list only; `bitchx-frontend.md` + `bitchx-formats.md` are
  the spec.
- `app.rs` (4180 ln) — **the state machine**. `App` holds the numbered
  `windows`, the input line, away state, `/set` vars, presence map,
  verification state, the splash logo. Keys and Matrix events go in,
  `Effect`s come out. Owns slash dispatch, `/window`, paging, the CP437
  splash-logo renderer (`Logo::pick`, `App::fit_logo`).
- `model.rs` — plain data shared by both sides: `Line`, `RoomSnapshot`,
  `Member`, `Presence`, and the two boundary enums **`MxEvent`**
  (Matrix→UI) and **`Effect`** (UI→Matrix). No SDK or terminal types here.
- `commands.rs` — parser for the text after a leading `/`. Pure; dispatch
  lives in `app.rs`.
- `channels.rs` — `/join` target normalisation (`#x` → `#x:<own homeserver>`,
  `matrix.to` links, `!ids`) and the `/list` directory lines.
- `format.rs` — renders `Line`s with BitchX 1.3 "default bx" FSETs from
  `include/color.h`. Ported formats, cited by file+line in comments.
- `status.rs` — the two blue status lines (`STATUS_FORMAT1`/`2`,
  `DOUBLE_STATUS_LINE`).
- `ui.rs` — the screen: scrollback from row 0, two status lines, input line
  on the last row. No borders, no box drawing.
- `input.rs` — small readline: cursor, Ctrl-A/E/U/K/W, history, Tab nick
  completion. Works on `char`s; display width belongs to the renderer.
- `text.rs` — **remote-text hygiene**. Every nick, display name, body,
  topic, room name and reason passes through here: F30 drops Unicode `Cf`
  format chars (bidi, zero-width); F31 replaces control/line-separator chars
  in one-line fields with spaces. Bodies keep their newlines.
- `mention.rs` — BitchX `AUTO_REPLY` highlight detection (own nick, mxid,
  display name, extra words; word-boundary, case-insensitive).
- `vars.rs` — the `/set` variables: `TIMESTAMP`, `TIMESTAMP_STRING`,
  `AUTO_AWAY`, `AUTO_AWAY_TIME`, `AWAY_NICK`, `AWAY_MSG`, `AUTO_AWAY_MSG`,
  `AUTO_REPLY`, `BEEP`, `DEBUG`, `DEBUG_FILTER`, `DEBUG_RAW`.
- `e2ee.rs` — pure text for the E2EE `-:-` lines (SAS emoji, decrypt
  failures, in-room `m.key.verification.*`).

### Matrix side — `src/`

- `main.rs` — CLI parse, logging setup, tokio runtime (2 worker threads),
  `umask(0o077)`, module wiring.
- `login.rs` — builds the client (sqlite state + crypto store), restores or
  creates a session, keeps `session.json` current.
- `oauth.rs` — redirect-based OAuth login.
- `device_link.rs` — OAuth 2.0 Device Authorization Grant (RFC 8628),
  headless login. Secrets wrapped in `Secret` (redacted `Debug`, no
  `Display`).
- `session_guard.rs` — `session.json.lock` advisory lock around every
  refresh and every write; `decide()` is the pure `M_UNKNOWN_TOKEN`
  recovery policy; `--smoke` runs as `Mode::ReadOnly`.
- `token_refresh.rs` — proactive refresh `REFRESH_MARGIN` before access
  token expiry (SDK auto-refresh is off on purpose: it would rotate the
  refresh token outside the lock).
- `matrix.rs` — the sync loop: timeline → `Line`s, room snapshots,
  history backfill, the `Effect` executor. Emits `MxEvent`s over a channel.
- `e2ee.rs` — verification handlers (to-device + in-room), SAS flow,
  `/devices`, `/trust`, `/recover`, `/encrypt`.
- `debug_tap.rs` — protocol debug tap (`/set debug on`): `tracing` layer
  for http/auth/crypto + sync hooks; redaction helpers strip query strings,
  `Authorization`, token/secret/key-ish JSON fields, and mask bare
  `mat_`/`mar_`/`syt_` tokens. Never blocks; drops on a full channel.
- `config.rs` — TOML/JSON config: profiles (`user_id`, `url`), directories
  for session / sqlite store / logs, `[settings]`, `[bitchx]`. Legacy `iamb`
  app dir is still read.
- `smoke.rs` — `--smoke`: restore session, one sync, render to a ratatui
  `TestBackend`, print as text. Never refreshes, never writes.
- `tui.rs` — raw mode + alternate screen; crossterm `EventStream` and the
  Matrix channel multiplexed in one `select!`.

## The one architectural rule

`src/bitchx/**` is **pure**. No SDK types, no terminal I/O, no clock, no
filesystem, no randomness. It is driven entirely by `App` + the two enums:

- UI → Matrix: `Effect` (in `model.rs`)
- Matrix → UI: `MxEvent` (in `model.rs`)

Anything that needs the network, the clock or the disk goes behind an
`Effect` variant. Time and randomness enter through `App`'s `clock`,
`fake_now`, `fake_mono` fields — that is how the UI is testable at all.

## Rules that turn CI red

1. **Import order** — `use std::…` / blank line / other crates / blank
   line / `use crate::…`. A braced `use` may pull from **one** module
   only (no `foo::{bar, baz::qux}`). Enforced by
   `.github/workflows/check-imports-format.sh`; it is a shell script, read
   it before guessing.
2. **`cargo +nightly fmt --all -- --check`** — nightly, because
   `.rustfmt.toml` uses unstable options (`imports_layout`,
   `force_multiline_blocks`, …). `max_width = 100`.
3. **`cargo clippy --locked --all-targets -- -D warnings`** — no warnings.
4. **`cargo test --locked`** — `Cargo.lock` is committed; always `--locked`.
5. CI runs on ubuntu/windows/macos; clippy only on ubuntu.

## Commands

    cargo build                          # dev build
    cargo build --release                # release binary
    cargo +nightly fmt --all             # format
    cargo clippy --all-targets -- -D warnings
    cargo test --locked

The local fast gate lives **outside the repo**: `/workspace/bin/bx-ci.sh`
(`fmt` / `quick` / `test` / `testall` / `clippy` / `build`). `quick` =
fmt + clippy check-only, no codegen — use it while iterating. `test` is
slow (codegen + link) and holds the shared build lock.

Run the app: `./target/release/iambitchx` (add `-P <profile>` for a
non-default profile). `--smoke` is the headless self-check.

## Conventions worth keeping

- **BitchX fidelity**: ported formats cite the upstream file and line
  (`include/color.h:199`, `source/parse.c:596`). Keep the citation when
  touching them — it is the only link to the original.
- **Remote text never reaches the screen raw** — route it through
  `bitchx/text.rs`. F30/F31 are security fixes, not cosmetics.
- **Secrets stay secret** — tokens, device codes and recovery keys are
  never logged or printed; use `Secret` in `device_link.rs` and the
  redaction helpers in `debug_tap.rs`.
- **Renames are semantic changes**: fixtures need same-length, same-sort-slot
  replacement names or `cargo fmt --check` and a dozen tests go red.
- **Tests live next to the code** — add a `#[cfg(test)]` module in the same
  file. `status.rs`, `tui.rs`, `oauth.rs`, `login.rs` and `e2ee.rs`
  currently have none; that is a real gap, not an oversight to copy.
- Comments explain *why* and cite sources; the code says what.

## Docs map (`docs/`)

- `project.md` — working notes: what it is, the CP437 splash-logo bug, the
  history rewrite, lessons. **Start here.**
- `v2-design.md` — design of record for `feat/bitchx-v2`.
- `bitchx-frontend.md` — the frontend spec of record.
- `bitchx-formats.md` — BitchX 1.3 default FSETs, with upstream citations.
- `bitchx-recon.md`, `polish-diff.md` — recon of real BitchX 1.3 behaviour
  and where v2 diverges.
- `ui-fixes.md` — UI security/display findings (F-numbers) and their fix status.
- `ci-supply-chain.md` — what the workflows pin and why.
- `release.md` — release pipeline; `sdk-0.20-e2e.md` — live e2e checklist.
- `vuln-bump.md`, `vuln-f03.md`, `vuln-lock-versions.md` — RustSec work.
- `iambitchx.1`, `iambitchx.5` — man pages (packaged).

## Branches and worktrees

    /workspace/iambitchx   main               main working tree (trunk)
    /workspace/dbg-wt      feat/dbg-mode      protocol debug tap
    /workspace/pres-wt     feat/presence      presence UI
    /workspace/ui-wt       feat/ui-fixes      UI/security fixes
    /workspace/rel-wt      feat/release-attest release attestation
    /workspace/sdk-wt      feat/sdk-0.20      matrix-sdk 0.20 upgrade

All worktrees share one `CARGO_TARGET_DIR` (`/workspace/iambitchx/target`) —
never run two cargo builds at once on this box.

## This machine (read before building)

- 4 cores, ~4 GB RAM, ~1.8 GB free disk on `/workspace` (76% used).
- Toolchain lives in `/workspace/.cargo` (HOME=`/workspace`); symlinks in
  `/usr/local/bin` survive, the rest does not after a VM restart.
- Disk is the scarce resource: incremental caches grow per worktree.
  `bx-ci.sh` has a disk guard that prunes them under 1.5 GB free — do not
  disable incremental compilation.
- Long builds must run as a background job (`cs-job`), never in the
  foreground of a turn.
- Only `/workspace` survives a VM restart.

## Never touch

- `target/`, `Cargo.lock` (unless a dependency bump is the task),
  `.github/workflows/*` pins.
- `/workspace/recon/` and `/workspace/ref/` — read-only reference material
  (fetched upstream BitchX sources, captures).
- `backup/*.bundle` — the rollback bundles for the history rewrite.
