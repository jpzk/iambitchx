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

### Changed

- The project page is pure UTF-8 text: one `<pre>`, box-drawing and block
  glyphs, every line within 72 columns. Monochrome: no CSS, no colours,
  no links (URLs are plain text), no graphics, no web fonts
  (JetBrains Mono and `site/fonts` are gone). Adds install (deb, rpm,
  tarball, musl, macOS, Windows) and verify (SHA256SUMS,
  `gh attestation verify`) sections.

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

[Unreleased]: https://github.com/jpzk/iambitchx/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/jpzk/iambitchx/releases/tag/v0.1.0
