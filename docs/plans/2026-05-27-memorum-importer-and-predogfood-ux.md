# 2026-05-27 Memorum Importer + Pre-Dogfood UX Plan

## Goal

Ship a non-destructive, idempotent backfill importer that copies existing Claude Code and OpenAI Codex CLI memories into Memorum, and close the four pre-dogfood UX gaps that block a normal user's first-run experience. Importer lives as a new `memoryd import` CLI subcommand, reuses the existing daemon write path through the same socket protocol as MCP writes (no new `RequestPayload` variants, but `GovernanceMeta` is extended additively to carry import provenance), and ships behind an interactive `memoryd init` wizard that detects existing harness memory and offers to import it.

**Idempotency model (refined in v0.3):** the daemon's existing duplicate-detection in `governance::contradiction` is the *correctness* mechanism for "don't double-write the same memory." The import state file is a *performance* optimization that lets the importer skip parse/classify/socket round-trip for sources already confirmed-imported. A crash mid-import is safe to recover by re-running: the daemon catches duplicates via content-equality and returns the existing memory_id; the state file fills in from those responses.

## Scope

### In scope

- **`GovernanceMeta` extension** to accept caller-supplied `entities`, `aliases`, `related`, `evidence`, `supersedes`, and `canonical_namespace_id` (all optional, additive, backward-compatible). `import` added as a `source_kind` value. Required prerequisite for the importer (T00 below).
- `memoryd import` CLI subcommand with `--dry-run`, `--harness {claude|codex|all}`, `--from-claude <path>`, `--from-codex <path>`, `--report <path>` flags.
- `memoryd init` interactive wizard: detects existing harnesses, wires MCP, offers the importer, prints next steps.
- Source parsers for Claude Code auto-memory (`~/.claude/projects/<encoded>/memory/`) and Codex CLI memory (`~/.codex/memories/MEMORY.md`).
- State file at `$MEMORUM_REPO/.memorum/import-state.json` as performance optimization (not load-bearing for correctness — see Goal section).
- Project-mapping helper: compute `proj_<hex>` from git remote (by direct call to existing `recall::project::resolve_project_binding`, no factor-out); prompt per unique non-git cwd at import time.
- Pre-dogfood UX deliverables: README "why" intro doc, top-level `docs/troubleshooting.md`, first-write success signal (CLI-only, client-side detection in v1), the init wizard above.
- Unit tests per parser; integration test against fixture corpora; `memoryd doctor` runs clean after import.

### Out of scope (v2 or later)

- CLAUDE.md and AGENTS.md files (user-authored instructions, not learned memory).
- Codex `raw_memories.md`, `memory_summary.md`, `skills/` (intermediate / orthogonal).
- Subagent memory (`.claude/agent-memory/`) and Claude `rules/*.md` (fringe surfaces).
- Batch write protocol changes; importer loops sequential `memory_write` / `memory_note` calls over the existing socket. No new `RequestPayload` variants.
- Source-grounding via `memory_capture_source` for Codex rollouts; we attach raw `file://` evidence refs instead (locked decision Q4-γ).
- Model-based entity extraction; entities come from source frontmatter / keywords only (Q6-A).
- Auto-running the importer from `memoryd serve --init`; the new `memoryd init` wizard is the explicit-consent path (Q11-C).
- **Stream G `NotificationEvent::FirstWriteCompleted` amendment** and dashboard surface for first-write — descoped to a v2 plan; v1 ships only the CLI-side first-write signal.
- **Codex `task_outcome` → confidence mapping** (success=0.85, partial=0.65, etc.). All imports get `confidence: 0.5` in v1. Outcome-based mapping is a v2 question once we have recall-ranking evidence.
- Re-importing CLAUDE.md / AGENTS.md as future v2 work.

## Locked decisions (source: `docs/explainers/2026-05-27-importer-decisions.html`)

| Question | Locked answer |
|---|---|
| Granularity | Adaptive Claude (1 file when single-fact, decompose by `##` when multi-fact dossier with 3+ substantive sections); 1 Codex Task Group = 1 memory |
| Wiki-links | Two-pass alias resolution into `related: [memory_id]` |
| Write primitives | `memory_write` for **all** imported memories — Claude topic files, Codex Task Groups, Claude `user_profile.md`, and Codex `extensions/ad_hoc/notes/`. **⚠ v0.3 override of locked decision Q3:** Codex ad-hoc notes were locked to `memory_note`, but `memory_note`'s `RequestPayload::WriteNote` hardcodes `source.ref = "memoryd.write_note"` with no harness, no original path — incompatible with invariant #3 (provenance). Use `memory_write` for ad-hoc notes too; pay the sequential governance cost (~seconds per write) in exchange for preserved provenance. Skip Claude `MEMORY.md` index, Codex `raw_memories.md` / `memory_summary.md` / `skills/`, rollout summaries (handled as evidence per Q4). |
| Rollout summaries | Raw `file://` refs in `evidence[]` on the parent Task Group memory (γ) |
| Non-git cwds | Prompt per unique cwd at import time, offering: generate `.memory-project.yaml`, drop to `me` scope, or skip |
| Entity extraction | Source-provided only (Codex `### keywords` + Claude frontmatter `name`) |
| State file | `$MEMORUM_REPO/.memorum/import-state.json` |
| Conflict UX | Skip and log to import report |
| Throughput | Sequential, accept seconds-per-write |
| Re-import | Auto-supersede on content-hash change |
| First-run UX | Interactive `memoryd init` wizard offers the import |
| Pre-dogfood UX | All four: why-doc, init wizard, first-write signal (CLI-only in v1), top-level troubleshooting doc |

## Architecture

### Module layout

New module tree inside `crates/memoryd/src/import/`:

```
crates/memoryd/src/import/
├── mod.rs               # public surface: ImportEngine, ImportReport
├── discovery.rs         # locate Claude / Codex memory paths from env + defaults
├── state.rs             # ImportState load/save; content-hash tracking
├── project_map.rs       # cwd → proj_<hex>; interactive prompts for non-git cwds
├── candidate.rs         # ParsedMemory candidate struct used by both parsers
├── sources/
│   ├── mod.rs
│   ├── claude.rs        # parse ~/.claude/projects/<encoded>/memory/
│   └── codex.rs         # parse ~/.codex/memories/MEMORY.md Task Groups
├── pipeline.rs          # write loop + conflict report + supersede-on-hash
└── report.rs            # ImportReport serialization (text + JSON)
```

CLI integration in `crates/memoryd/src/cli/import.rs` and `crates/memoryd/src/cli/init.rs`; subcommand wiring in `crates/memoryd/src/cli.rs`.

### Wire protocol

**No protocol changes.** Importer is purely client-side: parses sources, builds `RequestPayload::WriteMemory` and `RequestPayload::WriteNote` requests, loops them through the existing daemon socket. All privacy classification, governance, ID minting (per-request inside the daemon, not batched), and event-log machinery is reused without modification. This is by design (Q9-A: sequential, accept seconds-per-write, reuse all existing guarantees).

**ID minting is per-write.** Each `memory_write` request mints its own ID inside the daemon's existing flow. The importer learns each minted ID from the `WriteMemory` response and records it in the state file. Cross-memory wiki-link resolution runs as Pass 2 *after* all writes complete (the alias→memory-id map is built from response IDs, not pre-minted).

### State file schema

`$MEMORUM_REPO/.memorum/import-state.json`:

```json
{
  "schema_version": 1,
  "imports": {
    "claude:projects/-Users-treygoff-Code-atlasos/memory/feedback_X.md": {
      "memory_id": "mem_20260527_a1b2c3d4e5f60718_000087",
      "content_hash": "sha256:7f3a...",
      "imported_at": "2026-05-27T22:33:00Z",
      "harness": "claude-code",
      "source_path_at_import": "/Users/treygoff/.claude/projects/-Users-treygoff-Code-atlasos/memory/feedback_X.md",
      "supersession_chain": []
    },
    "codex:memories/MEMORY.md#task-group-3-atlasos-react-doctor": {
      "memory_id": "mem_20260527_a1b2c3d4e5f60718_000093",
      "content_hash": "sha256:9c1f...",
      "imported_at": "2026-05-27T22:34:12Z",
      "harness": "codex",
      "source_path_at_import": "/Users/treygoff/.codex/memories/MEMORY.md",
      "supersession_chain": [
        {
          "memory_id": "mem_20260527_a1b2c3d4e5f60718_000088",
          "content_hash": "sha256:5d2e...",
          "imported_at": "2026-05-27T22:33:08Z"
        }
      ]
    }
  }
}
```

