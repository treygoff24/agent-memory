#!/usr/bin/env bash
set -euo pipefail

started_at="$(date +%s)"

finish() {
  local ended_at
  ended_at="$(date +%s)"
  echo "check-dogfood duration: $((ended_at - started_at))s"
}
trap finish EXIT

phase() {
  echo
  echo "==> $*"
}

phase "fast dogfood gate"
./scripts/check-fast.sh

phase "seed dev substrate script safety"
bash scripts/seed-dev-substrate.test.sh

phase "TUI recall filter"
cargo test -p memoryd-tui --test inbox_render recall_filter_renders_recall_items_only -- --nocapture

phase "TUI panic restore"
cargo test -p memoryd-tui panic_restore -- --nocapture --test-threads=1

phase "doctor health"
cargo test -p memoryd --lib doctor_health -- --nocapture

phase "doctor CLI unhealthy exit"
cargo test -p memoryd --test cli_contract doctor_unhealthy_exit -- --nocapture

phase "startup recall peer-update references"
cargo test -p memoryd --lib surfaced_peer_update_references -- --nocapture

phase "live-harness wrapper skip honesty without provider keys"
env -u MEMORUM_EVAL_CLAUDE_KEY -u MEMORUM_EVAL_CODEX_KEY \
  cargo test -p memorum-eval --features live-harness --test live -- --nocapture --test-threads=1

phase "memoryd minimal feature compile"
cargo check -p memoryd --no-default-features --locked

phase "alpha daemon dogfood smokes"
dogfood_tmp="$(mktemp -d)"
dogfood_repo="$dogfood_tmp/repo"
dogfood_runtime="$dogfood_tmp/runtime"
dogfood_socket="$dogfood_runtime/memoryd.sock"
memoryd_pid=""
# The dashboard API is auth-gated: `web enable` mints a bearer token and every
# request must carry it via the `x-memorum-dashboard-auth` header, a cookie, or
# a one-shot `?auth=` query (see crates/memoryd-web/src/state.rs and auth.rs).
# The header is the simplest for a headless smoke — it authenticates GET and
# POST uniformly without the redirect/cookie-jar bootstrap a browser uses.
dashboard_auth_header="x-memorum-dashboard-auth"
dashboard_token=""
# Protected /api routes additionally require the CSRF token embedded in the
# dashboard HTML shell's <meta name="csrf-token"> tag. require_csrf is layered
# on every protected method, GET included (crates/memoryd-web/src/server.rs),
# so reads need both the dashboard auth header and the CSRF header.
csrf_header="x-memorum-csrf"
csrf_token=""

port_is_free() {
  local port="$1"
  ! (echo >/dev/tcp/127.0.0.1/"$port") >/dev/null 2>&1
}

choose_dogfood_port() {
  if [ -n "${MEMORUM_DOGFOOD_WEB_PORT:-}" ]; then
    printf '%s' "$MEMORUM_DOGFOOD_WEB_PORT"
    return
  fi
  local candidate
  for candidate in $(seq 7137 7199); do
    if port_is_free "$candidate"; then
      printf '%s' "$candidate"
      return
    fi
  done
  echo "error: no free dogfood web port found in 7137-7199; set MEMORUM_DOGFOOD_WEB_PORT" >&2
  exit 1
}

dogfood_port="$(choose_dogfood_port)"
echo "dogfood web port: $dogfood_port"

dogfood_cleanup() {
  if [ -S "$dogfood_socket" ]; then
    cargo_memoryd web disable --socket "$dogfood_socket" >/dev/null 2>&1 || true
  fi
  if [ -n "$memoryd_pid" ]; then
    kill "$memoryd_pid" 2>/dev/null || true
    wait "$memoryd_pid" 2>/dev/null || true
  fi
  rm -rf "$dogfood_tmp"
}
trap 'dogfood_cleanup; finish' EXIT

cargo_memoryd() {
  cargo run --quiet --bin memoryd -- "$@"
}

curl_expect() {
  curl -fsS -H "$dashboard_auth_header: $dashboard_token" -H "$csrf_header: $csrf_token" "$@"
}

assert_file_contains() {
  local file="$1"
  local needle="$2"
  local label="$3"
  if ! grep -Fq -- "$needle" "$file"; then
    echo "$label: missing expected output: $needle" >&2
    cat "$file" >&2
    exit 1
  fi
}

assert_file_not_matches() {
  local file="$1"
  local pattern="$2"
  local label="$3"
  if grep -Eiq -- "$pattern" "$file"; then
    echo "$label: output contains stale placeholder/deferred language matching $pattern" >&2
    cat "$file" >&2
    exit 1
  fi
}

extract_csrf_token() {
  local file="$1"
  tr '\n' ' ' <"$file" | sed -n 's/.*<meta[^>]*name="csrf-token"[^>]*content="\([^"]*\)".*/\1/p' | head -n 1
}

mkdir -p "$dogfood_repo" "$dogfood_runtime"
cargo_memoryd serve \
  --init \
  --repo "$dogfood_repo" \
  --runtime "$dogfood_runtime" \
  --socket "$dogfood_socket" \
  --force-unsafe-durability \
  >"$dogfood_tmp/memoryd.log" 2>&1 &
memoryd_pid="$!"

for _ in $(seq 1 60); do
  if cargo_memoryd status --socket "$dogfood_socket" >"$dogfood_tmp/status.json" 2>"$dogfood_tmp/status.err"; then
    break
  fi
  sleep 0.5
done
cargo_memoryd status --socket "$dogfood_socket" >"$dogfood_tmp/status.json"

