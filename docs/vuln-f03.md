# F03 — RUSTSEC-2026-0589: ruma-identifiers-validation key-ID panic

Status: analysis only. Nothing in `Cargo.toml`, `Cargo.lock` or `src/` was
changed. Data was collected on 2026-11-26 from the crates.io API, the crate
tarballs, and the local advisory-db checkout (commit c9f6c91, 2026-11-26).

## Summary

- Vulnerable: `ruma-identifiers-validation 0.13.0` (the `Cargo.lock` entry `name = "ruma-identifiers-validation"`; it was line 3816 at commit 91dbfe5, and the line number moves as vuln-lock edits the lock), pulled
  in by `matrix-sdk 0.19.1` -> `ruma 0.17.1` -> `ruma-common 0.20.1`.
- Bug: `key_id::validate` stores the byte index of the `:` as `as u8`. If the
  algorithm part (before the colon) is longer than 255 bytes, the index wraps.
  Slicing `s[idx+1..]` then panics when the wrapped index falls inside a
  multi-byte UTF-8 character ("byte index is not a char boundary").
  Upstream source: `ruma-identifiers-validation-0.13.0/src/key_id.rs`, lines 6–9.
- Fixed in `0.14.0`, which keeps the index as `usize`.
- Smallest fix: a **lock-only** bump, `ruma 0.17.1 -> 0.17.2`. Cargo.toml does
  not change and matrix-sdk stays at 0.19.1. The catch: every fixed release is
  newer than the 6-week cutoff (2026-10-15). The earliest one can be adopted on
  **2026-12-05**.

## (a) Fixed version + publish date

Publish dates from crates.io (`created_at`, UTC). A release can be adopted 42
days after it is published.

- `ruma-identifiers-validation 0.14.0` — 2026-10-24 — first patched version.
  Can be adopted from 2026-12-05.
- `ruma-identifiers-validation 0.14.1` — 2026-11-16. From 2026-12-28.
- `ruma-common 0.20.2` — 2026-10-24. Requires r-i-v `^0.14.0` and
  ruma-macros `=0.20.2`.
- `ruma-macros 0.20.2` — 2026-10-24. Requires r-i-v `^0.14.0`.
- `ruma 0.17.2` — 2026-10-24. Requires ruma-common `^0.20.2`, ruma-events
  `^0.35.2` and ruma-client-api `^0.25.2`, all published 2026-10-24.
- `ruma 0.18.0` / `ruma-common 0.21.0` — 2026-11-16. From 2026-12-28.
- `matrix-sdk 0.20.0` — 2026-11-25. Requires ruma `^0.18.0`. From 2027-01-06.
- `matrix-sdk 0.19.1` (current) — 2026-09-18. Requires ruma `^0.17.0`. All of
  matrix-sdk-base, -crypto, -common and -sqlite 0.19.1 also require
  `ruma ^0.17.0`.

So the release line that pulls a fixed version is **ruma 0.17.2**. matrix-sdk
0.19.1 already accepts it, and no matrix-sdk release is needed. As of today
(2026-11-26) **no fixed version passes the 6-week rule**. 0.13.0 and every
earlier version are affected (`< 0.14.0`).

Side finding for the vuln-lock goal and the operator: the committed lock's
`ruma 0.17.1`, `ruma-common 0.20.1` and `r-i-v 0.13.0` were published
2026-10-19. They also break the 6-week rule until 2026-11-30.

## (b) [patch] / semver-compatible paths

1. **Semver-compatible, lock-only (preferred).**
   `matrix-sdk 0.19.1` requires `ruma ^0.17.0`, so `ruma 0.17.2` fits.
   r-i-v jumps from 0.13 to 0.14, which is a breaking bump in 0.x. That is
   safe here because only `ruma-common` and `ruma-macros` depend on r-i-v (see
   the Cargo.lock `dependencies` lists), and both move to 0.20.2 together.
   matrix-sdk does not call r-i-v directly. The 0.14.0 API break is only the
   return type of `key_id::validate`, and ruma-common 0.20.1 ignores that value
   anyway: `ruma-common-0.20.1/src/identifiers/key_id.rs` re-finds the colon
   itself (line 103).
   The changelogs for ruma 0.17.2, ruma-common 0.20.2, ruma-events 0.35.2 and
   ruma-client-api 0.25.2 list only additions, plus the r-i-v bump.
   **Risk: low.**
