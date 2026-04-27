# Clean-code review — merge driver

Reviewer: reviewer-merge
Files reviewed:

- `crates/memory-substrate/src/merge/mod.rs` (12 LOC)
- `crates/memory-substrate/src/merge/three_way.rs` (501 LOC)
- `crates/memory-substrate/src/merge/field_rules.rs` (1 LOC, doc-only stub)
- `crates/memory-substrate/src/merge/lifecycle.rs` (1 LOC, doc-only stub)
- `crates/memory-substrate/src/merge/quarantine.rs` (1 LOC, doc-only stub)
- `crates/memory-merge-driver/src/main.rs` (35 LOC)
- `crates/memory-merge-driver/Cargo.toml`
- `crates/memory-substrate/tests/merge_rules.rs` (337 LOC, for context only)
- `crates/memory-merge-driver/tests/merge_driver_cli.rs` (99 LOC, for context only)

## Headline

The "release-certification candidate" framing is wrong. Three of four sibling modules in `crates/memory-substrate/src/merge/` are 1-line `//! …` stubs. The whole merge implementation lives in `three_way.rs` and only covers a subset of the spec's §14.4 / §14.5 field rules; many spec-mandated behaviors (true 3-way for `confidence` and `updated_at`, union-by-id for `evidence` / `tombstone_events` / `entities`, status-aware quarantine pairs, `_merge_diagnostics` shape, secret-sensitivity refusal, lifecycle event union under tombstone) are silently absent. Tests are shallow enough that those absences pass green. Recommend bouncing this back to Codex with the catalog below before any release-gate consideration.

## Blockers

### Spec-mandated behaviors missing or wrong

1. **`merge/field_rules.rs`, `merge/lifecycle.rs`, `merge/quarantine.rs` are 1-line stubs.** Each is just `//! Field merge rules.` / `//! Lifecycle merge rules.` / `//! Merge quarantine helpers.` — no code, no `pub fn`, no types. `mod.rs:3-6` declares them as private modules so they aren't even reachable. Either delete them or move the corresponding logic out of `three_way.rs` (where it currently lives in inline functions). Shipping module names that imply organization while putting all 500 lines into `three_way.rs` is misleading. **Fix:** decide whether you want sub-modules or a flat file; either delete the stubs or actually populate them. Right now they actively mislead a reader skimming the layout.

2. **`_merge_diagnostics` shape diverges from spec §6.10.** `three_way.rs:473-500` (`append_merge_diagnostic`) writes:

   ```json
   {"status": "...", "conflicting_fields": [...], "details": [...]}
   ```

   Spec §6.10 mandates top-level fields `merge_id` (ULID), `created_at` (datetime), `status`, `conflicting_fields`, `preserved_sources`, `evidence_near_duplicates`, `privacy_scans_preserved`, `add_add_alternates`, `unparsed_sides`, `lifecycle_notes`, `human_reason`. None of the typed top-level keys are emitted; everything is dumped into the synthetic `details[]` array. **Fix:** rewrite `append_merge_diagnostic` to emit the spec shape directly, with `merge_id`/`created_at` populated, and route per-field detail into the appropriate typed array (`add_add_alternates`, `unparsed_sides`, `evidence_near_duplicates`, …) instead of a free-form `details[]` bag.

3. **`status` value diverges from spec.** `three_way.rs:184, 252, 351, 387` write `"clean_with_diagnostics"`. Spec §6.10 enumerates `clean_with_warnings | quarantined`. There is no `clean_with_diagnostics` value in the schema. **Fix:** rename to `clean_with_warnings`. Mechanical fix; it just needs to actually happen.

4. **`add_add_alternates[]` cannot mechanically recover the original blobs.** `three_way.rs:459-465` emits:

   ```json
   {"id": ..., "frontmatter": <parsed-json>, "body": <plain-string>}
   ```

   Spec §6.10 mandates `{id, original_path, frontmatter_yaml_b64, body_sha256, body_b64 | body_artifact_ref}` and §14.6 says exit `0` "only if every original frontmatter and body is mechanically recoverable from the quarantined file". Round-tripping `serde_yaml`-derived JSON back to raw YAML cannot reproduce byte-for-byte original frontmatter (key order, comments, quoting, anchor reuse all evaporate during parse). **Fix:** capture the _raw bytes_ of the loser side at parse time (split-once on `---`/`---`) and base64-encode them as `frontmatter_yaml_b64` + `body_b64`; compute `body_sha256`. Don't try to reconstitute from the parsed `Memory`.

