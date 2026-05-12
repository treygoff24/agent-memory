#!/usr/bin/env bash
# seed-dev-substrate.sh
#
# Spin up a fresh memoryd against ~/memorum-dev (separate from your real
# ~/memorum) and seed it with fake memories that exercise every UI surface:
# healthy recall mix, review queue, supersession chains, tombstones,
# contradictions, Reality Check candidates, and privacy-encrypted samples.
# Optionally enables the web dashboard at http://127.0.0.1:7137.
#
# Wipe: bash scripts/seed-dev-substrate.sh --reset
#   or: rm -rf ~/memorum-dev ~/.memorum-dev
#
# Requires: memoryd on PATH (install via scripts/install-memorum.sh), jq.
#
# Notes on memoryd schema:
# - Memory `type` must be one of: project, claim, decision, pattern, playbook,
#   procedure, artifact. Other values are rejected at the protocol layer.
# - Built-in policies require_grounding for all four scopes (me/project/agent/
#   dreaming) with floors 0.85/0.70/0.82/0.95. Every governed write below
#   passes a real on-disk grounding file via source_ref=file:<path>#<anchor>.

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/seed-dev-substrate.sh [--reset] [--no-web] [--port N] [--repo PATH] [--runtime PATH]

Options:
  --reset       Kill any running dev daemon and wipe the dev repo before seeding.
  --no-web      Skip enabling the localhost web dashboard.
  --port N      Web dashboard port (default: 7137).
  --repo PATH   Canonical dev repo path (default: ~/memorum-dev).
  --runtime P   Per-device runtime path (default: ~/.memorum-dev).
  -h | --help   Show this help.
USAGE
}

repo="$HOME/memorum-dev"
runtime="$HOME/.memorum-dev"
port=7137
reset=0
enable_web=1

while [ "$#" -gt 0 ]; do
  case "$1" in
    --reset) reset=1; shift ;;
    --no-web) enable_web=0; shift ;;
    --port) port="${2:?--port requires a value}"; shift 2 ;;
    --repo) repo="${2:?--repo requires a value}"; shift 2 ;;
    --runtime) runtime="${2:?--runtime requires a value}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

socket="$runtime/memoryd.sock"
pid_file="$runtime/memoryd.pid"
log_file="$runtime/memoryd.log"
grounding_dir="$runtime/grounding"

if ! command -v memoryd >/dev/null 2>&1; then
  echo "error: memoryd not on PATH. Install with: bash scripts/install-memorum.sh" >&2
  exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq not found. Install with: brew install jq" >&2
  exit 1
fi
have_tui=0; have_web=0
command -v memoryd-tui >/dev/null 2>&1 && have_tui=1
command -v memoryd-web >/dev/null 2>&1 && have_web=1
if [ "$have_tui" -eq 0 ] || [ "$have_web" -eq 0 ]; then
  echo "warn: memoryd-tui or memoryd-web not on PATH; seeding will still work but" >&2
  echo "      'memoryd ui' and 'memoryd web enable' will fail until you install them:" >&2
  echo "        cargo install --path crates/memoryd-tui --locked" >&2
  echo "        cargo install --path crates/memoryd-web --locked" >&2
fi

stop_existing_daemon() {
  if [ ! -f "$pid_file" ]; then return; fi
  local existing_pid
  existing_pid="$(cat "$pid_file" 2>/dev/null || true)"
  if [[ -n "$existing_pid" && "$existing_pid" =~ ^[0-9]+$ ]] && kill -0 "$existing_pid" >/dev/null 2>&1; then
    kill "$existing_pid" >/dev/null 2>&1 || true
    for _ in 1 2 3 4 5; do
      kill -0 "$existing_pid" >/dev/null 2>&1 || break
      sleep 1
    done
    kill -KILL "$existing_pid" >/dev/null 2>&1 || true
  fi
  rm -f "$pid_file"
}

if [ "$reset" -eq 1 ]; then
  echo "==> reset: stopping daemon and wiping $repo + $runtime"
  stop_existing_daemon
  rm -rf "$repo" "$runtime"
fi

