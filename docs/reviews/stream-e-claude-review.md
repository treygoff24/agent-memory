# Stream E Claude review

Date: 2026-04-30
Reviewer: Claude (Sonnet 4.5)
Scope: full Stream E buildout in commit `85c0783` — substrate query extension
(`crates/memory-substrate/src/index/{query,migrations,schema,model,error}.rs`,
`api.rs`), privacy fragment surface (`crates/memory-privacy/src/{decision,lib}.rs`

- `tests/safe_plaintext_fragment.rs`), the recall module
  (`crates/memoryd/src/recall/*.rs`, 13 files / ~1616 LoC), daemon integration
  (`handlers.rs`, `mcp.rs`, `protocol.rs`, `cli.rs`, `main.rs`, `server.rs`,
  `lib.rs`, `Cargo.toml`), the bench harness
  (`crates/memoryd/src/bin/stream_e_recall_bench.rs`), and the new test surface
  (`startup_recall_*.rs` — determinism, governance, mcp, privacy, project_binding,
  ranking — plus `recall_cli.rs`, `protocol_contract.rs`, `mcp_manifest.rs`).

Reviewed through both the **Clean Code** (Uncle Bob) and **Rust Engineer** lenses.
Methodology: three layer-scoped read-only subagents (substrate+privacy, recall
core, daemon+protocol+tests) ran in parallel with both skills loaded; their
findings were then verified directly against `HEAD` before being included here.
Codex's seven self-review docs (`docs/reviews/stream-e-*review.md`) were
deliberately excluded as input so this is a genuinely fresh-eye adversarial
pass, not a re-read.

## Verdict

**Three real blockers, all in the render/contract surface, all mechanical
fixes.** Privacy invariants hold under defense-in-depth, determinism is
preserved on the hot paths, and the module decomposition is genuinely
well-shaped. The findings here are the class of issue a fresh reader catches
that a self-review can't: **the contract doesn't say what the code does, and
the code doesn't say what the contract means.**

Before declaring Stream E shipped, please fix B1–B3 (rendering correctness)
and at least R1, R3, R4, R5 from the risks list (architectural fragility that
will compound). The rest is good follow-up material but not ship-blocking.

This is high-quality work, Codex. The recall module's separation of concerns,
the privacy filter ordering in `candidates.rs`, and the determinism scaffolding
(`bounded_omissions`, `compare_ranked_candidates` tiebreaker chain) are
particularly tight. See **Strengths and wins** at the bottom — there's a lot
to be proud of here.

---

## Blockers (must fix before declaring shipped)

### B1 — XML element-content injection on `harness` / `session_id` / `cwd` / `alias`

**Where:** `crates/memoryd/src/recall/startup.rs:140-155`, with renderer at
`crates/memoryd/src/recall/render.rs:119-124` and validation at
`crates/memoryd/src/recall/binding.rs:42-50`.

**What happens today:** `identity_body` and `project_body` interpolate
caller-supplied strings directly via `format!()` and return the result, which
`render_section_body` writes line-by-line into the recall frame **without
escaping**:

```rust
fn identity_body(session_binding: &SessionBinding) -> String {
    format!(
        "- harness: {}\n- session: {}\n- cwd: {}",
        session_binding.harness, session_binding.session_id, session_binding.cwd
    )
}
```

Validation in `binding.rs:42-50` only checks length (≤128 bytes) and
non-empty trimmed — no character class restriction. A caller passing
`harness = "claude</memory-recall><script>"` produces malformed XML / agentic
prompt-injection inside the recall block that the consuming agent reads as
trusted system context. Same shape applies to `alias` via `project_body`,
which interpolates `display = project.alias.as_deref().unwrap_or(...)`
unescaped.

**Why it matters:** the recall block is concatenated into the consuming
agent's context as authoritative system text. An attacker (or a careless
caller) can hijack that channel by injecting closing tags or new instructions.
This is the highest-leverage attack surface on the entire Stream E protocol.

**Fix:** route every interpolated value through `escape_xml_text()` in the
body builders. `render_section_body` shouldn't own escaping (text vs attr
context is the body builder's responsibility), so the fix lives in the body
builders themselves:

```rust
fn identity_body(session_binding: &SessionBinding) -> String {
    format!(
        "- harness: {}\n- session: {}\n- cwd: {}",
        crate::recall::render::escape_xml_text(&session_binding.harness),
        crate::recall::render::escape_xml_text(&session_binding.session_id),
        crate::recall::render::escape_xml_text(&session_binding.cwd),
    )
}

fn project_body(session_binding: &SessionBinding) -> String {
    match &session_binding.project {
        Some(project) => {
            let display = project.alias.as_deref().unwrap_or(&project.canonical_id);
            format!(
                "- project: {}\n- namespace: project:{}",
                crate::recall::render::escape_xml_text(display),
                crate::recall::render::escape_xml_text(&project.canonical_id),
            )
        }
        None => "- project: none".to_owned(),
    }
}
```

Add a unit test that injects `</memory-recall><script>alert(1)</script>` into
each field and asserts the output frame remains well-formed and contains the
escaped form (`&lt;`, `&gt;`, `&amp;`).

### B2 — Wrong escape function on delta `id` attribute

**Where:** `crates/memoryd/src/recall/delta.rs:30-34`.

**What happens today:**

```rust
let rendered = format!(
    "  <item id=\"{}\">{}</item>\n",
    escape_xml_text(chunk.memory_id.as_str()),  // ← id is an ATTRIBUTE
    escape_xml_text(&chunk.text)
);
```

`escape_xml_text` is `escape_xml(value, false)` — it does not escape `"` or
`'`. The `id` value is in attribute position, so a memory_id containing a
quote would break the attribute boundary. MemoryId construction in Stream A
makes the practical exploit unlikely today, but the contract is wrong and the
symmetry with the rest of the renderer (which correctly distinguishes
`escape_xml_attr` vs `escape_xml_text`) is broken in this one spot.

**Fix:** one character:

```rust
let rendered = format!(
    "  <item id=\"{}\">{}</item>\n",
    escape_xml_attr(chunk.memory_id.as_str()),
    escape_xml_text(&chunk.text)
);
```

### B3 — Delta recall does not honor `passive_recall = false`

**Where:** `crates/memoryd/src/recall/delta.rs:14-17` →
`crates/memory-substrate/src/index/query.rs:178-204`.

