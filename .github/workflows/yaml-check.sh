#!/usr/bin/env bash
# Minimal structural YAML check for workflows (no parser available locally):
# no tabs, even indentation, balanced ${{ }} and quotes outside block scalars,
# required top-level keys, every job has runs-on, every uses: is SHA-pinned.
set -u; rc=0
for f in "${@:-.github/workflows/*.yml}"; do
  awk -v F="$f" '
    function err(m){ printf "%s:%d: %s\n", F, NR, m; bad=1 }
    /\t/ { err("tab") }
    { line=$0; sub(/^ */,"",line); ind=length($0)-length(line) }
    blk && (line=="" || ind>blkind) { next }   # inside block scalar
    { blk=0 }
    line=="" || line ~ /^#/ { next }
    ind%2 { err("odd indent " ind) }
    { o=gsub(/\$\{\{/,"&",line); c=gsub(/\}\}/,"&",line); if (o!=c) err("unbalanced ${{ }}") }
    { s=line; sub(/ #.*/,"",s); if (gsub(/"/,"&",s)%2) err("unbalanced double quote") }
    /^[^ #][^:]*:/ { k=$0; sub(/:.*/,"",k); top[k]=1 }
    /^  [A-Za-z0-9_-]+:[ ]*$/ && injobs { job=$0; gsub(/[ :]/,"",job); jobs[job]=1 }
    /^jobs:/ { injobs=1 } /^[^ #]/ && !/^jobs:/ { injobs=0 }
    /^    runs-on:/ { hasrun[job]=1 }
    /uses:/ && !/uses: *[^@ ]+@[0-9a-f]{40} # v?[0-9]/ && !/uses: *\.\// { err("unpinned uses") }
    /: *[|>][-+]?[ ]*$/ { blk=1; blkind=ind }
    END {
      if (!("on" in top)) { printf "%s: missing on:\n",F; bad=1 }
      if (!("jobs" in top)) { printf "%s: missing jobs:\n",F; bad=1 }
      for (j in jobs) if (!(j in hasrun)) { printf "%s: job %s lacks runs-on\n",F,j; bad=1 }
      exit bad }' "$f" || rc=1
done
[ $rc = 0 ] && echo "yaml-check: OK ($*)"
exit $rc