if [ -e "$repo/.memorum" ] || [ -e "$repo/events" ] || [ -S "$socket" ]; then
  echo "error: dev substrate already exists at $repo. Pass --reset to rebuild." >&2
  exit 3
fi

mkdir -p "$repo" "$runtime" "$grounding_dir"
chmod 700 "$runtime"
: >"$log_file"
chmod 600 "$log_file"

# Stream D requires a local age key in the runtime privacy key store before
# any governed write can run (PII routing encrypts at rest).
echo "==> provisioning device key (memoryd device onboard)"
memoryd device onboard --runtime "$runtime" >/dev/null

echo "==> starting memoryd against $repo (socket: $socket)"
nohup memoryd serve --init --repo "$repo" --runtime "$runtime" --socket "$socket" \
  </dev/null >>"$log_file" 2>&1 &
daemon_pid=$!
disown "$daemon_pid" >/dev/null 2>&1 || true
echo "$daemon_pid" > "$pid_file"

ready=0
for _ in $(seq 1 30); do
  if memoryd status --socket "$socket" >/dev/null 2>&1; then
    ready=1; break
  fi
  sleep 1
done
if [ "$ready" -ne 1 ]; then
  echo "error: daemon never became ready. Tail $log_file for details." >&2
  tail -20 "$log_file" >&2 || true
  exit 4
fi
echo "    daemon pid=$daemon_pid log=$log_file"

# ----- grounding fixtures ------------------------------------------------
# Built-in policies require_grounding for all four scopes. Every governed
# write below references a real on-disk file via source_ref=file:<path>#<anchor>.
make_grounding() {
  local label="$1" text="$2"
  local path="$grounding_dir/${label}.md"
  printf '%s\n' "$text" >"$path"
  printf 'file:%s#%s' "$path" "$label"
}

# ----- helpers -----------------------------------------------------------
meta_json() {
  # $1=namespace $2=type $3=confidence $4=source_kind $5=source_ref|"" $6=explicit_user_context(true|false)
  local ns="$1" t="$2" conf="$3" src_kind="$4" src_ref="$5" explicit="$6"
  jq -nc \
    --arg ns "$ns" --arg t "$t" --argjson conf "$conf" \
    --arg sk "$src_kind" --arg sr "$src_ref" --argjson exp "$explicit" \
    '{namespace:$ns, type:$t, confidence:$conf, source_kind:$sk,
      source_ref: (if ($sr|length) > 0 then $sr else null end),
      explicit_user_context:$exp}'
}

# Loud failure: any unexpected error from a governed write should stop the
# script so we don't silently lose memories like the first cut of this script did.
check_gw_response() {
  local resp="$1" allow_review="${2:-0}"
  local err; err="$(printf '%s' "$resp" | jq -r '.result.error.message // empty')"
  if [ -n "$err" ]; then
    echo "error: memoryd write failed: $err" >&2
    echo "response: $resp" >&2
    return 1
  fi
  local status; status="$(printf '%s' "$resp" | jq -r '.result.success.governance_write.status // .result.success.governance_supersede.status // empty')"
  if [ -z "$status" ]; then
    echo "error: unexpected response shape: $resp" >&2
    return 1
  fi
  case "$status" in
    promoted) return 0 ;;
    candidate|in_review|quarantined|refused|superseded|tombstoned)
      [ "$allow_review" -eq 1 ] && return 0
      echo "error: write status=$status not allowed here: $resp" >&2
      return 1
      ;;
    *)
      echo "error: unexpected status=$status: $resp" >&2
      return 1
      ;;
  esac
}

gw_id() { jq -r '.result.success.governance_write.id // empty'; }
gs_new_id() { jq -r '.result.success.governance_supersede.new_id // empty'; }

# Confidence floor per namespace (built-in policy floors).
ns_confidence() {
  case "$1" in
    me) echo 0.92 ;; project) echo 0.88 ;; agent) echo 0.90 ;; dreaming) echo 0.96 ;;
    *) echo 0.9 ;;
  esac
}

