# Live harvest: daemon-scheduled auto-import of harness auto-memory

**Date:** 2026-07-19
**Author:** Claude (coordinator), foundry build loop
**Status:** Amended after design review (Sol xhigh, 2026-07-19) — ready to implement

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

- Each iteration **re-reads the device-local config** (F8: enable/disable/interval
  take effect at the next tick, no restart, no watcher). Disabled → sleep a recheck
  interval (5 min) and loop.
- Due computation: run when `now >= last_success_at + interval` per
  `harvest-state.json`, or when the state file records no prior success. Startup is
  not special-cased (F7): a restart only harvests if a run is actually overdue.
- A due tick calls `run_import_session` with `SocketDaemonClient` pointed at the
  daemon's **own socket**, `FixedDispositionBackend(DeriveProject)`, `quiet`,
  `dry_run: false` — wrapped in `tokio::time::timeout` (10 min bound) and raced
  against the shutdown signal in the task's `select!` (F2): shutdown drops the
  in-flight future; the importer is crash-safe by construction (atomic state saves +
  daemon dedup), so cancellation mid-import is recoverable, not corrupting.
- Default interval **30 minutes**, clamp 5–1440.

Why not lifecycle hooks (SessionEnd) or a launchd timer: hooks require wiring into
every harness profile (9+ configs on this machine), fire on every session everywhere,
and only cover harnesses whose hook systems we wire; a launchd timer is a second
launchd unit with its own failure modes (this project has a scarred history there).
The daemon is already always-on (API embedding lane made always-warm the default,
2026-07-19); one internal task covers all sources with zero external wiring, and the
importer's flock already arbitrates against concurrent manual `memoryd import`.

### Change detection: none in v1 (review F1)

The originally proposed mtime fingerprint pre-scan was cut as the review's blocker:
an aggregate `(max mtime, file count)` is not a valid change detector (same-second
rewrites, non-max-file changes, delete/add substitutions can all alias), and a wrong
skip suppresses harvests indefinitely. The idempotent no-op pipeline run is already
sub-second at this corpus scale, so every due tick simply runs it. If corpus growth
ever makes that expensive, the correct future mechanism is a per-file content
manifest updated only after clean runs — not an aggregate fingerprint.

### Locking and re-entrancy (review F3)

- Scheduled runs acquire the import flock with a **zero timeout**
  (`ImportLockGuard::acquire_with_timeout(…, Duration::ZERO)` threaded through a
  scheduler-facing session entrypoint): contention with a manual `memoryd import`
  means log-at-debug and skip the tick — never a 5-second blocking sleep loop on a
  tokio worker.
- The self-socket client means harvest writes flow through the identical handler path
  as any client — privacy, governance, events, commit-on-write (F1) all apply. No new
  write path, importer invariant 2 preserved. (Traced in review: no lock cycle exists
  between the harvest task, connection handlers, the governance mutex, or the git
  commit worker.)
- Per-harness failure isolation (review F10): a discovery failure in one harness
  (e.g. a half-written Claude settings.json) is recorded as that harness's error for
  the tick and must not abort the other harness's plan.

### Config: `harvest` section in the **device-local** config (review F4/F7)

Harness stores, harvest cost, and cadence are per-device concerns; the synced
`config.yaml` is the wrong scope (one laptop's `disable` must not propagate to the
desktop). The `harvest` section therefore lives in the device-local config
(`LocalDeviceConfig` idiom, `#[serde(default)]`):

```yaml
harvest:
  enabled: true        # ABSENT = disabled (upgrade-safe: existing installs opt in)
  interval_minutes: 30 # clamp: min 5, max 1440
```

Default for an absent section is **disabled** — upgrading the daemon changes nothing
until the operator runs the explicit ceremony. Rationale for the reversal from the
draft's default-on: autonomous writes appearing after a routine upgrade is a consent
problem, and the runtime-loop "close by default" philosophy is satisfied by fresh
onboarding opting in explicitly instead (deferred to a future `memoryd init` pass —
out of scope here).

CLI ceremony: `memoryd config harvest enable|disable [--interval-minutes N]`, editing
the local config and emitting the v1 agent envelope like the embedding-lane ceremony.
Takes effect at the next scheduler iteration (config re-read per tick — no
`restart_required`). Additive amendment to `docs/api/memoryd-cli-contract-v1.md`.

### Observability (review F9/F11)

- New `harvest-state.json` in the runtime root, written **only** by the scheduler
  task (single writer, atomic tmp-with-unique-name + rename). `DaemonState` is not
  touched — its whole-file load/modify/save pattern would race a periodic writer
  (lost snoozes).
- Contents (bounded): `last_attempt_at`, `last_success_at`, `next_due`, per-harness
  `{parsed, written, refused, quarantined, skipped}`, bounded `last_error` string,
  `active_embedding_lane` at last run.
- `memoryd doctor` reports the block (enabled flag + interval from local config,
  history from harvest-state.json) — answering "why wasn't my memory harvested"
  without log spelunking. Doctor stays a raw daemon frame.
- Per-tick tracing: info on writes > 0, debug on skip/contention.

### Spec surface

Additive, no behavior change to existing surface → dated amendment blocks (per repo
convention) in:
- `docs/specs/system-v0.3.md` — harvest scheduler section (trigger, config, invariants
  preserved).
