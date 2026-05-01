# Stream F Final Gate E API/Contract Architecture Review

Date: 2026-05-01
Reviewer: Codex backend-architecture review lane
Scope: review-only API/schema/contract pass over the current Stream F diff and the active Stream F docs.

## Verdict

BLOCK final Gate E.

There are S1 and S2 contract findings. The implementation has strong internal test coverage around individual seams, but the public DreamNow surfaces are still fixture-shaped: they do not hydrate real Stream A/E inputs and do not persist Pass 2 candidates through Stream C/governance. The daemon DreamNow protocol also rejects the real harness names that the spec exposes, `memory_observe`'s implemented schema is not compatible with the normative v0.2 schema, and lease failure/force paths leave stale active leases.

## Findings

### S1 - Public DreamNow does not write Pass 2 candidates to the candidate queue or hydrate real substrate/active-memory inputs

Evidence:

- Spec invariant: Pass 2 candidates must go to the candidate queue under `dreaming-strict`, not auto-promote and not disappear: `docs/specs/stream-f-dreaming-v0.2.md:183-185`.
- Spec acceptance: Pass 2 produces candidates that land in the candidate queue under `dreaming-strict`: `docs/specs/stream-f-dreaming-v0.2.md:831-832`.
- API docs promise manual/scheduled runs select substrate fragments, write Pass 2 candidates to the canonical queue, and emit reports: `docs/api/stream-f-dreaming-api.md:130-140`.
- CLI DreamNow builds `DreamRunOptions` with `substrate_fragments: Vec::new()`, `active_memories: Vec::new()`, and `previous_questions: Vec::new()`: `crates/memoryd/src/main.rs:340-351`.
- CLI DreamNow then runs `DreamRunner` with `NoopCandidateWriter`: `crates/memoryd/src/main.rs:353-354`.
- Daemon DreamNow repeats the same empty inputs and `NoopCandidateWriter`: `crates/memoryd/src/handlers.rs:166-180`.
- `NoopCandidateWriter` always returns `accepted: false` with `reason: "noop_candidate_writer"`: `crates/memoryd/src/dream/run.rs:95-107`.

Impact:

The public API can produce a DreamRunReport and journal/question files, but it cannot satisfy the central Stream F promise that Pass 2 candidates enter the canonical candidate queue under `dreaming-strict`. It also cannot produce meaningful synthesis because the public path does not load real substrate fragments, active memories, or previous questions. Tests around `DreamRunner` with a recording writer prove the internal seam, not the actual daemon/CLI contract.

Required fix:

- Implement a production `CandidateWriter` backed by the existing governance/write path or a deliberately factored Stream C candidate-queue API.
- Hydrate `DreamRunOptions` from Stream A/E data for the selected scope/window: plaintext substrate fragments, safe encrypted descriptors, active memories/recall projection, and recent surfaced questions.
- Route both CLI and daemon DreamNow through that production writer and hydration path.
- Add acceptance coverage that invokes the public CLI/daemon DreamNow path with a valid Pass 2 candidate and then verifies a persisted candidate memory with `policy_applied: dreaming-strict`, `status: candidate`, `author.kind: dreaming`, and `grounding_rehydration_required: true`.

### S2 - Daemon DreamNow protocol rejects the real harness selection contract

Evidence:

- Spec exposes `RequestPayload::DreamNow { scope, force, cli_override }`, where `cli_override` bypasses per-scope priority for one run: `docs/specs/stream-f-dreaming-v0.2.md:235-240`.
- Spec says harness selection is per-scope and uses installed/authenticated harnesses after lease acquisition: `docs/specs/stream-f-dreaming-v0.2.md:466-476`.
- API docs say dreaming shells out to installed harness CLIs selected by synced priority/local availability: `docs/api/stream-f-dreaming-api.md:116-126`.
- The daemon handler accepts only `cli_override=echo`; every other value, including known v0.2 adapters, is rejected before selection: `crates/memoryd/src/handlers.rs:186-190`.
- The daemon selector only returns a deterministic Echo harness and rejects `None`, `claude`, `codex`, or disabled adapters: `crates/memoryd/src/handlers.rs:193-211`.

