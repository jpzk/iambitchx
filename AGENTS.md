# AGENTS.md — iamBitchX agent guide

BitchX 1.x-style Matrix client for the terminal. Fork of iamb. Read the
hard rules, then jump to the recipe you need.

## Hard rules (skim these if nothing else)

1. **`src/bitchx/**` is pure** — no SDK types, no terminal I/O, no clock,
   no network, no randomness. Anything impure goes behind an `Effect`.
2. **Remote text never reaches the screen raw** — every nick, name, body,
   topic, reason goes through `src/bitchx/text.rs` (F30/F31 are security
   fixes). New `MxEvent` fields carrying remote strings → `clean_event()`.
3. **Secrets never logged or printed** — `Secret` in `device_link.rs`,
   redaction helpers in `debug_tap.rs`, `/recover` never echoes the key.
4. **CI gate before every commit** — see *Definition of done* below. CI
   red = not done. Never `--no-verify`, never skip `--locked`.
5. **Never force-push `main`** without first writing a backup bundle to
   `/workspace/backup/`. Never touch `/workspace/recon/`, `/workspace/ref/`,
   `.github/workflows/*` pins, or `Cargo.lock` (unless bumping deps).

One heavy build at a time on this box: all worktrees share one target
dir, 4 cores, little disk. Long cargo runs go through `cs-job`.

## Live facts — run, don't memorise

