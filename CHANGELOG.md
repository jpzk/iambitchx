# Changelog

All notable changes to **iamBitchX** are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project uses [semantic versioning](https://semver.org/).

iamBitchX is a fork of [iamb](https://github.com/ulyssa/iamb) by Ulyssa
([iamb.chat](https://iamb.chat)). Everything before the fork point
(`fbc4700`) is iamb's work and is not listed here — see iamb's own history
for that.

> **Versioning note:** the first iamBitchX release is `v0.1.0`. The
> `0.14.0-alpha.1` we inherited from iamb is gone: our version line starts
> at `0.1.0`, independently of iamb's.

## [Unreleased]

### Security

- **M-04: DM windows need the right peer.** A room only opens as a
  `/query` window if it has at most two members and the query peer is
  joined or invited. A two-member room with another user (or a peer that
  left, was banned or only knocked) is shown as a normal channel, and
  `/msg` refuses it and names the peer's state.
- Server error text from admin commands is sanitised before it is printed.

### Added

- **`/lastlog [-i] TEXT`.** Searches the current window's scrollback
  (case-sensitive, `-i` ignores case) and prints the hits BitchX style
  between `Lastlog:` and `End of Lastlog`, each with its own timestamp (the
  `TIMESTAMP_STRING` format, even with `TIMESTAMP` off). `/lastlog -clear`
  removes the output. Earlier lastlog output is not searched again.
- **Logging to file, irssi style.** `/set LOG ON|OFF` (default `OFF`) and
  `/set LOG_DIR` (default `$XDG_DATA_HOME/iambitchx/logs`, must be
  absolute): each window appends plain text to
  `LOG_DIR/<server>/<window>.log` (DMs by peer nick, `status.log`), `HH:MM`
  stamps and `--- Log opened` / `--- Day changed` / `--- Log closed`
  markers. File names are sanitised; files are `0600`, directories `0700`,
  symlinks refused (`O_NOFOLLOW`). Debug-tap output and `/lastlog` views are
  never logged; both settings persist in `[settings.bitchx]`.
- **Reactions.** `m.reaction` shows as `* nick reacted <key> to "short quote"`
  (key and quote sanitised and clipped). `/react <emoji|text> [N]` reacts to
  the last message, or the Nth-last.
- **Typing.** `[typing: nick, nick]` in the status bar for the active window;
  typing notices go out while the input line has text (throttled, stopped on
  send or clear). `/set SEND_TYPING ON|OFF` (default ON).
- **Read receipts.** Opening or switching to a window sends `m.read` and the
  `m.fully_read` marker for its newest event. `/set SEND_RECEIPTS ON|OFF`
  (default ON).
- **Channel creation.** `/create #x [-public] [-noencrypt]` creates `#x` on
  your own homeserver: invite-only and encrypted by default. `-public` uses
  the public preset and lists the room in the directory (still encrypted;
  you get a warning that late joiners can't read older history).
  `/join #x:your.server` for a room that does not exist offers to create it
  (`[y/n]`). Other servers still say "no such channel".
- **Channel admin.** `/alias add|del`, `/publish on|off`, `/ban`, `/unban`,
  `/kickban`, `/power NICK 0..100` (refused above your own level).
- **`/ignore` on Matrix.** `/ignore NICK|@user:server` and `/unignore` use
  the account ignore list (`m.ignored_user_list`, via matrix-sdk
  `ignore_user`/`unignore_user`); bare `/ignore` lists it. A nick is
  resolved against the window members. Messages from ignored users are
  hidden client-side too. `/ignore` is no longer "Not supported on Matrix".
- **`/mode` on Matrix.** `+i/-i` join rule, `+m/-m` moderated
  (`events_default` 50/0), `+t/-t` topic lock, `+o/-o` op, `+b/-b` ban.
  Bare `/mode` prints the modes (e.g. `+it`), and the status bar shows them.
  Unknown letters are an error that names the supported ones.
- Spaces are tagged `[space]` in `/channels` and in their window.

### Changed

- A send refused with `M_FORBIDDEN` prints
  `iamBitchX: read-only channel (power N needed, you have M)`.
- `/join` prints `Joining #x...`. The "synched" line and NAMES wait for
  the first sync that has members, so there is no empty `[Users(0/0)]`.

## [0.1.2] — 2026-11-27

First release after `v0.1.0`. `v0.1.1` was tagged but never released (its
gated release run was cancelled, and it lacks the security fixes below);
everything listed under `[0.1.1]` ships in this release.

### Security

- **Terminal-escape sanitiser (F-04, AUTH-3, F-FMT-1).** Remote text (login
  errors, device-link instructions, `/list` directory entries) goes through
  one central sanitiser; Info/Error lines render as one row, so an embedded
  LF can no longer forge rows.
- **Config (CONF-1).** An empty or relative `XDG_CONFIG_HOME` is ignored.
- **Session pinning (CONF-2).** A saved session is pinned to its homeserver.
- **Debug tap.** Fail-closed free-text redaction, raw bodies masked,
  status-window only, session-scoped debug switches; debug is never
  persisted.
- **Verification / E2E (E2–E6, B1).** Incoming SAS flows are offered, never
  auto-accepted; `/verify start` never guesses a pending flow; the
  requesting device is shown as resolved by the SDK; what a verification
  verifies is named; a SAS that fails to start is an error; `/encrypt`
  prompts name the spoof-safe label plus the room id.
- **Room id (M-03).** A window named from `m.room.name` shows the room id.
- **DM member cap (M-04).** An `m.direct` room with more than two members is
  never treated as a private query; `/msg`/`/query` into one is refused.

### Added

- `/theme` with a live-preview picker: 62 embedded iamb themes plus a
  `terminal` theme that uses the terminal's own colours.
- Website FEATURES section, `llms.txt`, `robots.txt`, `sitemap.xml`.

### Changed

- A terminal too small for the splash (< 78 columns or < 36 rows) now
  shows no logo at all instead of the compact block wordmark or the figlet
  slant; the `[ iamBitchX-ver]` credit line stays. Resizing to a big
  terminal brings the original art back, shrinking drops it again.

### Fixed

- The splash swap on resize no longer duplicates or loses the version
  credit line: the logo slot is tracked by position, not by line kind.

## [0.1.1] — 2026-11-27 (tagged, never released)

### Changed

- **Module refactor (pure moves, no behaviour change).** `app.rs` became
  `app/` (`state`, `logo`, `verify`, `events`, `keys`, `away`, `debug`,
  with tests split by topic under `app/tests/`); `debug_tap` became
  `debug_tap/{mod,layer,redact,sync,tests}.rs`; `commands` became
  `commands/{mod,secret,tests}.rs`. No source file is over 1000 lines.
  Visibility narrowed to `pub(crate)`/`pub(super)`, every module has a
  `//!` doc, and remaining `unwrap`/`expect` in non-test code are justified.
- **Lints.** `Cargo.toml` gains a `[lints]` table: `unsafe_code` and
  `clippy::all` deny, `clippy::pedantic` warn with a short allow-list,
  plus `clippy.toml` (`doc-valid-idents: BitchX`). Pedantic warnings went
  from 174 to 0. Formatting now checked with stable rustfmt in CI.
- **Website redesign.** The project page is pure UTF-8 text: one `<pre>`,
  box-drawing and block glyphs, lines within 72 columns, a barely-there
  dark page (#000/#fff, monospace, 16px), no graphics, no web fonts
  (JetBrains Mono and `site/fonts` are gone). URLs are clickable links.
  Adds install (deb, rpm, tarball, musl, macOS, Windows) and verify
  (SHA256SUMS, `gh attestation verify`) sections.

### Added

- `SECURITY.md`: security policy with private vulnerability reporting,
  scope and release verification; linked from README and CONTRIBUTING.

### Fixed

- Changelog compare links: `[Unreleased]` now compares `v0.1.1...HEAD`,
  and `[0.1.1]` links to its release tag.

## [0.1.0] — planned first release

### Added

**Frontend (the point of the fork)**

- A BitchX-style frontend replacing iamb's Vim/modalkit UI: full-screen
  layout with scrollback from row 0, two blue status lines at the bottom and
  a `[#channel]` input line.
- IRC vocabulary throughout: `/join` `/part` `/msg` `/query` `/me` `/topic`
  `/invite` `/kick` `/op` `/deop` `/mode` `/away` `/back` `/nick` `/set`
  `/window` `/next` `/prev` `/clear` `/ignore` `/list` `/names` `/who`
  `/whois` `/help` `/version` `/quit`.
- Slash commands are dispatched onto iamb's own actions rather than
  reimplemented.
- BitchX message formats taken from the BitchX 1.3 sources: the space glyph
  (no `:-`), join/part/notice/action/public colours, `iamBitchX:` error
  lines, `[Users(#chan:n/m)]` names grids, the `/whois` ruler, `Starting
  conversation with …`, and the `[09:24pm][@nick] [#room]` line format.
- Windows: `/window split on|off|toggle` shows the split top bar; the
  numbered room list is grouped by homeserver.

**Encryption**

- `/encrypt (y|n)` to enable encryption for a room, `+e` on a room line.
- SAS emoji verification: `/verify` (to-device and in-room), `/devices`,
  `/trust`, `/recover` (masked secrets). A verification flow is kept per
  user; an ambiguous `/verify <cmd> <user>` refuses and lists the flows.
- `--smoke` prints an encryption report (cross-signing status, device list).

**Login and sessions**

- OAuth 2.0 device authorization grant ([d]evice link, RFC 8628) as an
  alternative to the password login.
- Proactive token refresh 60 s before the access token expires, adopted
  from the rotated `session.json` under the session lock; the existing
  401-recovery path is unchanged.
- Atomic `session.json` writes, a read-only `--smoke` mode, and live
  recovery from `M_UNKNOWN_TOKEN` (reload the session file, refresh under
  the lock, or relogin).

**Presence, away and nick**

- Peers' last-seen next to the away marker in the query status bar and
  `/names`, a `WhoisMark` row and a BitchX `WHOIS_IDLE` row in `/whois`.
- `AUTO_AWAY` / `AUTO_AWAY_TIME` (`/set`): the idle timer sets the presence
  to unavailable, the first keypress ends only automatic aways, and
  `You were /away for …` is reported on the way back.
- `AWAY_NICK`, `AWAY_MSG` and `AUTO_AWAY_MSG` (`/set`): going away swaps
  the display name and coming back restores the exact original.
- `/sync set_presence` follows the away state (unavailable while away,
  online otherwise).

**IRC details**

- `/channels` (with hidden `/rooms`) and `/list` for the server's public
  channel directory; `/join #x` also works with `matrix.to` links.
- `/set` with `TIMESTAMP` and `TIMESTAMP_STRING`, rendered per line from
  the event timestamp.
- Mention highlighting (BitchX `AUTO_REPLY`): word-boundary matching on
  localpart, `@localpart`, MXID and display name, own lines excluded, one
  BEL unless `/set beep off`.
- `/leave` as an alias of `/part`.

**Tooling, release and supply chain**

- Release artifacts with SLSA provenance and SBOM attestations,
  `SHA256SUMS`, raw binaries and a static musl check; the publish job is
  draft-only and gated behind `workflow_dispatch`.
- CI produces an SBOM (`cargo-cyclonedx`) and a vulnerability report
  (`cargo-audit`); CodeQL runs on Rust with `build-mode: none`.
- Every GitHub action is SHA-pinned, workflow permissions are least
  privilege (`contents: read` everywhere, `contents: write` only in the
  publish job), and a local structural YAML check needs no parser.
- A plain-text log file (no ANSI escapes).

**Splash and identity**

- The client calls itself iamBitchX: banner, `iamBitchX:` prefix, logo,
  `/version`, `/help` and the top bar.
- The startup splash is the **original BitchX 1.3 art**, decoded from CP437
  to Unicode — the original client ships raw CP437 bytes, so a UTF-8
  terminal has always shown mojibake. The 3D letters are drawn with
  overlapping segments positioned by ANSI cursor jumps, so the layout is
  replayed rather than copied.
- The logo is picked by the terminal's capability and size: the original
  art (78 columns, ~36 rows), the original "acidjazz" ASCII art, a
  compact block wordmark, or the original figlet slant. Resizing the
  terminal re-picks it.
- The splash credits the base code (`iamb.chat`, Ulyssa) and the artists
  of the artwork (`PhonyEye`, `Awe/Cia`).

**Docs and site**

- A new README (the "Why BitchX?" and "History" texts are quoted from
  [prime001/BitchX](https://github.com/prime001/BitchX) and credited), a
  rewritten `NOTICE`, and `COPYRIGHT-BITCHX` for the artwork's BSD-3
  notice.
- `docs/project.md`: the working notes — the CP437 story, the logo tiers,
  the history rewrite and the lessons.
- A project page at <https://jpzk.github.io/iambitchx/>, set in JetBrains
  Mono (SIL OFL 1.1, self-hosted).

### Changed

- Nicks are the MXID localpart, never the display name.
- The repository history was rewritten to remove personal names from the
  codebase and from every commit (content, messages and identities).
  Existing clones need to be refreshed (`git fetch --force`).
- Metadata: authors, repository and homepage now point at
  `jpzk/iambitchx`; the description says what the project is.
- Message formats, colours and status lines follow BitchX 1.3 rather than
  iamb's Vim UI.

### Fixed

- Display bugs found against the BitchX recon captures: case twins on the
  same server stay bare (F28), status display fixes (F29–F31),
  `percent_decode` no longer panics on `%` before a multi-byte character
  in `/join` links (F32), a broken sync loop leaves a persistent
  `[SYNC LOST]` marker (F37), and the verification flow fixes (F44).
- `/msg` reuses an open DM window instead of opening a second one.
- Starting the client no longer waits on a cursor-position query (which
  timed out on some terminals).
- The log file contains no ANSI escapes.

### Security

- Dependency bumps: **matrix-sdk 0.20.0** (ruma 0.18, rusqlite 0.41 —
  fixes RUSTSEC-2026-0459), vodozemac 0.11.2 (RUSTSEC-2026-0510, -0445,
  -0451), ruma 0.17.2 (RUSTSEC-2026-0589), tokio 1.54.1, url 2.5.9,
  rustix 1.1.6, ctutils 0.4.8.
- Workflow permissions reduced to least privilege; the release publish job
  is the only one that can write.
- Release builds are attestable: SLSA provenance, SBOM, checksums.

### Removed

- The iamb/modalkit (Vim) frontend, and the legacy `iamb/` directory with
  its `iamb://` and `frontend="iamb"` fallbacks.
- The `nix-flake-test` CI job and the two cachix actions (iamb leftovers —
  no cache or secret exists for this repository).
- A token-coordination refactor was reverted: the backend stays as it was.

### Credits

- **iamb** by Ulyssa — <https://iamb.chat> — Apache-2.0: the Matrix engine,
  sync loop, E2EE stack and store that iamBitchX is built on.
- **BitchX** by panasync (Colten Edwards) and contributors — the artwork
  (BSD-3-Clause) and the look and the attitude. iamBitchX shares no code
  with it.
- **matrix-rust-sdk** — Apache-2.0.

[Unreleased]: https://github.com/jpzk/iambitchx/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/jpzk/iambitchx/releases/tag/v0.1.2
[0.1.1]: https://github.com/jpzk/iambitchx/releases/tag/v0.1.1
[0.1.0]: https://github.com/jpzk/iambitchx/releases/tag/v0.1.0
