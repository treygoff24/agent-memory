# Feature — `memoryd export` v0.1

**Status:** draft (revision 5; revision history at end).
**Scope:** New `memoryd export` CLI subcommand that emits a portable JSON dump of substrate contents. Semantically read-only against substrate *content*; does not mutate memories, locks, events, or index rows. Note that opening the substrate at all triggers the standard runtime-init side effects (§7).

This is the first feature spec authored under the wright workflow. The acceptance items in §8 are the closure conditions an implementer agent must satisfy.

## 1. Goal

Provide a single command that dumps a portable, self-describing JSON snapshot of a Memorum substrate. Useful for: backup-and-restore, cross-instance migration, debugging recall drift by sharing snapshots with collaborators, and feeding offline analysis.

## 2. Non-goals (deferred to a later version)

- **Import.** The reverse direction is a separate feature (`feature-memoryd-import-v0.1`), not in scope here.
- **YAML output.** `--format yaml` is reserved as a future flag value. v0.1 only emits JSON.
- **Streaming/NDJSON output for huge substrates.** v0.1 emits one JSON document.
- **Decryption of encrypted bodies.** v0.1 deliberately refuses to decrypt; see §6.
- **Export of derived index state** (FTS, vector index).
- **Concurrent `memoryd serve` + `memoryd export` against the same `--repo`/`--runtime`.** v0.1's behavior under that scenario is undefined; the export may fail to open the substrate or may observe in-flight state. Operators must stop the serve daemon before exporting. A future v0.2 may add a "read-only attach" mode that interoperates with a live daemon.
- **Stable cross-substrate `project_id` field** (Memorum has no persisted project-identity concept yet).
- **Preservation of YAML-specific frontmatter constructs** — anchors, aliases, tags, literal block style. Memorum's existing frontmatter does not use these; the export emits whatever serde naturally maps through the YAML → `serde_json::Value` → JSON path (§4).

## 3. CLI surface

```
memoryd export [--repo <repo>] [--runtime <runtime>]
               [--out <path>]
               [--format json]
               [--since <ISO8601>]
```

| Flag | Default | Notes |
| --- | --- | --- |
| `--repo <path>` | `.` | Substrate repo path. Same semantics as `memoryd serve`. |
| `--runtime <path>` | `.memoryd` | Per-device runtime path. Same semantics as `memoryd serve`. |
| `--out <path>` | stdout | When set, the export is atomically written to `<path>` (write to a hidden sibling temp file named `.<target-file-name>.<pid>.<nanos>.tmp` → `fsync` the temp file → `rename` over the target → best-effort parent-directory fsync). Basename paths such as `export.json` are resolved relative to the current directory. On Unix, the output file is created with mode `0600`; existing symlink targets are refused. On any failure before rename, the temp file is removed best-effort. |
| `--format json` | `json` | Only `json` is accepted in v0.1. Any other value is rejected by argument parsing with exit code 2 and a stderr message listing `json`; no silent fallback. |
| `--since <ISO8601>` | none | If set, only memories whose `updated_at` is >= the parsed value are included. Parse failures are hard errors (exit 2). v0.1's intended contract is UTC-only input: `Z` or `+00:00`, not non-zero offsets. |

Exit codes:

- `0` on success.
- `2` on argument or filter parse errors.
- `1` on substrate open errors or I/O errors.

Standard streams:

- Stdout: the JSON document (when `--out` is not set). Stdout MUST NOT carry diagnostics under any flag combination.
- Stderr: a single-line success summary (`memory_count=<n> bytes=<n>`) on success, and any progress diagnostics. Diagnostics MUST NOT appear in stdout.

## 4. Output schema

A single JSON object. UTF-8, two-space indent, trailing newline. The producer emits keys in the order shown below; consumers MUST NOT rely on key ordering.

