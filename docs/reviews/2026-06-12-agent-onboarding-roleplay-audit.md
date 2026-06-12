# Agent onboarding role-play audit — 2026-06-12

Live test of the "hey claude, install this memory tool and import our memories" flow,
executed by Claude Code following `llms-install.md` → `docs/agent-onboarding.md` as a
naive agent would. Each entry: what happened, severity (blocker / friction / nit /
praise), and suggested change.

## Session log

### 0. Discovery

- **Praise:** `llms-install.md` at repo root is exactly where an agent looks first, and it
  delegates cleanly to `docs/agent-onboarding.md`. One hop, no ambiguity.

### 1. Install via `scripts/install-memorum.sh --agent`

- **BLOCKER (fixed in-run):** Installer verification expects a binary named
  `memoryd-merge-driver`, but the crate ships `memory-merge-driver` (canonical name —
  matches `.gitattributes` wiring, substrate code, and all tests). The installer built and
  installed all four binaries successfully, then **aborted at its own verify step** with
  `install verification failed: memoryd-merge-driver not found on PATH after install` —
  so it never created `~/memorum`, never started the daemon, and never printed the
  `MEMORUM_AGENT_SUMMARY_JSON` line. The wrong name appeared in
  `scripts/install-memorum.sh` (bins array at L290 + summary echo), the installer test,
  `README.md`, `docs/getting-started.md`, and `docs/install.md`. All five fixed in this
  session (`memoryd-merge-driver` → `memory-merge-driver`).
- **Friction:** the failure exit code is easy to mask — an agent piping installer output
  through `tail`/`tee` sees exit 0 from the pipe. `docs/install.md` / agent-onboarding
  should tell agents to check for the `MEMORUM_AGENT_SUMMARY_JSON=` line as the success
  sentinel (its absence = failure), not just the exit code.

### 2. Consent loop (Step 2 of agent-onboarding.md)

- **Friction:** Decision 3 (`--non-git-cwd-default`) confused the real user — "explain
  this one to me idk what you mean." The doc's suggested phrasing assumes the user knows
  what a project bucket is and why git-ness matters. The doc should give agents a
  self-contained plain-language script: memories are filed into per-project buckets keyed
  by git repo; non-git folders have no stable identity; skip/me/generate trade-offs in one
  sentence each. The explanation must live *inside the question*, not in surrounding text
  the user may never see.
- **Observation:** user picked `generate` once it was explained ("import everything,
  organized") — suggests `generate` may be the right *recommended* option for the
  "import all my memories" intent, while `skip` remains the right consent-free default.

### 3. Detect step

- **Praise:** `memoryd init --detect-only` worked first try, honored `CLAUDE_CONFIG_DIR`
  (found `~/.claude-personal/projects`, 4 candidates) and found 56 Codex candidates.
- **Nit (doc drift):** agent-onboarding.md Step 1 says the detection JSON reports
  "whether a Memorum repo already exists at the default location." It doesn't — the JSON
  has only `claude`, `codex`, `daemon`. Either add a `repo` field or fix the doc.

### 4. `--harness current` / `--wire-mcp current` silently no-op on dual-harness machines

- **BLOCKER (design):** `current` does not mean "the harness driving this session." In
  `crates/memoryd/src/setup/steps.rs` (`selected_import`, `selected_wire_targets`),
  `Current` resolves to the single detected harness *only if exactly one harness is
  detected*; with both Claude and Codex present it falls through to `Skip`. Ran
  `memoryd init --non-interactive --json --import --harness current --wire-mcp current
  --non-git-cwd-default generate --daemon on-demand` → exit **0**, `import: skipped
  ("no import harness selected")`, `wire_mcp: skipped ("no MCP harness selected")`,
  `restart_required: false`. A doc-following agent reports success while nothing was
  imported or wired. Dual-harness machines are Memorum's headline use case, and
  `current` is also the documented **default** for the non-interactive path — so the
  default init is a guaranteed no-op for exactly the users the README pitches.
- **Fixes to consider (any one of):**
  1. Resolve `current` from the session environment (`CLAUDECODE`/`CLAUDE_CODE_ENTRYPOINT`
     set → Claude; Codex equivalents → Codex), falling back to single-detected.
  2. When `current` is ambiguous, mark the step `failed` (or a new `ambiguous` status)
     with a message telling the agent to re-run with `--harness claude|codex|all` —
     never silently skip a step the flags explicitly requested.
  3. Have agent-onboarding.md steer agents to explicit values (`claude`/`codex`/`all`)
     and reserve `current` for the interactive wizard where the TTY context is known.
- Also: the skipped-step messages ("no import harness selected") actively mislead — a
  harness *was* selected on the command line. Message should say *why* it resolved empty.

### 5. Installer vs init daemon overlap

- **Friction:** `scripts/install-memorum.sh` auto-starts a background daemon
  (pid file + log under the runtime dir) before `memoryd init` ever asks the user how
  they want the daemon managed. User chose `on-demand`; the report's `ensure_daemon`
  said "no background service was started" — yet one *is* running, started by the
  installer a minute earlier. The two entry points need a composition story: either the
  installer shouldn't start a daemon when invoked as the precursor to `init` (maybe a
  `--no-serve` flag the agent docs use), or `init --daemon on-demand` should detect and
  reconcile/stop the installer-started daemon instead of describing a state that isn't true.