Impact:

The daemon protocol type advertises a real DreamNow operation, but the server-side implementation is fixture-only. Any client using the daemon protocol rather than the direct CLI path cannot run Stream F with `claude`/`codex` or config priority, and `cli_override: None` never performs automatic selection.

Required fix:

- Move the CLI `HarnessCliRegistry` selection behavior behind a shared production function usable from daemon handlers.
- Preserve `echo` only as an explicit test/admin fixture, not as the only daemon-supported harness.
- Add daemon-protocol tests for `cli_override: Some("codex")` with a PATH-stubbed/authenticated adapter and `cli_override: None` using configured priority.
- If daemon DreamNow is intentionally fixture-only, remove or version-gate `RequestPayload::DreamNow` from the public daemon contract and update spec/API docs. As written, the contract says it is real.

### S2 - `memory_observe` MCP/daemon schema is not compatible with the normative v0.2 request shape

Evidence:

- Spec defines the agent-facing `memory_observe` request with only `text`, `kind`, and optional `entities`: `docs/specs/stream-f-dreaming-v0.2.md:97-116`.
- Spec defines daemon `RequestPayload::Observe` with only `text`, `kind`, and defaulted `entities`: `docs/specs/stream-f-dreaming-v0.2.md:225-233`.
- Implementation requires extra MCP fields `cwd`, `session_id`, and `harness`: `crates/memoryd/src/mcp.rs:376-399`.
- Implementation also makes those fields part of `RequestPayload::Observe`: `crates/memoryd/src/protocol.rs:85-94`.
- The API doc now documents the expanded shape instead of the normative v0.2 shape: `docs/api/stream-f-dreaming-api.md:31-44` and `docs/api/stream-f-dreaming-api.md:67-81`.

Impact:

An agent/client written to the v0.2 spec cannot call `memory_observe`; the server rejects the spec-shaped payload as missing required fields. The added caller-binding data is reasonable as an internal invariant, but it is an incompatible public schema change unless the spec is amended or the fields are supplied by transport/session context rather than the tool's required arguments.

Required fix:

- Either amend `docs/specs/stream-f-dreaming-v0.2.md` explicitly before final gate, or restore compatibility by making caller-binding fields optional/defaulted at the MCP boundary and deriving them from the MCP/session context when possible.
- Add compatibility tests that the exact spec-shaped payload parses and either writes a fragment with derived binding or returns a typed `invalid_request` explaining missing binding under a versioned contract.
- Keep `memory_note` unchanged; do not backfill these fields into `memory_note`.

### S2 - Lease release/force semantics leave stale active leases

Evidence:

- Spec says if no eligible CLI is found after lease acquisition, the lease is released within the same second and the run reports `dream_unavailable`: `docs/specs/stream-f-dreaming-v0.2.md:476`.
- CLI DreamNow acquires the lease before harness selection: `crates/memoryd/src/main.rs:320-331`.
- Harness selection then exits on unknown/no eligible harness without any lease-release path: `crates/memoryd/src/main.rs:360-383`.
- Spec says `memoryd dream now --force` overwrites/releases an active lease and proceeds: `docs/specs/stream-f-dreaming-v0.2.md:570` and `docs/specs/stream-f-dreaming-v0.2.md:759`.
- Implementation of `force` only skips the active-lease check, then appends another lease record: `crates/memoryd/src/dream/lease.rs:133-153`.
- Active lease lookup returns the first unexpired record for the scope, so the stale foreign record can continue to dominate later non-force attempts: `crates/memoryd/src/dream/lease.rs:251-253`.
- Existing force coverage only asserts the new device string appears; it does not assert the old active lease is released/expired/ignored: `crates/memoryd/tests/dream_lease_election.rs:187-212`.

Impact:

