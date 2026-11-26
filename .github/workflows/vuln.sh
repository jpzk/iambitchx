#!/usr/bin/env bash
# vuln.sh <out.json>: cargo-audit (RustSec) over Cargo.lock as JSON.
# Never fails: findings -> report, scanner error -> error-JSON report.
set -uo pipefail
out=${1:?out}
mkdir -p "$(dirname "$out")"
cargo audit --json > "$out.tmp" 2> audit.stderr
rc=$?
if jq -e . "$out.tmp" >/dev/null 2>&1; then
  mv "$out.tmp" "$out"
  n=$(jq '.vulnerabilities.count // 0' "$out")
  echo "cargo-audit rc=$rc, vulnerabilities=$n"
  [ "$n" = 0 ] || echo "::warning::cargo-audit found $n vulnerabilities (see vuln.json)"
else
  jq -n --arg rc "$rc" --arg err "$(tail -c 4000 audit.stderr)" \
    '{scanner:"cargo-audit",version:"0.22.2",status:"error",exit_code:($rc|tonumber),stderr:$err}' > "$out"
  echo "::warning::cargo-audit failed (rc=$rc); wrote error report"
fi
rm -f "$out.tmp" audit.stderr
exit 0