# Promote-quality governed write.
write_governed() {
  # $1=namespace $2=type $3=title $4=body $5=grounding-label  rest=tags
  local ns="$1" t="$2" title="$3" body="$4" label="$5"; shift 5
  local conf; conf="$(ns_confidence "$ns")"
  local src_ref; src_ref="$(make_grounding "$label" "$title — $body")"
  local meta; meta="$(meta_json "$ns" "$t" "$conf" "agent_primary" "$src_ref" true)"
  local tag_args=(); local tag
  for tag in "$@"; do tag_args+=(--tag "$tag"); done
  local resp; resp="$(memoryd write --socket "$socket" --title "$title" "${tag_args[@]}" --meta "$meta" "$body")"
  check_gw_response "$resp" 0 >&2 || return 1
  printf '%s' "$resp" | gw_id
}

# Below-floor confidence (lands in review queue).
write_governed_low_conf() {
  local ns="$1" t="$2" title="$3" body="$4" label="$5"; shift 5
  local src_ref; src_ref="$(make_grounding "$label" "$title — $body")"
  local meta; meta="$(meta_json "$ns" "$t" 0.55 "agent_primary" "$src_ref" true)"
  local tag_args=(); local tag
  for tag in "$@"; do tag_args+=(--tag "$tag"); done
  local resp; resp="$(memoryd write --socket "$socket" --title "$title" "${tag_args[@]}" --meta "$meta" "$body")"
  check_gw_response "$resp" 1 >&2 || return 1
  printf '%s' "$resp" | jq -r '.result.success.governance_write.id // empty'
}

# Missing grounding (lands in review queue or refused by policy).
write_governed_no_grounding() {
  local ns="$1" t="$2" title="$3" body="$4"; shift 4
  local conf; conf="$(ns_confidence "$ns")"
  local meta; meta="$(meta_json "$ns" "$t" "$conf" "agent_primary" "" true)"
  local tag_args=(); local tag
  for tag in "$@"; do tag_args+=(--tag "$tag"); done
  local resp; resp="$(memoryd write --socket "$socket" --title "$title" "${tag_args[@]}" --meta "$meta" "$body")"
  check_gw_response "$resp" 1 >&2 || return 1
  printf '%s' "$resp" | jq -r '.result.success.governance_write.id // empty'
}

declare -a promoted_ids=()
declare -a supersede_chains=()
declare -a tombstone_ids=()
declare -a review_ids=()

remember() {
  local arr="$1" id="$2"
  if [ -n "$id" ]; then eval "$arr+=(\"$id\")"; fi
}

# ----- bucket A: healthy recall mix -------------------------------------
echo "==> bucket A: healthy recall mix"