`<source-key>` is harness-relative for portability: `claude:projects/<encoded>/memory/<file>.md` or `codex:memories/MEMORY.md#task-group-<index>-<slug>`. Content-hash dedup keys on `(harness, source-key)`. `supersession_chain` records prior `(memory_id, content_hash, imported_at)` triples when a new content-hash supersedes the prior memory (most recent entry at the end of the chain is the active one).

**Durability**: state file is atomically written (tmp + rename per-record) but parent-dir fsync runs only on the final canonical save at end of import. Per-record fsync of the parent dir is wasted I/O. Schema versioned for forward-compatibility.

### CLI shape

```
# Importer
memoryd import [--harness claude|codex|all] [--dry-run] \
               [--from-claude <path>] [--from-codex <path>] \
               [--report <file.json>] [--quiet] [--socket <path>]

# Wizard (new)
memoryd init [--repo <path>] [--runtime <path>] [--non-interactive]
```

`--from-claude` and `--from-codex` are *separate* flags so `--harness all` remains coherent when overriding source paths. If only one is supplied, the other uses its default discovery path. `memoryd init` is distinct from `memoryd serve --init`: the wizard runs interactive setup, the latter remains a non-interactive daemon-start flag.

### Worktree convention

The existing `scripts/spawn-task-worktree.sh` hardcodes a `stream-a/task-<id>-<slug>` branch prefix and `../agent-memory-wt/task-<id>/` path. **The importer plan does not use that script.** Workers create their own worktrees with this exact recipe:

```bash
# From repo root, for task T<NN>:
git worktree add ../agent-memory-wt/importer-task-<NN> -b importer/task-<NN>-<slug> main
```

Orchestrator (me) integrates each task with a direct `git worktree`-aware merge rather than `scripts/integrate-task-worktree.sh` (which similarly hardcodes the `stream-a` path layout).

### Invariants (must not violate)

1. **No source files are modified or deleted.** Import is read-only on the source side. The state file is the only write outside the Memorum repo, and it lives at `$MEMORUM_REPO/.memorum/`.
2. **The importer never bypasses the daemon write path.** Every memory goes through the daemon socket so Stream D privacy classification, Stream C governance, and event-log mirroring all fire. The importer is not allowed to call `Substrate::write_memory` directly. **Enforced via grep-fail test** (T05 acceptance signal).
3. **`source.kind = import`, `source.harness = "claude-code" | "codex"`, `source.ref = <absolute source path>`.** Provenance shape is fixed.
4. **Importer never passes a `Trusted` classification override**; daemon classifies sensitivity from body content per existing privacy filter. The importer cannot stamp `Trusted` to bypass classification because the `WriteMemory` request shape doesn't expose that field — invariant exists to document the policy, not enforce it.
5. **Idempotency is durable through the state file**. Re-running the importer with the same sources and an intact state file performs zero writes (idempotency check happens before any socket call).
6. **Supersession on content-hash change uses `memory_supersede`, not delete+rewrite.** The new memory's `supersedes: [prior_id]` field is set; the prior is moved into `supersession_chain` in the state file.
7. **Refusals never crash the import.** `status: refused` (any reason: `privacy`, `contradiction`, `tombstone`, `grounding`, etc.) appends to the import report and the import continues.
8. **All entities/tags/aliases supplied explicitly per memory.** No body-derived extraction (Memorum doesn't do it for writes; importer doesn't either).
9. **CLAUDE.md and AGENTS.md are not touched** by either the importer or the wizard, beyond the wizard offering to wire MCP into them.
10. **The wizard is non-destructive on existing setups.** If a `~/memorum` already exists, the wizard runs detection-only and reports; user must explicitly opt in to any change.
11. **No new `RequestPayload` variants.** Importer uses only `WriteMemory`, `Search`, `Get`, `Status`, `Supersede` over the existing socket protocol. `WriteNote` removed from the importer surface per v0.3 (B2 fix).
12. **`GovernanceMeta` extension is additive.** New fields (`entities`, `aliases`, `related`, `evidence`, `supersedes`, `canonical_namespace_id`) are all `Option`-wrapped with `None` defaults. Existing callers (MCP, CLI `memoryd write`) continue to work without changes. `deny_unknown_fields` is preserved.
13. **Single-pass topological writes resolve wiki-links without supersession.** Memories are written in dependency order so each write's `[[wiki_links]]` resolve against already-written IDs. Circular wiki-link loops break by source order with the back-edges left as inert text in body. No metadata-update supersession.

## Orchestrator vs. worker contract

This plan is executed by Claude (me) as orchestrator. Workers are:

- **Native Claude subagents** spawned via the `Agent` tool: `general-purpose` (sonnet), the shared `refactor-pilot`, ad-hoc opus subagents for hard problems.
- **`delegate` CLI** for Cursor Composer, Codex (safe + work), and Droid BYOK lanes (GLM, Kimi, Grok, Gemini, DeepSeek).
- **Custom project agents** if needed — none currently defined under `.claude/agents/`.

### Rules every worker follows

1. **Load `rust-engineer` skill first** for any task touching Rust code. For documentation-only tasks, this is optional but encouraged.
2. **Load `clean-code` skill first** for every review task. Reviews must judge against clean-code principles (small functions, single responsibility, no dead code, idiomatic Rust).
3. **Per-task narrow gate only.** Workers never run `bash scripts/check.sh` or `bash scripts/check-dogfood.sh` in a task worktree — those are orchestrator gates run only after the task is merged to `main`. Workers run the per-task gate listed below.
4. **Worktree isolation** for code changes: workers create their own worktree with `git worktree add ../agent-memory-wt/importer-task-<NN> -b importer/task-<NN>-<slug> main` (see "Worktree convention" above). For `delegate`-backed lanes, `delegate --isolation worktree codex work ...` is equivalent.
5. **Update the task's owned-files list** at task start, before any edits. If reality diverges from the planned list, surface that to me before proceeding.
6. **Stop and surface** if blocked on the same root cause for >30 min (per CLAUDE.md). Write a blocker note; do not retry in a loop.

### Orchestrator-only operations

- Merging completed task branches into `main` via `git merge --ff-only` (manual, not via the hardcoded `integrate-task-worktree.sh`).
- Running the workspace integration gate (`bash scripts/check-dogfood.sh` + `cargo test --workspace`) after each integration merge.
- Resolving cross-task conflicts.
- Managing the reviewer pipeline (opus plan-reviewer → codex safe review → glm safe review for the plan itself; per-task code-review fan-out for implementation tasks).
- Final integration test against fixture corpora.
- Updating `Cargo.lock` and `pnpm-lock.yaml` (workers only touch `Cargo.toml` / `package.json`).

## Task list

14 tasks across waves 0–5 (6 waves). Owned files listed per-task to prevent collisions when parallel-executing.

### Wave 0 — Daemon prerequisite + parallel independent UX (4 tasks, parallel)

These four start simultaneously after the plan is approved. T00 unblocks Wave 1 (and ultimately the whole importer); T09, T10, T12 are independent of T00 and of each other.

---

**T00 — Extend `GovernanceMeta` to accept importer provenance (daemon-side prerequisite)**

- **Owner:** native opus subagent (touches daemon handlers + governance contract; needs care)
- **Branch:** `importer/task-00-governance-meta-extension`
- **Worktree:** `../agent-memory-wt/importer-task-00/`
- **Owned files (write):**
  - `crates/memoryd/src/handlers/mod.rs` (extend `GovernanceMeta`, `GovernanceSourceKindMeta`, and `to_memory()` mapping)
  - `crates/memoryd/src/protocol.rs` (extend `WriteMemoryResponse` only if first-write signal needs it; per v0.3 fix B5, T12 is now pure client-side so this may be untouched)
  - Possibly `crates/memory-governance/src/...` if entity/related fields flow through the engine — to be discovered by the subagent at task start
