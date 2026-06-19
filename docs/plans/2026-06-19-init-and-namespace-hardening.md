# Init + namespace hardening plan — 2026-06-19

Implements the three follow-up findings (8, 9, 10) surfaced by the 2026-06-19 re-dogfood in `docs/2026-06-18-import-dogfood-log.md`, plus one LOW refinement (dream-scratch walk pruning). Branch: `init-namespace-hardening`. Owner: Claude (autonomous), rust-engineer discipline, subagent-implemented, plan-reviewed before build, Codex-reviewed before merge, then re-dogfooded sandbox-first then live.

## Root-cause confirmations (read before designing)

- **Import owns its own daemon.** `SystemSetupRuntime::run_import_session` (`setup/steps.rs:105-126`) starts a `TransientImportDaemon` bound to `plan.detection.daemon.socket_path` for the duration of the write and reaps it on exit. The importer writes through a `SocketDaemonClient`; it does **not** reuse the persistent (launchd/background) daemon. A dry-run starts no transient.
- **Init order forces socket competition.** `run_all_with_runtime` (`setup/steps.rs:39-52`) runs `ensure_daemon` *before* `run_import`. Under `Launchd`/`Background`, `ensure_daemon` binds the persistent daemon to the socket; then `run_import`'s transient wants the same socket → the persistent daemon crash-loops on `socket_in_use` until the transient reaps. Net: `verify` can't connect (init exits 1) and the restart churn can leave the search index short until a `doctor --reindex`.
- **Dream/CCD parity gap.** `install_launchd` (`setup/steps.rs:457-476`) invokes `scripts/install-launchd.sh` with a hardcoded `--daemon` (daemon agent only) and **no** `--claude-config-dir`. The script defaults to installing **both** daemon + dream agents when neither `--daemon` nor `--dream-scheduler` is passed, and supports `--claude-config-dir` to pin `CLAUDE_CONFIG_DIR` in the daemon env (`scripts/install-launchd.sh:10,14,98-104,133-138,200-204`).
- **Namespace prose leak.** `parse_applies_to_cwd` (`import/sources/codex.rs:221-223`) does `applies_to_field(..,"cwd=",..).map(PathBuf::from)` — it takes the whole `cwd=` field value up to the next `;`, so trailing prose (`` cmux` on PATH) ``) rides along. `derive_alias_for_dir` (`import/project_map.rs:372-380`) only trims/truncates — unlike `derive_canonical_id_for_dir` (line 362) which filters to `[a-z0-9-]` — so the raw prose lands as the on-disk `projects/<alias>/` directory.

## Locked design decisions

1. **Finding 8 — reorder `ensure_daemon` after `run_import`.** New init order: `ensure_repo → run_import → ensure_daemon → wire_mcp → verify`. The import's self-managed transient daemon does the writes uninterrupted; only after it reaps (and the socket is free) does the persistent launchd/background daemon bind. `verify` (which needs the daemon over the socket) still runs last. Harmless for `OnDemand`/`None` (nothing competes). No change to the import's own transient-daemon logic.

2. **Finding 8 — make `verify_status` tolerant of the daemon handoff.** After the reorder, the just-installed launchd daemon may take a beat to bind. Add a bounded retry/backoff (a few short attempts) in `verify_status` (`setup/steps.rs:290`) so a slow bind doesn't read as a hard failure. Keep `verify_doctor` (in-process substrate open) unchanged.

3. **Finding 8 — index-completeness is verified, repaired only if short.** Primary expectation: with the competition removed, the transient daemon finishes its writes + index uninterrupted, so the index should already be complete. Add a defensive post-import check (count canonical memories vs indexed; if short, trigger a reindex) **only if** the sandbox re-dogfood still shows a short index after the reorder. Do not add reindex complexity speculatively — gate it on observed need.

3. **Finding 9 — thread Claude config dir + provision both agents.** Add `claude_config_dir: Option<PathBuf>` to `DaemonStepRequest`. Source it from the active Claude profile (env `CLAUDE_CONFIG_DIR`, canonicalized; fall back to the detection precedence root if unset, else `None`). In `install_launchd`: drop the hardcoded `--daemon` (so the script installs **both** daemon + dream agents, matching the documented default and the manual restore done on 2026-06-19), and pass `--claude-config-dir <path>` when present. `start_background_daemon` is unaffected (no launchd plist). No literal-`~` is ever passed (the script rejects it; canonicalize first).

5. **Finding 10 — tighten Codex cwd extraction + sanitize the alias (defense in depth).**
   - In `import/sources/codex.rs`, restrict the parsed `cwd` to a leading path-shaped token: accept only a value beginning with `/` or `~/`, taking the prefix up to the first whitespace/backtick/comma; reject (→ `None`, falls back to non-cwd handling) if it isn't path-shaped. Keeps `cwd=unknown` rejection.
   - In `import/project_map.rs`, sanitize `derive_alias_for_dir` to the same safe charset family used by `derive_canonical_id_for_dir` (ASCII alphanumerics, `-`, plus `_`/`.` if already accepted elsewhere), falling back to a stable placeholder when empty. This protects the on-disk `projects/<alias>/` directory from *any* malformed cwd, not just Codex's.

6. **Dream-scratch walk prune (LOW refinement).** In the Claude import walker (`import/sources/claude.rs`), prune descent into project dirs whose encoded name marks them as `memoryd-dream-scratch-run-*` (their `memory/` is always empty, so zero candidates — pure walk cost). Confirm the exact walkdir structure in code before choosing the prune predicate; prefer a `WalkDir` `filter_entry` / skip over post-hoc filtering so subtrees aren't descended.

## Invariant guards (must hold)

- No spec/plan version bump; no policy change (CLAUDE.md §critical-invariants, §what-not-to-do).
- Every write still carries a `ClassificationOutcome`; `secret` never persisted; privacy classification still runs per-write (finding 10 only sanitizes the *directory alias*, not classification).
- `scripts/check.sh` is the gate, run at the coordinator on the integrated branch — never per-subagent (lesson: self-selected per-subagent verification skews green).
- Do not touch `bench/baseline.*.json`; no `cargo generate-lockfile`.

## Work breakdown (file ownership)

| ID | Change | Primary files | Agent / wave |
|----|--------|---------------|--------------|
| WS1 | Findings 8+9: reorder init, verify retry, `DaemonStepRequest.claude_config_dir`, install both agents + `--claude-config-dir` | `setup/steps.rs` (order, `DaemonStepRequest`, `install_launchd`, `verify_status`), `setup/detect.rs` (active CCD), `cli/init/*` if a decision/flag is needed | Coupled → one focused subagent (rust-engineer) |
| WS2 | Finding 10: Codex cwd path-shape extraction + `derive_alias_for_dir` sanitize | `import/sources/codex.rs`, `import/project_map.rs` (+ unit tests) | Independent subagent (rust-engineer), parallel with WS1 |
| WS3 | Dream-scratch walk prune | `import/sources/claude.rs` (+ test) | Independent subagent (rust-engineer), parallel with WS1/WS2 |

WS1/WS2/WS3 touch disjoint files → run all three subagents concurrently. Coordinator integrates and runs the gate once.

## Execution tail

1. Plan-reviewer pass on this doc (Codex-conventions caveat N/A — this is a Claude-authored plan) → address findings before building.
2. Build WS1/WS2/WS3 via parallel rust-engineer subagents; coordinator finishes any trailing call-site/test leftovers (lesson: opus subagents land meaty edits then cut off before polish — verify on disk).
3. `bash scripts/check.sh` green at coordinator on `init-namespace-hardening`.
4. `/delegate-agent` Codex code review → fix findings.
5. Fast-forward merge to `main`.
6. **Re-dogfood, sandbox first (non-destructive):** rebuild `memoryd`; run `init --import` against a throwaway `--repo` temp dir; assert exit 0, daemon binds cleanly (no `socket_in_use` crash-loop in the daemon err log), `search` returns the freshly-imported memories with no manual reindex, the plist carries `CLAUDE_CONFIG_DIR`, both launchd agents are present, and no malformed `projects/<alias>/` dirs exist. Iterate here until clean.
7. **Then one live confirmation:** back up `~/memorum`, `uninstall --purge` → `init --import` on the real repo, verify the same checks end-to-end, and restore/confirm the launchd daemon + dream agent. Update the dogfood log with findings 8–10 marked verified.