# Project memories (namespace=project, floor 0.70, requires_grounding).
# format: label:type:title:body
A_PROJECT=(
  "build-gate:decision:Full release gate command:Run CARGO_BUILD_JOBS=4 bash scripts/check.sh for the full release gate. Covers fmt, oxfmt, lint, baseline, specgate, workspace tests, doc, boundary, two-clone, durability, bench smoke and release, bench regression."
  "memoryd-socket:claim:Default memoryd socket path:memoryd serves on runtime/memoryd.sock by default. The MCP bridge is launched via 'memoryd mcp --socket <path>'."
  "merge-driver:claim:Memorum merge driver schema gate:The constant MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION is the single source of truth for the merge driver schema gate. Hard-coded numbers will fail review."
  "bench-policy:decision:Bench baselines are human-only:bench/baseline.<profile>.json files are updated only by explicit human-authored commits. The bench harness never overwrites them per spec section 17.6 and 18.9."
  "lockfile-policy:decision:Cargo lockfile workflow:Use cargo build --workspace --locked plus targeted cargo update -p <crate> for integration work. Never run cargo generate-lockfile."
  "stream-h-auth:claim:Stream H live LLM gating:Stream H live real-harness runs depend on authenticated Claude and Codex CLIs in the environment. Without keys the harness emits skip markers, not fabricated passes."
  "stream-i-config:claim:Per-project concurrent session mode:Stream I concurrent_session_mode is set per-project in config.yaml and controls whether the second session sees a peer-presence block."
  "claude-md-rule:decision:Don't modify Stream A unless redirected:Stream A modules are a frozen contract for downstream streams. Modifications require explicit authorization."
  "codex-worktree:claim:Codex per-task worktree convention:Each Codex task runs in ../agent-memory-wt/task-NN/ on a stream-x/task-NN-slug branch. Workers run narrow per-task gates only."
  "dogfood-readiness:claim:Dogfood-readiness gap-fix closed:Dogfood-readiness gap-fix plan closed out 2026-05-11 on branch dogfood/codex-readiness-2026-05-07 head 2a9a9ad 26 commits ahead of main. Full release gate green."
  "rust-test-handbook:claim:Handbook tests serialize:The 12 handbook integration tests in crates/memorum-eval/tests/handbook.rs use serial_test serial to avoid APFS fsync-visibility races under heavy parallel load."
  "spec-versioning:decision:Spec and plan versioning convention:Spec and plan files are versioned by suffix (v1.1.md v0.5.md). New versions supersede. Old versions stay on disk for history."
  "branding:claim:Project ships as Memorum:Memorum (Latin genitive plural of memor mindful) is the canonical product name. Captured in system spec v0.2 section 22."
  "mcp-tool-count:claim:Memorum exposes nine MCP tools:The agent-facing MCP surface is nine tools. memory_search memory_get memory_write memory_supersede memory_forget memory_reveal memory_startup memory_note memory_observe."
  "frontend-stack:claim:Web dashboard frontend stack:The localhost web dashboard frontend is React plus Tailwind plus Vite plus TanStack Router. Tested with Vitest unit plus visual regression plus Playwright e2e."
)
for entry in "${A_PROJECT[@]}"; do
  IFS='|' read -r label typ title body <<< "$(printf '%s' "$entry" | sed -E 's/^([^:]+):([^:]+):([^:]+):/\1|\2|\3|/')"
  id="$(write_governed project "$typ" "$title" "$body" "$label" memorum dogfood)"
  remember promoted_ids "$id"
done

# Personal/me memories (floor 0.85, requires_grounding).
A_ME=(
  "trey-tz:claim:Trey works in central US time:Trey primary timezone is US Central (Austin). Default scheduling assumes Central time unless he names another zone explicitly."
  "trey-editor:claim:Trey uses VS Code with vim bindings:Default editor on Trey macOS is VS Code with the Vim extension. Terminal-only sessions use plain vim."
  "trey-shell:claim:Trey uses zsh:Default shell on macOS is zsh configured with starship prompt. Scripts should work under both bash and zsh."
  "trey-coffee:claim:Trey drinks coffee black:Personal preference. No cream no sugar dark roast preferred."
  "trey-music:claim:Trey listens to ambient while coding:Defaults to ambient or instrumental hip-hop while in flow. Avoid recommending vocal-heavy music during work hours."
  "trey-glass:claim:Trey prefers prescription glasses to contacts:Long sessions are easier on glasses than contacts. Relevant for any eye strain or lens fatigue tangents."
  "trey-walks:claim:Trey takes daily walks for thinking:Hour-long walks around midday are a thinking ritual not optional. Don't schedule meetings 11:30-13:30 Central without checking."
  "trey-comm-style:claim:Trey prefers blunt direct feedback:When reviewing his work or proposals lead with the real concern. Hedging reads as evasion."
  "trey-py-vs-rust:decision:Trey prefers Rust for systems work:For long-lived daemons and substrate code Rust is the default. Python for scripts and one-off tooling."
  "trey-meeting-cap:decision:No meetings before 10am Central:Mornings are protected deep work time. Schedule meetings 10am Central onward unless urgent."
)
for entry in "${A_ME[@]}"; do
  IFS='|' read -r label typ title body <<< "$(printf '%s' "$entry" | sed -E 's/^([^:]+):([^:]+):([^:]+):/\1|\2|\3|/')"
  id="$(write_governed me "$typ" "$title" "$body" "$label" personal)"
  remember promoted_ids "$id"
done