5. **`unparsed_sides[]` shape diverges.** `three_way.rs:113-120` emits `{side, path, raw_b64, parse_error}` (one base64 blob for the whole file). Spec §6.10 mandates `{side, path, frontmatter_raw_b64, body_b64, parse_error}` (separated). **Fix:** split the raw input on the first frontmatter terminator before base64-encoding, even when the YAML inside is unparsable.

6. **Diagnostics live under `details[0]` rather than top level.** Symptom: `tests/merge_rules.rs:178-182` reaches `diagnostics["details"][0]["add_add_alternates"][0]`. Spec wants `_merge_diagnostics.add_add_alternates[0]`. The tests are passing only because they were written against the buggy shape. **Fix:** when `append_merge_diagnostic` is rewritten (item 2 above), update the tests to read top-level keys; treat any test still touching `["details"]` as a smell that should fail.

7. **`updated_at` and `created_at` are never merged.** Spec §14.4 explicitly says `updated_at = max of merged changes` and `created_at = min`. Search `three_way.rs` for `updated_at` / `created_at`: zero references in the merge logic. The merged file just keeps `ours.frontmatter.updated_at`. After a clean merge that takes theirs' summary, the file claims it was last updated at ours' (older) time. **Fix:** in `merge_frontmatter_scalars`, set `merged.updated_at = ours.max(theirs)` (or merge-time when diagnostics are emitted, with the displaced max preserved per spec) and `merged.created_at = ours.min(theirs)`.

8. **`confidence` same-field 3-way conflict is silently dropped.** `three_way.rs:161-165` only handles the asymmetric "ours unchanged → take theirs" case. The "all three differ" arm in spec §14.4 (later `updated_at` wins; quarantine if delta > 0.25) is **not implemented**. If base=0.5, ours=0.7, theirs=0.9, the merged file silently keeps 0.7 with no diagnostic. **Fix:** add the 3-way conflict arm with the >0.25 quarantine guard.

