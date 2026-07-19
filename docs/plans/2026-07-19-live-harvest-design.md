# Live harvest: daemon-scheduled auto-import of harness auto-memory

**Date:** 2026-07-19
**Author:** Claude (coordinator), foundry build loop
**Status:** Draft for design review

## Thesis and provenance

Harness-native auto-memory (Claude Code's per-project memory directories, Codex's
`~/.codex/memories/`) is where agents actually write, because harness system prompts
instruct them to — an in-context standing instruction beats any skill the model must
choose to reach for. Memorum's differentiated value is the substrate underneath every
write surface: cross-harness recall, namespacing, privacy classification, governance,
dreaming. The right architecture is therefore **harness memory = write buffer,
Memorum = harvester + archive + recall**, not competing for the write.

Provenance of the gap: on 2026-07-19 the coordinating agent for the namespace-leak fix
wave — working *inside this repo, with the daemon live* — wrote its session memory to
Claude Code's built-in memory, exactly as its harness instructs. That memory sits
unharvested until someone manually runs `memoryd import`. Trey's directive: close the
loop — "auto harvest harness level memories from all harnesses on my machine that have
auto memory."

This is also the lazy 80% of the "ambient capture" gap flagged by the 2026-06-25
runtime-loop dogfood (see `docs/specs/memorum-runtime-loop-foundation-v0.2.md`):
for the harness-auto-memory source class, a scheduled harvest of files the harnesses
already write is ambient capture with zero new capture machinery.

## Recon: which harnesses have local auto-memory (measured 2026-07-19)

Probed on Trey's machine (all six CLIs installed and used):

| Harness | Storage probed | Verdict |
| --- | --- | --- |
| Claude Code | `~/.claude{,-personal,-work}/projects/<slug>/memory/*.md` | **YES** — 13 project memory dirs across 3 profiles. Importer adapter exists (`sources/claude.rs`). |
| Codex CLI | `~/.codex/memories/` (`MEMORY.md` 42KB, `raw_memories.md` 383KB, `memory_summary.md`, `rollout_summaries/`, backing `memories_1.sqlite` stage1/phase2 pipeline) | **YES** — importer adapter exists (`sources/codex.rs`; reads `MEMORY.md` task groups + `extensions/ad_hoc/notes/*.md`). |
| Factory / droid | `~/.factory/` — sessions, missions, specs, skills, configs. `grep -i memory` over configs: only hits are the literal repo name `agent-memory` in session paths | **NO** local auto-memory store. |
| opencode | `~/.local/share/opencode/` (repos, snapshot, storage/{migration,session_diff}, tool-output), `~/.config/opencode/` | **NO** — session snapshots/diffs only, no memory store. |
| pi | `~/.pi/agent/` — settings.json, sessions, extensions, skills, themes | **NO** — no memory store, no memory extension installed. |
| omp | `~/.omp/agent/` — `agent.db` (auth/models/usage/settings tables), `history.db` (shell-history FTS), sessions | **NO** — no memory tables. |

**Conclusion: the two harnesses with auto-memory are exactly the two the importer
already supports.** No new source adapters are needed. The feature is purely: make the
existing importer run automatically.

Non-goal recorded with evidence so a future re-check is cheap: if factory/opencode/pi/omp
ship auto-memory later, that is a new `sources/<harness>.rs` adapter build, out of scope
here.

## What exists (measured against the shipped code)

- `run_import_session` (`import/pipeline/mod.rs`) is a complete reusable entrypoint:
  acquires the flock (`ImportLockGuard`, 5s timeout, pid file), loads
  `.memorum/import-state.json`, plans, executes through the daemon socket, saves state
  crash-safely. CLI and setup-engine both already call it.
- Idempotency is two-layer: state file (performance) + daemon duplicate-detection
  (correctness). Re-running against unchanged sources is cheap and safe.
- Re-ingestion-loop protection: `strip_memorum_recall_blocks` removes injected
  `<memory-recall>`/`<memory-delta>` blocks before parsing, so recall output written
  back into harness memory is not re-imported as fresh content.
- Provenance: every imported candidate is stamped `source_provenance: harness-auto-memory`,
  `confidence: 0.7` (below the hand-written 0.85 baseline), `source.kind = import`,
  `source.ref = <absolute path>`.
- Privacy/governance: all writes go through the daemon socket, so classification
  (including the API-lane fence) and governance quarantine apply unchanged.
- Non-interactive disposition: `FixedDispositionBackend(DeriveProject)` — never prompts,
  never loses memories; non-git cwds derive a project namespace from the path.
- Multi-profile discovery: the Claude root scan auto-detects `~/.claude-*` sibling
  profiles (`DiscoverySource::DetectedProfile`).
- The daemon already runs a background interval task (`spawn_reality_check_scheduler`,
  `server.rs`) — the exact pattern to graft onto — and per-feature config structs
  (`DreamConfig` et al.) show the config idiom.

## Design

### Trigger: daemon-internal periodic scheduler (not hooks, not launchd)

A tokio background task in `memoryd` (same shape as `spawn_reality_check_scheduler`):

- First tick **2 minutes after daemon startup** (settle window), then every
  `interval_minutes` (default **30**).
- Each tick: cheap change-detection pre-scan (below); if changed, call
  `run_import_session` with `SocketDaemonClient` pointed at the daemon's **own socket**,
  `FixedDispositionBackend(DeriveProject)`, `quiet`, `dry_run: false`.
- `MissedTickBehavior::Skip`; task exits on daemon shutdown signal.

