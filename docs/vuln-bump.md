# vuln-bump — RustSec fixes by version bump (goal vuln-bump, 2026-11-26)

Operator: 6-week age rule WAIVED for this project (fresh releases allowed).
Advisory DB: sec-audit/advisory-db @ c9f6c91 (2026-11-26). Scanner:
/workspace/goal/sec-audit/scan-advisories.sh. Publish dates: crates.io API
(`/api/v1/crates/<c>/versions`, fetched 2026-11-26, raw JSON kept in
/workspace/goal/vuln-bump/api/).

Scan outputs (in /workspace/goal/vuln-bump/):
- scan-before-a1b7a4b.txt — lock before vuln-lock + vuln-bump (12 findings)
- scan-before.txt — lock after vuln-lock (aa6f2cc), before this goal (7 findings)
- scan-after.txt — lock after this goal (4 findings, 1 vulnerability)

No src/ change, no Cargo.toml change. Every bump is `cargo update -p X --precise V`,
one at a time.

## Bumps (old -> new, publish date UTC)

Committed earlier by goal vuln-lock (aa6f2cc, details in docs/vuln-lock-versions.md):
- vodozemac 0.11.1 -> 0.11.2 — 2026-11-05 — RUSTSEC-2026-0510, -0445, -0451
- tokio 1.54.0 -> 1.54.1 — 2026-10-21 — RUSTSEC-2026-0384 (1.54.0 also yanked)
- url 2.5.8 -> 2.5.9 — 2026-11-20 — RUSTSEC-2026-0592
- form_urlencoded 1.2.2 -> 1.2.3 — 2026-11-20 — forced by url 2.5.9 (^1.2.3)

This goal:
1. `cargo update -p ruma --precise 0.17.2` — fixes RUSTSEC-2026-0589 (see docs/vuln-f03.md)
   - ruma 0.17.1 -> 0.17.2 — 2026-10-24
   - ruma-client-api 0.25.1 -> 0.25.2 — 2026-10-24 (transitive)
   - ruma-common 0.20.1 -> 0.20.2 — 2026-10-24 (transitive)
   - ruma-events 0.35.1 -> 0.35.2 — 2026-10-24 (transitive)
   - ruma-identifiers-validation 0.13.0 -> 0.14.1 — 2026-11-16 (transitive; patched >= 0.14.0)
   - ruma-macros 0.20.1 -> 0.20.2 — 2026-10-24 (transitive; 0.20.3 available, not taken)
2. `cargo update -p ctutils --precise 0.4.8` — fixes RUSTSEC-2026-0385 (patched >= 0.4.4)
   - ctutils 0.4.3 -> 0.4.8 — 2026-11-02. Took latest 0.4.x patch rather than the
     minimal 0.4.4 (2026-10-23): 0.4.5-0.4.8 are follow-up patch releases of the
     same minor, semver-compatible, nothing else in the lock moved.
3. `cargo update -p rustix --precise 1.1.6` — fixes RUSTSEC-2026-0486 [unsound]
   - rustix 1.1.5 -> 1.1.6 — 2026-10-27 (patched >= 1.1.6)

No other RUSTSEC vulnerability/unsound entry is fixable lock-only (see below).

## Remaining findings (scan-after) and why

- libsqlite3-sys 0.38.2 RUSTSEC-2026-0459 [vulnerability] — NOT fixable lock-only
  or Cargo.toml-version-only within matrix-sdk 0.19. Plan below.
- anymap2 0.13.0 RUSTSEC-2026-0319 [unmaintained] — documented, not replaced
  (operator: unmaintained-only advisories are not replaced). No vuln, no fix version.
- chrono 0.4.45 RUSTSEC-2026-0543 [unmaintained] — documented, not replaced.
- derivative 2.2.0 RUSTSEC-2024-0388 [unmaintained] — documented, not replaced.

## libsqlite3-sys RUSTSEC-2026-0459 — plan for the operator (NOT done)

Advisory: bundled SQLite < 3.54.0 can return uninitialized page-cache memory when a
db is opened together with a crafted `-wal` file of a different page size.
CVSS 4.0 AV:L/UI:P, VC:H. Patched: libsqlite3-sys >= 0.39.0 (SQLite 3.54.0).
iambitchx is affected because `default = ["bundled"]` -> matrix-sdk/bundled-sqlite.

Why no lock-only / Cargo.toml-only path exists (checked on crates.io 2026-11-26):
- chain: matrix-sdk 0.19.1 -> matrix-sdk-sqlite 0.19.1 -> rusqlite `^0.40.2`
  -> libsqlite3-sys `^0.38.2`.