```json
{
  "schema_version": 1,
  "exported_at": "2026-05-17T12:34:56.789Z",
  "source_device_id": "dev_ab12cd34",
  "filters": {
    "since": null
  },
  "memory_count": 42,
  "memories": [
    {
      "id": "mem_...",
      "scope": "project",
      "status": "active",
      "frontmatter": { "...": "..." },
      "body": "...",
      "body_marker": null,
      "created_at": "2026-05-01T10:00:00Z",
      "updated_at": "2026-05-10T14:22:00Z"
    }
  ]
}
```

Normative constraints:

- `schema_version` is exactly `1` for v0.1 outputs.
- `exported_at` is RFC3339, UTC (`Z` suffix), millisecond precision.
- `source_device_id` is read from the runtime device state; same source as the rest of Memorum.
- `filters.since` is the verbatim ISO string the operator passed, or `null` when no `--since` was supplied.
- `memory_count` MUST equal `memories.length` after filtering.
- `id` is the canonical memory id (e.g. `mem_<hash>`) as Stream A stores it.
- `scope` is the lowercase serialization of `memory_substrate::model::Scope` (serde `snake_case`). Permitted values: `"user"`, `"project"`, `"org"`, `"agent"`, `"subagent"`. The export MUST use serde's canonical serialization; it MUST NOT hand-stringify.
- `status` is the lowercase serialization of `memory_substrate::model::MemoryStatus` (serde `kebab-case`, which for v0.1 variants is identical to lowercase). Permitted values: `"candidate"`, `"active"`, `"pinned"`, `"superseded"`, `"archived"`, `"tombstoned"`, `"quarantined"`. Same serde-canonical rule.
- `frontmatter` is the substrate's `Frontmatter` value emitted via `serde_json` — i.e. whatever the path `YAML → serde_json::Value → JSON` produces. YAML-specific constructs (anchors, aliases, tags, literal block style) are NOT preserved (§2 non-goal). Existing Memorum frontmatter does not exercise them.
- `body` and `body_marker` together describe the body: see §6.
- `created_at` and `updated_at` are RFC3339 UTC with millisecond precision, taken from substrate state.

Memories are emitted in ascending `(updated_at, id)` order. Stable across re-runs against the same substrate; makes diffs across snapshots small.

## 5. Filters

`--since <ISO8601>`:

- Parsed strictly. Accepts the canonical UTC RFC3339 forms (`2026-05-01T00:00:00Z` or `2026-05-01T00:00:00+00:00`). Bare dates (`2026-05-01`) MUST be rejected with exit code 2 and a stderr message naming the canonical form. Non-zero offsets are out of contract for v0.1; callers should pass UTC.
- Compared against `updated_at` (>=, inclusive at the boundary).
- The export does not filter on creation time, status, scope, or sensitivity in v0.1.

## 6. Body, privacy, and tombstone handling

The export routes each memory by its `MemoryContent` variant (the three-way shape used elsewhere — see the inspector-body fix in commit `c9d16fc`):

| Substrate variant | Export `body` | Export `body_marker` |
| --- | --- | --- |
| `Plaintext(text)` with non-tombstoned status | `text` verbatim | `null` |
| Any content with `status == "tombstoned"` | `null` | `"tombstoned"` |
| `Ciphertext { .. }` | `null` | `"encrypted"` |
| `MetadataOnly` | `null` | `"metadata-only"` |

The export MUST NOT call any Stream D reveal-policy code path in v0.1. There is no flag in this version that opts into decryption. Operators who need cleartext continue to use the existing `memory_reveal` MCP tool / reveal flow.

Rationale: decryption is a governed audit event (Stream D / `EncryptedContentRevealed`); routing it through a bulk export would either silently bypass the audit log (wrong) or emit hundreds of audit events per export call (also wrong).

Privacy stance: v0.1 is a private backup/migration export, not a public redacted artifact. Plaintext memories are emitted verbatim regardless of sensitivity except for tombstoned memories, whose body is redacted by default. Encrypted memories are represented only by metadata and `body_marker`. Operators MUST treat stdout and any `--out` file as private data. The command MUST keep diagnostics out of stdout and MUST NOT echo memory bodies, ciphertext bytes, or frontmatter payloads in stderr.