### 6. Import crash: Claude project-dir decode is lossy (fixed in-run)

- **BLOCKER (fixed in-run):** `cwd_from_encoded_path` in
  `crates/memoryd/src/import/sources/claude.rs` decoded Claude's flattened project dirs
  with a naive `replace('-', "/")`. Any project path containing a hyphen — like
  **this repo, `agent-memory`** — decoded to a phantom path (`/Users/treygoff/Code/agent/memory`),
  which (a) defeated git-remote project resolution, (b) routed the memory through the
  non-git `generate` path, and (c) attempted to write `.memory-project.yaml` into the
  nonexistent phantom dir → `No such file or directory` → whole import step failed,
  exit 1. Had the phantom's parent existed, it would have **written a stray file into an
  unrelated directory.** Fixed with filesystem-aware resolution (`resolve_existing_path`:
  each hyphen boundary tried as `/` first, then literal `-`, recursively against
  on-disk dirs; naive decode kept as fallback for paths that no longer exist) + 3 tests.
  Dot-encoded segments (`.claude-personal` → `-claude-personal`) are still lossy in the
  fallback path — same as before, but worth a follow-up.

### 7. Failed-run artifacts are sticky; idempotent skip preserves wrong buckets

- **MAJOR:** the failed import (old decoder) was non-atomic — it had already ingested
  all 4 memories with **wrong bucketing** before erroring (the agent-memory repo memory
  got a generated `proj_memory-…` id from the phantom path; a Prospera-Policy memory sits
  under `projects/agent-memory/decisions/` with `namespace: agent-memory` but
  `canonical_namespace_id: proj_policy-…`). The corrected rerun reported
  `skipped_idempotent: 4` and **silently kept the wrong buckets** — the idempotency key
  evidently doesn't include scope/namespace assignment. Rerunning an import after fixing
  a mapping bug can't repair anything. Consider: (a) make import transactional per-run or
  per-memory with rollback on step failure, (b) include bucket assignment in the dedup
  identity, or (c) add a `memoryd import --repair-buckets` / re-place pass.
- Also: namespace/canonical-id disagreement within one file (policy canonical id under
  agent-memory namespace+directory) suggests directory placement and namespace derive
  from different inputs than `canonical_namespace_id` — worth a substrate-side invariant.

### 8. Bare `memoryd doctor` fails — doc and CLI defaults disagree

- **MAJOR (doc or CLI bug):** agent-onboarding.md Step 5 says run `memoryd doctor` and
  `memoryd status` with no flags unless paths are non-default. Bare `memoryd doctor`
  fails with `not a Memorum substrate: .` — it defaults to the **current working
  directory**, not `~/memorum`/`$MEMORUM_REPO`. Every doc-following agent hits this.
  Either doctor should default its repo to the same default `init` uses, or the doc must
  always pass `--repo`/`--runtime`. (Same check needed for `status` socket default.)
- **Nit (doc drift):** agent-onboarding says doctor/status have no JSON mode; doctor's
  output here was JSON-shaped. Align doc with reality.
- **Praise:** doctor's `harness_cli_warning` (claude CLI auth probe failed) with a
  concrete `repair:` string is exactly the right shape for agent consumption.

### 9. Non-interactive `generate` writes into cloud-synced dirs without the warning