cargo_memoryd web enable --socket "$dogfood_socket" --port "$dogfood_port" >"$dogfood_tmp/web-enable.json"
# `web enable` prints "Web dashboard enabled at http://127.0.0.1:<port>/?auth=<token>".
# The bearer token is only exposed on enable (never by `web status`), so capture
# it here before any dashboard request.
dashboard_token="$(sed -n 's/.*[?&]auth=\([0-9A-Fa-f]\{1,\}\).*/\1/p' "$dogfood_tmp/web-enable.json" | head -n 1)"
if [ -z "$dashboard_token" ]; then
  echo "alpha daemon dogfood smokes: web enable did not emit a dashboard auth token" >&2
  cat "$dogfood_tmp/web-enable.json" >&2
  exit 1
fi
# Bootstrap: fetch the dashboard shell (a bootstrap route needing only the
# dashboard auth token) to read the CSRF token, retrying until the web server is
# accepting connections. Every protected /api call below needs this token too.
index_html="$dogfood_tmp/index.html"
for _ in $(seq 1 60); do
  if curl -fsS -H "$dashboard_auth_header: $dashboard_token" "http://127.0.0.1:$dogfood_port/" \
      >"$index_html" 2>"$dogfood_tmp/curl-index.err"; then
    break
  fi
  sleep 0.5
done
curl -fsS -H "$dashboard_auth_header: $dashboard_token" "http://127.0.0.1:$dogfood_port/" >"$index_html"
csrf_token="$(extract_csrf_token "$index_html")"
if [ -z "$csrf_token" ]; then
  echo "alpha daemon dogfood smokes: missing CSRF token in dashboard shell" >&2
  cat "$index_html" >&2
  exit 1
fi

curl_expect "http://127.0.0.1:$dogfood_port/api/status" >"$dogfood_tmp/web-status.json"

phase "daemon-backed ROI smoke"
curl_expect "http://127.0.0.1:$dogfood_port/api/roi?window=90" >"$dogfood_tmp/roi.json"
assert_file_contains "$dogfood_tmp/roi.json" '"window_days":90' "roi smoke"
assert_file_not_matches "$dogfood_tmp/roi.json" 'deferred|not_implemented|placeholder|fixture' "roi smoke"

phase "daemon-backed notifications SSE smoke"
set +e
curl -fsS -H "$dashboard_auth_header: $dashboard_token" --max-time 3 -N "http://127.0.0.1:$dogfood_port/api/notifications/stream" \
  >"$dogfood_tmp/notifications.sse" 2>"$dogfood_tmp/notifications.err"
notifications_code=$?
set -e
if [ "$notifications_code" -ne 0 ] && [ "$notifications_code" -ne 28 ]; then
  cat "$dogfood_tmp/notifications.err" >&2
  exit "$notifications_code"
fi
assert_file_contains "$dogfood_tmp/notifications.sse" 'event: heartbeat' "notifications stream smoke"
assert_file_contains "$dogfood_tmp/notifications.sse" '"notifications"' "notifications stream smoke"
assert_file_not_matches "$dogfood_tmp/notifications.sse" 'not_implemented|placeholder|fixture' "notifications stream smoke"

phase "local artifact source capture smoke"
local_artifact="$dogfood_tmp/source-artifact.md"
cat >"$local_artifact" <<'ARTIFACT'
# Local artifact dogfood

Memorum local artifact source capture dogfood quote.
ARTIFACT
cargo_memoryd source capture \
  --socket "$dogfood_socket" \
  --file "$local_artifact" \
  --mode local-artifact \
  --excerpt "Memorum local artifact source capture dogfood quote." \
  >"$dogfood_tmp/source-capture.json"
assert_file_contains "$dogfood_tmp/source-capture.json" 'webcap:' "local artifact source capture smoke"
assert_file_contains "$dogfood_tmp/source-capture.json" 'local_artifact' "local artifact source capture smoke"

phase "policy editor GET/validate/write smoke"
curl_expect "http://127.0.0.1:$dogfood_port/api/policy-editor" >"$dogfood_tmp/policy-get.json"
assert_file_contains "$dogfood_tmp/policy-get.json" '"writable":true' "policy editor GET smoke"
cat >"$dogfood_tmp/policy-write.json" <<'JSON'
{
  "file_name": "project-standard.yaml",
  "raw_yaml": "name: project-standard\nversion: 2\nscope: project\nconfidence_floor: 0.71\nrequires_grounding: true\ntombstone_enforcement: review\ncontradiction_policy: supersede\nreview_gates:\n  - low_confidence\n"
}
JSON
curl_expect \
  -H "content-type: application/json" \
  -d @"$dogfood_tmp/policy-write.json" \
  "http://127.0.0.1:$dogfood_port/api/policy-editor" \
  >"$dogfood_tmp/policy-post.json"
assert_file_contains "$dogfood_tmp/policy-post.json" '"accepted":true' "policy editor write smoke"

phase "eval alpha release-set dry run"
cargo run --quiet -p memorum-eval --bin memorum-eval -- --harness mock --required-release-set alpha --output json \
  >"$dogfood_tmp/eval-alpha.json"
assert_file_not_matches "$dogfood_tmp/eval-alpha.json" '"deferred"[[:space:]]*:[[:space:]]*true' "eval alpha release-set"
assert_file_not_matches "$dogfood_tmp/eval-alpha.json" 'feature_deferred' "eval alpha release-set"

phase "eval regression metadata contract"
cargo test -p memorum-eval --test regression_meta -- --nocapture

echo
echo "check-dogfood passed"
