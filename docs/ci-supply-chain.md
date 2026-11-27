# CI supply chain pins

Policy (operator):

- every third-party GitHub Action is pinned by full 40-hex commit SHA with a
  `# vX.Y.Z` comment; never `@main` / `@vN`
- the pinned tag/release must be at least 6 weeks old at pin time
- every `cargo install` uses `--version X --locked` with a crates.io release
  at least 6 weeks old
- least-privilege `permissions:` per job (`contents: read` default,
  `contents: write` only on release upload jobs, `security-events: write`
  only on CodeQL)
- no new secrets

Pinned on 2026-11-26 (6-week cutoff: 2026-10-15). Dates verified via the
GitHub API (release `published_at`, tag commit date) and the crates.io API
(`versions[].created_at`).

## GitHub Actions

- actions/checkout v7.0.1
  - SHA 3d3c42e5aac5ba805825da76410c181273ba90b1
  - released 2026-07-20 (commit 2026-07-17)
- actions/upload-artifact v7.0.2
  - SHA cf430e030ddbb5b0abf93d22962f4752f3646cd9
  - released 2026-10-07 (commit 2026-10-07)
- actions/cache v6.1.0
  - SHA 55cc8345863c7cc4c66a329aec7e433d2d1c52a9
  - released 2026-06-26 (commit 2026-06-23)
- actions/download-artifact v8.0.2
  - SHA 9000827ccba6bdab643e8b6fd33ac0654aef8333
  - released 2026-10-07 (commit 2026-10-07)
  - used by release.yml `publish` to collect build/SBOM artifacts
- mozilla-actions/sccache-action v0.0.11
  - SHA fc920bf0ec8de6ee65d409111f7ec508035751ba
  - released 2026-07-29 (commit 2026-07-29)
- cachix/install-nix-action v31.11.1 (REMOVED 2026-11-26 with the nix-flake-test job)
  - SHA 13d8dd58da0234aa297dedd986986ccb8e7f3e24
  - released 2026-08-13 (commit 2026-08-13)
  - v31.11.2+ (2026-10-14 onward) skipped: at/inside the cutoff
- cachix/cachix-action v17 (REMOVED 2026-11-26 with the nix-flake-test job)
  - SHA 38b082610b782e7e93e209c35fd730d399dee866
  - released 2026-03-18; tag commit 2026-09-01 (no patch tags exist)
- github/codeql-action (init, analyze) v4.38.3
  - SHA 24c54180a607b1449ed407dd24f251e4e9147c8d
  - released 2026-10-08 (commit 2026-10-08)
  - v4.39.x (2026-11) skipped: too new
- actions/attest-build-provenance v4.2.2 (added 2026-11-26, release.yml `attest`)
  - SHA 4d101475d8b20a2381f78447822ac1eab6504dd8
  - released 2026-08-06 (commit 2026-08-06)
  - composite; internally pins actions/attest@508db95dd578ae2727ebd6217d5ba78e4fbda05d (v4.2.1, released 2026-07-29)
- actions/attest-sbom v4.1.0 (added 2026-11-26, release.yml `attest`)
  - SHA c604332985a26aa8cf1bdc465b92731239ec6b9e
  - released 2026-03-18 (commit 2026-03-03)
  - composite; internally pins actions/attest@59d89421af93a897026c735860bf21b6eb4f7b26 (v4.1.0, released 2026-02-26)
  - upstream marks it deprecated (prints a `::warning::` asking to use
    actions/attest with `sbom-path`); kept because the operator asked for
    attest-sbom explicitly. Swap path: actions/attest v4.2.x, same inputs.

### Removed third-party actions

- dtolnay/rust-toolchain: has no immutable release tags (`v1` and the
  toolchain branches are moved continuously; `v1` currently points at a
  2026-10-16 commit, inside the cutoff). Replaced by plain
  `rustup toolchain install` (rustup is preinstalled on GitHub runners).
- giraffate/clippy-action: wraps reviewdog, which it downloads at run time
  (unpinned) and needs the GITHUB_TOKEN. Replaced by
  `cargo clippy --all-targets -- -D warnings`.

## cargo install

- cargo-cyclonedx 0.5.9 — published 2026-03-19
  - 0.5.10 (2026-11-06) skipped: too new
  - SBOM: CycloneDX JSON, `--spec-version 1.5`
- cargo-audit 0.22.2 — published 2026-06-05
  - 0.23.0 (2026-11-10) skipped: too new
  - scans Cargo.lock against the RustSec advisory DB, `--json`
- cargo-deb 3.8.0 — published 2026-09-01
- cargo-generate-rpm 0.21.0 — published 2026-05-04
  - 0.21.1 (2026-10-17) skipped: too new

### Removed cargo installs

- cargo-tar: every published version (0.0.1, 0.0.2) is from 2026-10-21,
  inside the cutoff, so it cannot be pinned. The tarball is built by
  `.github/workflows/package-tar.sh` instead, which mirrors
  `[package.metadata.tar]` from Cargo.toml.

## Not pinned (runner-provided)

- rustup / Rust toolchains (`stable`, `1.96` incl. rustfmt) are
  fetched from static.rust-lang.org by rustup, signature-checked by rustup.
- the RustSec advisory DB is fetched fresh by cargo-audit on every run (by
  design: a stale DB defeats the scan).

## release.yml layout (least privilege)

Triggers: `push` of a `v*.*.*` tag, and `workflow_dispatch` with input
`dry_run` (boolean, default `true`). File names use `VERSION` = tag without
`v`, or `dev-<commit sha>` on a dispatch from a branch.

- top-level `permissions: {}`; every job opts in to the minimum.
- `build` matrix (contents: read): x86_64-unknown-linux-musl,
  aarch64-unknown-linux-gnu, x86_64/aarch64-apple-darwin,
  x86_64-pc-windows-msvc. Produces the raw binary
  `iambitchx-<v>-<target>[.exe]`, the tgz, and on linux deb/rpm, as
  workflow artifacts. The x86_64 linux build asserts the binary is static
  (`file` says static(-pie) linked, no PT_INTERP, no DT_NEEDED). Third-party
  build code never sees a write token or an OIDC token.
- `sbom` (contents: read): `iambitchx-<v>.cdx.json` + `vuln.json`; the
  scan never fails the release (error-JSON fallback in `vuln.sh`).
- `attest` (contents: read, id-token: write, attestations: write — the
  only job with these): downloads all artifacts, writes `SHA256SUMS`, then
  - SLSA build provenance (`attest-build-provenance`) over every file in
    dist: binaries, tgz, deb, rpm, cdx, vuln.json, SHA256SUMS
  - SBOM attestation (`attest-sbom`) binding the CycloneDX SBOM to the
    x86_64 musl binary and its tgz
  - uploads the attested set as artifact `release-dist`.
  It runs no build code: only download, sha256sum and the pinned actions.
- `publish` (contents: write — the only job with it): runs only on a `v*`
  tag ref and never for a dispatch with `dry_run=true`; downloads
  `release-dist`, re-checks `sha256sum -c SHA256SUMS`, creates the draft
  release if missing and uploads with `gh release upload --clobber`.
  A dispatch from a branch never publishes, whatever `dry_run` says.