Tombstone stance: tombstoned memories remain part of the substrate snapshot and are exported if present in the recall index, but their `body` is always `null` and `body_marker` is `"tombstoned"` regardless of the underlying `MemoryContent`. Frontmatter, including tombstone metadata, remains in the export so a restore/migration can preserve lifecycle state. Broader public/share-safe redaction is a future feature, not this contract.

## 7. Implementation boundaries

This feature MUST:

- Be invokable as `memoryd export …` (either a new subcommand on the existing `memoryd` binary or a sibling `bin/`, implementer's choice). The acceptance tests in §8 invoke the binary as a subprocess.
- Load synced privacy config from `--repo`/`--runtime`, install runtime privacy enforcement, then open the substrate via `memory_substrate::Substrate::open(Roots { repo, runtime })`. This mirrors the privacy-then-open sequencing the `serve` command performs, while keeping the export implementation independent of serve-only arguments (`socket`, `init`, `force_unsafe_durability`).
- Use serde-canonical serialization for `Scope`, `MemoryStatus`, and any other shared enum types — not hand-written strings.
- Inline the write-to-temp + fsync + rename pattern when `--out` is set. Use the same shape as `crates/memory-merge-driver/src/main.rs::persist_merged_output` (commit `db97fb2`). The workspace does not yet expose a public atomic-write helper; this inlines the precedent.
- Acknowledge the substrate open side effects in the user-facing docs (README / help text), since runtime-dir creation, index repair replay, and event-log mirror rebuild are NOT no-ops even though the export does not write substrate content.

This feature MAY:

- Add a single thin additive method on `Substrate` to iterate memory envelopes, if and only if the existing public APIs cannot satisfy §4 without unacceptable awkwardness. Currently visible public APIs on `Substrate`: `query_memory(MemoryQuery) -> Vec<QueryResult>` (returns only id/path/summary), `read_path_envelope(&RepoPath) -> MemoryEnvelope`. If the implementer chooses to add a method, the suggested signature is `pub fn iter_memory_envelopes(&self) -> impl Iterator<Item = Result<MemoryEnvelope, SubstrateError>>` and the PR description must justify the addition. Constraint: the new method is read-only and additive — no existing API shape or behavior changes.

This feature MUST NOT:

- Mutate any substrate memory content, claim lock, event-log entry, or index row beyond what `Substrate::open`'s standard runtime initialization already does.
- Touch any encrypted-content body without going through the Stream D reveal flow (and v0.1 does not call that flow at all).
- Bump any spec or invariant other than this one.
- Add multiple new public APIs on `memory-substrate`. If more than one new method seems necessary, escalate to a spec revision.

## 8. Acceptance items

Four items. Wright will queue them with the dependency ordering shown — `01` is the gating item that creates the subcommand file; `02`, `03`, `04` each `depends_on: ["export-json-shape-01"]` and extend the same file. Wright MUST NOT claim 02/03/04 until 01 is `implemented`.

All tests use the existing project convention of invoking the binary via `std::process::Command` (see `crates/memoryd/tests/review_queue.rs` for the pattern), against an in-process substrate built using the `privacy_e2e.rs` fixture pattern. They run single-device; multi-device coverage is out of scope.

### `export-json-shape-01` — Default JSON output validates against the v0.1 schema

**Depends on:** none (gating item).

**Closure:** A new integration test under `crates/memoryd/tests/export_json_shape.rs` builds a 3-memory fixture (one plaintext, one encrypted, one metadata-only) using the `privacy_e2e.rs` pattern. The test spawns `memoryd export --repo … --runtime …` via `std::process::Command`, captures stdout, parses it as JSON, and asserts:

- `schema_version == 1`
- `exported_at` parses as RFC3339 UTC, millisecond-precision
- `source_device_id` matches the fixture's device id, non-empty
- `filters.since` is JSON `null` (no `--since` was passed)
- `memory_count == 3` and `memories.length == 3`
- Each memory has `id`, `scope`, `status`, `frontmatter`, `body`, `body_marker`, `created_at`, `updated_at`
- Each memory's `scope` is one of the permitted serde-canonical strings from §4
- Each memory's `status` is one of the permitted serde-canonical strings from §4
- Memories are sorted by `(updated_at, id)` ascending
- The subprocess's captured stderr contains exactly one success-summary line matching the regex `^memory_count=\d+ bytes=\d+$` and no other lines

**Minimum scope hint:** new export subcommand wired into `crates/memoryd/`; new test file; an additive `Substrate` iterator method only if §7 MAY justifies it.

### `export-since-filter-02` — `--since <ISO>` filters by `updated_at >= since`

**Depends on:** `export-json-shape-01`.

**Closure:** Integration test under `crates/memoryd/tests/export_since_filter.rs` builds a 4-memory fixture with `updated_at` values at the exact instants `T0`, `T0+1d`, `T0+2d`, `T0+3d`. Runs `memoryd export --since <T0+2d-ISO>` as a subprocess. Asserts:

- `memory_count == 2`
- The included ids are exactly the two memories at `T0+2d` and `T0+3d` (the boundary memory at `T0+2d` is included — verifies `>=` is inclusive, not strict `>`)
- A second sub-case runs with `--since 2026-05-01` (bare date) and asserts exit code 2 plus a stderr message containing the canonical RFC3339 form

**Minimum scope hint:** filter logic in the same subcommand file from `01`; new test file.

### `export-encrypted-default-03` — Encrypted bodies are never emitted, no reveal-flow side effects

**Depends on:** `export-json-shape-01`.

**Closure:** Integration test under `crates/memoryd/tests/export_encrypted_default.rs` builds a single-device fixture with two memories: one plaintext, one ciphertext (use the encrypted-memory fixture pattern from `privacy_e2e.rs`). Runs `memoryd export` as a subprocess. Asserts:

- The plaintext memory's exported `body` equals the original plaintext, `body_marker` is `null`
- The ciphertext memory's exported `body` is `null`, `body_marker` is `"encrypted"`
- No `EncryptedContentRevealed` event appears in the current device's event log after the export (read via `Substrate::events()` from the same test substrate handle)
- The original ciphertext bytes do NOT appear anywhere in the captured stdout bytes (defense-in-depth check)

**Minimum scope hint:** body-variant routing in the same subcommand file from `01`; new test file. No Stream D code paths invoked from production code.

### `export-out-atomic-write-04` — `--out <path>` writes atomically and matches stdout output modulo `exported_at`

**Depends on:** `export-json-shape-01`.

**Closure:** Integration test under `crates/memoryd/tests/export_out_atomic.rs` reuses a 3-memory fixture. Runs `memoryd export --out <tempfile>` as a subprocess and also runs `memoryd export` (no `--out`) capturing stdout. Because v0.1 has no injectable clock, these are separate invocations and `exported_at` is expected to differ. Asserts:

- The raw stdout bytes and raw `--out` bytes are byte-for-byte identical after replacing only the volatile `exported_at` string with the same sentinel value in both byte streams.
- After the `--out` run, the temp directory containing the output file contains the target file and no leftover `.tmp` files.
- The output file ends with a trailing `\n`
- Basename output paths (`--out export.json` with `current_dir` set) succeed and leave no `.tmp` sidecars.
- On Unix, the output file mode is `0600` and existing symlink output targets are refused with exit code 1 and empty stdout.
- A negative sub-case: when `--out` points at a path whose parent directory does not exist, the command exits with code 1, stderr names the missing parent path, stdout is empty, and no partial file is left behind in the missing parent's grandparent.

**Minimum scope hint:** atomic-write helper inlined in the same subcommand file from `01`; new test file.

### Post-core hardening regression coverage

The core implementation should also retain focused regression tests for:

- Argparse failures (`--format yaml`) exit 2 and emit no partial JSON on stdout.
- Missing or invalid `source_device_id` fails with exit 1 and empty stdout.
- Unreadable indexed memory envelopes fail the export with exit 1 and empty stdout rather than silently shrinking `memory_count`.
- Tombstoned memories are still represented in the export, but their plaintext body is redacted with `body: null` and `body_marker: "tombstoned"`.

## 9. Open questions (flagged for implementer; do not pre-decide)

- **Substrate iteration choice.** §7 MAY allows a single additive `iter_memory_envelopes` method on `Substrate`. The implementer decides based on `query_memory`'s ergonomics. Either choice satisfies §8 as long as encrypted and metadata-only memories are reachable.
- **Filter inclusivity at sub-millisecond precision.** If `updated_at` is stored with sub-millisecond precision (it appears to be RFC3339 millis), `--since` comparisons should compare as `DateTime<Utc>` values, not as strings. The boundary test in item 02 verifies inclusivity but does not exercise sub-ms drift.
- **Frontmatter shape for JSON-incompatible YAML types.** If any Memorum substrate frontmatter contains YAML timestamp scalars (`!!timestamp` style) or other non-JSON-scalar types, serde's natural mapping will stringify them. The implementer should grep substrate fixtures and flag if real data hits this; otherwise the v0.1 lossy-conversion stance is fine.
- **Redacted/share-safe export mode.** v0.1 still intentionally includes non-tombstoned plaintext bodies. Any broader public/share-safe mode needs a new flag, schema semantics, and tests.

## Revision history

- **Rev 1** (initial draft): first pass, posted for adversarial review.
- **Rev 2** (post plan-reviewer): dropped `project_id` (no real source); resolved §7 SHOULD/MUST-NOT contradiction by allowing one additive iterator method; specified canonical serializations for `scope` and `status`; clarified single-device test fixture + event-log assertion; pointed at `privacy_e2e.rs` for ciphertext convention; removed false atomic-write-helper reference; extended §9 for MetadataOnly timestamps.
- **Rev 3** (post codex pass): added explicit `depends_on` ordering — items 02/03/04 depend on 01; reframed "read-only" honestly to acknowledge `Substrate::open`'s standard runtime side effects (§7 + new §2 non-goal for concurrent serve+export); replaced `assert_cmd` references with `std::process::Command` to match the actual convention; reframed §7 to require the privacy-then-open sequence explicitly and NOT to reuse `open_substrate_for_serve` (whose `&ServeArgs` shape doesn't fit export); spec'd `frontmatter` JSON conversion path honestly (no YAML anchor / tag / literal-block preservation, called out as a non-goal); added new acceptance item `export-out-atomic-write-04` to cover `--out` atomic-write which previous revisions silently dropped; tightened item 02 fixture to include an exact-boundary memory at `T0+2d` matching `--since`, verifying `>=` inclusivity; added explicit `filters.since: null` and stderr-format assertions to item 01.
- **Rev 4** (post implementation-alignment pass): removed accidental literal line-number prefixes; corrected `--repo`/`--runtime` defaults to `.` / `.memoryd`; documented clap-level `--format` rejection; clarified `--out` temp-file naming; added `quarantined` as a status value; removed stale epoch timestamp fallback language; clarified UTC-only `--since` intent and flagged enforcement as an open follow-up if implementation is currently more permissive; reframed the atomic-write comparison as byte-identical modulo volatile `exported_at`; documented the private-export stance and tombstoned-memory body redaction.
- **Rev 5** (current; post hardening pass): pinned raw-byte export comparison modulo only `exported_at`; added basename `--out`, Unix `0600`, symlink refusal, empty-stdout failure, and best-effort parent-fsync expectations; removed the UTC-only `--since` enforcement open question because implementation and tests now reject non-zero offsets.