- **Does NOT modify:** `Substrate::write_memory` itself, the on-disk frontmatter schema (already supports these fields per Stream A spec §6.2), or the MCP-facing tool schemas (the input schema accepts arbitrary `meta`; only the inner `GovernanceMeta` deserialization gates fields)
- **Deliverables:**
  - Extend `GovernanceMeta` with these `Option`-wrapped fields (all default `None`):
    - `entities: Option<Vec<EntityMeta>>` where `EntityMeta { id: String, label: String, aliases: Vec<String> }`
    - `aliases: Option<Vec<String>>`
    - `related: Option<Vec<String>>` (array of memory IDs)
    - `evidence: Option<Vec<EvidenceMeta>>` where `EvidenceMeta { ref_: String, quote: Option<String>, observed_at: Option<DateTime<Utc>> }` (matches Stream A §6.5 minus the auto-generated id/quote_norm_hash, which the daemon computes)
    - `supersedes: Option<Vec<String>>`
    - `canonical_namespace_id: Option<String>` (matches `^proj_[0-9a-f]{16}$` per Stream A §6.2)
  - Extend `GovernanceSourceKindMeta` enum with `Import` variant (Rust-side identifier; `#[serde(rename = "import")]` for wire JSON). Existing variants: `User, AgentPrimary, Subagent, File, WebCapture`.
  - In `to_memory()`, map `GovernanceSourceKindMeta::Import` to `Author { kind: AuthorKind::Agent, harness: Some("memoryd-import"), ..rest }` and `SourceKind::File` (the source IS a local file, even though the kind tag is "import"). Document the mapping in the `to_memory()` doc-comment.
  - **Audit pre-existing constraint (Codex review R-followup):** before locking the T00 acceptance test that asserts `deny_unknown_fields` still rejects unknown fields, grep MCP-client usage in this repo for any `meta` payload that sends fields beyond the documented set. If any internal client does so, surface to me (orchestrator) before completing T00 — we may need to relax `deny_unknown_fields` separately, not silently cement it.
  - Modify `to_memory()` to use caller-supplied fields when present, falling back to current defaults (empty vec, `DEFAULT_PROJECT_NAMESPACE`, etc.) when absent.
  - Preserve `deny_unknown_fields` on `GovernanceMeta`.
  - Confirm the Stream A spec §6 frontmatter schema already supports all these fields (it does per spec — this is daemon-side acceptance, not a spec change).
- **Per-task gate:** `cargo fmt --all -- --check && cargo clippy -p memoryd -p memory-governance --all-targets --all-features -- -D warnings && cargo test -p memoryd -p memory-governance --tests`
- **Acceptance signals:**
  - New unit tests: each new field round-trips through `GovernanceMeta` → `to_memory()` → `Memory::frontmatter`.
  - Backward-compat test: a `GovernanceMeta` JSON omitting all new fields still parses and `to_memory()` produces the same `Memory` as before (existing tests pass).
  - `deny_unknown_fields` test: an unknown field like `meta.zzz_unknown` still produces a deserialization error.
  - `import` source_kind round-trip test.
  - Backward-compat with existing `memoryd write` CLI command: `memoryd write --title X --body Y` still works without supplying any new fields.

---

**T09 — Pre-dogfood UX: README "why" intro doc**

- **Owner:** native sonnet subagent
- **Branch:** `importer/task-09-readme-why-doc`
- **Worktree:** `../agent-memory-wt/importer-task-09/`
- **Owned files (write):**
  - `README.md` (rewrite the top 50-70 lines; keep everything from "Install from this checkout" downward intact; ALSO stub the "Docs map" section's new entries for `docs/troubleshooting.md` and `docs/importer.md` so T10 and T13 don't need to touch README.md)
- **Deliverables:**
  - 2-3 paragraph user-story intro: what Memorum does *for the user*, not the architecture. Frame: "shared memory layer across Claude Code and Codex CLI sessions; one source of truth; backfill from existing memories."
  - Brief "what's different from CLAUDE.md / AGENTS.md" paragraph.
  - Keep the existing architecture diagram, but push it below the user-story.
  - Stub the "Docs map" entries for `docs/troubleshooting.md` and `docs/importer.md` (the files won't exist yet at integration; that's OK — the link will resolve once T10 and T13 land).
- **Per-task gate:** `oxfmt --check --ignore-path .oxfmtignore README.md`
- **Acceptance signals:**
  - First 5 sentences answer: "what is this, who is it for, why would I install it." No `bash` snippet in the first 30 lines.
  - Existing install / quickstart / docs-map sections unchanged structurally; only the two new docs-map entries added.

---

**T10 — Pre-dogfood UX: top-level troubleshooting doc**

- **Owner:** native sonnet subagent
- **Branch:** `importer/task-10-troubleshooting-doc`
- **Worktree:** `../agent-memory-wt/importer-task-10/`
- **Owned files (write):**
  - `docs/troubleshooting.md` (new)
  - `.oxfmtignore` (add `docs/troubleshooting.md` if it contains hand-authored prose tables)
- **Does NOT modify `README.md`** (T09 stubs that entry).
- **Deliverables:**
  - Hoist the troubleshooting block from `docs/runbooks/dogfooding-day-one.md` into a discoverable top-level doc.
  - Cover: `dream_disabled`, `dream_unavailable`, socket errors, `memoryd doctor` findings, MCP not listing tools, first-write returning nothing visible, common first-run failures.
  - Cross-link from `docs/getting-started.md` (T13 will add the reverse link).
- **Per-task gate:** `oxfmt --check --ignore-path .oxfmtignore docs/troubleshooting.md`
- **Acceptance signals:**
  - Doc renders cleanly in a Markdown previewer.
  - Each section has a "symptom → diagnosis → fix" structure with concrete commands.

---

**T12 — Pre-dogfood UX: first-write success signal (pure CLI-side detection)**

- **Owner:** native sonnet subagent (scope reduced from opus after Codex review B5 finding)
- **Branch:** `importer/task-12-first-write-signal`
- **Worktree:** `../agent-memory-wt/importer-task-12/`
- **Owned files (write):**
  - `crates/memoryd/src/cli/write.rs` or wherever the CLI's `memoryd write` dispatcher lives (modify to detect first-write client-side)
  - `crates/memoryd/src/cli/note.rs` if a separate write-note dispatcher exists (apply same change)
  - `crates/memoryd/tests/cli_first_write.rs` (new)
- **Scope reduction from v0.2:** No daemon-handler changes. No protocol changes. No state-file extension. Pure CLI-side detection: after the CLI receives a successful `WriteMemory` / `WriteNote` response, it issues a `memoryd status` query; if the response shows `memories_count == 1` (or the equivalent counter) and the just-written memory's ID is in the response, emit the banner.
- **Deliverables:**
  - Detection heuristic (client-side):
    1. CLI issues write request, receives `id` from response.
    2. CLI immediately issues `Status` request.
    3. If `status.memories_count == 1` AND the status response's most-recent-memory matches the just-returned `id`, emit the banner.
    4. Otherwise (memories_count > 1, or not first write): no banner.
  - Banner format (stderr):
    ```
    ✓ First memory saved: mem_20260527_…
      view: memoryd get --id mem_20260527_…
      list: memoryd search ""
      docs: docs/getting-started.md
    ```
  - **Does not surface via MCP.** MCP clients see only the standard response payload. Locked decision was "CLI-side success signal," and extending the MCP tool response shape is a Stream B amendment we don't need.
  - Race condition acknowledgement: if two writes land within the same status round-trip window, the banner might miss or fire twice. Acceptable on a fresh install where concurrent writes are unlikely.
- **Per-task gate:** `cargo fmt --all -- --check && cargo clippy -p memoryd --all-targets --all-features -- -D warnings && cargo test -p memoryd --test cli_first_write`
- **Acceptance signals:**
  - First write banner emitted, second write no banner, daemon restart + third write no banner (the `memories_count > 1` check survives daemon restart automatically — no in-memory state needed).
  - No changes to `protocol.rs` or `handlers/mod.rs`.

### Wave 1 — Foundation (1 task, depends on T00)

These tasks depend on T00's `GovernanceMeta` extensions being in place.

---

**T01 — Foundation: crate scaffolding, source discovery, state file**

- **Owner:** native sonnet subagent
- **Branch:** `importer/task-01-foundation`
- **Worktree:** `../agent-memory-wt/importer-task-01/`
- **Owned files (write):**
  - `crates/memoryd/src/import/mod.rs` (new)
  - `crates/memoryd/src/import/discovery.rs` (new)
  - `crates/memoryd/src/import/state.rs` (new)
  - `crates/memoryd/src/import/candidate.rs` (new)
  - `crates/memoryd/src/import/sources/mod.rs` (new)
  - `crates/memoryd/Cargo.toml` (add `serde_json`, `sha2`, `dirs`, `walkdir`, `dialoguer`, `regex` to dev-deps, `serial_test` to dev-deps if not already present)
  - `crates/memoryd/src/lib.rs` or `main.rs` (mod declaration only — single line)