# Agent memories (floor 0.82, requires_grounding) — learned patterns.
A_AGENT=(
  "rg-vs-grep:pattern:ripgrep beats grep on large trees:rg with default smart-case and gitignore handling is roughly 5x faster than grep -r on the agent-memory repo. Prefer rg for code search."
  "cargo-jobs:pattern:CARGO_BUILD_JOBS=4 keeps machine usable:Heavy compiles fully saturate Trey MBP at default parallelism. CARGO_BUILD_JOBS=4 lets the gate run while he keeps other tools responsive."
  "oxfmt-quirk:pattern:oxfmt walks directory args literally:oxfmt walks anything that looks like a path. Argument-split bugs that produce directories with spaces in names will surface as phantom files."
  "syspolicyd:pattern:macOS syspolicyd stalls long Rust runs:Long cargo runs against an existing target/ pin syspolicyd. Workaround. Isolated CARGO_TARGET_DIR plus PATH purge of cargo-nextest and sccache."
  "gh-pr-checks:pattern:gh pr checks is the canonical PR status:gh pr checks is the cleanest way to see CI status across providers. Prefer it over scraping web URLs."
)
for entry in "${A_AGENT[@]}"; do
  IFS='|' read -r label typ title body <<< "$(printf '%s' "$entry" | sed -E 's/^([^:]+):([^:]+):([^:]+):/\1|\2|\3|/')"
  id="$(write_governed agent "$typ" "$title" "$body" "$label" agent-learned)"
  remember promoted_ids "$id"
done

# Low-friction notes via write-note. WORKAROUND: keep these colon-free —
# the write-note path serializes text directly into a YAML summary field
# without escaping embedded ": ", which would break the substrate's frontmatter.
A_NOTES=(
  "Revisit handbook --test-threads=1 once we have a faster machine"
  "Stream G web dashboard renders Reality Check sessions at /reality-check"
  "Bench baselines are human-only commits per spec section 17.6"
  "memoryd doctor --reindex rebuilds the SQLite events_log mirror from JSONL"
  "Lesson from 5/11 — when a tool reports paths that stat denies, list the actual cwd first"
)
for note in "${A_NOTES[@]}"; do
  memoryd write-note --socket "$socket" "$note" >/dev/null
done

echo "    bucket A: ${#A_PROJECT[@]} project + ${#A_ME[@]} me + ${#A_AGENT[@]} agent + ${#A_NOTES[@]} notes"

# ----- bucket B: governance edges ---------------------------------------
echo "==> bucket B: governance edges (review queue + supersession + tombstones)"

# Below-floor confidence in me ns (floor 0.85) → review queue.
B_LOW_CONF=(
  "weak-claim-1:claim:Trey might use Helix sometimes:Uncertain — possibly seen in screenshots once. Low confidence, no recent confirmation."
  "weak-claim-2:claim:Trey may prefer dark roast over medium:Heard once in passing, not strongly grounded."
  "weak-claim-3:claim:Trey secondary office might be Houston:Unverified rumor, low confidence."
)
for entry in "${B_LOW_CONF[@]}"; do
  IFS='|' read -r label typ title body <<< "$(printf '%s' "$entry" | sed -E 's/^([^:]+):([^:]+):([^:]+):/\1|\2|\3|/')"
  id="$(write_governed_low_conf me "$typ" "$title" "$body" "$label" weak)"
  remember review_ids "$id"
done

# Missing grounding in project ns (requires_grounding) → review queue.
B_NO_GROUND=(
  "ungrounded-1:claim:Stream J might exist someday:Speculative — no spec, no plan, just a hunch from a corridor conversation."
  "ungrounded-2:decision:Maybe switch to TOML for policies:Tentative thought, no evidence on disk yet."
)
for entry in "${B_NO_GROUND[@]}"; do
  IFS='|' read -r label typ title body <<< "$(printf '%s' "$entry" | sed -E 's/^([^:]+):([^:]+):([^:]+):/\1|\2|\3|/')"
  id="$(write_governed_no_grounding project "$typ" "$title" "$body" speculative)"
  remember review_ids "$id"
done

