   1 # Feature — `memoryd export` v0.1
   2 
   3 **Status:** draft (revision 2; revision history at end).
   4 **Scope:** New `memoryd export` CLI subcommand that emits a portable JSON dump of substrate contents. Read-only against the substrate.
   5 
   6 This is the first feature spec authored under the wright workflow. The acceptance items in §8 are the closure conditions an implementer agent must satisfy.
   7 
   8 ## 1. Goal
   9 
  10 Provide a single command that dumps a portable, self-describing JSON snapshot of a Memorum substrate. Useful for: backup-and-restore, cross-instance migration, debugging recall drift by sharing snapshots with collaborators, and feeding offline analysis.
  11 
  12 ## 2. Non-goals (deferred to a later version)
  13 
  14 - **Import.** The reverse direction is a separate feature (`feature-memoryd-import-v0.1`), not in scope here.
  15 - **YAML output.** `--format yaml` is reserved as a future flag value. v0.1 only emits JSON.
  16 - **Streaming/NDJSON output for huge substrates.** v0.1 emits one JSON document. A future v0.2 may add `--format ndjson` if substrate size makes that necessary.
  17 - **Decryption of encrypted bodies.** v0.1 deliberately refuses to decrypt; see §6. A future v0.2 may add `--include-encrypted-body` that goes through the Stream D reveal-policy flow.
  18 - **Export of derived index state** (FTS, vector index). The export carries only canonical substrate content; consumers re-index on read.
  19 - **Stable cross-substrate `project_id` field.** Memorum does not yet have a persisted project-identity concept. The output schema does not include a project id in v0.1; consumers identify the source by `source_device_id` and the surrounding context (filename, transport, etc.). A future v0.2 may add `project_id` once Memorum's coordination layer surfaces one.
  20 
  21 ## 3. CLI surface
  22 
  23 ```
  24 memoryd export --repo <repo> --runtime <runtime>
  25                [--out <path>]
  26                [--format json]
  27                [--since <ISO8601>]
  28 ```
  29 
  30 | Flag | Default | Notes |
  31 | --- | --- | --- |
  32 | `--repo <path>` | required | Substrate repo path. Same semantics as `memoryd serve`. |
  33 | `--runtime <path>` | required | Per-device runtime path. Same semantics as `memoryd serve`. |
  34 | `--out <path>` | stdout | When set, the export is atomically written to `<path>` (write to `<path>.<pid>.<nanos>.tmp` → `fsync` → `rename`). |
  35 | `--format json` | `json` | Only `json` is accepted in v0.1. Any other value returns exit code 2 with a stderr message; no silent fallback. |
  36 | `--since <ISO8601>` | none | If set, only memories whose `updated_at` is >= the parsed value are included. Parse failures are hard errors (exit 2). |
  37 
  38 Exit codes:
  39 
  40 - `0` on success.
  41 - `2` on argument or filter parse errors.
  42 - `1` on substrate open errors or I/O errors.
  43 
  44 Standard streams:
  45 
  46 - Stdout: the JSON document (when `--out` is not set). Stdout MUST NOT carry diagnostics.
  47 - Stderr: a single-line success summary (memory count, byte count) and any progress diagnostics. Diagnostics MUST NOT appear in stdout under any flag combination.
  48 
  49 ## 4. Output schema
  50 
  51 A single JSON object. UTF-8, two-space indent, trailing newline. The producer emits keys in the order shown below; consumers MUST NOT rely on key ordering.
  52 
  53 ```json
  54 {
  55   "schema_version": 1,
  56   "exported_at": "2026-05-17T12:34:56.789Z",
  57   "source_device_id": "dev_ab12cd34",
  58   "filters": {
  59     "since": null
  60   },
  61   "memory_count": 42,
  62   "memories": [
  63     {
  64       "id": "mem_...",
  65       "scope": "project",
  66       "status": "active",
  67       "frontmatter": { "...": "..." },
  68       "body": "...",
  69       "body_marker": null,
  70       "created_at": "2026-05-01T10:00:00Z",
  71       "updated_at": "2026-05-10T14:22:00Z"
  72     }
  73   ]
  74 }
  75 ```
  76 
  77 Normative constraints:
  78 
  79 - `schema_version` is exactly `1` for v0.1 outputs.
  80 - `exported_at` is RFC3339, UTC (`Z` suffix), millisecond precision.
  81 - `source_device_id` is read from the runtime device state; same source as the rest of Memorum.
  82 - `filters.since` is the verbatim ISO string the operator passed, or `null`.
  83 - `memory_count` MUST equal `memories.length` after filtering.
  84 - `id` is the canonical memory id (e.g. `mem_<hash>`) as Stream A stores it.
  85 - `scope` is the lowercase serialization of `memory_substrate::model::Scope` (serde `snake_case`). Permitted values: `"user"`, `"project"`, `"org"`, `"agent"`, `"subagent"`. The export MUST use serde's canonical serialization of the enum; it MUST NOT hand-stringify.
  86 - `status` is the lowercase serialization of `memory_substrate::model::MemoryStatus` (serde `kebab-case`, which for v0.1 variants is identical to lowercase). Permitted values: `"candidate"`, `"active"`, `"pinned"`, `"superseded"`, `"archived"`, `"tombstoned"`. Same serde-canonical rule.
  87 - `frontmatter` is the canonical frontmatter from Stream A (sorted keys, YAML-1.1 compatible at the source), re-emitted here as a JSON object — keys and scalar values preserved verbatim, types as serde naturally maps them (strings as strings, numbers as numbers, etc.). YAML-specific quoting rules do not apply inside the JSON wrapper.
  88 - `body` and `body_marker` together describe the body: see §6.
  89 - `created_at` and `updated_at` are RFC3339 UTC, taken from substrate state, not synthesized.
  90 
  91 Memories are emitted in ascending `(updated_at, id)` order. This ordering is stable across re-runs against the same substrate and makes diffs across snapshots small. See §9 for the missing-`updated_at` and `MetadataOnly` edge cases.
  92 
  93 ## 5. Filters
  94 
  95 `--since <ISO8601>`:
  96 
  97 - Parsed strictly. Accepts the canonical RFC3339 form (`2026-05-01T00:00:00Z` or `2026-05-01T00:00:00+00:00`). Bare dates (`2026-05-01`) MUST be rejected with exit code 2 and a stderr message suggesting the canonical form.
  98 - Compared against `updated_at` (>=). Memories at the boundary are included.
  99 - The export does not filter on creation time, status, scope, or sensitivity in v0.1. Those are reserved for future flags.
 100 
 101 ## 6. Encrypted-memory handling
 102 
 103 The export routes each memory by its `MemoryContent` variant (the three-way shape used elsewhere in the codebase — see the inspector-body fix in commit `c9d16fc`):
 104 
 105 | Substrate variant | Export `body` | Export `body_marker` |
 106 | --- | --- | --- |
 107 | `Plaintext(text)` | `text` verbatim | `null` |
 108 | `Ciphertext { .. }` | `null` | `"encrypted"` |
 109 | `MetadataOnly` | `null` | `"metadata-only"` |
 110 
 111 The export MUST NOT call any Stream D reveal-policy code path in v0.1. There is no flag in this version that opts into decryption. Operators who need the cleartext of an encrypted memory continue to use the existing `memory_reveal` MCP tool / reveal flow.
 112 
 113 Rationale: decryption is a governed audit event (Stream D / `EncryptedContentRevealed`); routing it through a bulk export would either (a) silently bypass the audit log (wrong) or (b) emit hundreds of audit events for one export call (also wrong). v0.2 may add an opt-in path that records a single bulk-reveal audit event covering the whole export; that's a real design and outside v0.1.
 114 
 115 ## 7. Implementation boundaries
 116 
 117 This feature MUST:
 118 
 119 - Be invokable as a subprocess via `memoryd export …` (either as a new subcommand on the existing `memoryd` binary or as a sibling `bin/`, implementer's choice). The acceptance tests in §8 invoke the binary, so this is not optional.
 120 - Open the substrate via the existing `memoryd::serve_runtime::open_substrate_for_serve` (or its non-init sibling) so that runtime privacy / device-id plumbing matches the rest of Memorum.
 121 - Use serde-canonical serialization for `Scope`, `MemoryStatus`, and any other shared enum types — not hand-written strings. (See §4.)
 122 - Inline the write-to-temp + fsync + rename pattern when `--out` is set. Use the same shape as `crates/memory-merge-driver/src/main.rs::persist_merged_output` (commit `db97fb2`). There is no workspace-level public atomic-write helper today; the merge driver inlines the pattern and the export should follow that precedent.
 123 
 124 This feature MAY:
 125 
 126 - Add a single thin additive method on `Substrate` to iterate memory envelopes, if and only if the existing `query_memory` + per-id `read_path_envelope` pair cannot satisfy §4's data requirements without unacceptable awkwardness. Suggested signature: `pub fn iter_memory_envelopes(&self) -> impl Iterator<Item = Result<MemoryEnvelope, SubstrateError>>`. The implementer's PR description must justify the addition if exercised. Constraint: the new method is read-only and additive — it MUST NOT change existing API shapes, return types, or behavior.
 127 
 128 This feature MUST NOT:
 129 
 130 - Mutate any substrate file, lock, or index.
 131 - Touch any encrypted-content body without going through the Stream D reveal flow (and v0.1 does not call that flow at all).
 132 - Bump any spec or invariant other than this one.
 133 - Add multiple new public APIs on `memory-substrate`. If more than one new method seems necessary, escalate to a spec revision.
 134 
 135 ## 8. Acceptance items
 136 
 137 These are the three items wright will queue. Each names a specific closure condition (the test that must pass) and the minimum file scope an implementer is expected to touch. All three tests run against a single-device test substrate built in-process; multi-device coverage is out of scope.
 138 
 139 ### `export-json-shape-01` — Default JSON output validates against the v0.1 schema
 140 
 141 **Closure:** A new integration test under `crates/memoryd/tests/export_json_shape.rs` builds a 3-memory fixture (one plaintext, one encrypted, one metadata-only) using the same in-process substrate pattern as `crates/memoryd/tests/privacy_e2e.rs`. The test spawns the `memoryd export --repo … --runtime …` binary as a subprocess via `assert_cmd` (the convention already used in other integration tests), captures stdout, parses it as JSON, and asserts:
 142 
 143 - `schema_version == 1`
 144 - `exported_at` parses as RFC3339 UTC
 145 - `source_device_id` matches the fixture's device id
 146 - `memory_count == 3` and `memories.length == 3`
 147 - Each memory has `id`, `scope`, `status`, `frontmatter`, `body`, `body_marker`, `created_at`, `updated_at`
 148 - Each memory's `scope` is one of the permitted serde-canonical strings from §4
 149 - Each memory's `status` is one of the permitted serde-canonical strings from §4
 150 - Memories are sorted by `(updated_at, id)` ascending
 151 
 152 **Minimum scope hint:** new export subcommand wired into `crates/memoryd/`; new test file; an additive `Substrate` iterator method only if §7 MAY justifies it.
 153 
 154 ### `export-since-filter-02` — `--since <ISO>` filters by `updated_at >= since`
 155 
 156 **Closure:** Integration test under `crates/memoryd/tests/export_since_filter.rs` builds a 4-memory fixture with `updated_at` values at `T-10d`, `T-5d`, `T-1d`, `T-now`. Runs `memoryd export --since <T-3d-ISO>` as a subprocess. Asserts `memory_count == 2` and the included ids are exactly the two newest. A second sub-case asserts that a bare-date `--since 2026-05-01` returns exit code 2 with a stderr message naming the canonical RFC3339 form.
 157 
 158 **Minimum scope hint:** filter logic in the same subcommand file; new test file.
 159 
 160 ### `export-encrypted-default-03` — Encrypted bodies are never emitted, no reveal-flow side effects
 161 
 162 **Closure:** Integration test under `crates/memoryd/tests/export_encrypted_default.rs` builds a single-device fixture with two memories: one plaintext, one ciphertext. Use the encrypted-memory fixture pattern from `crates/memoryd/tests/privacy_e2e.rs`. Runs `memoryd export` as a subprocess. Asserts:
 163 
 164 - The plaintext memory's exported `body` equals the original plaintext, `body_marker` is `null`.
 165 - The ciphertext memory's exported `body` is `null`, `body_marker` is `"encrypted"`.
 166 - No `EncryptedContentRevealed` event appears in the current device's event log after the export (read via `Substrate::events()` from the same test substrate handle, which scopes to the device log for the single device under test).
 167 - The ciphertext bytes do NOT appear anywhere in the export output (defense-in-depth check: scan stdout bytes for any segment of the original ciphertext).
 168 
 169 **Minimum scope hint:** body-variant routing in the same subcommand file; new test file. No Stream D code paths invoked from production code.
 170 
 171 ## 9. Open questions (flagged for implementer; do not pre-decide)
 172 
 173 - **Substrate iteration choice.** §7 MAY allows a single additive `iter_memory_envelopes` method. The implementer decides based on `query_memory`'s ergonomics for full-substrate iteration. Either choice satisfies §8.
 174 - **Missing `updated_at` on legacy or `MetadataOnly` rows.** Treat missing/zero `updated_at` as the Unix epoch for sort purposes. Document the choice in the §8 test as a comment. If a `MetadataOnly` placeholder has zero-valued `created_at`/`updated_at`, the export emits them as `"1970-01-01T00:00:00Z"` rather than `null` so the schema stays uniform.
 175 - **Subcommand vs sibling binary.** Either is acceptable. Subcommand is likely cleaner (shared CLI parser, shared `serve_runtime`). The acceptance tests don't constrain the choice as long as the binary name `memoryd export` works.
 176 
 177 ## Revision history
 178 
 179 - **Rev 1** (initial draft): first pass, posted for adversarial review.
 180 - **Rev 2** (current): incorporates plan-reviewer findings — dropped `project_id` (no real source in Memorum yet); resolved §7 SHOULD/MUST-NOT contradiction by allowing one additive `Substrate` iterator method; specified canonical serializations for `scope` and `status`; clarified that acceptance tests use a single-device fixture and that the audit-event check reads the current device's log via `Substrate::events()`; pointed at `privacy_e2e.rs` as the convention reference for the ciphertext fixture; removed false reference to a workspace atomic-write helper and named the `persist_merged_output` precedent in commit `db97fb2`; extended §9 to cover `MetadataOnly` placeholder timestamps.