- **Friction:** the interactive wizard warns before writing `.memory-project.yaml` into
  a synced dir ("visible on other machines"). The non-interactive `generate` default
  wrote `/Users/treygoff/…/Dropbox/Prospera/Policy/.memory-project.yaml` with no
  warning surfaced. The report's `cwd_dispositions` does list the write (good), but a
  `synced_dir: dropbox` flag on the disposition + a stderr warning would let agents
  relay the same caution the wizard gives humans.

### 10. MCP wiring is project-scoped — defeats the product's core promise

- **MAJOR:** `wire_mcp` wrote the memorum server entry under
  `projects."/Users/treygoff/Code/agent-memory".mcpServers` in
  `~/.claude-personal/.claude.json` — i.e. **local scope, this repo only**. A user who
  installs a cross-project memory layer gets a tool that vanishes the moment they cd
  elsewhere. Wiring should be user-scope (`claude mcp add --scope user`, or the
  top-level `mcpServers` key). If project scope is intentional for some reason, the
  report message ("Claude: wired") must say which scope and the docs must say how to
  promote it.
- **Praise:** wiring honored `CLAUDE_CONFIG_DIR` (landed in `~/.claude-personal`, not
  `~/.claude`), and the on-demand `--auto-start true` arg shape looks right.

### 11. Import assigns every memory the session project's namespace/directory

- **MAJOR:** even on a fully clean rerun with the fixed decoder, all 4 memories got
  `namespace: agent-memory` and landed under `projects/agent-memory/decisions/` — the
  project of the directory `memoryd init` was *run from* — while their
  `canonical_namespace_id` values are correct and distinct (`proj_a17c…` for the repo
  memory, `proj_policy-…` for the three Policy ones). `build_write_meta`
  (`import/pipeline.rs`) sends only the generic string `"project"` as namespace plus the
  canonical id; the daemon write path then resolves "project" from the *session/daemon
  context* instead of honoring the per-memory canonical id. Consequences: directory
  placement is wrong, and project scoping at recall time returns Policy memories inside
  agent-memory sessions (observed). Placement and `namespace` must derive from
  `canonical_namespace_id` when the write meta carries one.

### 12. Recall during embedding warm-up; search smoke tests

- **Friction:** immediately after import (embedding worker not yet run; doctor showed
  `embedding_worker_idle`, vector table empty), a natural multi-word query
  (`"eval gated merge order worktree"`) returned 0 hits — strict FTS AND missed and no
  vector lane existed yet. After warm-up, single-term and paraphrase queries hit
  correctly. Onboarding docs should tell agents the first recall may be FTS-only and to
  prefer doctor's embedding status before judging search quality; or `memory_search`
  could surface `"vector_lane": "warming"` in its response.
- **Praise:** doctor's `embedding_worker_idle` finding described this state precisely,
  including the repair.

### 13. MCP bridge smoke tests (simulated harness restart)

- **Praise:** `memoryd mcp --auto-start true` works exactly as wired: handshake clean,
  daemon auto-started from cold, all nine tools listed, `memory_search` and
  `memory_startup` return well-formed payloads. The recall block (`stream-e-v0.6`)
  resolved the project via git remote correctly.
- **Bug:** `tools/call memory_startup` with **missing required arguments** (`cwd`,
  `session_id`, `harness`) returns a silent empty success (`result: {}`) instead of an
  MCP invalid-params error / `isError: true`. A harness misusing the tool gets nothing
  and no explanation. Also `{}` violates the tool's own declared `outputSchema`
  (five required properties).
- **Nit:** `memoryd status` against a not-yet-started on-demand socket prints a raw
  `Connection refused (os error 61)` error. Expected per docs, but a one-line hint
  ("daemon is on-demand; it starts when an MCP client connects") would prevent agents
  from misreading it as a failure.

### 14. restart_required semantics on reruns

- **Nit:** the run that actually wired MCP reported `restart_required: true`, but
  subsequent reruns (e.g. after fixing an import failure) report `wire_mcp: "already
  wired"` with `restart_required: false`. True for that run in isolation, but an agent
  that retries init after a failure and reads only the final report will skip the
  mandatory restart instruction. Consider making `restart_required` sticky whenever
  wiring exists that the current session has not yet loaded, or document that agents
  must OR the flag across runs.

## Summary of outcomes

Flow completed end-to-end after in-run fixes. Final state: binaries installed, repo at
`~/memorum`, 4 Claude memories imported (3 mis-bucketed under the session project —
finding 7/11), MCP wired project-scope for this checkout (finding 10), daemon on-demand
and verified via real MCP handshake + recall calls.

