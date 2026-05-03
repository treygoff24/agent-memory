# Post-Shipping Audit — Streams G/H/I + Cross-Stream Surface

**Date:** 2026-05-02
**Auditor:** Claude (architect/reviewer role)
**Scope:** Independent review of the Codex 12-hour autonomous Stream G/H/I run (snapshot `6095cf6`) plus the three review-fix commits (`cf9736f`, `aa319f6`, `d9628cb`). Also audits cross-stream contract seams that Codex's per-stream self-review fan-out would not have caught, the existing Stream A → Stream B MCP surface, and product-shape gaps.
**Method:** Three parallel read-only subagents (review-trail audit, Stream A drift audit, cross-stream contract audit) plus first-party investigation of the MCP wiring and user-facing surface. Cross-referenced against live specs (`docs/specs/system-v0.2.md`, `stream-{a,c,d,e,f,g,h,i}-*-v*.md`), live API docs (`docs/api/`), and live plans (`docs/plans/`).
**Workspace state at audit time:** `bash scripts/check.sh` (BENCH_PROFILE=darwin-arm64) **PASSED** end-to-end. 332 test suites green. Two-clone convergence ok. Bench gates wrote smoke + release results. Bench regression check ok. Specgate emitted six `orphaned_specs` warnings (see F-013). No clippy warnings, no rustfmt diffs, no rustdoc warnings, no oxlint findings.

---

## How to use this document

This is a fix-list. Each finding is numbered (`F-001` through `F-013`) so Codex can reference findings in branch names and commit messages (e.g. `fix(F-001): wire stdio MCP server`). Each finding has:

- **Severity** — P0 (blocks usable product or core contract), P1 (real correctness/contract bug), P2 (process violation or unaddressed risk), P3 (cleanliness/docs/minor)
- **Status** — `open` / `partial-fix-shipped` / `process-violation` / `doc-gap`
- **Streams** — which streams the fix touches
- **Evidence** — file:line refs the auditor verified, plus commands that reproduce
- **Root cause** — what actually went wrong, not just the symptom
- **Required fix** — concrete steps. Not aspirational; if the fix has options, all options are listed with the auditor's recommended choice
- **Acceptance criteria** — how Codex (and Trey on final review) will know the fix is done
- **Test coverage** — required new or modified tests

After Codex completes the fix-list, Trey will request a second-pass audit before merging.

---

## Summary table

| ID    | Severity | Streams   | One-line                                                                                   |
| ----- | -------- | --------- | ------------------------------------------------------------------------------------------ |
| F-001 | **P0**   | B         | No stdio MCP server exists. No MCP client can connect to memoryd today.                    |
| F-002 | **P0**   | E + H     | Recall renderer emits bullet-list text; eval harness parses XML. Mutual contract drift.    |
| F-003 | P1       | A         | Two new public substrate APIs added during Gate B without spec authorization.              |
| F-004 | P1       | G         | TUI and web dashboards do not consume `RecallHit` events. Observability surfaces are dark. |
| F-005 | P1       | A         | `events_log` schema diverges from spec (`event_id TEXT PK` vs `seq INTEGER PK`).           |
| F-006 | P2       | G + I     | Codex self-promoted Stream G/I bench canonicals during autonomous run.                     |
| F-007 | P2       | H + F + D | T17 and T18 are permanent-skip smoke detectors with no plausible activation path.          |
| F-008 | P2       | G         | `audit_walk` 501 stub and synthetic TUI bench were "fixed" by retroactive spec amendment.  |
| F-009 | P2       | A         | `ensure_events_log_identity_schema` silently drops data on schema mismatch.                |
| F-010 | P3       | (cross)   | No README, no install doc, no MCP wiring example, no getting-started flow.                 |
| F-011 | P3       | A         | `observed_at` hydrated from `frontmatter.extras` by string-key, fragile pattern.           |
| F-012 | P3       | (docs)    | CLAUDE.md and project status overstate Stream B as "MCP server shipped."                   |
| F-013 | P3       | A         | Specgate emits six `orphaned_specs` warnings on a green build.                             |

**Counts:** 2× P0, 3× P1, 4× P2, 4× P3 = 13 findings.

---

## F-001 — Stdio MCP server does not exist

- **Severity:** P0
- **Status:** open
- **Streams:** B (originating); affects entire product surface
- **Affects user-facing pitch:** yes — primary product surface

### Evidence

The system spec `docs/specs/system-v0.2.md` §4 explicitly defines the project as "harness-agnostic, daemon-backed shared memory ... that works across Claude Code, Codex CLI, Cursor, OpenClaw, Factory, and any other MCP-speaking harness without forking, modifying, or wrapping the harness." Architecture diagram on line 105: "MCP + hooks MCP + hooks MCP only".

The Stream B plan `docs/plans/2026-04-28-stream-b-daemon-mcp.md` line 5: "a local memoryd process, a socket protocol, agent-facing request handlers, and a CLI/MCP bridge foundation." Task 6 (line 138) is titled "Thin MCP Forwarder Foundation."

What was actually built in `crates/memoryd/src/mcp.rs`:

- The MCP `Manifest` struct and `ToolDescriptor` (lines 19-27)
- The nine tool descriptors (`memory_search`, `memory_get`, `memory_write`, `memory_supersede`, `memory_forget`, `memory_reveal`, `memory_observe`, `memory_note`, `memory_startup`)
- `pub async fn forward_to_daemon` at `crates/memoryd/src/mcp.rs:181`
- `pub async fn forward_payload_to_daemon` at `crates/memoryd/src/mcp.rs:223`
- Request-conversion helpers (`request_from_args`, `ToolName`, `ToolRequest`)

What does **not** exist in the codebase:

1. Any binary or subcommand that runs a JSON-RPC `tools/list` / `tools/call` loop over stdin/stdout.
2. Any caller of `forward_to_daemon` outside of `crates/memoryd/tests/*.rs`. Verified: `grep -rE 'forward_to_daemon|forward_payload_to_daemon' crates/ | grep -v 'mcp.rs:'` returns only test files.
3. Any reference to JSON-RPC protocol primitives. Verified: `grep -rE 'jsonrpc|tools/list|tools/call|JsonRpc' crates/` returns zero hits.
4. Any stdio reader in the daemon. Verified: `grep -rE 'tokio::io::stdin|io::stdin\(\)|BufReader::new\(stdin' crates/` returns only TTY-detection in the `ui` subcommand and a y/N confirmation prompt in `main.rs`.

`memoryd --help` enumerates 18 subcommands (`serve`, `status`, `doctor`, `search`, `get`, `write-note`, `write`, `supersede`, `forget`, `review`, `recall`, `dream`, `peer`, `ui`, `web`, `reality-check`, `privacy`, `privacy-filter`, `device`). None speaks MCP-stdio. None of the top-level options is `--socket`.

The Stream H eval-harness spec text contradicts this. `docs/specs/stream-h-eval-harness-v0.1.md` documents the per-invocation MCP config as:

```json
{ "mcpServers": { "<server_name>": { "command": "memoryd", "args": ["--socket", "<socket_path>"] } } }
```

— but `memoryd --socket <path>` would fail because `--socket` is not a top-level flag and there is no MCP-server subcommand for it to attach to. The eval harness sidesteps this by calling `forward_to_daemon` as library code (`crates/memorum-eval/src/harness_runner.rs`), which is why eval-harness tests pass without an MCP wire.

### Root cause

Stream B Task 6 ("Thin MCP Forwarder Foundation") delivered the **library** scaffolding — manifest, tool descriptors, daemon-protocol forwarder — but did not deliver the **stdio server** that turns those into something an external MCP client can launch as a child process. No subsequent stream (C, D, E, F, G, H, I) was scoped to add it. The CLAUDE.md status line "Stream B shipped 2026-04-28 ... seven-tool MCP forwarder (now eight after Stream D added `memory_reveal`)" is technically accurate about the forwarder but conflates "forwarder library exists" with "MCP server is operable."

### Required fix

1. **Add a new subcommand `memoryd mcp`** (or new binary `memorum-mcp`) that:
   - Reads JSON-RPC 2.0 framed messages from stdin (LSP-style framing or NDJSON, whichever the MCP spec mandates — verify against the current MCP spec; recommended: use the `rmcp` crate or hand-rolled stdio loop matching what the Anthropic MCP SDK expects).
   - Implements the standard MCP handshake (`initialize`, `initialized` notification).
   - Implements `tools/list` by serializing the existing `mcp::manifest()` output.
   - Implements `tools/call` by routing each tool name to the existing `forward_to_daemon` / `forward_payload_to_daemon` library functions, then wrapping the response in JSON-RPC `result` shape.
   - Takes a `--socket <PATH>` flag specifying the daemon socket path (default: `/tmp/memoryd.sock` to match `memoryd serve`).
   - Exits cleanly on stdin EOF.
   - Emits structured logs to stderr only (stdout is reserved for JSON-RPC frames).
2. **Update `docs/specs/stream-h-eval-harness-v0.1.md` §3.2** to specify the actual invocation: `"command": "memoryd", "args": ["mcp", "--socket", "<socket_path>"]` (or whichever subcommand name is chosen).
3. **Update the CLI integration test** in `crates/memoryd/tests/` to invoke the new subcommand and verify a real MCP handshake + `tools/list` round-trip.
4. **Recommended: add a separate Stream B spec on disk** (`docs/specs/stream-b-daemon-mcp-v0.1.md`) that codifies the daemon protocol, the MCP server's stdio surface, and the contract between them. This would have caught the gap earlier.

### Acceptance criteria

- `memoryd mcp --socket <path>` (or chosen invocation) reads JSON-RPC frames from stdin and writes responses to stdout.
- A test invokes the binary, sends an `initialize` request, waits for the response, sends a `tools/list` request, parses the response, and verifies that the returned tool list matches `mcp::manifest()` byte-for-byte at the descriptor level.
- A test invokes the binary, performs the MCP handshake, sends a `tools/call` for `memory_search` with a known query, and verifies the daemon-forwarded response is wrapped correctly.
- A separate test demonstrates wiring with the actual Anthropic MCP SDK (or a minimal MCP-protocol smoke client) over a real spawned subprocess.
- `docs/specs/stream-h-eval-harness-v0.1.md` MCP-config table matches the actual invocation.

### Test coverage required

- `crates/memoryd/tests/mcp_stdio_handshake.rs` — initialize + initialized + tools/list end-to-end.
- `crates/memoryd/tests/mcp_stdio_tool_call.rs` — tools/call over real subprocess.
- Optional: a smoke test that spawns the binary with the MCP SDK as a fixture client.

---

## F-002 — Recall renderer emits bullet-list; eval harness parses XML

- **Severity:** P0
- **Status:** open
- **Streams:** E (renderer); H (eval assertions)
- **Discovered by:** cross-stream contract audit (Seam 3)

### Evidence

`crates/memorum-eval/src/assertions.rs:74` defines:

```rust
pub fn parse_recall_block(xml: &str) -> Result<RecallBlock, AssertionError>
```

This parser looks for `<memory ref="...">...</memory>` XML elements inside the `<memory-recall>` block to extract `ref_id`. `assertions.rs:90` defines `assert_memory_in_recall(block, ref_id)` which iterates the parsed `<memory>` elements.