9. **`tombstone_events` never unioned (spec §14.4, §14.5 #1).** `copy_lifecycle` (`three_way.rs:275-290`) does `merged.frontmatter.tombstone_events.clone_from(&source.frontmatter.tombstone_events)` — wholesale overwrite from a single side. The spec for `status: tombstoned` says "If either side is `tombstoned`, result is `tombstoned`; **union tombstone events**". Same story for `superseded_by` (overwrite from source instead of union; spec §14.4 says "set union, then status-aware normalization"). **Fix:** `copy_lifecycle` should union the array fields with the _other_ side too (or, cleaner, hoist tombstone/supersession union out of `copy_lifecycle` and run it unconditionally in `merge_frontmatter_scalars`).

10. **Independent `entities`, `supersedes`, `superseded_by`, `related`, `tombstone_events` edits drop theirs.** `merge_frontmatter_scalars` only unions `tags`, `aliases`, `evidence`, `extras`. Every other array field listed in spec §14.4 ("set union by ID") is _not unioned_. They survive only via the initial `merged = ours.memory.clone()` at `three_way.rs:76`, so any independent edit on theirs is lost. **Fix:** add union handling for each array field per the §14.4 table.

11. **`evidence` dedups by full JSON whitespace-normalized equality, not by `id` (spec §14.4).** `three_way.rs:303-311` (`union_json_values`) uses `json_equivalent`, which compares whole values after whitespace folding. Spec rule for `evidence`: "union by evidence `id`; fallback to `(quote_norm_hash, ref)`; near-duplicates preserved in diagnostics". Two evidence entries with the same `id` but different `quote` text are kept as duplicates today, and near-duplicate quotes are silently merged with no `evidence_near_duplicates` diagnostic. **Fix:** dedupe primarily by `id`, fall back to `(quote_norm_hash, ref)` only when ids are absent, and emit `evidence_near_duplicates` entries when only the secondary key matches.

12. **`type`, `scope`, `canonical_namespace_id`, `namespace` immutability not enforced (spec §14.4 row 3).** Spec: "immutable; same-field conflict quarantines". Code: nothing. If both sides changed `type`, the merge silently keeps ours' value with no diagnostic. **Fix:** before `merge_frontmatter_scalars` returns, check each immutable field; if all three differ or both sides differ from base, return a quarantine path.

13. **`review_state`, `requires_user_confirmation` rules absent (spec §14.4).** Spec: "stricter state wins: pending > rejected > approved > null for review, true > false for confirmation; same-field `approved` vs `rejected` quarantines". Code: not implemented. **Fix:** add a small per-field merge with the documented order; emit quarantine for the `approved` vs `rejected` collision.

14. **`retrieval_policy` / `write_policy` per-key 3-way merge absent (spec §14.4).** Spec: "recursive per-key true 3-way; stricter value wins for safety keys". Code: `three_way.rs:198-205` clamps `index_body`/`index_embeddings`/`mask_personal_for_synthesis` based on the _post-merged_ sensitivity. That's an after-the-fact override, not a per-key 3-way merge of the policy objects. A theirs-side change to `passive_recall: false` (a deliberate downgrade) is silently lost when ours kept the base. **Fix:** add a generic recursive-3-way over the policy maps, with explicit "stricter wins" tiebreakers for `index_body`, `index_embeddings`, `mask_personal_for_synthesis`, `passive_recall`, `max_scope`.

15. **Lifecycle table §14.5 partially implemented.**
    - §14.5 #1 (tombstone clears `superseded_by`): `copy_lifecycle` copies `superseded_by` from the chosen side; never clears it when result is tombstoned. ⚠️
    - §14.5 #5 (archived vs superseded → quarantine unless both have lifecycle diagnostics): `merge_conflicting_lifecycle` (`three_way.rs:229-261`) just picks the higher `lifecycle_rank` and emits a "clean_with_diagnostics" diagnostic. Spec wants quarantine in this pair. ⚠️
    - §14.5 #4 (`superseded` beats active only if `superseded_by` survives validation): no validation. ⚠️
    - **Fix:** model the pair-table as data, not a `match` ladder of `lifecycle_rank` comparisons. The current `lifecycle_rank` ordinal doesn't match the spec's pair semantics — e.g. `lifecycle_rank(Quarantined)=5` outranks `Pinned=4`, but spec §14.5 #2 says quarantine vs tombstone keeps tombstone, and pinned/quarantined isn't even covered by the rank ordering. Replace with an explicit `match (ours_status, theirs_status)` table or a 2D lookup.

16. **`_merge_diagnostics` itself is not unioned across sides (spec §14.4 last semantic row).** Spec: "union diagnostics by ID/content hash". Code: `merged = ours.memory.clone()` at `three_way.rs:76` keeps ours' diagnostics; theirs' prior diagnostics are silently dropped. Spec §14.7: "It must be preserved by future merges until resolved by admin command." **Fix:** before returning, union ours.merge_diagnostics with theirs.merge_diagnostics (and base.merge_diagnostics) by stable id/content hash, even on clean merges.

17. **`secret`-sensitivity early refusal not implemented (spec §14.4 sensitivity row).** Spec: "`secret` is not a persisted value, so any side with `sensitivity: secret` causes the driver to exit `1` without writing a merged file". Today the `Sensitivity` enum (model.rs:69-78) has no `Secret` variant, so `serde` will fail to parse such input and the merge takes the `quarantine_unparsed_sides` path (assuming delimiters are intact), producing a quarantined file rather than exiting 1. `error::ValidationError::SecretSensitivityOnDisk` is _defined_ (error.rs:151) but never _used_ anywhere in the tree. **Fix:** before YAML parsing, scan each side's frontmatter text for an exact `sensitivity: secret` token and return a typed error that the CLI surfaces as exit 1 with `merge-driver: secret sensitivity refused`. Add a fixture mirroring the schema-version gate test.

18. **Validation-failure quarantine fallback missing (spec §14.2 #7).** Spec: "Revalidate. If validation fails, try status-aware normalization where specified. If still invalid, quarantine with diagnostics." Today `merge_markdown` calls `serialize_document` (which validates), and on failure does `.map_err(|err| MergeError::Parse(err.to_string()))` — propagated to the caller, CLI exits 1. **Fix:** on `serialize_document` failure for a clean-merge result, retry with `status: quarantined` + diagnostics describing the validation error; only exit 1 if even the quarantine output won't validate.

### Convergence / determinism

19. **`union_json_values` ordering is not commutative across clones — breaks two-clone convergence (§13.6.1).** `three_way.rs:303-311` builds `out = ours.to_vec(); for value in right { append-if-new }`. So output order = `ours-order, then theirs-only-in-theirs-order`. Two clones running the same logical merge swap which side is "ours": clone A merges with `(ours=local, theirs=remote)`, clone B merges with `(ours=local, theirs=remote)` from its own vantage, and after a round-trip they're operating on opposite labellings of the same logical pair. Result: `evidence` array bytes differ between clones, the canonical-content equality test in §13.6.1 fails, and the two-clone harness can never reach a fixed point. The `tags`/`aliases` paths happen to be safe because `union_sorted` sorts at the end. `evidence` and the future `entities`/`tombstone_events`/`superseded_by` unions all need a deterministic key. **Fix:** sort `union_json_values` by a stable JSON-canonical key (the existing `normalized_json_key` is almost what you want — make `union_json_values` always sort its result by that key), or define a per-field key (e.g. evidence by `id`, tombstone events by event id, entities by id). Verify with a fuzz test that runs the merge with `(ours, theirs)` and `(theirs, ours)` swapped and asserts identical bytes.

20. **`merge_extras` BTreeSet incantation is right but obfuscated.** `three_way.rs:319-322`:
    ```rust
    let mut keys: std::collections::BTreeSet<_> =
        base.keys().chain(ours.keys()).chain(theirs.keys()).cloned().collect();
    for key in keys.split_off("") {
    ```
    `split_off("")` returns the entire set (since `""` is the smallest possible key) and leaves `keys` empty. This is just `keys.into_iter()` written in a way that requires the reader to parse the `BTreeSet::split_off` contract. The iteration order _is_ deterministic (BTree order) which is good for convergence — but the cleverness has zero payoff. **Fix:** replace with `for key in keys { … }` or `for key in keys.into_iter()`. Add a `// deterministic order for two-clone convergence` comment if you want to make the _reason_ the BTree is used obvious.

## Risks

- **`three_way.rs` is 500 LOC of mixed concerns.** It does (a) schema-gating, (b) parsing dispatch, (c) the clean-merge fast paths, (d) field merging (sensitivity, lifecycle, extras, regression, evidence), (e) quarantine emission, (f) `_merge_diagnostics` JSON construction. Per the clean-code lens this is several responsibilities. Suggested split, keeping the spec-required public surface (`merge_markdown`, `MergeInput`, `MergeResult`):
  - `three_way.rs` keeps `merge_markdown` and the top-level dispatch (parse → schema-gate → clean-fast-paths → call `field_rules::merge_frontmatter` and `body::merge_body`).
  - `field_rules.rs` (currently a stub) gets `merge_frontmatter_scalars`, `merge_extras`, `merge_regression`, the union helpers, the immutable-field guards, the per-policy 3-way.
  - `lifecycle.rs` (currently a stub) gets `merge_lifecycle`, the §14.5 pair table, and `lifecycle_rank` (or its replacement).
  - `quarantine.rs` (currently a stub) gets `quarantine_unparsed_sides`, `quarantine_merge`, `add_add_quarantine`, and `append_merge_diagnostic`.
    This isn't a stylistic preference — Codex named those modules in `mod.rs:3-6` as if they existed. Either populate them or remove the misleading scaffold.

- **`raw_schema_version` early gate is fragile.** `three_way.rs:144-150` accepts only literal `---\n` prefix. A file with `\r\n` line endings, leading whitespace, or BOM bypasses the early gate and only gets caught by the post-parse check (which is fine; the spec says LF-only via §6.12). Low risk, but worth a comment so a future reader doesn't tighten the prefix check and accidentally skip valid LF files.

- **`raw_schema_version` returns `None` on `schema_version: 2foo`** (line 149's `parse().ok()` swallows malformed integers). Combined with `unwrap_or(MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION)` at the call site, malformed-version inputs slip past the early gate and only get caught by the YAML deserializer (probably as a typed error → "Parse" → quarantine). Defensible but worth a test fixture: `schema_version: 2x` should still surface as a schema-version exit 1, not a quarantine.

- **`add_add_quarantine` doesn't detect ID collisions (spec §14.6).** `three_way.rs:446-471` always treats add/add as different-ID. Spec wants distinct handling: "If frontmatter IDs match: produce a valid quarantined file and emit diagnostics that duplicate-ID repair is required; do not invent invalid suffix IDs." **Fix:** check `ours.memory.frontmatter.id == theirs.memory.frontmatter.id` first; emit a different diagnostic (`status: quarantined`, `human_reason: "duplicate-ID repair required"`) without populating `add_add_alternates` (since both files share the same logical ID).

- **`merge_regression` uses `BTreeMap` keyed on `id` — good — but silently drops occurrences without an `id` (line 376-378).** Spec §14.4: "occurrences union by ID **or per-device G-counter max**; never raw-count sum". Today, anonymous occurrences are dropped entirely. Probably fine if the schema mandates ids, but add a unit test pinning the behavior so it doesn't change accidentally.

- **`merge_body` is whole-blob 3-way only (`three_way.rs:419-427`).** Any conflict at all → `Conflict`, even when the two sides edited disjoint regions of the body. Spec §14.2 #5: "Merge body with diff3 semantics." Real diff3 would resolve disjoint hunks cleanly. The current behavior is conservative (it quarantines instead of dropping data) but the spec says diff3, and the acceptance test in `tests/merge_rules.rs:5-14` only exercises the asymmetric "one side unchanged" path. Worth either implementing diff3 (probably via the `diffy` or `imara-diff` crate) or downgrading the spec wording — flag for Trey.

- **`copy_lifecycle` injects a "lifecycle merge requires review" diagnostic only when `merged.frontmatter.merge_diagnostics.is_none()` (line 281-289).** That guard means if a _prior_ clean-with-diagnostics merge already populated diagnostics, a subsequent quarantine status copy will _not_ add the "requires review" note — the file ends up `status: quarantined` but with diagnostic content describing some unrelated earlier conflict. **Fix:** drop the `is_none()` guard; always append the lifecycle-quarantine reason when the resulting status is quarantined.

- **`merge_driver_cli.rs` test coverage is two cases.** `merge_driver_requires_args` and two flavors of schema-version-gate. No test exercises a clean merge through the CLI, no test exercises quarantine output through the CLI, no test asserts the CLI exits 0 for a valid merge. Library tests are healthier (12 cases) but most rely on `text.contains(...)` substring checks instead of parsing the merged YAML and asserting on the structured value, so a wrong-shaped `_merge_diagnostics` (see blocker 2-6) passes the contains-check.

## Nits

- `three_way.rs:81, 137, 443, 470` use `serialize_document(...).map(MergeResult::Clean).map_err(|err| MergeError::Parse(err.to_string()))`. The resulting `MergeError::Parse(...)` masks what's really a _serialize/validate_ error as a _parse_ error. Add `MergeError::Serialize(String)` (or reuse `ValidationError`) so the failure mode is identifiable in logs.

- `three_way.rs:174-180` computes `losing_side`/`losing_value` by comparing `resolved == ours.frontmatter.sensitivity`. If both sides happen to be equal (which can't reach this branch — there's a `!=` guard above), this would mislabel. Defensible given the guard, but a `let losing = if resolved == ours.frontmatter.sensitivity { (theirs, "theirs") } else { (ours, "ours") };` reads more clearly and avoids the doubled-up conditional.

- `three_way.rs:393-412` (`json_equivalent` / `normalized_json_key`) hand-rolls a normalization. Two notes: (a) `normalized_json_key` for a `String` does whitespace folding via `split_whitespace().join(" ")`, but for a `Bool`/`Number`/`Null` it falls through to `.to_string()` which round-trips fine — but for an `Object` nested _inside_ an `Array` it does `.join("|")` of stringified members, which means an object `{a:1, b:2}` and an object `{a:1, b:2}` (same content) compare equal, but two objects with the same content in _different array positions_ canonicalize to different strings. Subtle. Add a doc-comment explaining what `normalized_json_key` is meant to canonicalize, or replace with a recursive structural-equality hash.

- `merge/mod.rs:8` re-exports `merge_markdown, MergeInput, MergeResult` but not the `MergeError` type — which is needed by anyone embedding the library to handle exit codes. The CLI gets it via `memory_substrate::error::MergeError`, but a public `pub use crate::error::MergeError` from `merge/mod.rs` would be friendlier.

- `merge_driver_cli.rs:5` invokes `Command::new(env!("CARGO_BIN_EXE_memory-merge-driver")).output()` with no args and asserts non-zero exit. A successful test, but no assertion on the _error message_ — clap's "missing required argument" output is brittle across versions; pin a substring (`Usage:` or `error: the following required arguments`).

- `crates/memory-merge-driver/src/main.rs:18-24` uses `Box<dyn std::error::Error>` for `run`'s return type, then formats via `eprintln!("merge-driver: {err}")`. That works for `MergeError`'s Display, but if `fs::read_to_string` fails with a path-not-found error, the output is `merge-driver: No such file or directory (os error 2)` — no path context. Add the path with `.with_context()` (anyhow) or wrap manually with `format!("read {}: {err}", path.display())`. Low-stakes but the schema-version-gate test asserts an exact stderr substring; same posture should apply to other failure modes.

- `three_way.rs:34` early-returns `add_add_quarantine(input.ours, input.theirs)` only when `input.base.trim().is_empty()`. Trim-emptiness is a slightly weird signal; in practice git passes an empty string for missing base, but ANY whitespace-only base triggers add/add path. Defensible, but worth a comment ("git passes empty base for add/add; trim guards against trailing newline differences across platforms").

## Strengths worth keeping

- **`MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION` is genuinely the single source of truth** (mod.rs:11) and all schema gates reference the constant (three_way.rs:38-46, 58-65). No magic numbers anywhere I could find. ✓ This is a real, spec-aligned win.
- The CLI surface (`memory-merge-driver/src/main.rs`) is appropriately tiny — a thin wrapper over `merge_markdown`. The crate has zero ambient logic that could drift from the library's gate.
- The schema-version stderr format (`error.rs:254`) matches spec §14.2 #2 byte-for-byte ("schema_version=<n> exceeds supported=<m>; upgrade required") and is exercised end-to-end via `merge_driver_cli.rs:37, 68`. Good integration test.
- `serialize_document` (frontmatter/serialize.rs:11-15) calls `validate_frontmatter` before emitting, which means a merge result that violates cross-field constraints is at least _detected_ — though see blocker 18 about how the failure is then surfaced.
- `merge_frontmatter_scalars` taking `&mut merged, &base, &ours, &theirs` with consistent argument order is good. Don't let any refactor lose that — keep the four-arg `(merged, base, ours, theirs)` shape and propagate it into the to-be-extracted helpers.

## Open questions for Trey

1. Spec §14.2 #5 says "Merge body with diff3 semantics". The current implementation is whole-blob equality (no per-hunk merge). Was the intent to do real diff3, or was the spec writer using "diff3" loosely to mean "3-way"? If real diff3 is expected, that's a new dependency (`diffy`, `imara-diff`) and probably 100+ LOC of fixtures.

2. `tests/merge_rules.rs` does shallow `text.contains(...)` substring assertions on YAML output for `_merge_diagnostics` checks. If we accept the blocker-2 / blocker-3 / blocker-5 fixes that change the diagnostics shape, the existing tests (and their substring assertions) all need rewriting. Want me to draft the replacement fixtures alongside the spec-shape fix, or is that Codex's follow-up?

3. The `Sensitivity` enum has no `Secret` variant (model.rs:69-78). Was that an intentional design choice (secret is a runtime `ClassificationOutcome`, never persisted), and the merge driver is supposed to detect `sensitivity: secret` _textually_ before parse — i.e. is blocker 17's fix purely "add a textual prefilter to `merge_markdown`"? Or did the design also intend for Stream A's writer to refuse `secret` _in `Sensitivity::deserialize`_ and we should add the variant + custom deserialize?

4. Two-clone convergence (blocker 19) is a release-gate criterion (§17.7.4). Has the two-clone harness `scripts/two-clone-convergence.sh` actually been run against this code, or was the "release-certification candidate" claim made before it was wired up? If the latter, I'd argue that's the single most important next test to write — it'll surface convergence bugs the unit tests can't see.

5. The `field_rules` / `lifecycle` / `quarantine` sub-modules are 1-line stubs but are _declared as private modules_ in `mod.rs`. Was there an in-progress refactor that got abandoned, or did Codex always plan to leave the logic in `three_way.rs`? Either is fine to declare, but the current state (declared modules, no contents) reads as broken work.
