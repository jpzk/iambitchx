#!/usr/bin/env bash
# package-tar.sh <version> <target> <profile> <outdir>
# Replaces cargo-tar (no release old enough to pin, see docs/ci-supply-chain.md).
# Mirrors [package.metadata.tar] in Cargo.toml: assets are read from there, so
# the tarball stays in sync with the manifest.
# -> <outdir>/iambitchx-<version>-<target>.tar.gz
set -euo pipefail
version=${1:?version}; target=${2:?target}; profile=${3:?profile}; out=${4:?outdir}

name="iambitchx-$version-$target"
stage=$(mktemp -d)
root="$stage/$name"
mkdir -p "$root" "$out"

bin=iambitchx
case "$target" in *windows*) bin=iambitchx.exe ;; esac
cp "target/$target/$profile/$bin" "$root/$bin"
chmod 755 "$root/$bin"

# { source = "X", dest = "Y", mode = "Z" } lines inside [package.metadata.tar]
assets=$(sed -n '/^\[package\.metadata\.tar\]/,/^\]/p' Cargo.toml |
  sed -n 's/.*source *= *"\([^"]*\)".*dest *= *"\([^"]*\)".*mode *= *"\([^"]*\)".*/\1|\2|\3/p')
[ -n "$assets" ] || { echo "no [package.metadata.tar] assets found" >&2; exit 1; }

n=0
while IFS='|' read -r src dst mode; do
  shopt -s nullglob
  files=( $src )
  shopt -u nullglob
  [ ${#files[@]} -gt 0 ] || { echo "asset missing: $src" >&2; exit 1; }
  for f in "${files[@]}"; do
    if [[ "$dst" == */ ]]; then t="$root/$dst$(basename "$f")"; else t="$root/$dst"; fi
    mkdir -p "$(dirname "$t")"
    cp "$f" "$t"; chmod "$mode" "$t"
    n=$((n+1))
  done
done <<< "$assets"

tar -C "$stage" -czf "$out/$name.tar.gz" "$name"
rm -rf "$stage"
echo "wrote $out/$name.tar.gz ($n assets + $bin)"