Numbers in docs rot. Ask the repo instead:

    git -C /workspace/iambitchx status -sb      # branch, ahead/behind
    git worktree list                           # all worktrees + branches
    git log --oneline -1 main                   # trunk tip
    wc -l src/*.rs src/bitchx/*.rs | sort -n    # file sizes
    grep -L 'cfg(test)' src/*.rs src/bitchx/*.rs  # files with no tests
    grep -c '#\[test\]' src/*.rs src/bitchx/*.rs | awk -F: '{s+=$2}END{print s}'
    ls themes | wc -l                           # packaged themes
    df -h /workspace                            # disk (scarce!)

At v0.1.0 (`c53d2b6`) `main` and `feat/bitchx-v2` point at the same
commit; the main checkout sits on `feat/bitchx-v2`. Commit to the branch
you are on; trunk is `main`.

Files with no `#[cfg(test)]` (re-check with the grep above): `login.rs`,
`main.rs`, `e2ee.rs`, `oauth.rs`, `tui.rs`, `src/bitchx/status.rs`, plus
`src/bitchx/mod.rs` (module list only). A real gap — do not copy it.

## Tree map

    src/            the binary: Matrix side + wiring
    src/bitchx/     the frontend: pure state machine + renderer
    docs/           design docs, man pages, supply-chain notes
    themes/         iamb-format TOML themes (packaged only, see below)
    site/           GitHub Pages landing page
    .github/workflows/  ci, release, binaries, codeql, pages + scripts

Outside the repo (in `/workspace`): `recon/`, `ref/` (read-only
reference), `goal/` (goal ledgers), `bin/` + `scripts/` (CI gate,
dashboards), `*-wt/` (worktrees), `backup/` (rollback bundles).

## Module index

Frontend, `src/bitchx/` (pure):

- `app.rs` — **the state machine** and by far the biggest file. `App`
  owns windows, input, away, `/set` vars, presence, verification,
  splash logo. `handle_key`/`handle_mx` in, `Vec<Effect>` out;
  `command()` is the slash dispatch. Test helpers `app()`, `typed()`.
- `model.rs` — plain data (`Line`, `RoomSnapshot`, `Member`) and the two
  boundary enums `MxEvent` (Matrix→UI) and `Effect` (UI→Matrix).
- `commands.rs` — `parse()` for text after `/` → `BitchxCommand`;
  `COMMAND_NAMES` feeds `/help`.
- `channels.rs` — `/join` target normalisation, `/list` lines.
- `format.rs` — BitchX 1.3 FSETs, cited by upstream file:line.
- `status.rs` — the two blue status lines. `ui.rs` — the screen.
- `input.rs` — readline (cursor, Ctrl-keys, history, Tab completion).
- `text.rs` — remote-text hygiene (rule 2). `mention.rs` — highlights.
- `vars.rs` — `/set` variables: `NAMES`, `Vars`, `BitchxConfig`, persist.
- `e2ee.rs` — pure text for E2EE `-:-` lines.

Matrix side, `src/`:

- `main.rs` — CLI, logging, tokio runtime, `umask(0o077)`.
- `tui.rs` — event loop: keys + `MxEvent` channel + tick in one
  `select!`; hands each `Effect` to `matrix::spawn_effect`.
- `matrix.rs` — sync loop, timeline → `Line`, snapshots, backfill, and
  `run()`, the **`Effect` executor**.
- `e2ee.rs` — verification, SAS, `/devices`, `/trust`, `/recover`.
- `login.rs`, `oauth.rs`, `device_link.rs` — login paths.
- `session_guard.rs` — `session.json.lock`; `decide()` = token-loss policy.
- `token_refresh.rs` — proactive refresh (SDK auto-refresh is off on
  purpose: it would rotate the refresh token outside the lock).
- `debug_tap.rs` — `/set debug on` protocol tap, with redaction.
- `config.rs` — profiles, dirs, `[settings]`, `[settings.bitchx]`.
- `smoke.rs` — `--smoke`: one sync, render to text, never writes.

Known purity exception: `App::set` calls `Vars::persist` (writes the
config file) directly. Do not add more.

## Recipe: add a /slash command

1. `src/bitchx/commands.rs` — add a `BitchxCommand` variant; add the
   word to `COMMAND_NAMES` (keeps `/help` honest); add an arm in
   `parse()` using `Args` (`word`, `required_word`, `rest`, `finish`,
   `none`). Test in the same file's `mod tests` via `ok("...")`.
2. `src/bitchx/app.rs` — add the arm in `App::command()`. Purely local?
   mutate `self`, emit lines via `info`/`error`, return `vec![]`.
   Needs Matrix? return an `Effect` (next recipe). Not meaningful on
   Matrix? add it to the "Not supported on Matrix" arm.
3. Test in app.rs `mod tests`: `assert_eq!(typed(&mut app, "/foo x"),
   vec![Effect::Foo(..)])` — see `test_commands_to_effects`.
4. If BitchX has it, cite the upstream source (`source/*.c:line`).

## Recipe: add an Effect or MxEvent variant

Effect (UI → Matrix):

1. `src/bitchx/model.rs` — add the variant to `Effect`. Plain data only:
   ruma id types are fine, SDK handles (`Room`, `Client`) are not.
2. Emit it from `App` (usually in `command()`).
3. `src/matrix.rs` — add the arm in `run()`. Report results back with
   `tx.send(MxEvent::...)`; errors bubble as `anyhow` and are shown.
   E2EE work lives in `src/e2ee.rs`; call it from the arm.
4. Test the emit side in app.rs; test pure helpers in matrix.rs.

MxEvent (Matrix → UI):

1. Add the variant to `MxEvent` in `src/bitchx/model.rs`.
2. Send it from `src/matrix.rs` (sync loop or an `Effect` arm).
3. Handle it in `App::handle_mx()` in app.rs.
4. Carries remote strings? sanitise them in `clean_event()` (rule 2).
5. Test: feed `app.handle_mx(MxEvent::...)` and assert on lines/effects.

## Recipe: add a /set variable

All in `src/bitchx/vars.rs` unless noted:

1. Add the UPPERCASE name to `NAMES` (sorted — it is the listing order).
2. Add an `Option<..>` field to `BitchxConfig` (the config section).
3. Add the field to `Vars` + its BitchX default in `Default` (cite
   `include/config.h` / `source/vars.c` for the default).
4. Wire `from_config`, `get`, `set` (use `boolean()` for ON/OFF/TOGGLE)
   and the `persist` insert so the value round-trips.
5. Read it where it matters in app.rs via `self.vars`.
6. Test in vars.rs: set/get, bad value rejected, persist round-trip.

## Recipe: add a theme

`themes/` holds iamb-format TOML (`[theme.users]`, `[theme.default]`,
…). Nothing in `src/` reads them today — the BitchX frontend hardcodes
the `include/color.h` palette in `format.rs`/`status.rs`. They are only
packaged (deb/rpm/tar globs in `Cargo.toml`). So:

1. Copy an existing file, e.g. `themes/ayu-dark.toml`, keep the header
   comment block (palette legend), rename, recolour.
2. No code change; packaging picks it up via the glob.
3. Making themes actually apply is a feature, not a recipe — design it
   in `docs/` first.

## Definition of done

Run from the repo root (long ones as a `cs-job`):

- [ ] /workspace/bin/bx-ci.sh quick — nightly fmt check + clippy, no
      codegen. Use while iterating.
- [ ] `cargo +nightly fmt --all -- --check` (nightly: `.rustfmt.toml`
      uses unstable options; `max_width = 100`).
- [ ] `cargo clippy --locked --all-targets -- -D warnings`
- [ ] `cargo test --locked` (or bx-ci.sh testall).
- [ ] `.github/workflows/check-imports-format.sh` — import order:
      `std` / blank / external crates / blank / `crate::`. A braced
      `use` pulls from ONE module only. Read the script, don't guess.
- [ ] New behaviour has a test in the same file's `#[cfg(test)]`.
- [ ] Remote strings sanitised, no secrets in logs (rules 2–3).
- [ ] Docs touched if behaviour changed (`docs/bitchx-frontend.md`,
      man pages `docs/iambitchx.1` / `docs/iambitchx.5`).

CI also runs windows + macos; clippy only on ubuntu
(`.github/workflows/ci.yml`).

## Commit and push conventions

- Identity: `jpzk <jendrik@madewithtea.com>` — check `git config
  user.email` in a fresh worktree before committing.
- Messages: `area: lowercase summary` (`docs:`, `ci:`, `startup:`,
  `release:` …) — see `git log --oneline`.
- **Same-length rename rule**: renaming a fixture/identifier? use a
  replacement of the same length and sort slot, or fmt reflows and a
  dozen tests go red. History scrubs use the same rule.
- Never force-push `main`. If a rewrite is unavoidable: `git bundle
  create /workspace/backup/iambitchx-<what>-<date>.bundle --all`
  first, verify it, then push.
- Don't push unless asked; leave the branch ahead of origin.

## Commands

    cargo build --locked                    # dev build
    cargo build --release --locked          # ./target/release/iambitchx
    cargo +nightly fmt --all                # format
    /workspace/bin/bx-ci.sh quick|test|testall|clippy|build|fmt
    ./target/release/iambitchx -P <profile> # run
    ./target/release/iambitchx --smoke      # headless self-check

bx-ci.sh `test` = only bitchx tests; `testall` = everything. It forces
incremental on and has a disk guard that prunes caches when low.

## Conventions

- **BitchX fidelity**: ported formats cite upstream file+line
  (`include/color.h:199`, `source/parse.c:596`). Keep the citation.
- Time/randomness enter via `App`'s `clock`, `fake_now`, `fake_mono`.
- Comments say *why* and cite sources; code says what.
- Tests live next to the code; there is no `tests/` dir.

## Docs map

- `docs/project.md` — working notes and lessons. **Start here.**
- `docs/v2-design.md` — frontend design of record.
- `docs/bitchx-frontend.md` — frontend spec; `docs/bitchx-formats.md`
  — FSETs with upstream citations.
- `docs/bitchx-recon.md`, `docs/polish-diff.md` — real BitchX 1.3
  behaviour vs ours.
- `docs/ui-fixes.md` — F-numbered UI/security findings.
- `docs/ci-supply-chain.md` — workflow pins and why.
- `docs/release.md` — release pipeline; `docs/sdk-0.20-e2e.md` — e2e.
- `docs/vuln-bump.md`, `docs/vuln-f03.md`, `docs/vuln-lock-versions.md`.

## This machine

- 4 cores, ~4 GB RAM; disk is the scarce resource (`df -h /workspace`).
- Toolchain in `/workspace/.cargo` (HOME=`/workspace`), pinned in
  `rust-toolchain.toml`. Only `/workspace` survives a VM restart.
- Never disable incremental; never run two cargo builds at once.