- **Deliverables:**
  - `ImportState::load(path) -> Result<Self, ImportError>` and `ImportState::save_atomic(path)` (tmp + rename per-record; parent-dir fsync only on `save_canonical()` call at end of import). **State file is a performance optimization, not load-bearing for correctness** — see Goal section on the daemon-duplicate-detection idempotency model.
  - **Concurrent-import guard (GLM review R4):** state-file reads and writes are guarded by `flock` on a sibling lock file `$MEMORUM_REPO/.memorum/import-state.json.lock`. A second `memoryd import` invocation that can't acquire the lock within 5s fails with a clear error (`AnotherImportInProgress { pid }`); the pid comes from a `.memorum/import.pid` file written atomically on lock acquisition.
  - `discover_claude_memory_paths() -> Vec<ClaudeMemoryRoot>` honoring this precedence: (1) `--from-claude <path>` flag override if supplied, (2) `CLAUDE_CONFIG_DIR` env var if set, (3) `autoMemoryDirectory` setting in `~/.claude/settings.json` if set, (4) default `~/.claude/projects/<encoded>/memory/`. Document the precedence in the function's doc-comment.
  - `discover_codex_memory_paths() -> Option<CodexMemoryRoot>` honoring: (1) `--from-codex <path>` flag, (2) `CODEX_HOME` env var, (3) default `~/.codex/memories/`.
  - `ParsedMemory` candidate struct: `source_key`, `source_path`, `content_hash`, `harness`, `frontmatter_hint` (HashMap<String, Value>), `body`, `wiki_links: Vec<String>`, `cwd: Option<PathBuf>`.
  - `import::Error` enum via `thiserror`.
- **Per-task gate:** `cargo fmt --all -- --check && cargo clippy -p memoryd --all-targets --all-features -- -D warnings && cargo test -p memoryd --tests`
- **Acceptance signals:**
  - `cargo test -p memoryd` includes new tests for `ImportState::load` round-trip + atomic-save + corrupt-file handling.
  - `discover_*` functions tested against fixture env vars with explicit precedence-order tests (all four levels for Claude, all three for Codex).
  - No new public surface on `crates/memoryd/src/lib.rs` other than the `mod import;` declaration.

### Wave 2 — Parsers + project mapping (3 tasks, parallel; all depend on T01)

---

**T02 — Claude parser**

- **Owner:** native sonnet subagent
- **Branch:** `importer/task-02-claude-parser`
- **Worktree:** `../agent-memory-wt/importer-task-02/`
- **Owned files (write):**
  - `crates/memoryd/src/import/sources/claude.rs` (new)
  - `crates/memoryd/tests/fixtures/claude-memory/...` (a small fixture corpus mirroring `~/.claude/projects/<encoded>/memory/` shape, with one single-fact file, one multi-section dossier, one `MEMORY.md` index, one `user_profile.md`, one with `[[wiki_links]]`)
- **Deliverables:**
  - `ClaudeParser::parse(root: &Path) -> Result<Vec<ParsedMemory>>` that:
    1. Skips `MEMORY.md` index files.
    2. Reads each topic file's YAML frontmatter.
    3. Decides single-fact vs. multi-section based on heuristic: file has 3+ substantive `##` sections (each >3 lines of body, excluding `## Why:` / `## How to apply:` / `## How:` / `## When:` / `## Why this matters:` boilerplate).
    4. For single-fact: produces 1 `ParsedMemory`.
    5. For multi-section: produces N `ParsedMemory` instances, one per substantive section, with the parent frontmatter's `name` extended by ` — <section heading>`.
    6. Extracts `[[wiki_link]]` patterns from body into `ParsedMemory::wiki_links`.
    7. Computes content hash over `(frontmatter_canonical_yaml || body)`.
  - Fixture-driven unit tests covering each shape.
- **Per-task gate:** `cargo fmt --all -- --check && cargo clippy -p memoryd --all-targets --all-features -- -D warnings && cargo test -p memoryd --tests claude::`
- **Acceptance signals:**
  - Fixture corpus has at least 7 files covering: single-fact, multi-section, `MEMORY.md` index (skipped), `user_profile.md`, file with wiki-links, **one malformed YAML frontmatter** (parser returns `ImportError::Parse` for the file, continues with others), **one non-UTF-8 file** (parser returns `ImportError::Encoding` for the file, continues).
  - Multi-section heuristic is unit-tested against the boilerplate-`##` cluster (`## Why:`, `## How to apply:`, etc.).
  - **Empty-corpus test:** `ClaudeParser::parse(empty_dir) -> Ok(vec![])` (no error, just zero candidates).

---

**T03 — Codex parser**

- **Owner:** native sonnet subagent
- **Branch:** `importer/task-03-codex-parser`
- **Worktree:** `../agent-memory-wt/importer-task-03/`
- **Owned files (write):**
  - `crates/memoryd/src/import/sources/codex.rs` (new)
  - `crates/memoryd/tests/fixtures/codex-memory/MEMORY.md` (fixture: at least 3 Task Groups with different shapes — git cwd, non-git cwd, workflow-scope)
  - `crates/memoryd/tests/fixtures/codex-memory/extensions/ad_hoc/notes/sample.md` (one ad-hoc note)
- **Deliverables:**
  - `CodexParser::parse(root: &Path) -> Result<Vec<ParsedMemory>>` that:
    1. Reads `<root>/MEMORY.md`.
    2. Splits on `^# Task Group:` boundaries.
    3. For each block, extracts: header (after `# Task Group:`), `scope:` line, `applies_to:` line (parse `cwd=<path>` and `reuse_rule=<rule>`), all `## Task N:` sections (preserve in body), all `### keywords` lists (concat into `frontmatter_hint.tags`), all `### rollout_summary_files` entries (parse path + thread_id + updated_at + outcome → `frontmatter_hint.evidence_refs: Vec<EvidenceRef>`), plus the trailing `## User preferences` / `## Reusable knowledge` / `## Failures and how to do differently` sections (preserved verbatim in body).
    4. Reads `<root>/extensions/ad_hoc/notes/*.md` and produces a `ParsedMemory` per note (flagged for `memory_write` primitive — v0.3 override of locked decision per Codex review B2; `memory_note` loses provenance).
    5. Skips `raw_memories.md`, `memory_summary.md`, `rollout_summaries/` (the directory itself; specific files are referenced via evidence refs from Task Group memories), `skills/`.
    6. Computes content hash per Task Group / per ad-hoc note.
  - Fixture-driven unit tests.
- **Per-task gate:** Same as T02.
- **Acceptance signals:**
  - Each Task Group fixture round-trips: parser output's body re-renders to canonical Codex schema when re-formatted.
  - `applies_to: cwd=<path>` correctly parsed and surfaced on `ParsedMemory::cwd`.
  - At least 3 evidence refs (from `### rollout_summary_files`) per Task Group fixture are extracted with `(path, thread_id, updated_at, outcome)`.
  - **Empty-corpus test:** `CodexParser::parse(empty_dir) -> Ok(vec![])`.
  - **Malformed-file test:** one fixture Task Group missing the required `scope:` line returns `ImportError::Parse` for that block but the parser continues with subsequent blocks.

---

**T04 — Project mapping helper**

- **Owner:** native sonnet subagent
- **Branch:** `importer/task-04-project-mapping`
- **Worktree:** `../agent-memory-wt/importer-task-04/`
- **Owned files (write):**
  - `crates/memoryd/src/import/project_map.rs` (new)
  - `crates/memoryd/tests/import_project_map.rs` (new integration test)
