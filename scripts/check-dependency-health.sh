#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -gt 0 ]]; then
  lockfiles=("$@")
else
  mapfile -t lockfiles < <(
    find . \
      \( -path './.git' -o -path './target' -o -path './fuzz/target' -o -path './node_modules' \) -prune \
      -o -type f -name Cargo.lock -print | sort
  )
fi

for lockfile in "${lockfiles[@]}"; do
  if [[ ! -f "$lockfile" ]]; then
    echo "dependency health: missing $lockfile" >&2
    exit 1
  fi

  if grep -nE '^version = ".*\+deprecated"' "$lockfile"; then
    echo "dependency health: deprecated crate version found in $lockfile" >&2
    exit 1
  fi

  if awk '
  $0 == "name = \"serde_yaml\"" {
    print FILENAME ":" FNR ": deprecated serde_yaml dependency is not allowed"
    found = 1
  }
  END { exit found ? 1 : 0 }
' "$lockfile"; then
    :
  else
    echo "dependency health: serde_yaml is deprecated; use yaml_serde for serde-compatible YAML" >&2
    exit 1
  fi
done
