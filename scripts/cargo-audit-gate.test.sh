#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

cat >"$tmpdir/cargo" <<'SH'
#!/usr/bin/env bash
if [[ "${1:-}" != "audit" ]]; then
  echo "unexpected cargo subcommand: $*" >&2
  exit 99
fi
cat >&2 <<'OUT'
Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
Loaded 900 security advisories (from /home/test/.cargo/advisory-db)
error: 1 vulnerability found!
Crate: vulnerable
Advisory: RUSTSEC-2099-0001
OUT
exit 1
SH
chmod +x "$tmpdir/cargo"

set +e
PATH="$tmpdir:$PATH" "$repo_root/scripts/cargo-audit-gate.sh" >"$tmpdir/stdout" 2>"$tmpdir/stderr"
status=$?
set -e

if [[ "$status" -eq 0 ]]; then
  echo "expected cargo-audit-gate to fail on advisory output that mentions advisory-db" >&2
  cat "$tmpdir/stdout" >&2
  cat "$tmpdir/stderr" >&2
  exit 1
fi

if ! grep -q "\[FAIL\] cargo-audit reported" "$tmpdir/stderr"; then
  echo "expected failure output to report dependency advisories" >&2
  cat "$tmpdir/stderr" >&2
  exit 1
fi

cat >"$tmpdir/cargo" <<'SH'
#!/usr/bin/env bash
if [[ "${1:-}" != "audit" ]]; then
  echo "unexpected cargo subcommand: $*" >&2
  exit 99
fi
cat >&2 <<'OUT'
error: 1 vulnerability found!
Crate: network-vulnerable
Advisory: RUSTSEC-2099-0002
OUT
exit 1
SH
chmod +x "$tmpdir/cargo"

set +e
PATH="$tmpdir:$PATH" "$repo_root/scripts/cargo-audit-gate.sh" >"$tmpdir/stdout" 2>"$tmpdir/stderr"
status=$?
set -e

if [[ "$status" -eq 0 ]]; then
  echo "expected cargo-audit-gate to fail on advisory output mentioning a network-named crate" >&2
  cat "$tmpdir/stdout" >&2
  cat "$tmpdir/stderr" >&2
  exit 1
fi

cat >"$tmpdir/cargo" <<'SH'
#!/usr/bin/env bash
if [[ "${1:-}" != "audit" ]]; then
  echo "unexpected cargo subcommand: $*" >&2
  exit 99
fi
cat >&2 <<'OUT'
error: failed to fetch advisory database
Caused by: network error: could not resolve host github.com
OUT
exit 1
SH
chmod +x "$tmpdir/cargo"

PATH="$tmpdir:$PATH" "$repo_root/scripts/cargo-audit-gate.sh" >"$tmpdir/stdout" 2>"$tmpdir/stderr"

if ! grep -q "could not refresh the advisory database" "$tmpdir/stderr"; then
  echo "expected offline advisory-db refresh failure to be skipped" >&2
  cat "$tmpdir/stdout" >&2
  cat "$tmpdir/stderr" >&2
  exit 1
fi