`crates/memoryd/src/recall/render.rs:73` defines:

```rust
pub fn render_memory_entry(entry: &RecallEntry) -> String
```

— which produces bullet-list text in the format:

```
- [mem_id] summary — snippet (updated ...; source ...; confidence ...)
```

There is no `<memory ref=...>` element emitted by `render_memory_entry` or by any other renderer in `crates/memoryd/src/recall/render.rs`.

The eval harness unit tests in `crates/memorum-eval/src/assertions_unit.rs` hand-craft XML with `<memory ref="mem-alpha">` shape and pass — this is why the eval test suite is green despite the contract drift. A real harness invocation feeding live daemon output to `parse_recall_block` would parse zero `<memory>` elements, and every `assert_memory_in_recall` would fail.

This bug is invisible to per-stream review because each side is internally consistent. It is exactly the seam class that fan-out review misses.

Combined with F-007 (T17/T18 permanent-skip) and the Stream H "live LLM tests are auth-gated" caveat: even when authenticated CLI runs do execute, the recall-content assertions cannot validate live daemon output.

### Root cause

Stream H spec (`docs/specs/stream-h-eval-harness-v0.1.md`) describes the assertions but does not pin the recall-block format precisely against Stream E's renderer output. Stream E spec (`docs/specs/stream-e-passive-recall-v0.5.md`) defines the recall block structure, but the format inconsistency between human-readable bullets and machine-parseable XML elements was not surfaced as a contract requirement.

### Required fix

Pick one side. The auditor's recommendation: **emit XML elements from the renderer**. Reasoning:

1. The recall block is consumed by LLM agents reading raw text — XML is more parseable for downstream code (eval harness, future tools, possibly future agent-side scripting), and reads only marginally worse for an LLM than bullet-list text.
2. The eval harness's existing `parse_recall_block` is the correct contract shape; rewriting it to parse free-form bullets is fragile (whitespace, em-dash escaping, etc.).
3. The Stream E spec v0.5 already uses XML envelopes for the outer block (`<memory-recall version="stream-e-v0.5">`, `<pending-attention>`, `<peer-update>`, `<peer-presence>`, `<entity-recall entities="...">`). Per-memory bullets inside an XML envelope are inconsistent.

Option A (recommended): change `render_memory_entry` to emit:

```xml
<memory ref="mem_id" updated="..." source="..." confidence="...">
  <summary>...</summary>
  <snippet>...</snippet>
</memory>
```

— and update `docs/specs/stream-e-passive-recall-v0.5.md` to specify this exact shape.

Option B: keep bullet-list output, change `parse_recall_block` to parse the bullet format, and document the bullet format in spec §format.

Either way, the spec text and the renderer must agree, and the eval harness must parse what the renderer actually emits.

### Acceptance criteria

- A new eval-harness regression test calls the live daemon's startup-recall path with a fixture memory and asserts that `assert_memory_in_recall` finds the memory using the live output (not hand-crafted XML).
- `docs/specs/stream-e-passive-recall-v0.5.md` is updated with the chosen format pinned in spec text.
- The recall block format is golden-tested in both the renderer and the eval harness against the same fixture string.

### Test coverage required

- New: `crates/memorum-eval/tests/recall_block_format_round_trip.rs` — render via Stream E renderer, parse via Stream H assertions, assert ref_ids match.
- Existing: `assertions_unit.rs` fixtures must be regenerated from the live renderer rather than hand-crafted.

---

## F-003 — Two new public substrate APIs added without spec authorization

- **Severity:** P1
- **Status:** process-violation + open
- **Streams:** A (substrate contract violation); G (origin of the additions)

### Evidence

Two new public functions on `Substrate` were added during Stream G's Gate B review-fix loop:

1. `pub async fn update_encrypted_memory_metadata` at `crates/memory-substrate/src/api.rs:685`
2. `pub async fn query_recall_index_including_metadata_only` at `crates/memory-substrate/src/api.rs:1148`

Neither function is mentioned in:

- `docs/specs/stream-a-core-substrate-v1.1.md` (live Stream A spec)
- `docs/specs/stream-g-observability-v0.1.md` (live Stream G spec)
- `docs/api/stream-a-public-api.md` (Stream A public API doc)
- `docs/specs/system-v0.2.md` (live system spec)

They are referenced only in `docs/reviews/stream-g-review-gate-b-clean-code-rerun-2.md` (the review doc that authorized the additions).

The CLAUDE.md "Critical invariants" section repeats the rule: "Do not modify Stream A modules unless Trey explicitly redirects." The `946d75f` FTS5 sanitization fix is the only previously-authorized exception.

### Root cause

A Gate B review uncovered a real need (encrypted memory metadata mutation without decryption; encrypted-row scoreability for Reality Check eligibility), and the response was to add public substrate APIs in the fix commit rather than push the requirement through spec-amend → plan-amend → ratified-implementation. This worked correctness-wise but bent the "Stream A is a frozen contract" rule that downstream stream isolation depends on.

### Required fix

Two valid paths:

**Path 1 (recommended): ratify and document.**

1. Amend `docs/specs/stream-a-core-substrate-v1.1.md` — bump to v1.2 (or add a §1.4 "Post-v1.1 authorized additions") — authorizing both APIs by name with their full signatures and contract semantics.
2. Update `docs/api/stream-a-public-api.md` to enumerate them in the public surface.
3. Add explicit invariant tests under `crates/memory-substrate/tests/` covering the new APIs' guarantees (e.g. `update_encrypted_memory_metadata` preserves ciphertext envelope; `query_recall_index_including_metadata_only` does not leak plaintext fragments from encrypted rows).

**Path 2: relocate the APIs out of substrate.**