# Supersession chains. v1 promoted, then superseded with v2.
echo "    seeding 2 supersession chains"
seed_supersession() {
  local ns="$1" typ="$2" title_v1="$3" body_v1="$4" body_v2="$5" reason="$6" label_v1="$7" label_v2="$8"
  local old_id; old_id="$(write_governed "$ns" "$typ" "$title_v1" "$body_v1" "$label_v1" supersession)"
  if [ -z "$old_id" ]; then return; fi
  local conf; conf="$(ns_confidence "$ns")"
  local src_ref; src_ref="$(make_grounding "$label_v2" "$title_v1 (v2) — $body_v2")"
  local meta; meta="$(meta_json "$ns" "$typ" "$conf" "agent_primary" "$src_ref" true)"
  local resp; resp="$(memoryd supersede --socket "$socket" "$old_id" "$body_v2" --reason "$reason" --meta "$meta")"
  if ! check_gw_response "$resp" 0 >&2; then return; fi
  local new_id; new_id="$(printf '%s' "$resp" | gs_new_id)"
  supersede_chains+=("$old_id->$new_id")
  remember promoted_ids "$new_id"
}
seed_supersession project claim \
  "Default web dashboard port" \
  "The localhost web dashboard listens on port 7137 by default." \
  "The localhost web dashboard now defaults to port 7137 and accepts an override via --port." \
  "clarify override mechanism" web-port-v1 web-port-v2
# NOTE: the supersession target must stay in a namespace where the body
# survives Stream D classification as plaintext. me/ writes that mention a
# person name get encrypt-at-rest, and Stream A has no encrypted-supersession
# API yet — supersedes on those will refuse with reason=privacy.
seed_supersession project claim \
  "Default daemon socket path" \
  "memoryd serves on .memoryd/memoryd.sock by default." \
  "memoryd serves on runtime/memoryd.sock by default. The MCP bridge uses 'memoryd mcp --socket <path>'." \
  "tighten language and call out the MCP bridge entry point" socket-v1 socket-v2

# Tombstones — write then forget.
echo "    seeding 2 tombstones"
seed_tombstone() {
  local ns="$1" typ="$2" title="$3" body="$4" reason="$5" label="$6"
  local id; id="$(write_governed "$ns" "$typ" "$title" "$body" "$label" obsolete)"
  if [ -z "$id" ]; then return; fi
  memoryd forget --socket "$socket" "$id" --reason "$reason" >/dev/null
  tombstone_ids+=("$id")
}
seed_tombstone project claim \
  "Obsolete Stream A version" \
  "Stream A v0.1 substrate contract — long superseded by v1.1." \
  "spec version v0.1 obsolete, v1.1 is canonical" \
  obsolete-v0-1
seed_tombstone me claim \
  "Old keyboard preference" \
  "Trey used to prefer a Kinesis Advantage." \
  "switched away from this keyboard years ago" \
  old-keyboard

# ----- bucket C: contradictions + Reality Check candidates --------------
echo "==> bucket C: contradictions + Reality Check candidates"
# Pairs of conflicting facts in the same scope — contradiction policy decides
# supersede vs quarantine on the second write.
C_CONTRADICT=(
  "lang-pref-a:claim:Trey prefers Python for scripting:For ad-hoc scripts and one-off tooling Trey reaches for Python first."
  "lang-pref-b:claim:Trey prefers Bash for scripting:For ad-hoc scripts and one-off tooling Trey reaches for Bash first."
  "editor-pref-a:claim:Trey edits in VS Code primarily:Primary editing environment is VS Code with Vim bindings."
  "editor-pref-b:claim:Trey edits in Neovim primarily:Primary editing environment is Neovim with telescope and treesitter."
)
for entry in "${C_CONTRADICT[@]}"; do
  IFS='|' read -r label typ title body <<< "$(printf '%s' "$entry" | sed -E 's/^([^:]+):([^:]+):([^:]+):/\1|\2|\3|/')"
  # Contradictions may land as promoted, supersede, or quarantined depending
  # on policy. Allow review-status responses so we don't bail on quarantine.
  src_ref="$(make_grounding "$label" "$title — $body")"
  conf="$(ns_confidence me)"
  meta="$(meta_json me "$typ" "$conf" "agent_primary" "$src_ref" true)"
  resp="$(memoryd write --socket "$socket" --title "$title" --tag contradiction --meta "$meta" "$body")"
  if check_gw_response "$resp" 1 >&2; then
    id="$(printf '%s' "$resp" | gw_id)"
    remember promoted_ids "$id"
  fi