2. **`[patch.crates-io]` with a vendored backport (possible, not
   recommended).** Copy `ruma-identifiers-validation 0.13.0` into
   `vendor/` and replace the body of `key_id::validate` (keep the signature):

   ```rust
   let idx = s.find(':').ok_or(Error::MissingColon)?;
   if idx == 0 { return Err(Error::MissingColon); }
   K::validate(&s[idx + 1..])?;
   NonZeroU8::new(idx.min(255) as u8).ok_or(Error::MissingColon)
   ```

   Then add to Cargo.toml
   `[patch.crates-io] ruma-identifiers-validation = { path = "vendor/ruma-identifiers-validation" }`.
   This brings in no new third-party code, so it does not conflict with the
   6-week rule. Downsides:
   - It edits Cargo.toml and Cargo.lock, which collides with vuln-lock.
   - It adds a fork we have to maintain.
   - cargo-audit stops matching the path crate. That silences F03 even if the
     backport is wrong.

   Only worth it if the 9 days until 2026-12-05 are not acceptable.
3. **matrix-sdk 0.20.0 / ruma 0.18.** Fixed too, but it is a breaking upgrade
   (see (d)) and cannot be adopted until 2027-01-06. Not needed for F03.

## (c) Call paths in iambitchx (where remote key IDs get parsed)

iambitchx never names a `KeyId` type itself (`grep -rn KeyId src` returns
nothing). Every parse happens inside matrix-sdk/ruma when server JSON is
deserialized. An attacker needs a 256+ byte algorithm name containing a
multi-byte character in a key ID that the client deserializes. Anyone who
controls a homeserver, or can get odd device or cross-signing keys uploaded
there, can do that. They only need to share an encrypted room with the victim
or be someone the victim looks up.

Main sink: the `/keys/query` response. `ruma-client-api 0.25.1`
`keys/get_keys.rs` returns `Raw<DeviceKeys>` / `Raw<CrossSigningKey>`.
matrix-sdk-crypto then deserializes those (`identities/manager.rs` lines 351
and 642). Their `keys: BTreeMap<OwnedDeviceKeyId|OwnedCrossSigningKeyId, _>`
and `signatures` maps run `key_id::validate` on every key.

Reachable entry points in this repo:

1. **Sync task (main path, runs automatically).** `src/tui.rs:69` calls
   `matrix::spawn_sync`. That reaches `tokio::spawn` at `src/matrix.rs:339`,
   then `sync_with_callback` at `src/matrix.rs:402`. On every sync, matrix-sdk
   runs `send_outgoing_requests()` (matrix-sdk-0.19.1 `src/client/mod.rs`,
   lines 3193/3222), which sends and parses `/keys/query` for tracked users
   (room members of encrypted rooms whose device lists changed). No user action
   is needed.
2. **Event handlers inside the sync task.** The to-device verification handler
   at `src/e2ee.rs:38` and the in-room request handler at `src/e2ee.rs:50` run
   during sync processing, so the same task can hit the panic.
3. **User commands (spawned effect tasks).** `src/tui.rs:123` ->
   `matrix::spawn_effect` -> `tokio::spawn` at `src/matrix.rs:723`. This path
   covers:
   - `/devices` -> `get_user_devices` / `get_user_identity` (`src/e2ee.rs:279`,
     `src/e2ee.rs:280`)
   - `/verify` -> `get_device` / `get_user_identity` (`src/e2ee.rs:229`,
     `src/e2ee.rs:242`)
   - `/recover` -> `bootstrap_cross_signing_if_needed` (`src/e2ee.rs:362`)

   These either wait for the sync loop's key query or start their own.
4. **`--smoke` mode.** `client.sync_once` at `src/smoke.rs:62` runs directly
   inside the main future (`rt.block_on` at `src/main.rs:114`), not in a
   spawned task.

Other surfaces I did not trace fully: `m.key.verification.mac` key maps and
olm to-device payloads. They pass through the same sync task, so their blast
radius is the same as path 1.

## Panic blast radius

Panic strategy per profile (`Cargo.toml`):

- `release` (CI binaries, `.github/workflows/binaries.yml:78`) has no override,
  so it uses **unwind**.
- `release-lto` (`Cargo.toml:64`, used for the official release artifacts in
  `.github/workflows/release.yml:70`) inherits release, so it also uses
  **unwind**.
- `release-light` (`Cargo.toml:69`) sets `panic = "abort"` at `Cargo.toml:73`.
  README advertises it for small builds.

Consequences:

- **Unwind builds (release, release-lto, dev).** The runtime is multi-thread
  (`src/main.rs:104`). Tokio catches a panic inside a spawned task and ends
  only that task. The `JoinHandle` from `src/matrix.rs:339` is dropped, so
  nobody sees the `JoinError`.
  - The sync loop dies **silently**. The `Err` branches that print
    `sync: …` never run, so no new messages, invites or key updates arrive,
    while the UI stays up and looks connected.
  - The panic hook at `src/tui.rs:37` calls `restore()` (`src/tui.rs:27`) on
    **any** thread's panic. That leaves the alternate screen and turns off raw
    mode while the event loop keeps drawing, so the terminal is garbled and
    keystrokes echo. The client is effectively unusable even though the
    process is alive.
  - Likely persistent (inferred from the code path, not reproduced): the
    keys/query response never gets stored, so the user stays marked outdated
    in the crypto store. The next sync or the next launch re-queries and
    panics again, a crash loop until the attacker's keys change or the shared
    room is left.
  - In `--smoke` mode the panic unwinds out of `block_on` (`src/main.rs:114`)
    and the process exits.
  - Effect tasks (`src/matrix.rs:723`) lose only that one command, but the
    hook still breaks the terminal.
- **Abort builds (release-light).** Any panic in any task **aborts the whole
  process** at once. The terminal is restored by the hook first, but there is
  no clean shutdown. With the persistence above, the client crashes on every
  start.

Impact class: remote, unauthenticated-to-you denial of service. No memory
corruption: it is a checked slice panic. Confidentiality and integrity are not
affected.

## (d) Upgrade options — size/risk (from changelogs, no builds)

- **A. `ruma 0.17.1 -> 0.17.2` (lock-only).**
  - Moves 6 crates by a patch or minor step: ruma, ruma-common, ruma-events,
    ruma-client-api, ruma-macros, and r-i-v 0.13 -> 0.14.
  - Changelogs list only additive changes (`UserProfile::remove`, MSC4536
    profile query) and the validation fix.
  - No source changes expected. MSRV is unchanged (1.89 for ruma; matrix-sdk
    stays at 1.96).
  - Risk: **low**. Can be adopted on **2026-12-05**.
- **B. matrix-sdk 0.19.1 -> 0.20.0 (+ ruma 0.18, ruma-events 0.36,
  ruma-client-api 0.26).**
  - The matrix-sdk 0.20.0 changelog runs 576 lines, with 106 entries and 18
    marked `[breaking]` (QR login types, `http_client()` return type, event
    cache API, send queue handles, cross-signing reset auth).
  - At least `src/device_link.rs:461` (`client.http_client()`) is directly
    affected, and `qrcode`/`sso-login` features are enabled.
  - Needs Cargo.toml + lock + src changes.
  - Risk: **medium-high**. Can be adopted on **2027-01-06**. Out of scope for
    F03; plan it separately.

## (e) Interim mitigation (until 2026-12-05)

Pick any. None of them is applied by this analysis.

1. **Zero-code:** do not build or distribute `release-light` (abort) until the
   fix lands. The official artifacts (release-lto) already unwind.
2. **Small src patch (~20 lines, needs operator OK), contains the DoS:**
   - In `src/tui.rs:37`, restore the terminal only for panics on the main
     thread. Panics on `iambitchx-worker-*` threads should be logged via
     `tracing::error!` instead.
   - Keep the `JoinHandle` from `src/matrix.rs:339` and await it in a tiny
     supervisor. On `JoinError::is_panic()`, send
     `Line::Error("sync crashed: …")` to the UI, then restart sync with
     backoff, max 3 tries.

   Result: the user is told, instead of a silent freeze or a broken terminal.
3. **Actual fix before 12-05:** the vendored `[patch]` backport from (b)2.

## Recommendation

1. **Fix:** on or after **2026-12-05**, apply the lock-only bump. Coordinate
   with vuln-lock so only one goal edits Cargo.lock at a time:

   ```sh
   cargo update -p ruma@0.17.1 --precise 0.17.2
   ```

   Expected lock diff, and nothing else:
   - ruma 0.17.2
   - ruma-common 0.20.2
   - ruma-events 0.35.2
   - ruma-client-api 0.25.2
   - ruma-macros 0.20.2
   - ruma-identifiers-validation 0.14.0

   Then check:
   - `cargo audit` no longer reports RUSTSEC-2026-0589
   - `cargo test --locked` passes
   - `git diff --stat` touches only Cargo.lock
2. **Until then:** interim (e)1 now (no code), and optionally (e)2 as a
   defence-in-depth change worth keeping after the fix. Any future panic in the
   sync task has the same silent-death and broken-terminal effect.
3. **Do not** take matrix-sdk 0.20.0 for this advisory.

**Change the operator must OK:**
`cargo update -p ruma@0.17.1 --precise 0.17.2` (Cargo.lock only), scheduled
for 2026-12-05 or later. Optionally also the (e)2 patch to `src/tui.rs` and
`src/matrix.rs`.