**Shipped in this session:** merge-driver name fix (installer + 4 docs, finding 1);
filesystem-aware Claude cwd decoder + 3 tests (finding 6).

**Punch list by severity** (snapshot at dogfood time — the addenda below record the
fix waves; as of the evening addendum every major here plus 15/16/17 is fixed):
- Blockers (fixed): installer name mismatch (1); lossy cwd decode (6).
- Major (open): `current` silently no-ops on dual-harness machines + misleading skip
  messages (4); sticky wrong buckets from failed runs / non-atomic import (7);
  bare `doctor` defaults to cwd, contradicting docs (8); project-scoped MCP wiring (10);
  namespace/placement ignores canonical_namespace_id (11); memory_startup silent `{}`
  on missing args (13).
- Friction (open): exit-code masking / success sentinel guidance (1); Decision-3
  explanation script for users (2); installer-vs-init daemon overlap (5); synced-dir
  warning absent on non-interactive generate (9); FTS-only warm-up window (12).
- Nits: detection JSON lacks promised repo field (3); doctor/status JSON-mode doc drift
  (8); status connection-refused hint (13); restart_required stickiness (14).

## Addendum — fix wave, 2026-06-12 afternoon

All six open majors were fixed via delegated lanes (Codex: findings 4, 7+11, 13;
Cursor: findings 8, 10) with orchestrator review and integration fixes, plus a new
`memoryd uninstall` subcommand (Opus subagent) reversing init/installer provisioning.
The fix wave surfaced two NEW findings:

### 15. Daemon write path hardcodes the project namespace

- **MAJOR (open):** `crates/memoryd/src/handlers/mod.rs` declares
  `const DEFAULT_PROJECT_NAMESPACE: &str = "agent-memory"` — a dev placeholder. Every
  project-scoped write that does not carry a `canonical_namespace_id` in its meta
  buckets under the literal directory `projects/agent-memory/`, regardless of which
  project the session is in. The fix wave routed *import* writes correctly (they now
  carry canonical id + alias), but ordinary `memory_write` calls from live MCP sessions
  still hit the constant. The write path needs session-binding-aware placement (the
  daemon already resolves the project for `memory_startup`; writes should use the same
  binding). This explains finding 11's directory symptom at the deepest level.

### 17. `memoryd uninstall` first dogfood: three defects in one run

- **MAJOR (open):** ran the new `uninstall --non-interactive --json --purge` against a
  live install. (a) `stop_daemon` failed — daemons launched via `memoryd mcp
  --auto-start` write no pid file, so the pid-file SIGTERM path can't find them; needs a
  socket-holder fallback (shutdown RPC, or lsof/fuser on the socket). (b) Despite the
  failed stop, `purge_data` **proceeded and deleted repo+runtime out from under the
  running daemon**, leaving an orphaned process whose socket "verified gone" only
  because the directory was deleted — purge must be gated on the daemon actually being
  stopped. (c) The run exited **0 with a failed step**, violating the init-mirrored
  contract (non-zero when any step fails). Everything else (print-only preview, scoped
  unwire of the stale project-scope entry, purge, leftover-binary report) behaved
  exactly as specced.

### 18. Symlinked memory dirs are invisible to import discovery (fixed in-run)

- **MAJOR (fixed in-run):** Claude discovery walked with `follow_links(false)`, so any
  project whose `<encoded>/memory/` is a symlink contributes zero candidates, silently.
  This is the normal state on shared-profile machines — C-Mux's `~/.claude-shared`
  migration symlinked the memory dirs mid-day, and detection dropped from the project's
  real corpus to whatever wasn't yet migrated. After flipping to `follow_links(true)`
  (+ regression test), detection went from 3 candidates to **228**. The morning runs'
  "4 candidates" were already partially suppressed. Lesson for the eval harness: assert
  candidate counts against a known fixture corpus, not just "import succeeded".

### 19. Bulk import at real scale: repair-supersede corrupts the chunk index

