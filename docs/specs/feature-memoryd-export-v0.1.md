   1 # Feature — `memoryd export` v0.1
   2 
   3 **Status:** draft.
   4 **Scope:** New `memoryd export` CLI subcommand that emits a portable JSON dump of substrate contents. Read-only against the substrate. No new public Rust API on `memory-substrate`.
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
  19 
  20 ## 3. CLI surface
  21 
  22 ```
  23 memoryd export --repo <repo> --runtime <runtime>
  24                [--out <path>]
  25                [--format json]
  26                [--since <ISO8601>]
  27 ```
  28 
  29 | Flag | Default | Notes |
  30 | --- | --- | --- |
  31 | `--repo <path>` | required | Substrate repo path. Same semantics as `memoryd serve`. |
  32 | `--runtime <path>` | required | Per-device runtime path. Same semantics as `memoryd serve`. |
  33 | `--out <path>` | stdout | When set, the export is atomically written to `<path>` using the project's standard write-to-temp + fsync + rename pattern. |
  34 | `--format json` | `json` | Only `json` is accepted in v0.1. Any other value returns a hard error (no silent fallback). |
  35 | `--since <ISO8601>` | none | If set, only memories whose `updated_at` is >= the parsed value are included. Parse failures are hard errors. |
  36 
  37 Exit codes:
  38 
  39 - `0` on success.
  40 - `2` on argument or filter parse errors.
  41 - `1` on substrate open errors or I/O errors.
  42 
  43 Standard streams:
  44 
  45 - Stdout: the JSON document (when `--out` is not set). Stdout MUST NOT carry diagnostics.
  46 - Stderr: a single-line success summary (memory count, byte count) and any progress diagnostics. Diagnostics MUST NOT appear in stdout under any flag combination.
  47 
  48 ## 4. Output schema
  49 
  50 A single JSON object. Keys appear in the order listed below (insertion order; consumers MUST NOT rely on key ordering but the producer MUST emit stably). UTF-8, two-space indent for human readability, trailing newline.
  51 
  52 ```json
  53 {
  54   "schema_version": 1,
  55   "exported_at": "2026-05-17T12:34:56.789Z",
  56   "project_id": "prj_d4027a09c7f7",
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
  81 - `project_id` is read from the substrate's `project.json` (or equivalent identity store); the export does NOT derive its own id.
  82 - `source_device_id` is read from runtime device state; same source as the rest of Memorum.
  83 - `filters.since` is the verbatim ISO string the operator passed, or `null`.
  84 - `memory_count` MUST equal `memories.length` after filtering.
  85 - Each memory object's `frontmatter` is the canonical frontmatter as Stream A's frontmatter serializer would produce it (sorted keys, YAML-1.1 compatible), but emitted here as a JSON object — keys and scalar values are preserved verbatim; YAML quoting rules do not apply inside the JSON wrapper.
  86 - `body` and `body_marker` together describe the body: see §6.
  87 - `created_at` and `updated_at` are RFC3339 UTC, taken from substrate state, not synthesized.
  88 
  89 Memories are emitted in ascending `(updated_at, id)` order. This ordering is stable across re-runs against the same substrate and makes diffs across snapshots small.
  90 
  91 ## 5. Filters
  92 
  93 `--since <ISO8601>`:
  94 
  95 - Parsed strictly. Accepts the canonical RFC3339 form (`2026-05-01T00:00:00Z` or `2026-05-01T00:00:00+00:00`). Bare dates (`2026-05-01`) MUST be rejected with a hard error suggesting the canonical form.
  96 - Compared against `updated_at` (>=). Memories at the boundary are included.
  97 - The export does not filter on creation time, status, scope, or sensitivity in v0.1. Those are reserved for future flags.
  98 
  99 ## 6. Encrypted-memory handling
 100 
 101 The export reads the substrate as plaintext-or-ciphertext per the memory's `MemoryContent` variant (the same three-way shape as the inspector body fix in `c9d16fc`):
 102 
 103 | Substrate variant | Export `body` | Export `body_marker` |
 104 | --- | --- | --- |
 105 | `Plaintext(text)` | `text` verbatim | `null` |
 106 | `Ciphertext { .. }` | `null` | `"encrypted"` |
 107 | `MetadataOnly` | `null` | `"metadata-only"` |
 108 
 109 The export MUST NOT call any Stream D reveal-policy code path in v0.1. There is no flag in this version that opts into decryption. Operators who need the cleartext of an encrypted memory continue to use the existing `memory_reveal` MCP tool / reveal flow.
 110 
 111 Rationale: decryption is a governed audit event (Stream D / `EncryptedContentRevealed`); routing it through a bulk export would either (a) silently bypass the audit log (wrong) or (b) emit hundreds of audit events for one export call (wrong). v0.2 may add an opt-in path that records a single bulk-reveal audit event covering the whole export; that's a real design and outside v0.1.
 112 
 113 ## 7. Implementation boundaries
 114 
 115 This feature SHOULD:
 116 
 117 - Live in `crates/memoryd/src/bin/` or as a new subcommand on the existing `memoryd` binary (implementer's call). Either way, it reuses `crates/memoryd/src/serve_runtime.rs` to open the substrate.
 118 - Use the existing substrate read APIs (`Substrate::iter_*` or equivalents already shipped). It MUST NOT add new public methods to `memory-substrate`.
 119 - Use the workspace's existing atomic-write helper (the one introduced for the merge driver atomic-write fix) rather than calling `fs::write` directly. If no such helper exists at workspace scope, the export inlines the write-to-temp + fsync + rename pattern with the same shape.
 120 - Use `serde_json::to_writer_pretty` with a `BufWriter` over either `stdout().lock()` or the temp file (when `--out` is set). Streaming write — the producer does not hold the entire JSON value in memory if it can avoid it.
 121 
 122 This feature MUST NOT:
 123 
 124 - Mutate any substrate file, lock, or index.
 125 - Touch any encrypted-content body without going through the Stream D reveal flow (and v0.1 does not call that flow at all).
 126 - Add a new public Rust API on `memory-substrate`.
 127 - Bump any spec or invariant.
 128 
 129 ## 8. Acceptance items
 130 
 131 These are the three items wright will queue. Each names a specific closure condition (the test that must pass) and the minimum file scope an implementer is expected to touch.
 132 
 133 ### `export-json-shape-01` — Default JSON output validates against the v0.1 schema
 134 
 135 **Closure:** A new integration test under `crates/memoryd/tests/export_json_shape.rs` builds a 3-memory fixture (one plaintext, one encrypted, one metadata-only), runs `memoryd export --repo … --runtime …` capturing stdout, parses the result as JSON, and asserts:
 136 
 137 - `schema_version == 1`
 138 - `exported_at` parses as RFC3339 UTC
 139 - `project_id` and `source_device_id` are non-empty strings
 140 - `memory_count == 3` and `memories.length == 3`
 141 - Each memory has `id`, `scope`, `status`, `frontmatter`, `body`, `body_marker`, `created_at`, `updated_at`
 142 - Memories are sorted by `(updated_at, id)` ascending
 143 
 144 **Minimum scope hint:** new subcommand file in `crates/memoryd/`, new test file, no substrate touches.
 145 
 146 ### `export-since-filter-02` — `--since <ISO>` filters by `updated_at >= since`
 147 
 148 **Closure:** Integration test under `crates/memoryd/tests/export_since_filter.rs` builds a 4-memory fixture with `updated_at` values at `T-10d`, `T-5d`, `T-1d`, `T-now`. Runs `memoryd export --since <T-3d ISO>`. Asserts `memory_count == 2` and the included ids are exactly the two newest. A second sub-case asserts that a bare-date `--since 2026-05-01` returns exit code 2 with a stderr message naming the canonical RFC3339 form.
 149 
 150 **Minimum scope hint:** the same subcommand file (filter logic), new test file.
 151 
 152 ### `export-encrypted-default-03` — Encrypted bodies are never emitted without an opt-in flag (which v0.1 does not provide)
 153 
 154 **Closure:** Integration test under `crates/memoryd/tests/export_encrypted_default.rs` builds a 2-memory fixture: one plaintext, one ciphertext. Runs `memoryd export`. Asserts:
 155 
 156 - The plaintext memory's exported `body` equals the original plaintext, `body_marker` is `null`.
 157 - The ciphertext memory's exported `body` is `null`, `body_marker` is `"encrypted"`.
 158 - No `EncryptedContentRevealed` audit event was recorded during the export (assert via Stream A's event log read API).
 159 - The ciphertext bytes do NOT appear anywhere in the export output (defense-in-depth check).
 160 
 161 **Minimum scope hint:** the same subcommand file (body-variant routing), new test file. No Stream D code paths invoked.
 162 
 163 ## 9. Open questions
 164 
 165 These are flagged for the planner/implementer to surface in PR review, not pre-decided here:
 166 
 167 - **Subcommand vs new binary.** The spec leaves this to the implementer. Subcommand is probably right (shared option parsing, shared `serve_runtime`), but if it forces uncomfortable coupling, a sibling `bin/memoryd_export.rs` is acceptable.
 168 - **`Substrate::iter_*` shape.** If the substrate does not yet expose a clean iterator over memories with the metadata needed for export, the implementer should flag that rather than synthesizing one ad-hoc; the right answer might be a small additive method on `Substrate`, which is in scope for the implementer's PR but should be reviewed as a separate concern.
 169 - **Sort key stability.** `(updated_at, id)` is the v0.1 order. If `updated_at` is missing for legacy rows, the implementer should treat missing-as-epoch and document the choice in the test.
