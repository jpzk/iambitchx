#!/usr/bin/env bash
# sbom.sh <label> <outdir> -> <outdir>/iambitchx-<label>.cdx.json
# CycloneDX JSON (spec 1.5) from Cargo.lock via pinned cargo-cyclonedx.
set -euo pipefail
label=${1:?label}; out=${2:?outdir}
mkdir -p "$out"
cargo cyclonedx --format json --spec-version 1.5
src=$(find . -maxdepth 1 -name '*.cdx.json' -print -quit)
[ -n "$src" ] || { echo "no *.cdx.json produced" >&2; exit 1; }
dst="$out/iambitchx-$label.cdx.json"
mv "$src" "$dst"
jq -e '.bomFormat == "CycloneDX" and (.specVersion | tonumber) >= 1.5' "$dst" >/dev/null
echo "wrote $dst ($(jq '.components | length' "$dst") components)"
