# vuln-lock — version decisions (cutoff: published <= 2026-10-15)

Source: crates.io API `/api/v1/crates/<c>/versions` (curl -4, 2026-11-26), raw JSON in api/.
Advisory ranges: sec-audit/advisory-db @ c9f6c91 (2026-11-26).

## vodozemac (F01 RUSTSEC-2026-0510, F02 RUSTSEC-2026-0445/0451)
- locked: 0.11.1 (published 2026-09-30)
- patched: >= 0.11.2 for all three; only fix = 0.11.2, published 2026-11-05T14:04:23Z
- 0.11.2 > cutoff (21 days old) -> NOT BUMPED. Becomes eligible 2026-12-17.
- semver: compatible (dry-run: `Updating vodozemac v0.11.1 -> v0.11.2`, nothing else moves)
- no older escape: 0.10.0 is unaffected only by 0445; still hit by 0510/0451, and matrix-sdk 0.19 needs 0.11.

## tokio (F04 RUSTSEC-2026-0384, 1.54.0 yanked)
- locked: 1.54.0 (published 2026-10-19 — itself younger than cutoff, and yanked)
- patched: >= 1.54.1 (2026-10-21T21:05:43Z) -> > cutoff, NOT eligible (eligible 2026-12-02)
- unaffected: < 1.54.0 -> candidate 1.53.3, published 2026-10-13T20:51:46Z (<= cutoff), not yanked
- semver: Cargo.toml tokio = "1.24.1" -> 1.53.3 allowed; dry-run: `Downgrading tokio v1.54.0 -> v1.53.3`, nothing else moves
- decision: pending (lock-only DOWNGRADE, not a bump)

## url (F05 RUSTSEC-2026-0592)
- locked: 2.5.8 (2026-01-05); Cargo.toml pins url >= 2.5.8
- patched: >= 2.5.9, published 2026-11-20T23:03:07Z -> > cutoff (6 days old), NOT BUMPED. Eligible 2027-01-01.

## libsqlite3-sys (F06 RUSTSEC-2026-0459)
- locked: 0.38.2 (2026-08-08)
- patched: >= 0.39.0 (2026-10-24), 0.39.1 (2026-10-25), 0.40.0 (2026-11-11) — all > cutoff
- 0.38 -> 0.39 is a semver-major (0.x) change; needs rusqlite 0.41 / matrix-sdk-sqlite release, i.e. Cargo.toml dep change -> SKIPPED (both reasons).
- mitigation note: only exploitable if a less-trusted party can write the sqlite db + -wal files (local).

## OPERATOR DECISION (2026-11-26, via cs-ask): policy exception granted
"Grant exception for vodozemac 0.11.2 (HIGH) + tokio 1.54.1 + url 2.5.9, log as policy exceptions"

Applied (cargo update -p <c> --precise <v>, one at a time):
- vodozemac 0.11.1 -> 0.11.2   published 2026-11-05T14:04:23Z   POLICY EXCEPTION (21d old)
- tokio     1.54.0 -> 1.54.1   published 2026-10-21T21:05:43Z   POLICY EXCEPTION (36d old)
- url       2.5.8  -> 2.5.9    published 2026-11-20T23:03:07Z   POLICY EXCEPTION (6d old)
- form_urlencoded 1.2.2 -> 1.2.3  published 2026-11-20T23:02:49Z  POLICY EXCEPTION, forced
  transitive (url 2.5.9 requires ^1.2.3; --precise 1.2.2 rejected by resolver). Same rust-url release.
- libsqlite3-sys: NOT bumped (semver-major 0.38->0.39 needs rusqlite/matrix-sdk Cargo.toml change; also young).

NOTE: criterion 3 ("all <= 2026-10-15") is intentionally NOT met for the four above — the
operator's exception overrides it. No other crate versions changed. Side effect: Windows-only
dep edges (errno, quinn-udp, rustix, rustls-platform-verifier, tempfile, tokio, winapi-util)
re-resolved windows-sys 0.61.2 -> 0.52.0. Both versions were already locked, no new crate.
vodozemac 0.11.2 also enables zeroize features on its crypto deps (the 0445 fix).

Advisory scan (sec-audit/scan-advisories.sh, db c9f6c91): before 12 hits, after 7 hits.
Removed: 0384, 0592, 0445, 0451, 0510. Added: none. See scan-before.txt / scan-after.txt.