- rusqlite 0.40.x: 0.40.2 is the last 0.40 release (2026-08-08); there is no 0.40.x
  on libsqlite3-sys 0.39. rusqlite 0.41.0 (2026-10-24) is the first with `^0.39.0`.
- matrix-sdk-sqlite 0.19.x: only 0.19.0 / 0.19.1 exist, both `rusqlite ^0.40.2`.
  No 0.19.2. 0.20.0 (2026-11-25) is the first with `rusqlite ^0.41.1`.
- iambitchx has no direct rusqlite/libsqlite3-sys dependency, so a version
  requirement in Cargo.toml cannot lift it; only `[patch.crates-io]` with a forked
  matrix-sdk-sqlite would, which is out of scope (unreviewed fork).

Fix path = matrix-sdk 0.20 (forbidden by this goal's rules -> operator decision):
1. Cargo.toml: `[dependencies.matrix-sdk] version = "0.19.0"` -> `"0.20.0"`
   (published 2026-11-25; rust-version 1.96 = same as iambitchx, rustc 1.96.1 OK).
2. Lock moves (expected): matrix-sdk{,-base,-common,-crypto,-sqlite,-store-encryption,...}
   0.19.1 -> 0.20.0; ruma 0.17.2 -> 0.18.0 (ruma-common 0.21.0, ruma-events 0.36.0,
   ruma-client-api 0.26.0, ruma-identifiers-validation 0.14.1); rusqlite 0.40.2 -> 0.41.1
   (2026-10-28); libsqlite3-sys 0.38.2 -> 0.39.x (0.39.1, 2026-10-25, SQLite 3.54.1).
3. Breaking changes from the 0.20 / ruma 0.18 CHANGELOGs, vs. what src/ uses
   (grep of every `matrix_sdk::`/`ruma::` path in src/):
   - matrix-sdk: LoginWithQrCode/GrantLogin* lifetimes, CrossSigningResetHandle::auth
     required arg, SendReactionHandle/SendRedactionHandle removed, RoomSendQueue::send_raw,
     SecureChannelError split, subscribe_to_ignore_user_list_changes -> Vec<OwnedUserId>,
     Client::event_cache -> Option, Room::event_cache removed, search ordering,
     send_state_event_raw, msc_4388 gating.  -> grep: NONE of these are used in src/.
   - matrix-sdk-base: RoomInfo/Room::fully_read_event_id removed,
     RoomInfoNotableUpdateReasons::FULLY_READ removed, StickyEventsUpdate::removed type.
     -> not used in src/.
   - ruma-common 0.21: OAuthClientScope From<str> -> TryFrom<str>; IdDst repr;
     serde::ignore_invalid_vec_items renamed. -> not used in src/.
   - ruma-events 0.36: ToDeviceForwardedRoomKeyEventContent.shared_history removed;
     StaticStateEventContent bound. -> not used.
   - ruma-client-api 0.26: get_media_config upload_size Option, sync v5 -> msc4186 renames,
     unstable module renames. -> not used (src uses presence::{get,set}_presence::v3,
     directory::get_public_rooms_filtered::v3, profile::DisplayName — unchanged per log).
   src files that touch the matrix-sdk/ruma API and must be compile-checked:
   src/matrix.rs, src/login.rs, src/oauth.rs, src/device_link.rs, src/e2ee.rs,
   src/session_guard.rs, src/smoke.rs, src/config.rs, src/tui.rs,
   src/bitchx/app.rs, src/bitchx/e2ee.rs, src/bitchx/model.rs, src/bitchx/ui.rs.
   Highest-risk: src/oauth.rs + src/login.rs (authentication::oauth::{registration,
   ClientId, OAuthAuthorizationData, OAuthSession, UserSession}, utils::local_server,
   OAuth flows changed to stable `urn:matrix:client:api:*` scopes in 0.20) and
   src/device_link.rs + src/e2ee.rs (encryption::verification, deserialized_responses::
   {WithheldCode, UnableToDecryptReason}).
   Changelog review predicts zero or few src breaks, but that is a prediction, not a
   compile result: the step is `cargo check` on a scratch branch, then fix whatever
   breaks, then bx-ci.sh testall + a live login/E2EE smoke test.
4. Disk: a matrix-sdk 0.20 build roughly doubles the matrix-sdk artifacts in target/
   (~1.2G free now) — run with the bx-ci disk guard or clean old matrix-sdk 0.19
   artifacts first.

Interim mitigation without any change: build with `--no-default-features` (drops
`bundled`, links system SQLite) on hosts whose system SQLite is >= 3.54.0. The lock
still carries libsqlite3-sys 0.38.2, so scanners keep flagging it. Exposure is local
only: an attacker must be able to write the session store db + `-wal` under the
user's data dir, which already implies access to the session tokens stored there.