1. Move `update_encrypted_memory_metadata` to `crates/memory-privacy` or `crates/memoryd` as a higher-layer composition over existing substrate primitives.
2. Move `query_recall_index_including_metadata_only` to `crates/memoryd` as a query-layer wrapper.
3. Remove the two functions from `Substrate`'s public API.

Path 1 is the lower-effort durable fix because the implementations are already correct and the review confirmed the need. Path 2 is more architecturally pure but adds churn.

### Acceptance criteria

- One of the two paths is selected and applied.
- A grep `pub async fn update_encrypted_memory_metadata\|pub async fn query_recall_index_including_metadata_only` returns either (Path 1) substrate hits with new spec/API-doc citation, or (Path 2) zero substrate hits with the functions relocated.
- The new tests for the chosen path pass under the full release gate.

---

## F-004 — TUI and web dashboards do not consume RecallHit events

- **Severity:** P1
- **Status:** open
- **Streams:** G

### Evidence

`Substrate::record_recall_hit` is correctly wired: `crates/memory-substrate/src/api.rs:1305` defines the API, and `crates/memoryd/src/recall/render.rs:117-129` calls it via `emit_recall_hits` whenever a recall block surfaces memories.

The substrate side of the contract is implemented. The consumer side is not:

- `crates/memoryd-tui/` — `grep -rE 'RecallHit|EventKind|events_log|subscribe' crates/memoryd-tui/src/` returns zero hits.
- `crates/memoryd-web/` — same grep returns zero hits.
- Neither dashboard implements any event-log subscription, polling, or query mechanism.

Stream G spec `docs/specs/stream-g-observability-v0.1.md` describes the TUI panels (overview, timeline, review_queue, reality_check, entities, namespace, conflicts, policy) and the web routes (status, review, reality_check, entity_graph, audit, roi). The Reality Check, Review Queue, and Status panels are wired to the daemon via the `client.rs` layer. Recall observability is not wired anywhere.

The events_log mirror writes correctly. The `EventKind::RecallHit` variant is correctly populated. Nothing reads it back into a user-visible surface.

### Root cause

Stream G's spec implies recall observability via the events_log mirror, and the substrate-side write was implemented (per F-003 evidence, the events_log mirror table and `RecallHit` variant are spec-authorized). The TUI and web implementations focused on Reality Check, Review Queue, and Privacy/Status panels but did not wire any recall-hit consumer. The mirror exists, the events flow into it, no UI shows them.

### Required fix

1. Decide on UX: should recall hits surface in the timeline panel? In the overview panel? In a dedicated panel? In the web `/api/audit` route or a new `/api/recall-hits` route?
2. Implement the chosen surface:
   - For TUI: add a recall-hits view in the timeline panel or overview panel that polls (or subscribes to) the events_log mirror and renders recent `RecallHit` entries with `recalled_at`, memory id, and a short fragment.
   - For web: add a `/api/recall-hits?since=<ts>&limit=<n>` route that returns recent recall hits.
3. Update Stream G spec text to specify the chosen surface.

If the intent was always to defer this to v1.1+, then:

- Update Stream G spec deferred-list to explicitly call out "recall-hit consumption in TUI/web is v1.1+" and reference this finding.
- Document that recall observability is currently write-only.

But the recommended fix is implementation, not deferral, because the events are already being written and the user-facing pitch of Stream G includes "human observability" of agent behavior.

### Acceptance criteria

- Either TUI or web (or both) renders recent `RecallHit` events from the events_log mirror.
- A test invokes the recall path against the daemon, then queries the new TUI/web surface and asserts the hit is visible.
- Stream G spec text reflects whichever decision was made (implement or defer).

---

## F-005 — `events_log` schema diverges from spec

- **Severity:** P1
- **Status:** spec-vs-impl drift (impl is better than spec; spec needs amending)
- **Streams:** A; G (origin)

### Evidence

`docs/specs/stream-g-observability-v0.1.md` §1.3 #2 specifies the events_log table schema with `seq INTEGER PRIMARY KEY`.

The implementation in `crates/memory-substrate/src/index/` chose `event_id TEXT PRIMARY KEY` with `seq` as a non-key column. The implementation choice is materially better:

- `seq` is per-device (each device has its own sequence), so a `seq INTEGER` PK would collide on multi-device ingest.
- `event_id` is globally unique by construction, so it can serve as the PK for a multi-device merged table.

A migration guard `ensure_events_log_identity_schema` exists to handle databases created with the spec's `seq INTEGER PK` schema by dropping and recreating the table. Since v4 is the only version that creates this table and both schema variants land in the same commit, the guard should always be a no-op in practice. But this is also F-009 (silent data drop).

### Root cause

The spec was written before the multi-device PK collision was fully thought through; the implementation correctly identified the issue during build; the spec was not amended to match.

### Required fix

1. Amend `docs/specs/stream-g-observability-v0.1.md` §1.3 #2 to specify `event_id TEXT PRIMARY KEY` with `seq INTEGER NOT NULL` as a non-PK column. Add a "Revision goal" entry at the top of the spec documenting why.
2. Add a CREATE TABLE statement in the spec block that exactly matches what `crates/memory-substrate/src/index/migrations/` produces.
3. Update `docs/dev/stream-g-architecture.md` to reflect the correct schema.

This is purely a spec correction; no code change required.

### Acceptance criteria

- `grep -nE 'CREATE TABLE.*events_log' docs/specs/stream-g-observability-v0.1.md` matches the actual migration code byte-for-byte.
- Spec revision goal documents the change.

---

## F-006 — Codex self-promoted bench canonicals during autonomous run

- **Severity:** P2
- **Status:** process-violation
- **Streams:** G + I

### Evidence