- **BLOCKER (open, substrate — Codex's stream):** the 228-candidate import wrote 202
  memories (privacy refusals firing correctly along the way), then aborted
  deterministically on a RepairBucket supersede with `write failed: index failed after
  commit (retryable=true)`. After that, `memoryd doctor` reports `operator repair
  required: UNIQUE constraint failed: memory_chunks.chunk_id` and `doctor --reindex`
  fails replaying the events log with the same constraint — committed duplicate chunk
  events survive replay, so the index cannot be rebuilt. Recall returns zero hits on
  the corrupted store. Matches the supersession-FK/bulk-reindex issue in
  `docs/2026-06-10-for-substrate-owner-supersession-fk-bulk-reindex.md`; live repro
  preserved in `~/memorum` and peer note left at
  `docs/2026-06-12-for-codex-import-repair-supersede-index-corruption.md`.
- **Nit:** lane D's PartialExecute message ("aborted ... after N memories had already
  been written") worked exactly as designed — keep it.
- **Nit:** git-remote projects bucket under raw canonical-id directories
  (`projects/proj_ffe3aa…/`); friendly aliases (repo basename) would make the repo
  browsable. Policy projects with a `.memory-project.yaml` alias got readable dirs.

### 16. Non-init socket commands resolve a different default runtime

- **Friction (open):** `memoryd search/get/mcp/...` default their socket via
  `default_runtime_root()` (`$MEMORUM_RUNTIME` / `~/.local/share/memorum/runtime`),
  while init/doctor/status now share the init-aligned default
  (`$MEMORUM_REPO`→`~/memorum`, runtime `<repo>/.memoryd`). Two different "default
  socket" answers in one binary. Align the remaining commands on the shared helper
  (surfaced by the lane B implementation report).

## Addendum — fix wave 2, 2026-06-12 evening

Findings 15, 16, and 17 root-caused (orchestrator), designed, and fixed via three
parallel `delegate codex work` worktree lanes with orchestrator review; two review
catches fixed at integration. All on `main`, full `scripts/check.sh` green.

### 15 — FIXED (`fix(write): resolve project namespace from session cwd` + follow-up)

Root cause ran deeper than the constant: live `memory_write`/`memory_supersede` meta
carries no project identity at all, so the daemon had nothing to place by. Fix is
stateless cwd-based resolution (a session_id→binding registry was rejected — the
on-demand daemon can restart between `memory_startup` and a write): the MCP bridge
injects its process cwd into write/supersede meta when absent; the daemon resolves it
through the same `resolve_project_binding` as startup; explicit
`canonical_namespace_id` wins; unresolvable project writes are refused with an
actionable message (no silent fallback); `DEFAULT_PROJECT_NAMESPACE` is deleted.
**Review catch:** the lane's supersede inheritance deferred to `cwd` when present —
but the bridge injects cwd into *every* supersede, so superseding a project-X memory
from a project-Y terminal would have relocated it to `projects/Y/`. Inheritance from
the old memory's frontmatter now beats cwd (foreign-cwd regression e2e added).

### 16 — FIXED (`fix(cli): unify daemon socket defaults`)

One canonical client default in `cli/paths.rs::default_socket()`:
`$MEMORUM_RUNTIME` → `<resolved repo>/.memoryd/memoryd.sock`. All connect-only
commands (import, memory ops, review, web, ui, reality-check, source, peer, mcp,
setup detect) route through it; the XDG `socket::default_runtime_root()` and the two
duplicate resolvers in `cli/mod.rs` are deleted. `McpArgs --repo/--runtime` moved
from cwd-relative clap defaults to canonical resolution (wired configs pass explicit
flags, so installed entries are unaffected). Side catch: the pre-fix socket mismatch
had left a stray `.memorum/import-state.json` in this repo's root from a misdirected
import run — removed, and `.memorum/` added to `.oxfmtignore`.

### 17 — FIXED (`fix(uninstall): stop pidless daemons safely` + follow-up)

Deepest root cause: **no code path ever wrote `<runtime>/memoryd.pid`** — uninstall's
SIGTERM path read a file nothing creates. (a) `memoryd serve` now writes the pid file
at startup via a drop guard that only removes it if it still holds its own pid; every
daemon (direct serve, launchd, MCP auto-start) funnels through serve, so all get it.
(b) For pid-file-less daemons, `stop_daemon` asks the live socket for its pid via the
`Status` RPC (2s timeout) before SIGTERM. (c) Purge is gated: a failed stop refuses
to purge, preserving repo+runtime. (d) Non-zero exit on any failed step pinned by an
end-to-end regression test (real serve child, real SIGTERM). **Review catch:** the
lane's purge gate fired before the `--purge` check, reporting a phantom purge refusal
on non-purge runs — reordered, with a unit test pinning Skipped.
