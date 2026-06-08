   1 # Feature — `memoryd export` v0.1
   2 
   3 **Status:** draft (revision 3; revision history at end).
   4 **Scope:** New `memoryd export` CLI subcommand that emits a portable JSON dump of substrate contents. Semantically read-only against substrate *content*; does not mutate memories, locks, events, or index rows. Note that opening the substrate at all triggers the standard runtime-init side effects (§7).
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
  16 - **Streaming/NDJSON output for huge substrates.** v0.1 emits one JSON document.
  17 - **Decryption of encrypted bodies.** v0.1 deliberately refuses to decrypt; see §6.
  18 - **Export of derived index state** (FTS, vector index).
  19 - **Concurrent `memoryd serve` + `memoryd export` against the same `--repo`/`--runtime`.** v0.1's behavior under that scenario is undefined; the export may fail to open the substrate or may observe in-flight state. Operators must stop the serve daemon before exporting. A future v0.2 may add a "read-only attach" mode that interoperates with a live daemon.
  20 - **Stable cross-substrate `project_id` field** (Memorum has no persisted project-identity concept yet).
  21 - **Preservation of YAML-specific frontmatter constructs** — anchors, aliases, tags, literal block style. Memorum's existing frontmatter does not use these; the export emits whatever serde naturally maps through the YAML → `serde_json::Value` → JSON path (§4).
  22 
  23 ## 3. CLI surface
  24 
  25 ```
  26 memoryd export --repo <repo> --runtime <runtime>
  27                [--out <path>]
  28                [--format json]
  29                [--since <ISO8601>]
  30 ```
  31 
  32 | Flag | Default | Notes |
  33 | --- | --- | --- |
  34 | `--repo <path>` | required | Substrate repo path. Same semantics as `memoryd serve`. |
  35 | `--runtime <path>` | required | Per-device runtime path. Same semantics as `memoryd serve`. |
  36 | `--out <path>` | stdout | When set, the export is atomically written to `<path>` (write to `<path>.<pid>.<nanos>.tmp` → `fsync` the temp file → `rename` over the target). On any failure before rename, the temp file is removed. |
  37 | `--format json` | `json` | Only `json` is accepted in v0.1. Any other value returns exit code 2 with a stderr message; no silent fallback. |
  38 | `--since <ISO8601>` | none | If set, only memories whose `updated_at` is >= the parsed value are included. Parse failures are hard errors (exit 2). |
  39 
  40 Exit codes:
  41 
  42 - `0` on success.
  43 - `2` on argument or filter parse errors.
  44 - `1` on substrate open errors or I/O errors.
  45 
  46 Standard streams:
  47 
  48 - Stdout: the JSON document (when `--out` is not set). Stdout MUST NOT carry diagnostics under any flag combination.
  49 - Stderr: a single-line success summary (`memory_count=<n> bytes=<n>`) on success, and any progress diagnostics. Diagnostics MUST NOT appear in stdout.
  50 
  51 ## 4. Output schema
  52 
  53 A single JSON object. UTF-8, two-space indent, trailing newline. The producer emits keys in the order shown below; consumers MUST NOT rely on key ordering.
  54 
  55 ```json
  56 {
  57   "schema_version": 1,
  58   "exported_at": "2026-05-17T12:34:56.789Z",
  59   "source_device_id": "dev_ab12cd34",
  60   "filters": {
  61     "since": null
  62   },
  63   "memory_count": 42,
  64   "memories": [
  65     {
  66       "id": "mem_...",
  67       "scope": "project",
  68       "status": "active",
  69       "frontmatter": { "...": "..." },
  70       "body": "...",
  71       "body_marker": null,
  72       "created_at": "2026-05-01T10:00:00Z",
  73       "updated_at": "2026-05-10T14:22:00Z"
  74     }
  75   ]
  76 }
  77 ```
  78 
  79 Normative constraints:
  80 
  81 - `schema_version` is exactly `1` for v0.1 outputs.
  82 - `exported_at` is RFC3339, UTC (`Z` suffix), millisecond precision.
  83 - `source_device_id` is read from the runtime device state; same source as the rest of Memorum.
  84 - `filters.since` is the verbatim ISO string the operator passed, or `null` when no `--since` was supplied.
  85 - `memory_count` MUST equal `memories.length` after filtering.
  86 - `id` is the canonical memory id (e.g. `mem_<hash>`) as Stream A stores it.
  87 - `scope` is the lowercase serialization of `memory_substrate::model::Scope` (serde `snake_case`). Permitted values: `"user"`, `"project"`, `"org"`, `"agent"`, `"subagent"`. The export MUST use serde's canonical serialization; it MUST NOT hand-stringify.
  88 - `status` is the lowercase serialization of `memory_substrate::model::MemoryStatus` (serde `kebab-case`, which for v0.1 variants is identical to lowercase). Permitted values: `"candidate"`, `"active"`, `"pinned"`, `"superseded"`, `"archived"`, `"tombstoned"`. Same serde-canonical rule.
  89 - `frontmatter` is the substrate's `Frontmatter` value emitted via `serde_json` — i.e. whatever the path `YAML → serde_json::Value → JSON` produces. YAML-specific constructs (anchors, aliases, tags, literal block style) are NOT preserved (§2 non-goal). Existing Memorum frontmatter does not exercise them.
  90 - `body` and `body_marker` together describe the body: see §6.
  91 - `created_at` and `updated_at` are RFC3339 UTC, taken from substrate state. For rows where the underlying timestamps are absent or zero (legacy rows, `MetadataOnly` placeholders), the export emits `"1970-01-01T00:00:00Z"` rather than `null` so the schema stays uniform.
  92 
  93 Memories are emitted in ascending `(updated_at, id)` order. Stable across re-runs against the same substrate; makes diffs across snapshots small.
  94 
  95 ## 5. Filters
  96 
  97 `--since <ISO8601>`:
  98 
  99 - Parsed strictly. Accepts the canonical RFC3339 form (`2026-05-01T00:00:00Z` or `2026-05-01T00:00:00+00:00`). Bare dates (`2026-05-01`) MUST be rejected with exit code 2 and a stderr message naming the canonical form.
 100 - Compared against `updated_at` (>=, inclusive at the boundary).
 101 - The export does not filter on creation time, status, scope, or sensitivity in v0.1.
 102 
 103 ## 6. Encrypted-memory handling
 104 
 105 The export routes each memory by its `MemoryContent` variant (the three-way shape used elsewhere — see the inspector-body fix in commit `c9d16fc`):
 106 
 107 | Substrate variant | Export `body` | Export `body_marker` |
 108 | --- | --- | --- |
 109 | `Plaintext(text)` | `text` verbatim | `null` |
 110 | `Ciphertext { .. }` | `null` | `"encrypted"` |
 111 | `MetadataOnly` | `null` | `"metadata-only"` |
 112 
 113 The export MUST NOT call any Stream D reveal-policy code path in v0.1. There is no flag in this version that opts into decryption. Operators who need cleartext continue to use the existing `memory_reveal` MCP tool / reveal flow.
 114 
 115 Rationale: decryption is a governed audit event (Stream D / `EncryptedContentRevealed`); routing it through a bulk export would either silently bypass the audit log (wrong) or emit hundreds of audit events per export call (also wrong).
 116 
 117 ## 7. Implementation boundaries
 118 
 119 This feature MUST:
 120 
 121 - Be invokable as `memoryd export …` (either a new subcommand on the existing `memoryd` binary or a sibling `bin/`, implementer's choice). The acceptance tests in §8 invoke the binary as a subprocess.
 122 - Open the substrate via `memory_substrate::Substrate::open(Roots { repo, runtime })` and install runtime privacy enforcement via `memoryd::runtime_privacy::install_privacy_runtime_from_roots(&repo, &runtime)` BEFORE the open call. This mirrors the privacy-then-open sequencing the `serve` command performs in `crates/memoryd/src/main.rs` (around the `Command::Serve` arm, post commit `db97fb2`); the export does NOT reuse `serve_runtime::open_substrate_for_serve` because that function takes `&ServeArgs` with serve-specific fields (`socket`, `init`, `force_unsafe_durability`) the export does not have.
 123 - Use serde-canonical serialization for `Scope`, `MemoryStatus`, and any other shared enum types — not hand-written strings.
 124 - Inline the write-to-temp + fsync + rename pattern when `--out` is set. Use the same shape as `crates/memory-merge-driver/src/main.rs::persist_merged_output` (commit `db97fb2`). The workspace does not yet expose a public atomic-write helper; this inlines the precedent.
 125 - Acknowledge the substrate open side effects in the user-facing docs (README / help text), since runtime-dir creation, index repair replay, and event-log mirror rebuild are NOT no-ops even though the export does not write substrate content.
 126 
 127 This feature MAY:
 128 
 129 - Add a single thin additive method on `Substrate` to iterate memory envelopes, if and only if the existing public APIs cannot satisfy §4 without unacceptable awkwardness. Currently visible public APIs on `Substrate`: `query_memory(MemoryQuery) -> Vec<QueryResult>` (returns only id/path/summary), `read_path_envelope(&RepoPath) -> MemoryEnvelope`. If the implementer chooses to add a method, the suggested signature is `pub fn iter_memory_envelopes(&self) -> impl Iterator<Item = Result<MemoryEnvelope, SubstrateError>>` and the PR description must justify the addition. Constraint: the new method is read-only and additive — no existing API shape or behavior changes.
 130 
 131 This feature MUST NOT:
 132 
 133 - Mutate any substrate memory content, claim lock, event-log entry, or index row beyond what `Substrate::open`'s standard runtime initialization already does.
 134 - Touch any encrypted-content body without going through the Stream D reveal flow (and v0.1 does not call that flow at all).
 135 - Bump any spec or invariant other than this one.
 136 - Add multiple new public APIs on `memory-substrate`. If more than one new method seems necessary, escalate to a spec revision.
 137 
 138 ## 8. Acceptance items
 139 
 140 Four items. Wright will queue them with the dependency ordering shown — `01` is the gating item that creates the subcommand file; `02`, `03`, `04` each `depends_on: ["export-json-shape-01"]` and extend the same file. Wright MUST NOT claim 02/03/04 until 01 is `implemented`.
 141 
 142 All tests use the existing project convention of invoking the binary via `std::process::Command` (see `crates/memoryd/tests/review_queue.rs` for the pattern), against an in-process substrate built using the `privacy_e2e.rs` fixture pattern. They run single-device; multi-device coverage is out of scope.
 143 
 144 ### `export-json-shape-01` — Default JSON output validates against the v0.1 schema
 145 
 146 **Depends on:** none (gating item).
 147 
 148 **Closure:** A new integration test under `crates/memoryd/tests/export_json_shape.rs` builds a 3-memory fixture (one plaintext, one encrypted, one metadata-only) using the `privacy_e2e.rs` pattern. The test spawns `memoryd export --repo … --runtime …` via `std::process::Command`, captures stdout, parses it as JSON, and asserts:
 149 
 150 - `schema_version == 1`
 151 - `exported_at` parses as RFC3339 UTC, millisecond-precision
 152 - `source_device_id` matches the fixture's device id, non-empty
 153 - `filters.since` is JSON `null` (no `--since` was passed)
 154 - `memory_count == 3` and `memories.length == 3`
 155 - Each memory has `id`, `scope`, `status`, `frontmatter`, `body`, `body_marker`, `created_at`, `updated_at`
 156 - Each memory's `scope` is one of the permitted serde-canonical strings from §4
 157 - Each memory's `status` is one of the permitted serde-canonical strings from §4
 158 - Memories are sorted by `(updated_at, id)` ascending
 159 - The subprocess's captured stderr contains exactly one success-summary line matching the regex `^memory_count=\d+ bytes=\d+$` and no other lines
 160 
 161 **Minimum scope hint:** new export subcommand wired into `crates/memoryd/`; new test file; an additive `Substrate` iterator method only if §7 MAY justifies it.
 162 
 163 ### `export-since-filter-02` — `--since <ISO>` filters by `updated_at >= since`
 164 
 165 **Depends on:** `export-json-shape-01`.
 166 
 167 **Closure:** Integration test under `crates/memoryd/tests/export_since_filter.rs` builds a 4-memory fixture with `updated_at` values at the exact instants `T0`, `T0+1d`, `T0+2d`, `T0+3d`. Runs `memoryd export --since <T0+2d-ISO>` as a subprocess. Asserts:
 168 
 169 - `memory_count == 2`
 170 - The included ids are exactly the two memories at `T0+2d` and `T0+3d` (the boundary memory at `T0+2d` is included — verifies `>=` is inclusive, not strict `>`)
 171 - A second sub-case runs with `--since 2026-05-01` (bare date) and asserts exit code 2 plus a stderr message containing the canonical RFC3339 form
 172 
 173 **Minimum scope hint:** filter logic in the same subcommand file from `01`; new test file.
 174 
 175 ### `export-encrypted-default-03` — Encrypted bodies are never emitted, no reveal-flow side effects
 176 
 177 **Depends on:** `export-json-shape-01`.
 178 
 179 **Closure:** Integration test under `crates/memoryd/tests/export_encrypted_default.rs` builds a single-device fixture with two memories: one plaintext, one ciphertext (use the encrypted-memory fixture pattern from `privacy_e2e.rs`). Runs `memoryd export` as a subprocess. Asserts:
 180 
 181 - The plaintext memory's exported `body` equals the original plaintext, `body_marker` is `null`
 182 - The ciphertext memory's exported `body` is `null`, `body_marker` is `"encrypted"`
 183 - No `EncryptedContentRevealed` event appears in the current device's event log after the export (read via `Substrate::events()` from the same test substrate handle)
 184 - The original ciphertext bytes do NOT appear anywhere in the captured stdout bytes (defense-in-depth check)
 185 
 186 **Minimum scope hint:** body-variant routing in the same subcommand file from `01`; new test file. No Stream D code paths invoked from production code.
 187 
 188 ### `export-out-atomic-write-04` — `--out <path>` writes atomically and matches stdout output byte-for-byte
 189 
 190 **Depends on:** `export-json-shape-01`.
 191 
 192 **Closure:** Integration test under `crates/memoryd/tests/export_out_atomic.rs` reuses a 3-memory fixture. Runs `memoryd export --out <tempfile>` as a subprocess and also runs `memoryd export` (no `--out`) capturing stdout. Asserts:
 193 
 194 - The two outputs are byte-for-byte identical (same key ordering, same indentation, same trailing newline)
 195 - After the `--out` run, the temp directory containing the output file has exactly two entries: the target file and a directory entry (no leftover `.tmp` files)
 196 - The output file ends with a trailing `\n`
 197 - A negative sub-case: when `--out` points at a path whose parent directory does not exist, the command exits with code 1 and stderr names the missing parent path; no partial file is left behind in the missing parent's grandparent
 198 
 199 **Minimum scope hint:** atomic-write helper inlined in the same subcommand file from `01`; new test file.
 200 
 201 ## 9. Open questions (flagged for implementer; do not pre-decide)
 202 
 203 - **Substrate iteration choice.** §7 MAY allows a single additive `iter_memory_envelopes` method on `Substrate`. The implementer decides based on `query_memory`'s ergonomics. Either choice satisfies §8 as long as encrypted and metadata-only memories are reachable.
 204 - **Filter inclusivity at sub-millisecond precision.** If `updated_at` is stored with sub-millisecond precision (it appears to be RFC3339 millis), `--since` comparisons should compare as `DateTime<Utc>` values, not as strings. The boundary test in item 02 verifies inclusivity but does not exercise sub-ms drift.
 205 - **Frontmatter shape for JSON-incompatible YAML types.** If any Memorum substrate frontmatter contains YAML timestamp scalars (`!!timestamp` style) or other non-JSON-scalar types, serde's natural mapping will stringify them. The implementer should grep substrate fixtures and flag if real data hits this; otherwise the v0.1 lossy-conversion stance is fine.
 206 
 207 ## Revision history
 208 
 209 - **Rev 1** (initial draft): first pass, posted for adversarial review.
 210 - **Rev 2** (post plan-reviewer): dropped `project_id` (no real source); resolved §7 SHOULD/MUST-NOT contradiction by allowing one additive iterator method; specified canonical serializations for `scope` and `status`; clarified single-device test fixture + event-log assertion; pointed at `privacy_e2e.rs` for ciphertext convention; removed false atomic-write-helper reference; extended §9 for MetadataOnly timestamps.
 211 - **Rev 3** (current; post codex pass): added explicit `depends_on` ordering — items 02/03/04 depend on 01; reframed "read-only" honestly to acknowledge `Substrate::open`'s standard runtime side effects (§7 + new §2 non-goal for concurrent serve+export); replaced `assert_cmd` references with `std::process::Command` to match the actual convention; reframed §7 to require the privacy-then-open sequence explicitly and NOT to reuse `open_substrate_for_serve` (whose `&ServeArgs` shape doesn't fit export); spec'd `frontmatter` JSON conversion path honestly (no YAML anchor / tag / literal-block preservation, called out as a non-goal); added new acceptance item `export-out-atomic-write-04` to cover `--out` atomic-write which previous revisions silently dropped; tightened item 02 fixture to include an exact-boundary memory at `T0+2d` matching `--since`, verifying `>=` inclusivity; added explicit `filters.since: null` and stderr-format assertions to item 01.
