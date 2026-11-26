# Releasing iambitchx

Pipeline: `.github/workflows/release.yml`. Supply-chain pins and job
permissions: `docs/ci-supply-chain.md`.

## Prove the pipeline (no tag, no release)

Actions → Release → Run workflow on `main`, `dry_run` = true (default).
Or via the REST API:

    curl -X POST -H "Authorization: Bearer $GH_TOKEN" \
      https://api.github.com/repos/jpzk/iambitchx/actions/workflows/release.yml/dispatches \
      -d '{"ref":"main","inputs":{"dry_run":"true"}}'

It runs build (all targets) + SBOM + checksums + attestations and leaves
everything as workflow artifacts (`release-dist` holds the attested set).
Files are named `dev-<commit sha>` instead of a version. The publish job is
skipped. Attestations ARE created (they are bound to file digests, not to a
release), so `gh attestation verify` works on the dry-run artifacts too.

## Cut a release

1. bump `version` in Cargo.toml (and `[package.metadata.generate-rpm]`),
   commit, merge to `main`, CI green.
2. tag and push:

       git tag -s vX.Y.Z -m vX.Y.Z
       git push origin vX.Y.Z

3. the tag push runs release.yml: build → sbom → attest → publish. publish
   creates a DRAFT release `vX.Y.Z` and uploads the assets.
4. review the draft (assets, SHA256SUMS, notes), then publish it.

## Release assets

For X.Y.Z and each target in x86_64-unknown-linux-musl (static),
aarch64-unknown-linux-gnu, x86_64-apple-darwin, aarch64-apple-darwin,
x86_64-pc-windows-msvc:

- `iambitchx-X.Y.Z-<target>` (`.exe` on windows) — bare binary
- `iambitchx-X.Y.Z-<target>.tar.gz` — binary + man pages, desktop files
- linux only: `iambitchx_*.deb`, `iambitchx-*.rpm`
- `iambitchx-X.Y.Z.cdx.json` — CycloneDX 1.5 SBOM (from Cargo.lock)
- `vuln.json` — cargo-audit RustSec report at build time
- `SHA256SUMS` — sha256 of every file above

Attestations (stored by GitHub, not as assets; signed via Sigstore with
the workflow's OIDC identity):

- SLSA build provenance for every file above
- SBOM attestation: the cdx bound to the x86_64 linux binary and its tgz

## Verify a download

Checksums (download SHA256SUMS next to the files):

    sha256sum -c --ignore-missing SHA256SUMS

Provenance — proves the file was built by this repo's release workflow
(needs the GitHub CLI, `gh` 2.49+):

    gh attestation verify iambitchx-X.Y.Z-x86_64-unknown-linux-musl \
      --repo jpzk/iambitchx

Stricter: pin the workflow and the tag ref:

    gh attestation verify iambitchx-X.Y.Z-x86_64-unknown-linux-musl \
      --repo jpzk/iambitchx \
      --signer-workflow jpzk/iambitchx/.github/workflows/release.yml \
      --source-ref refs/tags/vX.Y.Z

SBOM attestation (x86_64 linux binary / tgz):

    gh attestation verify iambitchx-X.Y.Z-x86_64-unknown-linux-musl \
      --repo jpzk/iambitchx --predicate-type https://cyclonedx.org/bom

Without gh, the raw bundles are at
`https://api.github.com/repos/jpzk/iambitchx/attestations/sha256:<digest>`.