A `dream_unavailable` or unknown-harness failure can leave an active lease for the full lease window. A forced takeover can leave two active records for the same scope and later readers may still observe the stale one. That undermines the daemon/MCP protocol contract and the scheduled/manual split.

Required fix:

- Add an explicit release/expiry record or active-record supersession rule and make readers choose the latest non-released lease per scope.
- On post-acquisition harness-selection failure, release the just-acquired lease before returning/exiting.
- Make `--force` release/expire the prior active lease deterministically, not just append a second active record.
- Add tests for: no active lease remains after `dream_unavailable`; forced takeover leaves only the forced holder active; subsequent non-force acquisition sees the forced holder, not the stale holder.

### S3 - Public API docs do not document stable CLI exit-code mappings for Stream F errors

Evidence:

- Spec requires stable typed error codes and lists Stream F codes/retryability: `docs/specs/stream-f-dreaming-v0.2.md:746-758`.
- API docs describe exit behavior in prose but omit concrete exit codes for `invalid_request`, `dream_unavailable`, `lease_*`, `dream_pass_failed`, `privacy_error`, and `dream_disabled`: `docs/api/stream-f-dreaming-api.md:109-115`.
- Implementation has concrete exit behavior for dream lease errors and pass failure: `crates/memoryd/src/main.rs:197-202` and `crates/memoryd/src/main.rs:459-461`.

Impact:

Operators and wrappers still have to inspect prose/stderr rather than a documented stable CLI contract. This is lower severity than the runtime contract blockers because the typed daemon protocol exists, but it is a docs-truthfulness gap for the CLI/admin API.

Required fix:

- Add a small exit-code table to `docs/api/stream-f-dreaming-api.md` matching implementation and spec retryability.
- Add/adjust CLI contract tests to pin the table for `invalid_request`, `dream_unavailable`, `lease_held`, `lease_unavailable`, `lease_dirty_tree`, and `dream_pass_failed`.

## Required fixes

1. Replace public-path `NoopCandidateWriter` with a production governance-backed candidate writer and hydrate real Stream A/E dream inputs.
2. Make daemon DreamNow support the same harness registry/priority/override contract as CLI DreamNow, or remove/version-gate the daemon DreamNow public contract.
3. Resolve `memory_observe` schema compatibility by either amending the v0.2 spec or restoring the spec-shaped request at MCP/protocol boundaries.
4. Implement lease release/overwrite semantics for harness-selection failure and `--force`, with tests that stale active leases cannot continue to win.
5. Document Stream F CLI exit-code mappings in the API doc and pin them with contract tests.

## Residual risks

- I did not run the full workspace release gate because this was a review-only API/contract pass. The findings above are from source inspection and narrow tests, not from full fmt/clippy/test/doc/bench validation.
- Grounding rehydration exists at review-approval time, but because public DreamNow currently uses `NoopCandidateWriter`, I did not verify an end-to-end dream-authored candidate from public DreamNow through later approval/quarantine.
- The current diff is large and includes many untracked Stream F files; line evidence is based on the live worktree as of this review.

## Commands run