`bench/stream-g-observability-results.darwin-arm64.json` and `bench/stream-i-cross-session-results.darwin-arm64.json` exist as canonical files in the working tree. `.proposed` variants exist alongside both. The Stream G plan `docs/plans/2026-05-01-stream-g-observability.md` Task 19 (line 1344) explicitly states: "Updating `bench/stream-g-observability-results.darwin-arm64.json` happens only through an explicit `--write-output` invocation and **human-authored commit**."

CLAUDE.md "Critical invariants" §7: "Performance baselines at `bench/baseline.<profile>.json` are updated only by explicit human-authored commits — the bench harness never overwrites them (spec §17.6, §18.9)." The same rule extends to stream-specific result files per the plan citation above.

The snapshot commit `6095cf6` was Trey-staged and acknowledged this with "canonical, not .proposed" — but Codex committed the canonical files autonomously during the overnight run before Trey staged the snapshot.

### Root cause

The promotion rule was stated in the plan but not enforced by tooling. Codex's bench harness wrote both `.proposed` and canonical files when it could, treating both as legitimate outputs.

### Required fix

1. Modify the bench harness in `crates/memory-substrate/src/bin/` (or the relevant Stream G/I bench binaries) so that without an explicit `--promote-canonical` flag, only `.proposed` is written. The canonical file is touched only when the flag is set.
2. The `--promote-canonical` flag's documentation must state that this should only run from a human shell session, never from automation.
3. Audit the existing `bench/stream-g-observability-results.darwin-arm64.json` and `bench/stream-i-cross-session-results.darwin-arm64.json` — Trey should review both files against the corresponding `.proposed` variants and either:
   - Accept the canonicals as-is via an explicit human-authored "promote stream G/I bench canonicals" commit, then delete the `.proposed` files.
   - Reject and replace from `.proposed`.
4. Add a CI check that fails if a canonical bench file changes in a commit not authored by an allowlisted human committer (or simpler: fails if the commit message lacks a `bench-promote: true` trailer that humans add explicitly).

### Acceptance criteria

- The bench harness defaults to `.proposed`-only output.
- `bench/stream-g-observability-results.darwin-arm64.json` and `bench/stream-i-cross-session-results.darwin-arm64.json` exist either as Trey-promoted canonicals (with explicit human commit) or are removed in favor of `.proposed`.
- A documented promotion flow exists in `docs/runbooks/` or in a top-level `bench/README.md`.

---

## F-007 — T17 and T18 are permanent-skip smoke detectors

- **Severity:** P2
- **Status:** honest-skip-now-but-no-tracking-decision
- **Streams:** H (test); F (T17 dep); D (T18 dep)

### Evidence

The Stream H review fix in `aa319f6` correctly removed orchestrator-layer unconditional skips for T17 and T18 (B1 in the review-trail audit). The tests now self-skip via their internal `MEMORUM_EVAL_SKIP:` mechanism — an improvement.

But:

- **T17** depends on Stream F re-entrant lease behavior for the same device. Stream F shipped a different lease model; re-entrant same-device lease is not a shipped contract. T17 will skip in every realistic environment.
- **T18** depends on `keys/decommissioned/` and `keys/active.json` both existing after a `rotate-keys` probe. The `rotate-keys` CLI does not exist as a top-level subcommand. `grep -E 'RotateKeys' crates/memoryd/src/cli.rs` shows it lives under `device rotate-keys` (`DeviceArgs`), but the underlying contract is incomplete and T18 will skip in every realistic environment.

The auditor verified both via `crates/memoryd/src/cli.rs` subcommand enumeration and the test files' skip conditions. The skip is honest, but the underlying contracts (Stream F re-entrant lease, Stream D full key rotation) remain unshipped. T17 and T18 are real test code defending real contracts that don't exist yet — they will never run.

### Root cause

T17 and T18 were specified ahead of the contracts they validate. The Stream H plan included them in the 19-test catalog. Without the underlying contracts, they're forever-skip placeholders.

### Required fix

Pick one path per test:

**Per test, choose:**

- **Ship the underlying contract.** If Stream F re-entrant same-device lease is desired (T17) or full Stream D key rotation including `keys/decommissioned/` is desired (T18), open a new spec/plan to ship it, then T17/T18 will activate.
- **Rewrite the test to exercise something that exists today.** E.g. T17 could exercise the actually-shipped lease model rather than re-entrant behavior.
- **Mark as deferred and document.** Move the test fixtures to a `tests/eval/deferred/` subdirectory with an explicit README explaining the deferred contract dependency. Update the catalog count in spec text and `docs/api/stream-h-eval-api.md` to reflect the deferred status (e.g. "19 tests authored, 17 active, 2 deferred pending Streams F/D contract additions").

The auditor's recommendation: **defer + document** (option C) for both, with spec amendments on Streams F and D opening the door to future ship. This is honest about current capability without losing the test bodies.

### Acceptance criteria

- T17 and T18 status is documented in `docs/api/stream-h-eval-api.md` and `docs/specs/stream-h-eval-harness-v0.1.md`.
- Either the underlying contracts ship (and the tests activate), or the tests are explicitly marked as deferred with a tracking spec citation.
- The eval-harness JSON report distinguishes "test self-skipped due to absent feature" from "test passed."

---

## F-008 — `audit_walk` 501 stub and synthetic TUI bench were "fixed" by retroactive spec amendment

- **Severity:** P2
- **Status:** pattern-flag, not a single bug
- **Streams:** G

### Evidence

Per the review-trail audit:

- **R4 (audit_walk):** Stream G spec §4.3 originally specified `GET /api/audit/:id/walk` as a v1 surface. The implementation shipped it as a `deferred_response("audit_walk")` stub returning 501. The fix in `cf9736f` did not implement the route — it amended the spec to retroactively defer audit_walk to v1.1+.
- **R2 (synthetic TUI bench):** The TUI panel-switch p95 measurements (e.g. 0.001ms) were taken against in-memory state transitions on a 144-byte mock frame, not against a real terminal backend. The fix in `cf9736f` did not run the bench against a real terminal — it added a `TODO(v1.1)` comment in the spec calling for terminal-emulator integration benchmarks. The synthetic measurements remain in the canonical baseline as the official regression floor.

Both are individually defensible (audit*walk's 501 is honest; synthetic bench numbers are labeled as such). But the \_pattern* — "review found a gap; fix moved the spec to match the code rather than fixing the code" — deserves a process-level call.

### Root cause

When review finds an implementation that doesn't satisfy the spec, the question "should we fix the code or amend the spec?" has two valid answers depending on cost/value. In this case Codex consistently chose "amend the spec" without surfacing the trade-off explicitly. The risk is that this becomes the default move and we drift toward "the spec describes whatever code we shipped" rather than "the code implements what the spec specified."

### Required fix

This is a process finding more than a code bug. Two concrete asks:

1. **Add a "review-fix decision policy" section** to `docs/specs/system-v0.2.md` or to a top-level `docs/CONTRIBUTING.md`-style file. Spec it: when review finds a code-vs-spec gap, the default is to fix the code; spec amendments require an explicit reason (cost prohibitive, contract was wrong, deferral with concrete tracking). Document the criteria.
2. **For the two specific cases:**
   - **audit_walk:** decide whether to actually implement the route in v1 (recommended — it's a small route over an existing data shape) or accept the deferral. If implementing: ship the route, add an integration test, remove the spec deferral text. If accepting: open a tracking issue with a target version.
   - **TUI bench:** the canonical baseline is currently a regression floor that won't catch real terminal regressions. Either (a) replace it with bench measurements taken against a real ratatui backend (e.g. the `ratatui` crate's test backend or a real terminal subprocess), or (b) explicitly remove TUI panel-switch measurements from the canonical baseline and document that TUI performance is not measured in v1.

### Acceptance criteria

- A documented decision policy for review-fix code-vs-spec gaps exists.
- Each of audit_walk and TUI bench has an explicit decision (ship vs defer) recorded in the spec or a tracking doc.

---

## F-009 — `ensure_events_log_identity_schema` silently drops data

- **Severity:** P3 (in practice; principle issue)
- **Status:** open
- **Streams:** A

### Evidence

The migration helper `ensure_events_log_identity_schema` in `crates/memory-substrate/src/index/migrations/` (per the Stream A drift audit) detects databases that were created with the spec's `seq INTEGER PRIMARY KEY` schema and drops + recreates the table with the correct `event_id TEXT PRIMARY KEY` schema. No warning is emitted. No data preservation is attempted.

In practice this is harmless because:

- The events_log mirror is a derived projection; the JSONL event log is the source of truth.
- Both schema variants land in the same commit (v4), so any database created during dev would reload from JSONL on next index rebuild.

In principle it's a silent destructive operation that a future migration scenario could surprise.

### Root cause

The migration was written to handle an edge case (somebody hand-crafted a database with the spec's exact schema, or read an early commit) without considering that silent drop + rebuild diverges from the convention that migrations preserve data or fail loudly.

### Required fix

Either:

1. Replace silent drop with `eprintln!` warning + structured log emission documenting that the table was recreated and the source-of-truth (JSONL) will be replayed on next rebuild.
2. Or: remove the guard entirely and rely on v4 being the only schema version that exists. If anyone has a v3 db, a regular migration handles it; if anyone has a hand-crafted spec-shape db, that's their problem.

The auditor's recommendation: option 1 (warn loudly). The cost is minimal and it preserves the principle that migrations are observable.

### Acceptance criteria

- `ensure_events_log_identity_schema` either logs a warning on drop+recreate, or is removed.
- A test demonstrates the migration path with the warning visible (or documents removal).

---

## F-010 — No README, no install doc, no MCP wiring example

- **Severity:** P3 (won't block code merge; will block first user)
- **Status:** doc-gap
- **Streams:** product surface

### Evidence

```
$ ls README* INSTALL* 2>/dev/null
(no matches)
$ ls docs/ | grep -iE 'install|setup|getting|quickstart|onboard'
(no matches)
$ find . -name '*.json' -path '*mcp*' -not -path './target/*' -not -path './node_modules/*'
(no matches)
```

No README, no install doc, no getting-started doc, no sample MCP configuration, no walkthrough.

CLAUDE.md is the closest thing to a project README, but it's targeted at AI assistants and assumes substantial context.

### Root cause

The project was built spec-first, plan-driven, in service of a specific person (Trey) who knows the architecture cold. User-facing onboarding was never scoped.

### Required fix

(Note: this fix should be done **after** F-001 lands, because the MCP wiring doc depends on the stdio MCP server existing.)

1. Create `README.md` at repo root with:
   - One-paragraph description of what Memorum is.
   - Architecture diagram (text-art or mermaid) of the substrate + daemon + MCP server + agents.
   - Install path: `cargo install --path crates/memoryd` (and similar for `memoryd-tui`, `memoryd-web`).
   - Quickstart:
     - `mkdir ~/memorum && cd ~/memorum`
     - `memoryd serve --init &`
     - Wire into Claude Desktop via the sample config below.
     - Open Claude, write a memory, recall it.
   - Sample MCP configuration snippet for Claude Desktop / Claude Code / Codex CLI (depends on F-001).
   - Pointer to `docs/specs/system-v0.2.md` for the deep architecture.
   - Pointer to `CLAUDE.md` for AI-assistant context.
2. Create `docs/getting-started.md` with the long-form version of the quickstart, including how to verify each step worked (`memoryd doctor`, `memoryd status`, etc.).
3. Create `docs/mcp-wiring.md` with per-harness wire-up guides (Claude Desktop, Claude Code, Codex CLI).

### Acceptance criteria

- A new contributor (or an LLM agent with no project context) can follow the README from clone to first successful `memory_write` round-trip from an MCP client.
- Sample MCP config snippets are tested via the eval harness.

---

## F-011 — `observed_at` hydrated from `frontmatter.extras` by string-key

- **Severity:** P3
- **Status:** open
- **Streams:** A

### Evidence

Per the Stream A drift audit: `observed_at` is a Stream A spec field (§6.1) but is not promoted to the typed `Frontmatter` struct. The helper `observed_at_for_index()` extracts it by string key from `frontmatter.extras`. This works, but it's inconsistent with how every other typed frontmatter field is hydrated. If the field is ever promoted to the typed struct and extras-extraction is removed, this silently breaks.

### Root cause

`observed_at` was added later than the initial `Frontmatter` struct definition; promoting it to the typed struct would have been a wider migration; the string-key lookup was the path of least resistance.

### Required fix

Promote `observed_at` to the typed `Frontmatter` struct as `Option<DateTime<Utc>>` (or whatever type the spec calls for). Update parsing/validation/serialization/defaults/schema modules accordingly. Update `observed_at_for_index()` to read from the typed field. Add a migration test asserting that pre-existing frontmatter with `observed_at` in extras is correctly hydrated into the new typed field on read.

### Acceptance criteria

- `Frontmatter` struct contains `observed_at: Option<DateTime<Utc>>` (or equivalent).
- All extras-extraction of `observed_at` is removed.
- Round-trip test: parse a frontmatter file with `observed_at`, hydrate, serialize, parse again, assert equality.

---

## F-012 — Documentation overstates Stream B as "MCP server shipped"

- **Severity:** P3
- **Status:** doc-gap
- **Streams:** docs

### Evidence

CLAUDE.md (project context, repo root) line:

> Stream B: Claude landed the daemon + MCP bridge in `f9d9c2b` (2026-04-28). Substrate-backed Status/Doctor/Search/Get/WriteNote handlers, seven-tool MCP forwarder (now eight after Stream D added `memory_reveal`)...

This is technically accurate (the **forwarder library** is shipped and has eight, then nine, tool descriptors after Stream F added `memory_observe`). But a reader naturally interprets "MCP forwarder" as "MCP server that an MCP client can connect to" — which is false (per F-001).

The system spec `docs/specs/system-v0.2.md` similarly describes MCP as live across the architecture without flagging that the stdio server itself isn't built.

### Root cause

Documentation was written assuming the MCP wire was a small remaining task, then never revisited.

### Required fix

(This fix should land **with** F-001, not before — fixing the doc without fixing the code makes things worse.)

1. CLAUDE.md: update the Stream B status to clarify "MCP forwarder library shipped; stdio MCP server added in F-001."
2. `docs/specs/system-v0.2.md`: any text implying MCP-client connectivity should be cross-referenced to the new Stream B spec (per F-001 recommendation).
3. Add an explicit "MCP integration status" section to the README that lists which MCP clients have been tested end-to-end.

### Acceptance criteria

- CLAUDE.md and `docs/specs/system-v0.2.md` describe the MCP surface accurately.
- A new contributor reading either doc understands what "MCP shipped" actually means.

---

## F-013 — Specgate emits six `orphaned_specs` warnings on a green build

- **Severity:** P3
- **Status:** open
- **Streams:** A

### Evidence

Final gate output included:

```
"orphaned_specs": [
  { "module_id": "stream-a/frontmatter", "spec_path": "modules/stream-a-frontmatter.spec.yml", "path_glob": "crates/memory-substrate/src/frontmatter/**/*" },
  { "module_id": "stream-a/git-merge", "spec_path": "modules/stream-a-git-merge.spec.yml", "path_glob": "crates/memory-substrate/src/{git,merge}/**/*" },
  { "module_id": "stream-a/index-vector", "spec_path": "modules/stream-a-index-vector.spec.yml", "path_glob": "crates/memory-substrate/src/index/**/*" },
  { "module_id": "stream-a/io-events", "spec_path": "modules/stream-a-io-events.spec.yml", "path_glob": "crates/memory-substrate/src/{markdown,events,runtime}/**/*" },
  { "module_id": "stream-a/tests-quality", "spec_path": "modules/stream-a-tests-quality.spec.yml", "path_glob": "{scripts,fixtures,crates/memory-test-support}/**/*" },
  { "module_id": "stream-a/tree-config-ids", "spec_path": "modules/stream-a-tree-config-ids.spec.yml", "path_glob": "crates/memory-substrate/src/{tree,config,ids}/**/*" }
]
```

The gate exits 0 because these are warnings, not errors. But six Stream A modules are flagged as having no matched files — which seems wrong given those globs visibly do contain files (`crates/memory-substrate/src/frontmatter/` exists, etc.).

### Root cause

Either:

1. `specgate`'s "orphaned_specs" terminology means something other than "no files matched" (possibly: "the spec module has no covering spec sentence cross-references" or similar). Verify by reading `specgate` source.
2. The spec.yml files themselves have a structural issue (wrong field name, missing required field) causing specgate to silently skip them but still report them as orphaned.
3. The path globs are correct but specgate is matching against a different working directory.

### Required fix

1. Check what `orphaned_specs` actually means in the specgate version installed (or its source if vendored). The output structure suggests "specs with no matching code files," but the code files clearly exist.
2. Either fix the specgate config to remove the warnings, or document why the warnings are expected and harmless.
3. If the warning is real (spec.yml files have a structural issue), fix the spec.yml files.

### Acceptance criteria

- `bash scripts/check.sh` produces zero `orphaned_specs` warnings, OR the warnings are explicitly documented as expected with a citation.

---

## Process / structural concerns (no findings, surfaced for awareness)

These are not findings to fix; they are structural observations the auditor surfaces for Trey's strategic awareness.

### PC-1 — Per-stream self-review fan-out has known blind spots

Codex's review pattern (spawn subagents for clean-code, security, performance, test, contract reviews per stream) is good at catching within-stream issues. F-001 (no MCP server), F-002 (recall-block format mismatch), and F-004 (TUI/web don't consume RecallHit) are all examples of issues a per-stream fan-out cannot catch by construction. Future streams should explicitly include cross-stream contract review as a separate review gate, ideally executed by an agent that has not implemented either side of the seam.

### PC-2 — "Spec-amend to match code" is becoming a default move

F-005 (events_log schema), F-008 (audit_walk + TUI bench), and partially F-003 (substrate APIs ratified after the fact) all involved spec amendments where the code didn't change. Sometimes that's right. But it should be a deliberate decision per case, not a default. F-008's recommended fix includes a "decision policy" doc.

### PC-3 — Documentation truth-in-advertising is drifting

F-010, F-012 are examples. The project shipped a lot of capability fast, and documentation lagged. Before the next major surface lands, a documentation pass that aligns CLAUDE.md, README, system spec, and per-stream specs against actual shipped behavior would prevent compounding drift.

### PC-4 — Stream H "shipped" carries a permanent live-LLM caveat

The Stream H eval harness ships with assertion counts and JSON reporting that work — but the most important tests (T13, T15) require authenticated Claude/Codex CLI presence. In any environment without those credentials, T13/T15 skip. The eval harness can prove the substrate behaves correctly under simulated conditions; it cannot prove that real Claude+real-MCP+real-substrate works end-to-end in CI. F-001 unblocks part of this (without an MCP server, even local manual testing was impossible). The remaining "live LLM in CI" gap is unsolvable without a credentials-bearing CI environment, which is a Trey-strategic call (cost vs assurance).

---

## Appendix A — Gate run summary

```
Command: BENCH_PROFILE=darwin-arm64 bash scripts/check.sh
Exit code: 0
Test suites passing (debug + release): 332 (per `grep -c '^test result: ok' /tmp/check-gate.log`)
Test failures: 0
Clippy warnings (with -D warnings): 0
Rustfmt diffs: 0
Rustdoc warnings (with -D warnings): 0
Oxlint findings: 0
Specgate validate: ok
Specgate check: ok (with 6 orphaned_specs warnings, see F-013)
Specgate doctor ownership: ok
Rust boundary check: ok
Two-clone convergence: ok (full mode)
Durability matrix (apfs,tmpfs,ext4,einval,best-effort): ok
Bench gate smoke: wrote bench/results.darwin-arm64.smoke.json
Bench gate release: wrote bench/results.darwin-arm64.json
Bench regression check: ok
```

The build is canonically green by the project's own gate. All findings in this audit exist in spite of a green gate, which is itself an observation: **a green gate is necessary but insufficient**. The findings above are the work the gate does not catch.

---

## Appendix B — Source review docs cross-referenced

Findings drew on these existing review docs (consulted but not duplicated here):

- `docs/reviews/stream-g-spec-review.md`, `docs/reviews/stream-g-plan-review.md`
- `docs/reviews/stream-g-review-gate-b-clean-code-rerun-2.md` (origin of F-003)
- `docs/reviews/stream-h-spec-review.md`, `docs/reviews/stream-h-plan-review.md`
- `docs/reviews/stream-i-spec-review.md`, `docs/reviews/stream-i-plan-review.md`
- `docs/reviews/stream-i-review-gate-d-test-rerun-3.md`, `docs/reviews/stream-i-review-gate-d-security-rerun-3.md`
- `docs/reviews/stream-ghi-combined-plan-review.md`, `docs/reviews/stream-ghi-combined-plan-review-pass-2.md`
- `docs/reviews/system-v0.2-spec-review.md`

---

## Appendix C — Suggested fix order

The auditor recommends Codex tackle findings in roughly this order to minimize churn and keep the workspace in a known-good state:

1. **F-001** (stdio MCP server) — unblocks demonstrability and is independent of all other findings.
2. **F-002** (recall-block format) — depends on a format decision; touch one file in render.rs and one in assertions.rs; small.
3. **F-006** (bench promotion safety) — small code change, prevents future violations.
4. **F-003** (unauthorized substrate APIs) — pick path 1 (ratify) or path 2 (relocate); spec/doc updates.
5. **F-004** (TUI/web RecallHit consumption) — depends on UX decision; medium.
6. **F-005** (events_log schema spec) — pure spec amendment.
7. **F-009** (silent migration drop) — small.
8. **F-007** (T17/T18 status) — decision + doc updates.
9. **F-008** (review-fix decision policy + audit_walk + TUI bench) — process doc + per-case decisions.
10. **F-011** (observed_at typed promotion) — refactor with migration test.
11. **F-013** (specgate orphaned_specs) — investigation, then fix or document.
12. **F-010 + F-012** (README + doc accuracy) — land last, when the surface has stabilized post-F-001.

After the fix-list is complete, Trey will request a second-pass audit. Before that audit, Codex should run the full release gate (`bash scripts/check.sh` with `BENCH_PROFILE=darwin-arm64`) and confirm exit code 0. If any finding's fix introduces a gate failure, that finding should be flagged in the second-pass audit request rather than worked around.

---

_End of audit._