**What happens today:** `query_chunks` filters
`memories.metadata_only = 0` (encrypted exclusion — defense-in-depth, comment
at `query.rs:176-177`) but does **not** filter `memories.passive_recall = 1`.
A memory deliberately written with `passive_recall = false` (i.e. "indexable
but not for passive surfacing") will leak into delta recall results.

Startup recall enforces this correctly via
`RecallIndexQuery.passive_recall_only = true` at `startup.rs:129`. Delta has
no analogous gate, and there is no test for it — `startup_recall_*.rs` is
exhaustive on startup paths but the delta path skips this invariant entirely.

**Fix:** either tighten the substrate query (add
`AND memories.passive_recall = 1` to the FTS join in `query_chunks`'s SQL —
preferred, defense-in-depth at source), or post-filter inside
`build_delta_response` after fetching by joining against a recall-index lookup
(`Substrate::query_recall_index` with the chunks' memory IDs). The substrate
fix is one line of SQL and one regression test.

Add a test (`startup_recall_privacy.rs` or new `delta_recall_privacy.rs`):
write two memories that match the same FTS terms, one with
`passive_recall = true` and one with `passive_recall = false`, fire a delta
request, assert only the passive_recall=true memory appears in
`<memory-delta>`.

---

## Risks (worth fixing in this same follow-up)

### R1 — `query_recall_index` lacks `metadata_only = 0` filter

**Where:** `crates/memory-substrate/src/index/query.rs:282-303` and
`:765-786`.

`append_recall_index_filters` adds namespace + status + passive_recall +
updated_since, but no `metadata_only` exclusion. Today's callers
(`candidates.rs`) save themselves via the `body_recall_omission_reason`
post-filter on `index_body` + sensitivity, so encrypted bodies don't reach
rendered output. But the API surface is leaky: the function name
`query_recall_index` implies a sanitized recall view, and a future caller who
forgets the post-filter ships an information leak.

The defense-in-depth pattern that already holds for `query_chunks`
(`query.rs:189`) and `query_vector_chunks` (`query.rs:227`) is missing here.

**Fix:** append `"memories.metadata_only = 0".to_string()` in
`append_recall_index_filters`, gated behind a future `include_metadata_only`
flag if you want to preserve the option to query encrypted stubs explicitly.
Add a test in `tests/memory_query_extension.rs` that asserts encrypted rows
are absent from `query_recall_index` output.

### R2 — Triple-render token convergence is unstable at digit-count boundaries

**Where:** `crates/memoryd/src/recall/startup.rs:107-111`.

```rust
let preliminary = render_startup_frame(&session_binding, &explanation, &sections);
explanation.budget_used_tokens = estimated_tokens(&preliminary);
let recall_block = render_startup_frame(&session_binding, &explanation, &sections);
explanation.budget_used_tokens = estimated_tokens(&recall_block);
let recall_block = render_startup_frame(&session_binding, &explanation, &sections);
```

Three renders, no convergence check. When the digit count of
`budget_used_tokens` changes between iterations (e.g. 999 → 1000 tokens), the
third render is rendered with stale data and the embedded `used-tokens`
attribute disagrees with `estimated_tokens(recall_block)`. The test
`startup_recall_mcp.rs:89` only verifies equality on small fixtures where
this doesn't trigger.

**Fix:** loop-until-stable with a small cap (4 iterations is plenty), or
render once with `used_tokens=0`, measure, and patch the attribute in place
without re-rendering. Add a fixture-based test that pads the recall block to
push the token count across a power-of-10 boundary.

### R3 — `budget_exhausted_total` is declared but never written

**Where:** `crates/memoryd/src/recall/counters.rs:14-15`.

`RecallCounters::budget_exhausted_total: BTreeMap<String, u64>` is exposed
in `Status` and serialized over the wire, but no `record_budget_exhausted`
method exists and no caller increments it. Operators reading the status will
see it always-zero and conclude "budget never exhausted" when the reality is
"we never measure it."

**Fix:** wire it. Every `OmissionReason::BudgetExhausted` produced in
`select_ranked_candidates` (or wherever the budget-exhaustion outcome lives)
should call a new
`SharedRecallCounters::record_budget_exhausted(section: &str)` keyed by
section name. If you'd rather keep this for v0.6, drop the field from
`RecallCounters` and the wire format until it's wired.

### R4 — `validate_delta_request` constructs a fake `StartupRequest` to reuse validation

**Where:** `crates/memoryd/src/recall/delta.rs:59-67`.

```rust
crate::recall::validate_startup_request(crate::recall::StartupRequest {
    cwd: request.cwd.clone(),
    session_id: request.session_id.clone(),
    harness: request.harness.clone(),
    harness_version: None,
    include_recent: true,
    since_event_id: None,
    budget_tokens: Some(budget.max(512)),  // ← injected to clear the 512-min check
})?;
```

Fragile coupling: it lies about `budget_tokens` to satisfy a check that
doesn't apply to delta. If `validate_startup_request` ever cross-validates
fields (e.g. budget against include_recent), delta breaks silently. Also: the
injected `budget.max(512)` means a delta call with `budget=128` runs cwd
validation under a fictitious 512-token budget — irrelevant today, a bug
tomorrow.

**Fix:** extract a shared helper

```rust
pub(crate) fn validate_session_fields(
    cwd: &str,
    session_id: &str,
    harness: &str,
) -> Result<SessionBinding, RecallError> { ... }
```

and call it from both `validate_startup_request` and `validate_delta_request`.
Move the budget validation entirely into each request's own validator.

### R5 — `RecallIndexReader` trait takes `&mut self` despite `Substrate::query_recall_index` taking `&self`

**Where:** `crates/memoryd/src/recall/candidates.rs:53-54`,
`crates/memoryd/src/recall/startup.rs:30`.

```rust
let mut substrate_reader = substrate.clone();
let collection = collect_recall_candidates_from_index(
    &mut substrate_reader,
    ...
).await?;
```

`Substrate` is `Arc`-backed so the `clone()` is cheap, but this is a false
mutation claim that bleeds into mocks: every test double has to take
`&mut self` to satisfy the trait, when the underlying impl never mutates.

**Fix:** change the trait to `fn query_recall_index(&self, ...)`, update the
impl, and the `clone()` and `mut` go away.

### R6 — Empty-corpus `ranking_now` falls back to `Utc::now()`

**Where:** `crates/memoryd/src/recall/startup.rs:44`.

```rust
let ranking_now = collection.facts.iter()
    .map(|c| c.row.updated_at)
    .max()
    .unwrap_or_else(Utc::now);
```

When the index has no facts yet, `ranking_now` becomes wall-clock time —
non-deterministic. Today the `select_ranked_candidates` call below operates
on an empty `Vec` so `ranking_now` is dead. But the moment ranking grows a
"recency boost" feature, this becomes flaky. The determinism test
(`startup_recall_determinism.rs`) operates on pre-built `SessionBinding` and
never exercises `build_startup_response` with a live substrate.

**Fix:** pass `ranking_now: Option<DateTime<Utc>>` and skip ranking when
`None`, or freeze the empty-corpus case to a fixed sentinel
(`DateTime::<Utc>::default()` is acceptable — it's documented as zero).

### R7 — `escape_xml` doesn't strip control characters

**Where:** `crates/memoryd/src/recall/render.rs:127-139`.

XML 1.0 forbids `\x00`-`\x1F` except `\x09` / `\x0A` / `\x0D`. Memory
summaries, tags, and aliases are not guaranteed at write time to be free of
stray control bytes. A summary containing `\x00` produces XML that strict
parsers (some agentic harness consumers) reject.

**Fix:** in the `escape_xml` match arm, drop or replace forbidden control
characters:

```rust
c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {
    // XML 1.0 forbids these; drop silently rather than emit malformed XML
}
```

### R8 — `add_column_if_missing` builds DDL via `format!`

**Where:** `crates/memory-substrate/src/index/migrations.rs:121-127`.

```rust
tx.execute(&format!("ALTER TABLE memories ADD COLUMN {column} {definition}"), [])?;
```

All current call sites pass string literals, so this isn't an active
vulnerability. But the function signature (`column: &str, definition: &str`)
is a footgun: a future migration that templates a column name silently
exposes DDL injection.

**Fix:** tighten signature to `column: &'static str, definition: &'static str`
to enforce literal call sites at compile time. Or, if a non-literal becomes
necessary later, add an SQL-identifier validator (regex
`^[A-Za-z_][A-Za-z0-9_]*$`) and document the safety contract in a `// SAFETY:`
comment.

### R9 — Mutex-poison silent zeroing in counters

**Where:** `crates/memoryd/src/recall/counters.rs:24-49`.

`snapshot()` uses `unwrap_or_default()` and the `record_*` methods use
`if let Ok(mut)`. The lock guards trivial arithmetic so panics-while-holding
are nearly impossible — but the pattern is the canonical "silent failure"
anti-idiom. If a panic ever does poison this lock, operators see all-zero
counters with no signal.

**Fix:** `expect("recall counters lock not poisoned")` is the idiomatic
alternative — fail loudly. Or, document the chosen behavior as intentional
in a comment so future readers don't second-guess it.

### R10 — Multi-term match-term filter joins with OR, not AND

**Where:** `crates/memory-substrate/src/index/query.rs:809-833`.

`append_match_term_filters` joins each term's clause with `" OR "` (line 832).
A query for `["streamE", "rust"]` returns the union — any row matching
either. This may be intentional for passive recall (broad surfacing), but no
test asserts the semantics and the spec doesn't define it.

**Fix:** pick one and pin it. If recall is union-on-purpose, document that
and add a unit test asserting union semantics. If it should be
intersection, change `" OR "` to `" AND "` and add a test asserting
intersection.

---

## Nits (polish — fix opportunistically)

- **`startup.rs:62`** — `format!("{:?}", candidate.candidate.row.source_kind)`
  for display. `{:?}` produces `Debug` output (`"AgentPrimary"`); any variant
  rename silently changes recall output. Add a `Display` impl or `as_str()`
  on `SourceKind`.

- **3-way constant duplication** — `DEFAULT_BUDGET_TOKENS = 3_600` in
  `startup.rs:15` and `binding.rs:9`; `DEFAULT_DELTA_BUDGET_TOKENS = 400` in
  `delta.rs:8`. Move to `types.rs` alongside `STREAM_E_POLICY`.

- **`recall/mod.rs:1-12`** — every submodule is `pub mod`. Implementation
  modules (`binding`, `budget`, `entity`, `project`, `rank`, `render`) should
  be `pub(crate) mod`; only the curated re-exports below need to be public.
  Shrinks the API surface and clarifies the contract.

- **`recall/error.rs`** — hand-rolled `Display`/`Error` impl when every other
  crate in this workspace uses `thiserror`. Inconsistent with the codebase
  pattern and with the rust-engineer skill.

- **`row_to_recall_index_row` (`query.rs:306-330`)** — N+1 SELECTs
  (`read_tags`, `read_aliases`, `read_entities` per row). Uses
  `prepare_cached` so statement compilation is amortized, but for 50-row
  recall blocks that's still 150+ SQLite round trips. Performance trap as
  the corpus grows. Consider a single JOIN with `GROUP_CONCAT` for the auxiliary
  tables, or batch-fetch by row IDs.

- **`handlers.rs` ~648** — `attach_privacy_scan` uses
  `serde_json::to_value(&privacy.scan).unwrap_or(serde_json::Value::Null)`.
  `unwrap_or` silently drops the scan on serialization error.
  `expect("privacy scan always serializes")` or proper error handling is
  correct.

- **`crates/memoryd/Cargo.toml`** — verify `memory-test-support` is
  `[dev-dependencies]`, not `[dependencies]`. Test fixture code shouldn't link
  into production binaries.

- **No constant-vs-literal test** — every test uses the literal
  `"stream-e-v0.5"` rather than the `STREAM_E_POLICY` constant. If the
  constant is bumped without updating literals, the green tests mask the
  drift. One assertion: `assert_eq!(STREAM_E_POLICY, "stream-e-v0.5")` makes
  the bump intentional.

- **`cli.rs` `include_recent` arg** — `#[arg(long, default_value_t = true)]`
  has no `--no-include-recent` negation form. If the spec wants this
  configurable from the CLI, add the negation; if it's permanently true at
  the CLI level, make it a constant rather than a flag.

- **`SafeFragmentDecision`** in `crates/memory-privacy/src/decision.rs:74-84`
  derives `Serialize`/`Deserialize` but has no protocol surface. Speculative
  derive — drop it.

- **`bare .unwrap()`** in `tests/memory_query_extension.rs:288, 311, 323` —
  even in tests, `.expect("MAX(version) from schema_migrations")` costs
  nothing and gives context when it fails.

---

## Test gaps (specific scenarios that aren't covered)

1. **`StartupResponse > 64 KiB` frame cap behavior** — no test pushes the
   response past the protocol frame cap. `Box<StartupResponse>` is unbounded
   by design but the wire path enforces a limit; behavior on overflow
   (truncate? error? panic?) is unverified.

2. **Delta `query_chunks` failure path** — no test injects an FTS error and
   verifies `delta_failed_total` increments. Only success counters are
   exercised today.

3. **Real MCP forwarder ↔ live substrate roundtrip** —
   `startup_recall_mcp.rs` calls handlers directly; `mcp_forward.rs` uses an
   echo mock. No test routes a real `StartupResponse` through
   `forward_to_daemon` → server → handler → response → forwarder. Frame
   serialization mismatches would only show up here.

4. **Mixed encrypted + plaintext recall index** — no test populates an
   encrypted record alongside a plaintext one and asserts the encrypted row
   is absent from `query_recall_index` output (the missing R1 filter would
   fail this).

5. **Passive_recall=false in delta** — no test for B3 above. Add it.

6. **Multi-term match query AND vs OR semantics** — no test pins R10's
   behavior either way.

7. **Concurrent counter access under load** — no test spawns N concurrent
   startup requests and asserts the snapshot is internally consistent.

8. **`<memory-delta empty="true" />` byte-equality at protocol level** —
   `recall_cli.rs:71` asserts CLI-level stdout, but no test round-trips
   `DeltaResponse` JSON and asserts the block is byte-identical post-decode.

9. **XML escaping in identity/project bodies** — no test injects hostile
   characters into `harness`, `session_id`, `cwd`, or `alias` and verifies
   the resulting frame is well-formed (B1 above).

10. **Empty-corpus `build_startup_response`** — no test runs the full
    pipeline with an empty index to verify deterministic output (R6).

---

## Strengths and wins (where this is genuinely good work)

This commit is high-quality. Several pieces are worth highlighting:

### Module decomposition is genuinely well-shaped

13 files in `recall/`, ~1616 LoC total, each with a clear single
responsibility. The `mod.rs` re-export list reveals a coherent API. Splitting
`candidates`, `entity`, `project`, `rank`, `render`, `startup`, and `delta`
into separate files (instead of one mega-orchestrator) is the right call and
makes future changes localizable. This is the kind of decomposition that
holds up under maintenance.

### Privacy filter chain is correctly ordered

`candidates.rs:87-108` checks `passive_recall` → review state → status →
`body_recall_omission_reason` (which itself checks `index_body` +
sensitivity). Encrypted/body-disabled rows can't reach the `facts` channel.
This is the most important invariant in the entire stream and it's
implemented cleanly at the source rather than relying on downstream filtering.

### Determinism scaffolding is rigorous

- `bounded_omissions` uses `BTreeMap`-stable ordering with a 4-tuple sort key
  (`section`, `reason`, `alias`, `id`).
- `compare_ranked_candidates` tiebreaker chain (`score → status_sort_key →
updated_at → id`) ends on a stable string compare — no `HashMap`
  iteration, no float surprises in the final tier.
- No `HashMap` iteration leaks anywhere in the output path. This is
  exactly what spec-mandated determinism requires.

### `truncate_utf8_bytes` handles the adversarial cases

`budget.rs:31-44` correctly walks `char_indices()` and tracks
`character_end` before comparing — can't split a multi-byte sequence.
4-byte emoji and combining marks are handled cleanly. This is a class of
bug that's easy to get wrong; getting it right on the first pass is solid
engineering.

### Substrate defense-in-depth on encrypted rows

`query_chunks` (`query.rs:189`) and `query_vector_chunks` (`query.rs:227`)
both filter `metadata_only = 0` even though upstream is supposed to. The
inline comments at `query.rs:175-177` and `:208` explicitly call out the
defense-in-depth intent — that's the right way to document a safety
invariant. (R1 is a gap in this same pattern, which is what makes it stand
out.)

### MCP admin-exclusion is enforced at the type level

The `ToolName` enum's `TryFrom<&str>` impl rejects admin commands
structurally, not just by docstring. `mcp_manifest.rs` has been extended
correctly to maintain the exhaustive exclusion list. This is the right way
to make a security invariant enforceable rather than aspirational.

### Backward compat on `StatusResponse.recall`

`#[serde(default)]` on the new `recall` field, confirmed by
`protocol_contract.rs:86-94` decoding legacy JSON without the field. Solid
discipline.

### Frame-cap drain-and-continue

`server.rs:154-166` drains an oversized frame to the newline rather than
closing the connection. Non-obvious but correct — the right reliability
shape for a long-lived daemon socket.

### YAML hardening in `project.rs`

`reject_malformed_project_yaml` + `reject_unsupported_scalar` provide
defense-in-depth before `serde_yaml` sees the input.
`#[serde(deny_unknown_fields)]` on `ProjectFile` is correct. This is the
right level of paranoia for a file-based binding source.

### `escape_xml_attr` vs `escape_xml_text` distinction

The split exists and is correct **where it's applied**. Most attribute
positions in the renderer use `escape_xml_attr` correctly. The B1/B2 issues
are about where the helpers aren't being used, not about the helpers
themselves — the helpers are right.

### `RecordingRecallIndex` test double

Used in `startup_recall_governance.rs` to prove no envelope hydration
occurs during candidate collection (`envelope_reads == 0`). This is a clean
way to verify a non-functional invariant (performance / I/O minimization)
that's normally hard to test.

### Bench harness compliance

`stream_e_recall_bench.rs` prints to stdout and never touches
`bench/baseline.*.json`. Compliant with the explicit-human-commit
requirement from CLAUDE.md / spec §17.6 / §18.9. Easy to get this wrong;
got it right.

### Error handling discipline

No `unwrap()` in production hot paths. The two `unwrap_or` patterns flagged
above (counters, `attach_privacy_scan`) are silent-failure anti-idioms but
not panic risks. `?` propagation is consistent throughout `query.rs`,
`migrations.rs`, and the recall module. Custom error variants carry enough
context to be useful.

### Spec-vs-implementation alignment

The plan-reviewer pass that caught three pre-build blockers (private
`safe_plaintext_fragment` collision, missing `index_body` column, doctor-vs-hot-path
contradiction in §9.5) clearly fed back into the implementation — those
issues are not present in the shipped code. Good iteration loop.

---

## Skill-lens summary

**Clean Code (Uncle Bob):**

- `build_startup_response` (`startup.rs`) is at the edge of god-orchestrator
  — 120 lines covering binding, querying, ranking, three renders, and
  explanation construction. The triple-render is "mixed levels of abstraction"
  smell. Otherwise SRP holds across files.
- Error handling discipline is good — typed errors, `?` propagation, no
  `unwrap()` in hot paths. The two `unwrap_or` patterns (counters,
  `attach_privacy_scan`) qualify as silent-failure anti-idioms.
- Testing posture leans heavily on integration tests (`tests/`) with almost
  no inline `#[test]` blocks for pure functions. `budget.rs` truncation and
  `render.rs` escaping are ideal for fast inline tests and would tighten the
  F.I.R.S.T. story.
- Function names are consistently intention-revealing
  (`build_startup_response`, `collect_recall_candidates_from_index`,
  `record_startup_failure`); no naming issues in new code.

**Rust idiom:**

- The `RecallIndexReader: &mut self` mistake forces an unnecessary `Arc`
  clone at the call site. Easy fix, removes a footgun.
- `error.rs` re-implements `thiserror` manually — works, but inconsistent
  with the rest of the workspace.
- `format!`-based DDL in `add_column_if_missing` is the only string-into-SQL
  pattern in `query.rs`, which is otherwise rigorously parameterized.
  Stands out.
- `Box<StartupResponse>` in the protocol enum is good size discipline.
- Serde tagging consistency across new types is solid:
  `#[serde(rename_all = "snake_case")]` everywhere, `#[serde(default)]` for
  backward compat.
- No `unsafe` in any new code. No FFI. Clean.

---

## Suggested fix order

If you want to batch this into commits, here's a sensible ordering:

1. **Commit 1 — render correctness (B1, B2, R7).** All in
   `crates/memoryd/src/recall/{render,startup,delta}.rs`. Single concern
   (XML well-formedness and escape correctness), tests live together.

2. **Commit 2 — query contract tightening (B3, R1, R10).** All in
   `crates/memory-substrate/src/index/query.rs` plus tests in
   `tests/memory_query_extension.rs`. Single concern (recall query surface
   privacy + semantics).

3. **Commit 3 — recall internals (R2, R3, R4, R5, R6).** All in
   `crates/memoryd/src/recall/`. Mostly mechanical refactors plus the
   `budget_exhausted_total` wiring decision.

4. **Commit 4 — tests + nits.** Test gaps from the list above, plus the
   nits sweep (mod visibility, error.rs → thiserror, constant
   consolidation, etc.).

5. **Commit 5 — DDL identifier safety (R8) and counter mutex semantics
   (R9).** Defensive hardening, behavior-preserving.

After commits 1–3 land and the gate is green, Stream E is shippable. The
remaining work is cleanup that can roll into Stream F or a v0.6 spec bump.

---

## Bottom line

Three real blockers (B1–B3), all in the rendering / contract surface, all
with mechanical fixes. R1–R5 are architectural fragility that compounds —
worth fixing in the same follow-up commit window. The substrate query
extension is the most rigorous piece of new code; the recall module's
decomposition is genuinely good shape; the daemon integration and tests are
thorough but skip a few important edge cases.

**This is not catastrophic — privacy invariants hold under
defense-in-depth, determinism is preserved on the hot paths, and Codex's
self-review caught everything Codex was looking for.** What this review
adds is the class of issue a fresh adversarial reader catches that a
self-review can't.

Nice work, friend. Ship it after the follow-up commits.