```text
sed -n '1,220p' /Users/treygoff/.agents/skill-library/clean-code/SKILL.md
sed -n '1,220p' /Users/treygoff/.agents/skill-library/tdd/SKILL.md
sed -n '1,240p' /Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md
rg -n "Stream F|stream-f|dreaming|Stream A|Stream B|Stream C|Stream D|Stream E|agent-memory" /Users/treygoff/.ai-profiles/runtime/codex/personal/memories/MEMORY.md | head -80
rg -n "Stream F|stream-f|dreaming" /Users/treygoff/.ai-profiles/runtime/codex/personal/memories/MEMORY.md
git status --short
git diff --stat
git diff --name-only
nl -ba docs/plans/2026-04-30-stream-f-dreaming.md | sed -n '1,260p'
nl -ba docs/specs/stream-f-dreaming-v0.2.md | sed -n '1,320p'
nl -ba docs/reviews/stream-f-contract-map.md | sed -n '1,320p'
nl -ba docs/api/stream-f-dreaming-api.md | sed -n '1,360p'
rg -n "Observe|MemoryObserve|DreamNow|DreamStatus|Dream|lease_|lease|dream_disabled|dream-disabled|scope|cli_override|prompt_transport|NotACanonicalMemory|read_path_envelope|memory_observe|target|ObserveTarget|secret|SecretRefused" crates/memoryd/src crates/memory-substrate/src crates/memoryd/tests crates/memory-substrate/tests docs/api README.md CLAUDE.md
find crates/memoryd/src/dream -maxdepth 2 -type f | sort | xargs -I{} sh -c 'echo --- {}; nl -ba {} | sed -n "1,240p"'
nl -ba crates/memoryd/src/protocol.rs | sed -n '1,280p'
nl -ba crates/memoryd/src/cli.rs | sed -n '1,300p'
nl -ba crates/memoryd/src/main.rs | sed -n '1,430p'
nl -ba crates/memoryd/src/mcp.rs | sed -n '1,470p'
nl -ba crates/memoryd/src/handlers.rs | sed -n '1,560p'
nl -ba crates/memoryd/src/handlers.rs | sed -n '2090,2140p'
nl -ba crates/memory-substrate/src/api.rs | sed -n '1,520p'
nl -ba crates/memory-substrate/src/model.rs | sed -n '1070,1135p'
nl -ba crates/memory-substrate/src/config/mod.rs | sed -n '1,520p'
nl -ba crates/memory-substrate/src/tree/validate.rs | sed -n '1,420p'
nl -ba crates/memory-substrate/src/tree/layout.rs | sed -n '1,280p'
nl -ba crates/memory-substrate/src/merge/three_way.rs | sed -n '1,360p'
nl -ba crates/memory-substrate/src/git/commit.rs | sed -n '1,260p'
rg -n "enum HandlerError|struct HandlerError|impl HandlerError|from_lease|dream_unavailable|fn code|retryable|exit_dream_error|lease_held|lease_unavailable|lease_dirty_tree" crates/memoryd/src/handlers.rs crates/memoryd/src/main.rs crates/memoryd/src/dream/lease.rs
rg -n "is_noncanonical_stream_f_repo_path|RepoPath::try_new|allowed|top-level|substrate/archive" crates/memory-substrate/src/model.rs crates/memory-substrate/src/error.rs crates/memory-substrate/tests/dream_canonical_isolation.rs crates/memory-substrate/tests/tree_validation.rs
rg -n "Release|release|commit_lease|LeaseCommitAction::Release|lease is released|lease.*release" crates/memoryd/src/dream crates/memoryd/tests docs/specs/stream-f-dreaming-v0.2.md
rg -n "CandidateWriter|NoopCandidateWriter|candidate_writer|dreaming-strict|grounding_rehydration|required|candidate" crates/memoryd/src/dream crates/memoryd/src/handlers.rs crates/memoryd/tests/dream_pass_pipeline.rs crates/memoryd/tests/dream_grounding_rehydration.rs docs/specs/stream-f-dreaming-v0.2.md docs/api/stream-f-dreaming-api.md
cargo test -p memoryd --test dream_lease_election force_overrides_active_foreign_lease -- --nocapture  # passed
cargo test -p memoryd --test mcp_manifest mcp_manifest_memory_observe_schema_declares_stream_f_shape -- --nocapture  # passed
cargo test -p memoryd --test dream_cli dream_now_cli_runs_echo_end_to_end -- --nocapture  # 0 tests matched; wrong filter
cargo test -p memoryd --test handler_contract -- --nocapture  # passed
cargo test -p memoryd --test dream_cli -- --list
cargo test -p memoryd --test dream_cli dream_now_echo_runs_pipeline_after_acquiring_lease -- --nocapture  # passed
git status --short docs/reviews/stream-f-final-api-contract-review.md
git diff --no-index --check /dev/null docs/reviews/stream-f-final-api-contract-review.md || true  # no whitespace errors printed
```