- **Does NOT modify:** `crates/memoryd/src/recall/project.rs` (read-only reference; reused via direct call).
- **Deliverables:**
  - `ProjectMapper` with:
    - `pub async fn resolve(&mut self, cwd: Option<&Path>) -> Result<ScopeBinding>`
    - `ScopeBinding { scope: MemoryScope, namespace: Option<String>, canonical_namespace_id: Option<String>, resolution: ResolutionKind }`
    - `ResolutionKind` ∈ `{GitRemote, YamlOverride, PromptedNewProject, PromptedDropToMe, PromptedSkip, UserScope}`.
  - For git cwds: **call `recall::project::resolve_project_binding` directly**. Do not factor out, do not duplicate the SHA normalization. If the function's signature isn't import-friendly (e.g., it requires a session-binding context the importer doesn't have), expose a thinner wrapper in `recall::project` as a `pub fn` rather than copying logic.
  - For non-git cwds: collect all unique non-git cwds across both parsers' outputs, present them as a deduplicated list at the start of the import, and prompt per-cwd:
    1. `(g)enerate .memory-project.yaml here` (with derived `canonical_id` from dir basename + checksum suffix to avoid collision)
    2. `(m)e — drop these memories into user scope`
    3. `(s)kip these memories entirely`
  - **Sync-dir warning (GLM review R5):** the prompt explicitly shows the full path where `.memory-project.yaml` would be written. If that path matches common synced-dir patterns (`Dropbox/`, `iCloud/`, `OneDrive/`, `Google Drive/`, `pCloud/`), append a warning: `⚠ This directory appears to be synced via <service>. The .memory-project.yaml file will be visible on other machines using that service. Continue? (y/N)`.
  - Prompts use `dialoguer` or equivalent; testable via injected `PromptBackend` trait.
- **Per-task gate:** Same as T02, plus `cargo test -p memoryd --tests project_map`.
- **Acceptance signals:**
  - `recall::project::resolve_project_binding` is reused (no duplication of git-remote normalization). Grep the new module for `git_origin_remote` and `normalize_remote` calls — they should not appear except as imports.
  - Non-git cwd prompts have an injected `PromptBackend` trait so tests can simulate user input.
  - `.memory-project.yaml` generation creates a file matching the schema in spec §4.2; round-trip tested.

### Wave 3 — Write pipeline split into two tasks (T05 then T06, sequential)

---

**T05 — Parse pipeline + state-file dedup + topological ordering**

- **Owner:** native sonnet subagent
- **Branch:** `importer/task-05-parse-pipeline`
- **Worktree:** `../agent-memory-wt/importer-task-05/`
- **Owned files (write):**
  - `crates/memoryd/src/import/pipeline.rs` (new, Phase 1 — parse + plan, no socket writes)
  - `crates/memoryd/src/import/mod.rs` (extend `ImportEngine` public surface)
  - `crates/memoryd/tests/import_invariants.rs` (new; the grep-fail invariant test)
- **Deliverables:**
  - `ImportEngine::plan(opts: ImportOptions) -> Result<ImportPlan>` doing:
    1. **Pass 0:** Source discovery (Claude + Codex via T01 functions).
    2. **Pass 1:** Parse all sources via T02/T03 parsers; collect `Vec<ParsedMemory>` with provisional source-keys.
    3. **Pass 2:** Per-cwd prompts via `ProjectMapper` (T04).
    4. **Pass 3:** State-file dedup. For each `ParsedMemory`: classify as `PlanAction::SkipUnchanged`, `PlanAction::Supersede(prior_memory_id)`, or `PlanAction::WriteNew`. No socket calls yet.
    5. **Pass 4 (new in v0.3):** **Topological sort by wiki-link dependency.** For each `WriteNew`/`Supersede` action, look at its `ParsedMemory.wiki_links`. Build a DAG where edges go from source memory → target memory (the link points at). Topologically sort actions so each write happens after its wiki-link targets. Detect cycles; break them by breaking the back-edge that points at the lower-index source (deterministic). Mark back-edges as `unresolved_at_plan_time` so T06's write loop knows to leave them as inert text. This **replaces the v0.2 Pass-5 supersession scheme** (which would have been refused by governance per Codex review B4).
  - `ImportPlan { actions: Vec<PlannedWrite>, prompted_dispositions: Vec<...>, source_discovery_summary: ..., unresolved_back_edges: Vec<WikiLinkRef> }`. `PlannedWrite` holds the ordered action plus a `wiki_link_targets_resolvable: Vec<String>` and `wiki_link_targets_back_edge: Vec<String>` split.
  - **Invariant grep test** (`crates/memoryd/tests/import_invariants.rs`): a `#[test]` that builds a string from every `.rs` file under `crates/memoryd/src/import/` (excluding `// ` line comments to avoid false-positives on documentation), and asserts the regex `\bwrite_memory\b` matches zero times outside of `// SAFETY:`-tagged lines. Catches both `Substrate::write_memory` and `substrate.write_memory(...)` (Codex review R1 fix).
- **Per-task gate:** `cargo fmt --all -- --check && cargo clippy -p memoryd --all-targets --all-features -- -D warnings && cargo test -p memoryd --tests`
- **Acceptance signals:**
  - At least three behavioral tests on `ImportPlan`:
    - First-run plan on fixture corpus: all actions are `WriteNew`.
    - Second-run plan with pre-populated state file: all unchanged actions are `SkipUnchanged`; one mutated source produces a `Supersede(prior_id)` action.
    - Wiki-link topo sort: a fixture with A→B→C ordering produces actions in the order C, B, A. A circular A→B→A fixture produces an action ordering A then B with the A→B edge marked as back-edge (or B then A with the B→A edge marked, deterministic on source-key order).
  - Invariant grep test passes against the importer module tree.

---

**T06 — Single-pass write loop + report + nuanced response handling**

- **Owner:** native opus subagent (write-loop orchestration; subtle response branching and partial-state recovery)
- **Branch:** `importer/task-06-write-loop`
- **Worktree:** `../agent-memory-wt/importer-task-06/`
- **Owned files (write):**
  - `crates/memoryd/src/import/pipeline.rs` (extend with `execute(plan)` method; T05 wrote `plan()`)
  - `crates/memoryd/src/import/report.rs` (new)
- **Deliverables:**
  - `ImportEngine::execute(plan: ImportPlan, opts: ExecuteOptions) -> Result<ImportReport>` doing:
    1. **Pass 5: Single-pass topologically-ordered write loop.** Walk `plan.actions` in topological order (each write happens after its wiki-link dependencies). Maintain a running `alias_to_id: HashMap<String, MemoryId>` that's seeded with existing state-file entries and grows as new writes complete.
       - For each action, resolve its `wiki_link_targets_resolvable` against the current `alias_to_id` map → list of memory IDs to pass as `related`.
       - Leave `wiki_link_targets_back_edge` entries as inert `[[name]]` text in the body (per topological-ordering decision in T05).
       - For `WriteNew`: build `RequestPayload::WriteMemory { body, title, tags, meta }`. `meta` (the extended `GovernanceMeta` from T00) carries `source_kind = "import"`, `source_ref = <abs source path>`, plus the new fields: `entities` (from Codex `### keywords` or Claude `name`), `aliases` (source filename), `related` (resolved IDs from this pass), `evidence` (Codex rollout `file://` refs), `canonical_namespace_id`, `namespace`, `confidence: 0.7`, `requires_user_confirmation: false`. **Confidence bumped from 0.5 to 0.7 (GLM review R1)** to keep imported memories out of Reality Check's "low confidence, needs review" surface — these aren't speculative agent guesses, they're already-vetted content from the user's prior harness sessions. `requires_user_confirmation: false` keeps them out of the review queue for the same reason. No `WriteNote` requests — all imports use `WriteMemory` per v0.3 B2 fix.
       - For `Supersede(prior_id)`: same shape but include `supersedes: [prior_id]` in `meta`.
       - For `SkipUnchanged`: no socket call.
       - If `opts.dry_run`: log the request and continue without socket call.
    2. **Pass 5b: Response branching (Codex review B3 fix).** For each response, branch on `status` AND `existing_id` AND `next_actions`:
       - `status = promoted` AND no `existing_id` → new memory written; record `id` in state file as the import's memory_id; insert into `alias_to_id`.
       - `status = promoted` AND `existing_id` is set (dedup against an existing memory) → log to report as "dedup against existing"; record `existing_id` as the import's memory_id (this is how the daemon enforces idempotency at the substrate level); insert into `alias_to_id`.
       - `status = candidate` with `next_actions: ["memory_supersede"]` and `existing_id` set → governance says "you should supersede this existing one"; issue the follow-up `memory_supersede(existing_id)` call now, then record the resulting memory_id.
       - `status = candidate` without `next_actions` → record as written-as-candidate; future review.
       - `status = quarantined` → record id in state file and log to report's quarantined section.
       - `status = refused` → log to report with reason and source-key; do NOT record in state file; continue to next action. **Inline display (GLM review UX-3):** the progress line for a refused write shows the reason inline, e.g. `[47/500] REFUSED (privacy): claude/projects/<encoded>/memory/feedback_X.md` — refusals are visible in real-time, not only in the final report.
    3. **Pass 6: Atomic state-file save per record** (tmp+rename) plus final `save_canonical()` with parent-dir fsync at end. Per v0.3, state file is a performance optimization, not load-bearing for correctness — daemon's duplicate-detection re-establishes idempotency on any re-run that finds the state file truncated.
    4. **Pass 7: Emit `ImportReport`** (text to stdout + JSON to `--report` path if supplied).
  - `ImportReport` includes: per-harness counts (parsed, written-new, dedup-existing, superseded, written-candidate, quarantined, skipped-idempotent, refused-{privacy,contradiction,tombstone,grounding,policy,other}), per-refusal details (source-key, reason, suggested next action), per-dedup details (source-key, existing_id), unresolved-back-edge wiki-links list, prompted non-git cwd dispositions.
- **Per-task gate:** `cargo fmt --all -- --check && cargo clippy -p memoryd --all-targets --all-features -- -D warnings && cargo test -p memoryd --tests`
- **Acceptance signals:**
  - Behavioral tests via mocked daemon socket covering each response branch:
    - First-run import on fixture corpus succeeds, writes N memories, state file contains N entries.
    - Second run on unchanged sources skips everything (zero socket writes; assertable via mock).
    - Second run with one source's content hash changed issues exactly one `memory_supersede` and updates the state file's `supersession_chain`.
    - **Promoted-with-existing-id (dedup):** mock returns `{status: "promoted", existing_id: "mem_X"}` → state file records the source as importing `mem_X`, no spurious new memory expected.
    - **Candidate-supersede:** mock returns `{status: "candidate", next_actions: ["memory_supersede"], existing_id: "mem_Y"}` → importer issues `RequestPayload::Supersede { existing_id: "mem_Y", ... }` follow-up; final state file records the supersede chain.
    - **Refusal cases:** privacy / contradiction / tombstone refusals all appended to report; state file untouched.
    - **Wiki-link topo resolution:** fixture A→B writes B first; B's response ID lands in `alias_to_id`; A's write carries `related: [B_id]`. A circular A↔B fixture writes A first (with B as back-edge `[[B]]` text), B second (with A in `related`).
    - `--dry-run` performs zero socket calls and produces a report.
  - Report is round-trip valid JSON.

### Wave 4 — CLI subcommand (1 task, depends on T06)

---

**T07 — `memoryd import` CLI subcommand**

- **Owner:** native sonnet subagent
- **Branch:** `importer/task-07-cli`
- **Worktree:** `../agent-memory-wt/importer-task-07/`
- **Owned files (write):**
  - `crates/memoryd/src/cli/import.rs` (new)
  - `crates/memoryd/src/cli.rs` (add `Command::Import(ImportArgs)` variant + dispatcher)
- **Deliverables:**
  - `clap` subcommand with flags listed in Architecture / CLI shape.
  - Default behavior: `--harness all`, no `--dry-run`, report to stdout.
  - `--from-claude <path>` and `--from-codex <path>` are independent overrides; either can be supplied without the other.
  - `--report <file.json>` writes structured JSON.
  - `--quiet` suppresses progress lines but keeps the summary.
  - Progress lines to stderr.
  - Help text describes the locked decisions briefly (granularity, non-destructive, idempotent, state-file location).
- **Per-task gate:** Same as T06.
- **Acceptance signals:**
  - `memoryd import --help` renders cleanly and references the locked decisions.
  - `memoryd import --dry-run` against a fixture corpus produces a deterministic report.
  - Exit codes: 0 on clean import; non-zero on hard failure (socket unavailable, state file corrupt, etc.); 0 on import with refusals (refusals are reported, not fatal).

### Wave 5 — Integration test + wizard + user docs (3 tasks, T08 + T11 parallel; T13 after both)

---

**T08 — Importer integration test against fixture corpora**

- **Owner:** native sonnet subagent
- **Branch:** `importer/task-08-integration-test`
- **Worktree:** `../agent-memory-wt/importer-task-08/`
- **Owned files (write):**
  - `crates/memoryd/tests/import_end_to_end.rs` (new; **`#[serial]` mandatory** on every test in the file — uses `DaemonScaffold`)
  - `crates/memoryd/tests/fixtures/import/` (combined fixture set — extends T02/T03 fixtures)
- **Deliverables:**
  - End-to-end test:
    1. Spin up a real `memoryd serve --init` in a temp repo (uses the existing `DaemonScaffold` test harness from `memorum-eval`).
    2. Run `memoryd import` against the combined Claude + Codex fixtures.
    3. Assert the resulting repo state: N canonical memory files at expected paths, M event-log entries, state file matches expectations.
    4. Run `memoryd doctor`; assert no findings.
    5. Run `memoryd import` a second time; assert zero socket writes and unchanged state file timestamp on dedup paths.
- **Per-task gate:** `cargo test -p memoryd --test import_end_to_end -- --test-threads=1` (serial; uses DaemonScaffold).
- **Acceptance signals:**
  - All tests in the file carry `#[serial]` per the 5/11 handbook precedent and the 3e6e0ff fix-set. Confirm `DaemonScaffold` uses the post-`3e6e0ff` `short_socket_path` with `AtomicU64` counter (no cross-test socket-path collisions).
  - Test runs cleanly under `--test-threads=1`.
  - **Daemon-crash recovery test (GLM review test-coverage):** start daemon, run import that targets a 20-memory fixture, kill the daemon process mid-import (after 5–10 writes), restart daemon, re-run import; assert final state matches a single-pass import (no duplicates written, all 20 sources present, state file reflects all writes including those that the daemon caught as `existing_id` duplicates on the retry).
  - **Empty-corpus integration test:** `memoryd import` against an empty `~/.claude/projects/` + empty `~/.codex/memories/` exits 0 with a zero-write report.
  - **Concurrent-import test:** start two `memoryd import` processes simultaneously; assert the second one exits with `AnotherImportInProgress { pid: <first> }` within 5s (validates T01's `flock` guard).

---

**T11 — `memoryd init` wizard**

- **Owner:** native opus subagent
- **Branch:** `importer/task-11-init-wizard`
- **Worktree:** `../agent-memory-wt/importer-task-11/`
- **Owned files (write):**
  - `crates/memoryd/src/cli/init.rs` (new)
  - `crates/memoryd/src/cli.rs` (add `Command::Init(InitArgs)`)
  - `docs/runbooks/init-wizard.md` (new runbook)
- **Deliverables:**
  - Interactive wizard that:
    1. Detects whether `$MEMORUM_REPO` exists and has a substrate; if yes, switch to detection-only mode (offers re-import, not re-init).
    2. Prompts for repo path / runtime path / socket path with sensible defaults; uses `dialoguer` or equivalent.
    3. Runs `scripts/install-memorum.sh` equivalent in-process (or shells out to it), starts the daemon, polls for readiness.
    4. Prints the MCP client snippet (per the existing installer pattern).
    5. Detects Claude Code memory (`~/.claude/projects/`) and Codex memory (`~/.codex/memories/`) and reports counts.
    6. If memory found: prompts "Would you like to import? (Y/n)" — default `yes` because detection found content, and the whole point of the wizard is to be the on-ramp (GLM review UX-1). `y` / Enter invokes `memoryd import --harness all`.
    7. Prints next-steps: TUI, dashboard, MCP wiring instructions, troubleshooting doc link.
  - `--non-interactive` mode honors env-var defaults and runs no prompts; suitable for CI.
- **Per-task gate:** `cargo fmt --all -- --check && cargo clippy -p memoryd --all-targets --all-features -- -D warnings && cargo test -p memoryd --tests init`
- **Acceptance signals:**
  - Wizard tested via injected `PromptBackend` (same trait as T04).
  - Runbook at `docs/runbooks/init-wizard.md` walks through the wizard's flow with screenshots-as-ASCII.
  - On second run (existing substrate), the wizard reports detection-only and does not re-init.

---

**T13 — Importer user docs**

- **Owner:** native sonnet subagent
- **Branch:** `importer/task-13-importer-docs`
- **Worktree:** `../agent-memory-wt/importer-task-13/`
- **Owned files (write):**
  - `docs/importer.md` (new — user-facing importer documentation)
  - `docs/getting-started.md` (modify: add a step 0 referencing `memoryd init` wizard; add a paragraph linking to `docs/importer.md`; add reverse link to `docs/troubleshooting.md` from T10)
  - `.oxfmtignore` (add `docs/importer.md` if oxfmt mangles the prose tables)
- **Does NOT modify `README.md`** (T09 stubbed the docs-map entry).
- **Deliverables:**
  - `docs/importer.md` covers: what gets imported, what's skipped and why, the locked decisions in plain English, the state file format, the conflict report format, the re-import semantics, the cwd-prompt UX, troubleshooting. **Additional sections required by GLM review:**
    - **Imported memories vs. hand-written**: explain that imported memories start at `confidence: 0.7` (lower than the `0.85` default for hand-written), which affects recall ranking. Hand-written memories edited by the user later still take precedence in entity-overlap recall.
    - **Re-run semantics asymmetry**: if you edit the *Memorum copy* of an imported memory, a re-import will NOT supersede your edit (content-hash check is against the source, not the Memorum copy — your edit is preserved). If you edit the *source file*, a re-import WILL supersede with the new source content.
    - **Rollback options**: there's no `memoryd import --undo` in v1. To undo, use `memoryd forget <id>` on individual imported memories (the import report lists all IDs), or `memoryd search "source.harness=claude-code"` to bulk-find them. Bulk-undo is a v2 feature.
    - **Dashboard limitations**: the v1 dashboard doesn't have a "filter by import source" view. Imported memories are visible in the global memory list with `source.kind = import`; future v2 will add filtering.
    - **Post-import dream cycle**: after a large import, the first 1-2 Stream F dream cycles may produce noisier candidate output as the system absorbs the new entity space. This self-corrects after a couple of runs.
  - Updated `docs/getting-started.md` flow opens with `memoryd init` and points to `memoryd import` as an optional follow-up.
- **Per-task gate:** `oxfmt --check --ignore-path .oxfmtignore docs/importer.md docs/getting-started.md`
- **Acceptance signals:**
  - All locked decisions documented in user-facing language.
  - Cross-links between `docs/getting-started.md`, `docs/troubleshooting.md`, `docs/importer.md`, and `README.md` all resolve.

## Per-task review cadence (code reviews fan out after each task lands)

After each implementation task's per-task gate passes and the task branch is rebased on current `main`, but **before** integration merge:

1. **Code review fan-out** — three parallel review lanes:
   - Native sonnet subagent with `clean-code` + `rust-engineer` skills loaded — primary review.
   - `delegate codex safe` with `clean-code` + `rust-engineer` skill instructions in the prompt — second-source review against a worktree copy.
   - `delegate droid glm safe` with `clean-code` + `rust-engineer` skill instructions — third-source review for model diversity.
2. **I (orchestrator) read all three reports**, dedupe findings, write a single consolidated review summary at `docs/reviews/2026-05-27-importer-t<NN>-review.md`.
3. **If findings are blockers**: I either fix them directly (if mechanical) or hand the touched files back to the original implementer subagent with the review summary.
4. **Once clean**: I run `git merge --ff-only` from `main` to integrate the task branch, then `git worktree remove ../agent-memory-wt/importer-task-<NN>` and `git branch -d importer/task-<NN>-<slug>`.

All reviewers must judge against `clean-code` skill principles: small functions, single responsibility, no dead code, idiomatic Rust ownership/error-handling, no `unwrap()` outside tests, no defensive try/catch where invariants guarantee correctness, no slop comments.

## Per-task gate definitions (reference)

| Task type | Gate command |
|---|---|
| Rust crate (memoryd only) | `cargo fmt --all -- --check && cargo clippy -p memoryd --all-targets --all-features -- -D warnings && cargo test -p memoryd --tests` |
| Rust + memoryd-web | Above + `cargo test -p memoryd-web --tests` + `pnpm typecheck && pnpm lint && pnpm test` from `crates/memoryd-web/frontend/` |
| Docs (Markdown only) | `oxfmt --check --ignore-path .oxfmtignore <files>` |
| Integration test | `cargo test -p memoryd --test <test-file> -- --test-threads=1` (always serial when using DaemonScaffold) |

Per CLAUDE.md "Repository state strategy": workers never run `bash scripts/check.sh` or `bash scripts/check-dogfood.sh` inside a task worktree. Those are orchestrator gates.

## Integration gate (orchestrator-only)

After all 13 tasks are merged to `main`:

1. `bash scripts/check-dogfood.sh` (the canonical pre-install gate).
2. `cargo test --workspace --no-fail-fast`.
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
4. Frontend gate from `crates/memoryd-web/frontend/`: `pnpm typecheck && pnpm lint && pnpm test && pnpm build`.
5. **Live dogfood smoke**: I run `memoryd init` against my own machine (real Claude + Codex memory), import, verify in dashboard.

Per CLAUDE.md "Lessons from past autonomous runs": pre-bake the macOS `syspolicyd` workaround (`CARGO_TARGET_DIR=$(mktemp -d)` + PATH purge of `cargo-nextest` and `sccache`) if any gate runs >1hr.

## Risks and mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| T04 accidentally duplicates `recall::project::resolve_project_binding` instead of reusing | Low (pre-decided as direct call) | Owned-files list explicitly excludes `recall/project.rs`; grep-test in T04 acceptance signals fails build if duplication detected. |
| Importer sequential throughput is unacceptable at >2000 memories | Low (current users have <500) | Locked decision is sequential; if it becomes a real problem post-dogfood, build a batch path as v2 work. Plan does not block on this. |
| Non-git cwd prompts interrupt the wizard's flow awkwardly | Medium | Wizard prompts all cwds in a single batch up-front (per T04 design); user sees one prompting session, not N interleaved. |
| `memory_capture_source` evidence-ref alternative (file://) creates a second-class evidence type that confuses `memoryd doctor` | Low | Q4-γ locked; staleness surfaces as doctor warning, not panic. If doctor's dangling-reference check fires false positives on missing rollout summaries, we add an `evidence.kind = "external-source-not-tracked"` flag in a follow-up. |
| Two parallel Codex consolidation runs race on `~/.codex/memories/.git/` while the importer reads — Codex's Phase 2 holds a singleton lock, but we read without one | Low | Importer reads files; if Phase 2 rewrites a file mid-read, the file's content is internally consistent (Codex writes atomically). Worst case is we import a snapshot 30 seconds older than the latest. Acceptable. |
| State file gets corrupted mid-import | Low | Atomic tmp+rename per-record + final canonical save with parent-dir fsync; on load, malformed JSON triggers a clear error and the user can rerun (the corrupt file is preserved at `.memorum/import-state.json.corrupt-<ts>` for diagnosis). |
| Fixture corpora drift from real Claude/Codex schema after their next release | Medium | Fixtures are versioned in-tree; if a real-world parse fails we add the new shape to fixtures and patch the parser. Document this in the runbook. |
| Wiki-link back-edges in circular link graphs are lost as inert text | Low | Single-pass topological ordering resolves forward edges; cyclic links break deterministically (lowest-source-key back-edge becomes inert `[[name]]` text in body). Empirically wiki-links are rare in observed corpora (Codex review's grep showed 0 wiki-links in 98 Codex Task Groups; Claude memory was unreachable from Codex's isolated worktree but is expected to be sparser than initial estimate). Inert text is still grep-able by users. |
| T00 `GovernanceMeta` extension breaks existing callers if backward-compat isn't preserved | Low (T00 acceptance signals explicitly cover this) | All new fields are `Option`-wrapped with `None` defaults; existing test suite must pass after T00 changes; backward-compat round-trip test in T00 acceptance signals. |
| `confidence: 0.5` flat default makes imported memories rank uniformly mid-pack | Medium | Locked tradeoff: better to ship flat than to ship a guessed mapping. v2 outcome-based mapping is a real follow-up. |

## Acceptance signals (plan-level)

This plan is complete when:

1. All 13 tasks have landed on `main` with per-task gates green.
2. Each implementation task has at least one consolidated review summary in `docs/reviews/2026-05-27-importer-t<NN>-review.md`.
3. The integration gate runs clean: `bash scripts/check-dogfood.sh` + `cargo test --workspace` + workspace clippy + frontend gate all green.
4. **Live smoke on my machine**: `memoryd init` runs end-to-end, imports my actual Claude and Codex memory non-destructively, `memoryd doctor` exits 0 after import, dashboard renders the imported memories.
5. The pre-dogfood UX deliverables are visible: a normal user reading `README.md` knows what Memorum is in 60 seconds; `docs/troubleshooting.md` answers the top-5 first-run failure modes; the first `memory_write` produces a visible CLI success signal.

## Plan revision history

- 2026-05-27 v0.4: Patched after delegate-droid-GLM safe review pass (third reviewer; bias on system-interaction risks, operator UX, refactor blast radius). Changes:
  - **R1 (Reality Check flood)**: Bumped import confidence `0.5 → 0.7`. Added `requires_user_confirmation: false` flag for imported memories. Rationale: these aren't speculative agent guesses; they're already-vetted content from the user's prior harness sessions. RC's "low confidence, needs review" surface would otherwise show 200+ items right after a 500-memory import.
  - **R3 (`Import` enum naming + `to_memory()` mapping)**: T00 deliverable now specifies Rust-side `Import` variant with `#[serde(rename = "import")]` for wire JSON, plus the exact `to_memory()` mapping to `Author { kind: Agent, harness: "memoryd-import" }` + `SourceKind::File`. Removes ambiguity for the T00 worker.
  - **R4 (concurrent-import state-file race)**: T01 now requires `flock` guard on state-file access via `$MEMORUM_REPO/.memorum/import-state.json.lock`. Second `memoryd import` invocation fails with `AnotherImportInProgress { pid }` if it can't acquire the lock within 5s. T08 has a concurrent-import test that exercises this.
  - **R5 (`.memory-project.yaml` in synced dirs)**: T04 prompt now shows full path and detects synced-dir patterns (Dropbox, iCloud, OneDrive, Google Drive, pCloud), appending an explicit warning so the user knows the file will be visible on other machines.
  - **B1 audit (downgraded from blocker)**: T00 now includes a pre-flight audit step — grep MCP-client code for `meta` payloads with unknown fields before locking the `deny_unknown_fields` acceptance test. If any internal client relies on unknown fields, surface to orchestrator before cementing the constraint. The existing `deny_unknown_fields` is pre-existing and not introduced by T00, but T00 shouldn't cement it without verifying no breakage.
  - **Test coverage gaps closed (GLM review test-coverage)**:
    - T02 / T03: empty-corpus tests + malformed-file tests (YAML parse failure for Claude, missing-`scope:` for Codex) + non-UTF-8 file test for Claude.
    - T08: daemon-crash mid-import recovery test, empty-corpus integration test, concurrent-import test (verifies T01's flock guard).
  - **UX-3 (inline refusal display)**: T06 acceptance now requires the progress line for a refused write to show the reason inline (e.g., `[47/500] REFUSED (privacy): claude/...`), not only in the final report.
  - **UX-1 (wizard default)**: T11's import prompt is now `(Y/n)` default-yes (was `(y/N)`) when memory is detected.
  - **T13 doc additions**: imported-vs-hand-written confidence semantics, re-run asymmetry (source-edit vs Memorum-edit), no-rollback workflow with workarounds, dashboard limitation note, post-import dream-cycle noise note.
  - **N1**: Lockbox table Q3 row annotated with the v0.3 `memory_note` override visible inline (not just in body).
- 2026-05-27 v0.3: Patched after delegate-codex safe review pass. Substantial reworking:
  - **B1 (biggest)**: Added new prerequisite task **T00** extending `GovernanceMeta` to accept caller-supplied `entities`, `aliases`, `related`, `evidence`, `supersedes`, and `canonical_namespace_id`. The current `deny_unknown_fields` `GovernanceMeta` cannot carry importer provenance; my Stream B grounding missed this because the report covered the outer MCP shape, not the inner governance meta. T00 is daemon-side, additive, backward-compatible. New invariant #12 documents the additive constraint. Without T00 the importer cannot land any of its required metadata.
  - **B2**: Override of locked write-primitive decision for Codex ad-hoc notes. Lockbox said `memory_note`; Codex review found `WriteNote` hardcodes `source.ref = "memoryd.write_note"` with no harness, no original path — violates provenance invariant #3. Switched all imports to `memory_write`. Cost: governance fires on each ad-hoc note (acceptable per Q9 sequential decision); benefit: provenance preserved. T03's deliverable updated; locked-decisions table annotated with override.
  - **B3**: T06 response handling refined. Codex review caught that `Promoted` can mean "deduplicated against existing memory" (carries `existing_id`) and `Candidate` can mean "you should now call `memory_supersede`" (carries `next_actions`). Simplistic "status=promoted → record id" would poison state. Now branches explicitly on `status`, `existing_id`, and `next_actions`. Pass 5b added with all branches enumerated; new acceptance tests for each.
  - **B4**: Wiki-link Pass 5 supersession scheme dropped entirely. Codex review found `memory_supersede` runs full governance against the body; same-body metadata-only update would be refused by contradiction detection as a duplicate. Replaced with **single-pass topologically-ordered writes**: T05 builds a DAG from wiki-link refs, sorts actions, marks cyclic back-edges as inert text; T06 walks the sorted plan and resolves each write's wiki-links against the running `alias_to_id` map. No metadata-update supersession. New invariant #13 documents this.
  - **B5**: T12 first-write signal further descoped to **pure CLI-side detection**. Daemon emits no banner; CLI issues a follow-up `Status` query post-write and checks `memories_count == 1`. No protocol changes, no handler changes. Daemon-restart safe automatically (the counter is persistent). Owned-files list updated; T12 no longer touches `handlers/mod.rs`.
  - **B6**: State-file durability reframed. Per-record fsync is wasted because the daemon's existing duplicate-detection is the real correctness mechanism. State file is now explicitly a performance optimization. On crash, re-run finds the file truncated; the daemon catches re-attempts as duplicates and returns `existing_id`, which the importer records. Documented in Goal section + Architecture.
  - **R1**: Grep-fail invariant test regex broadened from literal `Substrate::write_memory` to `\bwrite_memory\b` (catches `substrate.write_memory(...)` method calls), with `//`-comment lines stripped to avoid false-positives.
  - **R2**: Dropped the "<30% wiki-link amplification" estimate from risks; not load-bearing given the topological design, and Codex review showed the estimate wasn't reproducible from observable data.
  - **N1**: Removed reference to spec §4.2 from T04 (`.memory-project.yaml` schema lives in Stream E docs, not Stream A §4.2 as previously cited).
  - **Cargo.toml additions** explicit per-task: T01 adds `serde_json`, `sha2`, `dirs`, `walkdir`, `dialoguer`, `regex` (dev-deps), `serial_test` (dev-deps).
  - Wave structure: was Wave 1–5 (13 tasks); now Wave 0–5 (14 tasks) with T00 as Wave 0 prerequisite alongside the three pre-dogfood UX tasks.
- 2026-05-27 v0.2: Patched after opus plan-reviewer pass. Changes:
  - **B1**: Dropped invented `next_memory_ids` batched daemon call; per-write ID minting via existing daemon flow. Wiki-link resolution runs as Pass 5 after all writes, using response-supplied IDs.
  - **B2**: Cut all references to `scripts/spawn-task-worktree.sh` and `scripts/integrate-task-worktree.sh` (both hardcode `stream-a/` paths). Workers create worktrees manually; orchestrator integrates via `git merge --ff-only`.
  - **B3**: Split `--from <path>` into `--from-claude <path>` and `--from-codex <path>`.
  - **B4**: T11 descoped (now T12) to CLI-only first-write signal. Detection via events-log lookup; daemon-restart safe. No protocol changes.
  - **R1**: T05 (originally one big task) split into T05 (parse + dedup + plan) and T06 (write loop + report). All subsequent tasks renumbered (+1).
  - **R3**: T04 deliverable pre-decided to call `recall::project::resolve_project_binding` directly; if signature isn't import-friendly, expose a thinner `pub fn` wrapper rather than copying logic.
  - **R4**: Locked `confidence: 0.5` flat default; outcome-based mapping deferred to v2 explicitly.
  - **R5**: T09 (README) stubs docs-map entries for `docs/troubleshooting.md` and `docs/importer.md` so T10 and T13 don't touch README.md.
  - **R7**: Clarified atomic-state-file save semantics — tmp+rename per record, parent-dir fsync only on canonical final save.
  - **R8**: Locked `discover_claude_memory_paths` precedence: flag override → `CLAUDE_CONFIG_DIR` → `autoMemoryDirectory` → default. Locked Codex precedence: flag → `CODEX_HOME` → default.
  - **R9**: Added grep-fail invariant test in T05 enforcing invariant #2 (no direct `Substrate::write_memory` calls in importer).
  - **R10**: `#[serial]` mandatory on all T08 tests (was T07).
  - **N1**: Rephrased invariant #4 from "leave sensitivity unset on the wire" (technically vacuous — request shape has no sensitivity field) to "never pass `Trusted` classification override; daemon classifies."
  - **N2**: Removed open question #7 (Codex handoff — not actionable).
  - **N5**: Updated per-task gate reference table to include `cargo test -p memoryd-web` for tasks touching the web crate.
  - **N6**: State-file schema example now shows a non-empty `supersession_chain`.
  - Added invariant #11 (no new `RequestPayload` variants) explicitly.