Why not lifecycle hooks (SessionEnd) or a launchd timer: hooks require wiring into
every harness profile (9+ configs on this machine), fire on every session everywhere,
and only cover harnesses whose hook systems we wire; a launchd timer is a second
launchd unit with its own failure modes (this project has a scarred history there).
The daemon is already always-on (API embedding lane made always-warm the default,
2026-07-19); one internal task covers all sources with zero external wiring, and the
importer's flock already arbitrates against concurrent manual `memoryd import`.

### Change detection: mtime fingerprint pre-scan

Before invoking the pipeline, scan the discovered source roots (Claude memory dirs
across profiles + Codex memory root) and compute a fingerprint: max `(mtime, path
count)` over the candidate files the adapters would read. If equal to the previous
tick's fingerprint (held **in memory** — a daemon restart just re-runs one cheap
idempotent harvest), skip the tick entirely. This keeps steady-state cost at one
directory walk per interval, no parsing, no socket traffic.

### Locking and re-entrancy

- The harvest tick calls `run_import_session`, which takes the existing flock. If a
  manual import holds it, the tick logs at debug and skips (`AnotherImportInProgress`
  is expected contention, not an error).
- The self-socket client means harvest writes flow through the identical handler path
  as any client — privacy, governance, events, commit-on-write (F1) all apply. No new
  write path, importer invariant 2 preserved.

### Config: `harvest` section in synced `config.yaml`

```yaml
harvest:
  enabled: true        # default ON — the loop should close by default
  interval_minutes: 30 # clamp: min 5, max 1440
```

Struct `HarvestConfig` following the `DreamConfig` idiom (serde, `Default` impl,
validation on load). Default **on**: harvest is read-only against sources, idempotent,
and fully fenced by classification/governance; a memory system that only harvests when
the operator remembers to ask recreates the exact gap this build closes. Machine-local
runtime data (last-run info) does NOT go in the synced config (invariant 4 discipline).

CLI: `memoryd config harvest enable|disable [--interval-minutes N]`, emitting the v1
agent envelope like the embedding-lane ceremony. Additive amendment to
`docs/api/memoryd-cli-contract-v1.md`.

### Observability

- `DaemonState` (local runtime state) gains a `harvest` block: `last_run_at`,
  `last_outcome` (`imported {n}` / `skipped_unchanged` / `skipped_locked` / error
  string), `last_imported_count`.
- `memoryd doctor` reports it: enabled flag, interval, last run, last outcome. Doctor
  stays a raw daemon frame (no envelope change).
- Per-tick tracing: info on writes > 0, debug on skip.

### Spec surface

Additive, no behavior change to existing surface → dated amendment blocks (per repo
convention) in:
- `docs/specs/system-v0.3.md` — harvest scheduler section (trigger, config, invariants
  preserved).
- `docs/api/memoryd-cli-contract-v1.md` — `config harvest` subcommand.
- `docs/runbooks/` — short operator note (how to disable, how to read doctor output).

## Cost basis (measured)

- Source corpus today: 13 Claude project memory dirs (largest file ~40KB), Codex
  `MEMORY.md` 42KB + notes. A full no-op pipeline run is sub-second; the fingerprint
  pre-scan reduces steady state to a directory walk (tens of files) every 30 min.
- Write volume: bounded by what harnesses author — historically single-digit
  files/day on this machine. Classification volume rises accordingly; the quarantine
  review flow (`memoryd review`) is the existing relief valve and doctor already
  surfaces quarantine counts.

## Testing strategy

- Unit: `HarvestConfig` defaults/validation/clamps; fingerprint scan (change → run,
  no change → skip, missing roots → skip quietly); scheduler tick gating with a mock
  client; lock-contention tick skips without error.
- Integration (existing harness patterns in `memoryd` tests): end-to-end tick against
  a temp repo + fake source dir → memory lands with import provenance; second tick
  no-ops; source file edit → supersession path (already covered by importer tests,
  spot-assert only).
- Gate: scoped `cargo test -p memoryd -- --test-threads=2` inner loop; full
  `bash scripts/check.sh` once on the integrated trunk at the end.
- Live acceptance (the bug hunt, on `~/memorum`): rebuild + restart daemon; confirm
  first tick harvests today's real unharvested memory
  (`cross-namespace-recall-leak-fixed.md` update written 2026-07-19); confirm doctor
  block; confirm steady-state skip; write a fresh Claude memory, wait one interval,
  confirm it lands classified + namespaced; confirm manual `memoryd import` still
  works alongside.

## Non-goals

- No new harness adapters (recon table above — nothing to adapt to).
- No event-driven per-session hooks for harvest (the 30-min interval is "live enough";
  revisit only with evidence of a real staleness pain).
- No ingestion of Codex `raw_memories.md` / `rollout_summaries/` (deliberate: the
  adapter reads the distilled `MEMORY.md`; raw stage-1 material is noise at recall).
- No MCP surface changes (frozen at 10 tools).
- No changes to import parsing, project mapping, or dedup semantics.

## Wave plan

Single implementation wave (the change is one scheduler + config + doctor surface,
~4 files touched plus tests):

1. **Wave 1 (Codex Sol, `work`):** `HarvestConfig` + config plumbing + CLI subcommand;
   scheduler task in `server.rs`; fingerprint pre-scan; `DaemonState.harvest` block +
   doctor; unit/integration tests; doc amendments.
2. **Review round:** coordinator riskiest-file read (scheduler + self-socket client
   interaction) + native Opus adversarial review (reduced-foundry config: Codex
   authored, Claude-family reviews). Fix round on accepted findings; re-review until dry.
3. **Live acceptance** per testing strategy, on the real `~/memorum` daemon.

Constraints inherited from the repo: CPU discipline (scoped gates only, one final
`scripts/check.sh`), commits ungated / push gated, bench baselines untouched.