- `docs/api/memoryd-cli-contract-v1.md` — `config harvest` subcommand.
- `docs/runbooks/` — short operator note (how to disable, how to read doctor output).

## Cost basis (measured; embedding economics per review F12)

- Source corpus today: 13 Claude project memory dirs (largest file ~40KB), Codex
  `MEMORY.md` 42KB + notes. A full no-op pipeline run is sub-second; steady state is
  48 sub-second idempotent runs/day at the default interval.
- Write volume: bounded by what harnesses author — historically single-digit
  files/day on this machine. Classification volume rises accordingly; the quarantine
  review flow (`memoryd review`) is the existing relief valve and doctor already
  surfaces quarantine counts.
- Embedding: on the API lane (this machine's live config, always-warm HTTP client at
  11–17MB) per-memory cost is one embed call — negligible. On a **local-lane** install
  with the 15-min idle unload, a trickle of one new memory per tick can cycle the
  6.2GB model once per interval; the runbook documents this and recommends local-lane
  installs choose an interval ≥ their unload window or accept the reload cost.
  `active_embedding_lane` in harvest-state makes the operative lane visible.

## Testing strategy

- Unit: `HarvestConfig` defaults/validation/clamps + absent-section-is-disabled;
  due computation (never-run → due; recent success → not due; overdue after restart →
  due); config re-read picks up disable/interval mid-loop; lock-contention tick skips
  without error and without blocking; per-harness discovery-failure isolation;
  harvest-state.json atomic write + bounded error truncation.
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

1. **Wave 1 (Codex Sol, `work`):** `HarvestConfig` in the device-local config + CLI
   ceremony; scheduler task in `server.rs` (due-based, config re-read per tick,
   zero-timeout lock, shutdown race, per-tick timeout); per-harness discovery
   containment; single-writer `harvest-state.json` + doctor rendering;
   unit/integration tests; doc amendments.
2. **Review round:** coordinator riskiest-file read (scheduler + self-socket client
   interaction) + native Opus adversarial review (reduced-foundry config: Codex
   authored, Claude-family reviews). Fix round on accepted findings; re-review until dry.
3. **Live acceptance** per testing strategy, on the real `~/memorum` daemon.

Constraints inherited from the repo: CPU discipline (scoped gates only, one final
`scripts/check.sh`), commits ungated / push gated, bench baselines untouched.

## Design-review dispositions (Sol xhigh via delegate, 2026-07-19)

13 findings (1 blocker, 12 major). Every finding triaged in writing:

| # | Finding | Disposition |
| --- | --- | --- |
| 1 | Fingerprint pre-scan can suppress harvests indefinitely (aggregate mtime aliases) | **Accept — cut the feature.** Every due tick runs the sub-second idempotent pipeline. |
| 2 | Self-socket lifecycle/shutdown ordering | **Accept-reduced.** Tick future raced against shutdown in `select!` (drop = cancel; importer is crash-safe) + 10-min per-tick timeout. No full drain choreography. |
| 3 | 5s blocking flock poll on a tokio worker | **Accept.** Zero-timeout acquisition for scheduled runs → true skip-on-contention. Parse work is ms at measured corpus scale; no executor offload. |
| 4 | Synced config is the wrong scope for a per-device behavior | **Accept.** `harvest` moves to the device-local config. |
| 5 | Cross-device dedup not guaranteed for overlapping sources | **Waived for v1.** Single-device live deployment; `source_identity` is profile-relative by design; residual multi-device duplicates are bounded and consolidated by governance/dreaming. Revisit before multi-device GA. |
| 6 | `DeriveProject` mints junk/unstable namespaces for weird cwds | **Accept-reduced.** Keep DeriveProject (shipped non-interactive default; `Skip` would permanently orphan non-git dirs — the exact gap this build closes). Persisted per-source bindings in import state stabilize re-runs. Watch item. |
| 7 | Default-on is unsafe for upgrades; unconditional startup tick defeats cadence | **Accept.** Absent section = disabled; explicit enable ceremony; due-based scheduling (restart only harvests if overdue). |
| 8 | CLI config command lacks an effective-runtime contract | **Accept — dissolved.** Scheduler re-reads local config each tick; changes take effect next tick, no restart semantics needed. |
| 9 | Harvest fields in `DaemonState` create lost-update/temp-file races | **Accept.** Separate single-writer `harvest-state.json` with unique-temp atomic rename. |
| 10 | One harness's discovery failure suppresses the other | **Accept-reduced.** Per-harness containment: record the error, continue the other harness. |
| 11 | Observability can't answer "why wasn't this harvested" | **Accept-reduced.** Bounded harvest-state block (per-harness counts, last error, next due, lane) + doctor rendering. Not a full report ledger. |
| 12 | Cost model omitted embedding/resident-model behavior | **Accept as documentation.** API lane (live config) ≈ zero; local-lane cadence guidance in runbook; lane surfaced in harvest-state. |
| 13 | Amendment misclassified — autonomous ingestion warrants a spec version bump | **Accept-reduced.** With default-off-on-upgrade, absent config changes no behavior, so dated amendments stand; Sol's v0.4-bump argument is flagged to Trey (version bumps are his call by standing repo rule). |