done

# ----- bucket D: privacy samples ----------------------------------------
echo "==> bucket D: privacy + dreaming"
# PII content triggers the Stream D classifier to encrypt at rest.
D_PRIVACY=(
  "contact-email:claim:Personal contact email:Reach Trey on personal email lawrencegoffiii@gmail.com for off-channel matters."
  "contact-phone:claim:Mobile contact number:Trey mobile is +1-512-555-0142 (placeholder format)."
  "address:claim:Mailing address:Mailing address is 123 Example Lane, Austin, TX 78701."
)
for entry in "${D_PRIVACY[@]}"; do
  IFS='|' read -r label typ title body <<< "$(printf '%s' "$entry" | sed -E 's/^([^:]+):([^:]+):([^:]+):/\1|\2|\3|/')"
  src_ref="$(make_grounding "$label" "$title — $body")"
  conf="$(ns_confidence me)"
  meta="$(meta_json me "$typ" "$conf" "agent_primary" "$src_ref" true)"
  resp="$(memoryd write --socket "$socket" --title "$title" --tag contact --meta "$meta" "$body")"
  if check_gw_response "$resp" 0 >&2; then
    id="$(printf '%s' "$resp" | gw_id)"
    remember promoted_ids "$id"
  fi
done

# Dreaming requires (a) a harness CLI on PATH (claude -p / codex exec) AND
# (b) a git remote 'origin' for cross-device lease coordination. The dev
# substrate has neither by default, so we skip auto-dreaming and just print
# the manual command for completeness.
echo "    skipping dreaming (requires git remote + harness CLI; dev substrate has neither)"
echo "    to populate dream surfaces later: memoryd dream now --repo $repo --runtime $runtime --scope project:agent-memory"

# ----- web dashboard ----------------------------------------------------
dashboard_url=""
if [ "$enable_web" -eq 1 ]; then
  echo "==> enabling web dashboard on port $port"
  if memoryd web enable --socket "$socket" --port "$port" >/dev/null 2>&1; then
    dashboard_url="http://127.0.0.1:$port"
  else
    echo "    web enable failed; you can retry manually:"
    echo "      memoryd web enable --socket $socket --port $port"
  fi
fi

# ----- summary ----------------------------------------------------------
cat <<SUMMARY

==============================================================
seed-dev-substrate: complete
==============================================================
dev repo:       $repo
dev runtime:    $runtime
daemon pid:     $(cat "$pid_file" 2>/dev/null || echo "unknown")
daemon log:     $log_file
MCP socket:     $socket

seeded:
  bucket A (healthy mix):  ${#A_PROJECT[@]} project + ${#A_ME[@]} me + ${#A_AGENT[@]} agent + ${#A_NOTES[@]} notes
  bucket B (gov. edges):   ${#review_ids[@]} review-queue + ${#supersede_chains[@]} supersession chains + ${#tombstone_ids[@]} tombstones
  bucket C (contradict):   ${#C_CONTRADICT[@]} conflicting claims (2 pairs)
  bucket D (privacy):      ${#D_PRIVACY[@]} PII-sensitive samples
  promoted total:          ${#promoted_ids[@]}

try these:
  memoryd ui --socket $socket                                # TUI
  memoryd reality-check run --socket $socket                 # Reality Check session
  memoryd review queue --socket $socket                      # Review queue items
  memoryd search "trey" --socket $socket                     # search
  memoryd peer status --socket $socket                       # cross-session view
SUMMARY

if [ -n "$dashboard_url" ]; then
  echo "  open $dashboard_url                                          # web dashboard"
fi

cat <<'SUMMARY'

reset:
  bash scripts/seed-dev-substrate.sh --reset
  # or: rm -rf ~/memorum-dev ~/.memorum-dev
==============================================================
SUMMARY
