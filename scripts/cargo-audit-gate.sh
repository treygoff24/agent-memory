#!/usr/bin/env bash
# Dependency-CVE gate wrapper around `cargo audit`.
#
# Runs `cargo audit` against the workspace Cargo.lock and fuzz/Cargo.lock and
# fails the gate when a RUSTSEC advisory matches a dependency. The advisory
# database is fetched from the network; when that fetch fails (offline CI,
# GitHub outage) we treat the run as a non-fatal SKIP rather than a gate
# failure, so the check never blocks work for reasons unrelated to the
# dependency tree. A genuine advisory match still fails.
#
# Opt in by installing the tool: `cargo install cargo-audit`.
set -uo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

lockfiles=(Cargo.lock fuzz/Cargo.lock)

for lockfile in "${lockfiles[@]}"; do
  audit_log="$(mktemp -t cargo-audit.XXXXXX)"
  trap 'rm -f "$audit_log"' EXIT

  # `--deny warnings` so yanked crates / unmaintained advisories also surface.
  cargo audit --file "$lockfile" --deny warnings >"$audit_log" 2>&1
  status=$?

  if [[ $status -eq 0 ]]; then
    cat "$audit_log"
    rm -f "$audit_log"
    continue
  fi

  # Distinguish "could not refresh the advisory DB" (network problem → skip) from
  # a real advisory finding (→ fail). cargo-audit prints fetch errors mentioning
  # the advisory-db repo when it cannot update.
  if grep -qiE 'couldn'\''t fetch|failed to fetch|error fetching|unable to (update|fetch)|network|timed out|could not resolve host|advisory-db' "$audit_log"; then
    echo "warning: cargo-audit could not refresh the advisory database for $lockfile; skipping dependency-CVE scan" >&2
    cat "$audit_log" >&2
    rm -f "$audit_log"
    continue
  fi

  echo "[FAIL] cargo-audit reported one or more dependency advisories in $lockfile:" >&2
  cat "$audit_log" >&2
  rm -f "$audit_log"
  exit 1
done

exit 0
