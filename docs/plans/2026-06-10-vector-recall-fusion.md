# 2026-06-10 — Vector/Embedding Recall Fusion (Hybrid FTS+Vector Delta Recall)

**Status:** Draft — pending Trey review. The Stream E v0.5→v0.6 bump in Wave 0 requires explicit Trey sign-off at plan approval (see §4).
**Author:** Claude (orchestrator), from three scouting reports against the live tree (branch `onboarding/agent-driven-onboarding`).
**Executor model:** Trey's Claude session acts as PURE ORCHESTRATOR. Every unit of work — exploration, implementation, docs, tests — is delegated to a `delegate codex work` lane (isolated git worktree via `--isolation worktree`, disjoint file fence, per-lane gate), a `delegate cursor work` lane, or a native Claude opus subagent (spec/prose/review work). The orchestrator runs the integrated gate (`scripts/check.sh`) on trunk only, reviews every diff, cherry-picks lane branches to trunk, and owns all commits. **Workers never run `scripts/check.sh`** — it is orchestrator-only and trunk-only (a worktree carries stub/unstarted state that fails the workspace gate for the wrong reason; CLAUDE.md "Repository state strategy").

## Plan revision history

- r2 (2026-06-10) — patched per plan-reviewer findings B1-B3, R1-R7, N2.
- v0.1 (2026-06-10): initial draft from the three scouting reports. Ground truth file:line refs verified against the tree before writing; see the verification note at the end of this plan's authoring message.

---

## 1. Context — the gap

Embeddings are computed and stored today, but **no production recall handler ever reads them.** Every write enqueues `pending_embedding_jobs`; the memoryd worker (Qwen3-Embedding-0.6B, fastembed candle path) drains them into per-triple sqlite-vec tables; governance contradiction detection consumes those vectors through KNN. But delta recall passes `vector: None`:

```rust
// crates/memoryd/src/recall/delta.rs:66-69
let chunks = substrate
    .query_chunks(ChunkQuery { text: Some(message.to_owned()), triple: None, vector: None })
    .await?;
```

The Stream A v1.1 spec already carries the honest correction (commit `d4b9b39`, 2026-06-10):

> **Correction 2026-06-10 (hybrid recall production status):** … Production writes now populate active-triple vectors, and governance contradiction detection consumes those vectors through KNN, but no production recall handler embeds recall queries or passes `vector` into `ChunkQuery`. Production retrieval remains FTS-only bm25 today; hybrid keyword+vector recall is still future work.

This plan closes that gap: wire query-time embedding + vector KNN into delta recall, fuse it with the existing bm25 FTS lane, measure the lift with the armed quality eval, and land the change inside the spec contract.

**Why now.** The quality baseline is armed (`bench/quality-baseline.json`, gated by `report_is_well_formed` and `check-baseline-discipline.sh`). That gives us a before/after instrument — we can measure the fused lane's lift (and any trap-rate regression) *before* we re-arm the baseline, instead of shipping blind.

**Expected lift rationale.** SQLite FTS5 uses AND-phrase / token-overlap matching; it misses paraphrase and synonymy entirely (a query "how do I deploy" finds no chunk that says "shipping to production"). Both the fictional and private golden corpora score nDCG@5 in the 0.09–0.17 range on FTS-only recall — the floor where semantic retrieval has the most headroom. RRF over the two rank lists lets vector hits surface paraphrased matches that bm25 structurally cannot, while bm25 keeps exact-term precision.

---

## 2. Scope boundary (read first)

| In scope | Out of scope |
|---|---|
| **Delta recall** seam → hybrid FTS+vector. | **Startup recall** stays structural. `startup.rs:59-104` never sees query text and never calls `query_chunks`; there is no query to embed. Stated explicitly so no lane tries it. |
| Substrate gains a recall-membership-respecting **hybrid query surface**. | Reranker models, MMR / diversity, contextual-retrieval rewriting — none of it. |
| Fusion **policy** (RRF k=60) in memoryd recall code, exported for eval reuse. | Cross-triple mixing. Triple is identity (invariant 3); never one ranked list across triples. |
| `memory_search` MCP tool (`search_response`) wired to the same hybrid surface + RRF + degradation (Task 2.5 — in Lane R's fence). | Dreaming / governance changes. Governance already consumes vectors via its own KNN seam. |

---

## 3. Decisions (made — encode, do not reopen)

**D1 — Scope.** Delta recall becomes hybrid FTS+vector. Startup stays structural (no query text at startup). `memory_search` MCP (`search_response`, `handlers/memory_ops.rs:80-117`) is wired to the same hybrid surface — it has its own handler (it does NOT share the delta seam) but it lives inside Lane R's fence, so Task 2.5 ships it, not a follow-up. System spec §11 already promises `memory_search` is "hybrid keyword + vector + recency"; shipping hybrid recall while leaving `memory_search` FTS-only would widen a known spec gap.

**D2 — Division of labor.** The substrate gains a recall-membership-respecting hybrid query surface — `Substrate::query_hybrid_chunks(text, triple, vector, limit)` — returning per-MEMORY candidates, each carrying a `score_breakdown` (bm25 rank + cosine similarity), collapsing chunks→memory (min distance / best bm25 per memory), with filters identical to the existing recall exclusion contract. This matches Stream A §10.4: Stream A owns "hybrid result assembly with per-hit score_breakdown inputs, NOT final policy ranking." The fusion **policy** (RRF, k=60) lives in memoryd recall code and is exported so memorum-eval calls the identical function (no second implementation to drift).

**D3 — Degradation.** Recall copies the governance degradation ladder verbatim in shape (the pattern at `policy.rs:310-388`) and adds one recall-only rung for the latency contingency — **SEVEN stable string markers**: `no_embedding_provider` / `no_active_triple` / `triple_mismatch` / `embedding_failed` / `no_vector_table` / `knn_failed` / `embedding_timeout` (the seventh is the D7 timeout rung). `DeltaResponse` gains an optional additive field `vector_recall_degraded: Option<String>` (serde `default`, additive protocol change — no version-breaking shape change). An empty provider slot (`MEMORUM_DISABLE_EMBEDDING_WORKER`) yields FTS-only + a marker, never an error. **No silent fallback** (invariant 3): `vector=Some, triple=None` must never silently degrade to FTS the way `query_chunks` does today — the hybrid surface takes both or neither. The seven-marker enumeration is identical everywhere: this list, the S1 Stream E v0.6 spec section, and Task 2.3's "test each rung" (= seven rungs).

**D4 — Config.** New `recall:` block in `config.yaml`, loaded alongside the dynamics-config pattern (`dynamics/mod.rs:44-126`):
```yaml
recall:
  vector_recall:
    enabled: true             # bool, default true
    knn_limit: 20              # usize, default 20
    rrf_k: 60                  # u32, default 60
    embed_timeout_ms: 50       # u64, default 50 — D7 latency contingency
```
No env var governs this layer (there is no `MEMORUM_DYNAMICS` precedent at this layer — the knob is net-new and config-only). `MEMORUM_DISABLE_EMBEDDING_WORKER` already gates whether the provider slot is populated; the `enabled` flag is the explicit operator off-switch on top of that.

**D5 — Determinism.** `FixtureProvider` (deterministic hashed bag-of-words, L2-normalized, asymmetric query/doc, `synthetic_test_triple()` matching the eval's default active triple) backs all gates and tests. Ranking-formula tests stay embedding-free: the query embed happens during candidate collection, *outside* the deterministic ranking-formula test boundary (Stream E §8.2 forbids embedding calls inside the ranking tests). Byte-stability (§2 #7) holds **per-device**: same repo state + request + budget + clock ⇒ byte-identical block on one device. Per-device fp drift (Metal fp16 vs CPU f32) is acceptable and the v0.6 amendment must say so.

**D6 — Specs.** Stream E **v0.5 → v0.6 version bump** (removes the §15 deferral item, codifies the RRF fusion algorithm, the degradation ladder, the additive response field, and the per-device byte-stability note). **⚠ TREY SIGN-OFF REQUIRED — see §4.** Stream A v1.1 gets a **dated additive amendment** for the new `query_hybrid_chunks` surface (additive public surface is allowed in-version per CLAUDE.md "Spec/plan conventions" and Stream A §10.4). Both authored by a native opus docs subagent, reviewed by the orchestrator.

**D7 — Latency.** The Stream E §13 budgets (delta non-matching ≤60ms p95, delta 5-entity ≤120ms p95 at 1K memories) are binding and enforced in release by `stream_e_recall_bench.rs:286-306`. Query-time embed + KNN must fit, or the v0.6 amendment renegotiates with measured numbers — never a silent budget bust. **Contingency mechanism (decided, ships in Lane R from day one):** Lane R wraps the `spawn_blocking` embed in `tokio::time::timeout` with config knob `recall.vector_recall.embed_timeout_ms` (default 50); on timeout ⇒ the seventh degrade marker `embedding_timeout` ⇒ FTS-only, never a blown p95. The off-gate Wave 4 real-model measurement then informs whether the default timeout value or the §13 budgets themselves need adjustment — but the timeout machinery is not deferred to measurement time; it is present from the first Lane R commit.

---

## 4. Spec impact — ⚠ TREY SIGN-OFF REQUIRED

> **🚩 Stream E v0.5 → v0.6 is a VERSION BUMP and a behavior change to recall block contents.** Repo rule (CLAUDE.md "What NOT to do"): *don't bump spec or plan versions without Trey's explicit ask.* This plan cannot start Wave 0's Stream E task until Trey grants the bump at plan approval.

| # | Spec change | Kind | Gate |
|---|---|---|---|
| S1 | Stream E v0.5 → **v0.6**: delete §15 deferral line 1044 ("semantic embeddings for recall ranking…"); add a fusion section (RRF k=60 over bm25-order and distance-order rank lists, deterministic id tie-break); document the `vector_recall_degraded` response field and the **seven-marker degradation ladder** (`no_embedding_provider` / `no_active_triple` / `triple_mismatch` / `embedding_failed` / `no_vector_table` / `knn_failed` / `embedding_timeout`); document per-device byte-stability and the §13 latency impact (or renegotiated budgets, post-measurement). Also fix the surviving stale "v0.3" references in §15 prose (N2). **Behavior change ⇒ version bump ⇒ Trey sign-off.** | Version bump | plan-reviewer + Trey |
| S2 | Stream A v1.1 **dated additive amendment**: `query_hybrid_chunks` surface — signature `(text, triple, vector, limit) -> Result<_, VectorError>`, per-hit `score_breakdown` inputs, the recall membership filter it enforces (`metadata_only = 0 AND passive_recall = 1`, exclude superseded/tombstoned), chunk→memory collapse, partial-vector-coverage tolerance, and the **`UnknownEmbeddingTriple` contract** (vec-table-absent ⇒ `Err(UnknownEmbeddingTriple)` mirroring `query_vector_chunks` at `query.rs:398-400`, never silent empty; §5 #7). Additive public surface, allowed in-version (§10.4). | In-version amendment | orchestrator review |

§15 line 1051-1052 of the live Stream E spec is explicit: *"If an implementation needs one of these to pass the … acceptance tests, the spec should be revised before coding continues."* So S1 lands as a draft in Wave 0 (parallel with Wave 1 code) but **must be approved before Wave 2 code review completes** — the v0.6 contract is what Wave 2 is implementing against.

---

## 5. Spec invariants every lane must respect (restate in each brief)

1. **Triple is identity** (invariant 3, Stream A §10.2.2 #6). Never mix triples in one ranked list. Mismatch is a typed error (`UnknownEmbeddingTriple` / `DimensionMismatch`), never silent fallback.
2. **Recall membership filter.** The vector lane keeps `metadata_only = 0 AND passive_recall = 1` and excludes superseded/tombstoned memories (Stream H tests t19 etc.). This is *different* from governance KNN: `knn_active_memories` deliberately **omits** `passive_recall = 1` (write-governance semantics, see `d4b9b39` and `governance_passive_recall_excluded_knn_e2e.rs`) — so **`knn_active_memories` is NOT reusable for recall.** The hybrid surface needs its own query that keeps the passive-recall filter.
3. **Query side of the asymmetric pair.** Recall queries call `embed_query` (instruction-prompted), never `embed_document`. Collapsing the two measurably degrades retrieval (dynamics-eval-hardening plan line 169). 512-token truncation already applies (`fastembed_provider.rs:40`).
4. **No silent fallback.** Empty provider slot ⇒ FTS-only + visible degrade marker. Triple mismatch ⇒ marker. Never fabricate or quietly drop the vector lane without a marker.
5. **Partial vector coverage is normal** (Stream A §10.5). Pending embedding jobs mean some chunks lack vectors; the hybrid surface tolerates a chunk having a bm25 hit but no vector row, and vice versa.
6. **Per-device byte-stability** (§2 #7): deterministic given fixed vectors; every tie-break chain ends in lexicographic memory id.
7. **`UnknownEmbeddingTriple` contract handoff (Lane S errors, Lane R recovers).** `query_hybrid_chunks` returns `Result<_, VectorError>`. Vec-table-absent / dropped-triple ⇒ `Err(UnknownEmbeddingTriple)` (never a silent empty vec — matching `query_vector_chunks` at `query.rs:398-400`). Table present but some chunks unembedded ⇒ tolerated, those chunks simply contribute no vector rank (partial coverage, Stream A §10.5). On the recall side: Lane R catches `Err(UnknownEmbeddingTriple)` → `no_vector_table` marker → FTS-only, exactly mirroring `policy.rs:363-367`; any other `Err(_)` → `knn_failed` marker → FTS-only. Both lane briefs restate this rule identically.

---

## 6. Metric-collision note (load-bearing for the fusion choice)

`ChunkResult.score` is overloaded: bm25 (lower = better) for FTS, raw L2 distance (lower = better) for vector — **different scales, neither normalized.** No spec defines a fusion algorithm (RRF appears only in the non-binding reference handbook). We DECIDE **Reciprocal Rank Fusion, k=60**, over the two *rank* lists (bm25 order, vector-distance order). RRF is scale-free — it sidesteps the bm25-vs-L2 collision by fusing ordinal ranks, not raw scores — and its tie-break is deterministic by memory id. `cosine_from_l2_distance = (1 - d²/2).clamp(-1,1)` (`query.rs:1850`, assumes unit vectors; both Qwen3 and FixtureProvider emit normalized) is the similarity surfaced in the `score_breakdown` for explanation/trust-artifact display — it does NOT drive ranking (RRF does), so its absolute scale never matters to order.

---

## 7. Wave / lane topology (dependency-ordered)

```
Wave 0  Specs & contracts        (native opus docs subagent)   ── parallel with Wave 1; lands before Wave 2 review completes
Wave 1  Lane S — substrate hybrid query surface   (delegate codex work, worktree)
Wave 2  Lane R — memoryd recall integration       (delegate codex work, worktree; depends on Lane S merged)
Wave 3  Lane E — eval vector lane                 (delegate codex work, worktree; depends on Lane R merged)
Wave 4  Orchestrator closeout — integrate, off-gate real-model measurement, single [bench-update] re-arm, run record
```

Lane S and Wave 0 run concurrently (disjoint files). Lane R is hard-blocked on Lane S merged to trunk (it calls the new surface). Lane E is hard-blocked on Lane R merged (it switches `rank_via_search` to the shared fusion helper that Lane R exports), but its fused-switch commit is **held off trunk** until Wave 4's atomic re-arm (B1 — see Wave 4). Wave 4 is orchestrator-only.

### Lane-brief skeleton (every code lane uses this shape)

- **Owned files (fence):** exact globs the lane may write. Nothing outside.
- **Forbidden files:** explicit don't-touch list.
- **Spec invariants:** restate §5 items relevant to the lane.
- **Per-lane gate:** the exact commands the worker runs in its worktree. **Never `scripts/check.sh`.**
- **Commit discipline:** subject ≤72 chars; soft-wrapped body (no mid-sentence `\n`); no `--amend`; stage files by name (never `git add -A`).
- **Report format:** branch name; files touched; gate output (pass/fail + timing); any spec-contract surprises; for Lane R/E, the measured fused-vs-FTS numbers from the side report.

---

## Wave 0 — Specs & contracts (native opus docs subagent)

### Task 0.1 — Stream E v0.6 draft + Stream A amendment

- **Owner:** native Claude opus docs subagent (prose/spec work, not Codex).
- **Owned files:** `docs/specs/stream-e-passive-recall-v0.6.md` (new file — copy v0.5, add "Revision goal" block, apply S1); `docs/specs/stream-a-core-substrate-v1.1.md` (append a dated additive amendment block for S2 — do NOT mutate prior content).
- **Forbidden:** any code; any older spec version (v0.5 stays on disk untouched).
- **Acceptance:** v0.6 carries a "Revision goal" entry naming the deferral removal + RRF codification + response field + seven-marker ladder + byte-stability note + §13 impact. When copying v0.5 → v0.6, **fix the stale "v0.3" references in the surviving §15 prose** (lines ~1040, 1051 say "Stream E v0.3" / "v0.3 acceptance tests" — N2). The Stream A amendment is purely additive and dated. plan-reviewer pass clean. **Trey has signed off on the v0.6 bump.**
- **Dependencies:** Trey sign-off on D6 / §4 before this task's Stream E file is written. Stream A amendment may proceed independently.
- **Gate (docs):** `scripts/check-doc-cli-surface.sh` if any CLI surface is named; otherwise orchestrator prose review. No Rust gate.

---

## Wave 1 — Lane S: substrate hybrid query surface (delegate codex work, worktree)

### Task 1.1 — `Substrate::query_hybrid_chunks`

- **Owned files (fence):** `crates/memory-substrate/src/**` only (`api.rs`, `index/query.rs`, and their `#[cfg(test)]` modules; new test files under `crates/memory-substrate/tests/`). Optional: extend `crates/memory-substrate/src/bin/` stream_a bench if one exists for vector queries.
- **Forbidden:** `crates/memoryd/**`, `crates/memorum-eval/**`, `crates/memory-governance/**`, any `docs/**`, `Cargo.lock`, `pnpm-lock.yaml`. `Cargo.toml` edits allowed (deps only); lockfile is orchestrator-merged.
- **Spec invariants (restate):** §5 #1 (triple identity, typed error not fallback), #2 (recall membership filter `metadata_only=0 AND passive_recall=1`, exclude superseded/tombstoned — and **do NOT reuse `knn_active_memories`** which omits the passive filter), #4 (no silent fallback — take both `text` and `vector`+`triple` or neither), #5 (partial vector coverage), #6 (deterministic tie-break ending in lexicographic id), **#7 (the `UnknownEmbeddingTriple` contract — Lane S errors here: vec-table-absent ⇒ `Err(UnknownEmbeddingTriple)` mirroring `query_vector_chunks` at `query.rs:398-400`, never a silent empty; partial coverage tolerated)**.
- **What to build:** a new query method `query_hybrid_chunks(text, triple, vector, limit) -> Result<_, VectorError>` returning per-MEMORY candidates, each carrying `score_breakdown { bm25_rank: Option<usize>, cosine_similarity: Option<f32> }` (a memory may appear in one lane, the other, or both). Collapse chunks→memory: best bm25 per memory, min L2 distance per memory (the same min-distance collapse `knn_active_memories` does correctly, CHUNK_FANOUT over-fetch). Membership filter as §5 #2. Return `Err(UnknownEmbeddingTriple)` (never silent empty, per §5 #7 / `query.rs:398-400`) when the triple's vec table is absent; tolerate unembedded chunks as no-vector-rank (partial coverage). **Fusion is NOT done here** — the surface returns both per-lane rank inputs; RRF lives in Lane R.
- **Acceptance criteria:**
  - Unit test: membership filter excludes `metadata_only=1`, `passive_recall=0`, superseded, and tombstoned rows from both lanes.
  - Unit test: chunk→memory collapse picks best bm25 / min distance per memory; no duplicate memory ids in output.
  - Unit test: partial coverage — a chunk with a bm25 hit but no vector row appears with `cosine_similarity: None`; a vector-only hit appears with `bm25_rank: None`.
  - Unit test: triple absent ⇒ `UnknownEmbeddingTriple`, not empty vec.
  - Determinism test: fixed vectors ⇒ identical output ordering across runs; tie-break ends in lexicographic id.
- **Per-lane gate:** `cargo test -p memory-substrate --tests --no-fail-fast` + `cargo clippy -p memory-substrate --tests -- -D warnings` + `cargo fmt -p memory-substrate --check` + `RUSTDOCFLAGS="-D warnings" cargo doc -p memory-substrate --no-deps` (`check.sh:116` enforces it; ff1acac is live history of doc-link bounces).
- **Report:** branch, files, gate output, and whether the new surface needed any signature change to `ChunkResult` / `score_breakdown` types that downstream crates will see.

---

## Wave 2 — Lane R: memoryd recall integration (delegate codex work, worktree; depends on Lane S merged)

### Task 2.1 — Thread the embedding provider into the recall layer

- **Owned files (fence):** `crates/memoryd/src/recall/**` (includes `recall/types.rs` — the additive `DeltaResponse` field — and `recall/delta.rs`), `crates/memoryd/src/handlers/memory_ops.rs` (includes `search_response`, Task 2.5), a new config module file (e.g. `crates/memoryd/src/recall/config.rs` or fold into existing recall config), `crates/memoryd/tests/` (new files + touched recall tests), `crates/memoryd/src/bin/stream_e_recall_bench.rs`.
- **Forbidden:** `crates/memoryd/src/embedding/**` (read-only use of the trait/slot), `crates/memoryd/src/handlers/governance/**`, `crates/memoryd/src/dynamics/**`, `crates/memory-substrate/**`, `crates/memorum-eval/**`. `Cargo.lock`.
- **Precondition (orchestrator verifies before lane launch):** confirm `EmbeddingProvider` (trait) and the provider `Arc` are importable from `recall::` (they live in `src/embedding/`, which is read-only for Lane R). **If visibility is insufficient, Lane R gets a narrow fence exception: visibility/re-export lines ONLY in `crates/memoryd/src/embedding/mod.rs`, nothing else in that module.**
- **Signature change required:** `build_delta_response_with_coordination` (and its `_inner`) must accept the provider. `delta_response` (`handlers/memory_ops.rs:21-43`) already holds `&HandlerState` and reaches `state.embedding_provider` (verified: the `EmbeddingProviderSlot` lives on `HandlerState`, `handlers/mod.rs:114-158`). **Keep the no-state `build_delta_response` compiling** — it is called by `bin/stream_e_recall_bench.rs:11`; use an optional param or a new overload that passes `None` provider (⇒ FTS-only).
- **Acceptance:** the provider (or `None`) flows from `HandlerState` to the delta candidate-collection site; no behavior change yet beyond plumbing; existing delta tests stay green.

### Task 2.2 — embed_query + KNN + RRF in delta candidate collection

- **Owned files:** `crates/memoryd/src/recall/delta.rs` (+ a shared fusion helper module, **exported** for memorum-eval).
- **Spec invariants:** §5 #1, #3 (`embed_query`, not `embed_document`), #4, #6.
- **What to build:** in delta candidate collection (not the ranking-formula boundary), when the provider is present and the active triple matches: `spawn_blocking(|| provider.embed_query(message))` → `substrate.query_hybrid_chunks(text, triple, vector, knn_limit)` → **RRF fuse** the two rank lists with `rrf_k`, deterministic id tie-break → map fused order to `DeltaRecallItem`. The fusion helper is a free function `pub fn fuse_rrf(...)` that memorum-eval will call identically. The query embed sits in *candidate collection*, outside any ranking-formula test (Stream E §8.2).
- **Acceptance:**
  - Fused delta surfaces a paraphrase hit that the FTS-only path misses (deterministic FixtureProvider fixture).
  - RRF order is deterministic; tie-break ends in lexicographic id.
  - With `recall.vector_recall.enabled = false`: behaves exactly as today (FTS-only), no provider call.

### Task 2.3 — Degradation ladder + response field

- **Owned files:** `crates/memoryd/src/recall/delta.rs`, `crates/memoryd/src/recall/types.rs` (the additive `DeltaResponse` field).
- **Spec invariants:** §5 #4, **#7 (the recovery side: Lane R catches `Err(UnknownEmbeddingTriple)` → `no_vector_table` marker mirroring `policy.rs:363-367`; any other `Err(_)` → `knn_failed` marker; both ⇒ FTS-only)**.
- **What to build:** copy the governance ladder shape (`policy.rs:310-388`) with the **SEVEN** stable string markers from §3 D3 (the seventh, `embedding_timeout`, is the D7 contingency rung). Add `DeltaResponse.vector_recall_degraded: Option<String>` with **`#[serde(default, skip_serializing_if = "Option::is_none")]`** — name both attributes; `types.rs:53-57` currently has no field-level serde attrs, and the healthy-path response must NOT emit a `null` field. Empty slot / no active triple / triple mismatch / embed failure / `UnknownEmbeddingTriple` / other KNN error / embed timeout ⇒ FTS-only + the corresponding marker, never an error.
- **Acceptance:** test **each of the seven rungs** sets its marker and falls back to FTS without error (the `embedding_timeout` rung tested via an injected deliberately-slow fake provider); `None` marker on the healthy fused path; the additive field round-trips through the protocol serializer with old clients unaffected (default-on-absence) AND is **absent** from serialized output on the healthy path (`skip_serializing_if`).

### Task 2.4 — `recall:` config knob

- **Owned files:** new config module file; `crates/memoryd/src/recall/**` wiring.
- **What to build:** the `recall.vector_recall { enabled, knn_limit, rrf_k }` block (D4), loaded with the dynamics-config pattern (`dynamics/mod.rs:44-126`), all fields defaulted. No env var at this layer.
- **Acceptance:** defaults apply when the block is absent; values thread to Task 2.2's fusion call; `enabled=false` disables the vector lane cleanly.

### Task 2.5 — Wire `memory_search` (`search_response`) to the hybrid surface

- **Owned files:** `crates/memoryd/src/handlers/memory_ops.rs` (`search_response`, `:80-117` — already inside Lane R's fence).
- **Ground truth:** `search_response` does NOT share the delta seam — it has its own handler and calls `query_chunks` text-only today. It is in this fence, so this lane ships the fix; it is not a follow-up.
- **Rationale:** System spec §11 already promises `memory_search` = "hybrid keyword + vector + recency search." Shipping hybrid recall while leaving `memory_search` FTS-only would widen a known spec gap.
- **What to build:** route `search_response` through the same `query_hybrid_chunks` surface + the shared `fuse_rrf` helper + the same degradation handling as delta (degrade ⇒ FTS-only). Surface the degrade marker only if the search-response shape has a natural home for it; otherwise log it — **do NOT invent response-shape churn** for `memory_search`. The recency term stays exactly as it is today (out of scope: if `memory_search` has no recency term today, this task does not add one).
- **Acceptance:** `search_response` uses the hybrid surface with a test (paraphrase hit the FTS-only path misses); every degrade rung falls back to FTS-only without error; no change to the search-response wire shape unless a marker field already had a home.

### Task 2.6 — Bench vector phase + threshold

- **Owned files:** `crates/memoryd/src/bin/stream_e_recall_bench.rs`; new results file `bench/stream-e-recall-results.<profile>.json.proposed` (`.proposed` convention — human promotion only, never auto-promoted).
- **Spec invariants:** §3 D7 (latency budgets binding; the `embedding_timeout` mechanism already ships in Lane R, so the bench *measures* it rather than introducing it).
- **Ground truth:** the bench currently calls the no-state `build_delta_response` at `stream_e_recall_bench.rs:151` (no provider) — the FTS-only path. That path stays unchanged; the vector phase is additive.
- **What to build — enumerate all three:**
  - (a) **A new provider-carrying bench call** to whatever entry Lane R ships (e.g. `build_delta_response_with_provider(substrate, request, Some(Arc::new(FixtureProvider::...)))` — FixtureProvider for bench determinism, real model behind a flag). The existing `:151` no-state call stays for the FTS-only phase.
  - (b) **A new `BenchReport` field `delta_with_vector_p95_ms`** alongside the existing per-phase p95 fields.
  - (c) **A new branch in `enforce_thresholds`** (release-only, same pattern as `:286-307`) gating `delta_with_vector_p95_ms` against the §13 delta budgets.
  - The bench therefore measures BOTH paths: FTS-only delta (unchanged) + the new vector phase.
- **Acceptance:** bench runs in release, emits the `.proposed` file with `delta_with_vector_p95_ms`, the new `enforce_thresholds` branch gates it against §13, and either it meets budget or the `embedding_timeout` path keeps p95 under budget (verified by the timeout test in Task 2.7); renegotiation numbers, if any, reported to the orchestrator.

### Task 2.7 — Tests (degraded paths + determinism)

- **Owned files:** `crates/memoryd/tests/` (new files).
- **Acceptance:** integration tests for the healthy fused path, every degrade rung, `enabled=false`, partial vector coverage end-to-end, and per-device byte-stability of the delta block given fixed FixtureProvider vectors.

- **Lane R per-lane gate (all of 2.1–2.7):** `cargo test -p memoryd --tests --no-fail-fast` + `cargo clippy -p memoryd --tests -- -D warnings` + `cargo fmt -p memoryd --check` + `RUSTDOCFLAGS="-D warnings" cargo doc -p memoryd --no-deps`. Bench is `cargo run --release --bin stream_e_recall_bench` (manual, results to `.proposed`).
- **Lane R report:** branch, files, gate output, the bench p95 numbers (fused vs FTS), the `memory_search` finding, and whether any §13 budget renegotiation is needed.

---

## Wave 3 — Lane E: eval vector lane (delegate codex work, worktree; depends on Lane R merged)

### Task 3.1 — Deterministically populate vectors in the eval scratch substrate

- **Owned files (fence):** `crates/memorum-eval/**` only.
- **Forbidden:** everything outside `crates/memorum-eval/`; `bench/quality-baseline.json` (the re-arm is Wave 4, human-only).
- **Context:** the eval scratch substrate's active triple defaults to `synthetic/stream-a-test/32` (`api.rs:2448-2462`); writes enqueue pending embedding jobs but **no worker runs** ⇒ vector tables empty. `FixtureProvider::synthetic_test_triple()` matches that default exactly. memorum-eval already depends on memoryd.
- **What to build:** populate the scratch substrate's vectors deterministically — either drain pending jobs through `FixtureProvider`, or call `update_embedding` directly per chunk with FixtureProvider vectors. Must be reproducible run-to-run.
- **Acceptance:** after population, the active triple's vec table is non-empty and KNN orders deterministically.

### Task 3.2 — Switch `rank_via_search` to the shared fusion helper

- **Owned files:** `crates/memorum-eval/src/quality.rs`.
- **Context:** `rank_via_search` (`quality.rs:284-300`) calls `query_chunks` text-only today. `ranking_lane` (set at `quality.rs:651`) is descriptive metadata only — the gate never reads it.
- **What to build:** `rank_via_search` calls Lane R's **exported `fuse_rrf` helper** over `query_hybrid_chunks` with the FixtureProvider-populated vectors — the *identical* fusion the production delta path uses. Provide a **side-report mode** (flag / `--output-file` without `--check`) that emits fused-lane metrics WITHOUT touching the armed `bench/quality-baseline.json` gate, so each prior wave can measure lift mid-development.
- **Acceptance:** side-report mode produces nDCG@5 / trap_rate@5 / abstention for the fused lane; the armed `--check` gate is untouched until the lockstep step (3.3).

### Task 3.3 — Update `ranking_lane` + `report_is_well_formed` IN LOCKSTEP with the re-arm

- **Owned files:** `crates/memorum-eval/src/quality.rs` (`ranking_lane` string), `crates/memorum-eval/tests/quality_baseline.rs` (`report_is_well_formed`, which hard-asserts 56 cases / 6 abstention / exactly 2 seams / abstention arithmetic).
- **Hard sequencing (atomic, see Wave 4 step 5):** these edits — together with Lane E's `rank_via_search` fused switch (Task 3.2) and the re-armed `bench/quality-baseline.json` — land as **ONE atomic `[bench-update]` trunk commit, landed last.** `compare_to_baseline` (`quality.rs:713-749`) keys per-seam (`search`, `startup`); ANY seam-set change vs the committed baseline trips the gate, and `quality_baseline.rs` panics on `Regressed` under `cargo nextest run --workspace` inside `check.sh`. So the fused `rank_via_search`, the new `search`-seam numbers, the `ranking_lane` string, and the well-formed assertion are inseparable — never on trunk apart from each other. Until that commit, the fused switch stays on Lane E's branch (held back in Wave 4 step 2); the side-report mode is what measures lift mid-flight.
- **Acceptance:** the `--check` gate is green against the re-armed baseline the moment all four land together; `--corpus-root` still works (refuses `--check`).

- **Lane E per-lane gate:** `cargo test -p memorum-eval --lib` + `cargo build -p memorum-eval --tests` + `cargo clippy -p memorum-eval --tests -- -D warnings` + `cargo fmt -p memorum-eval --check` + `RUSTDOCFLAGS="-D warnings" cargo doc -p memorum-eval --no-deps`. **Do NOT run the armed `--check` baseline gate inside the lane** — that's Wave 4.
- **Lane E report:** branch, files, gate output, and the side-report fused-vs-FTS numbers (nDCG@5, trap_rate@5, abstention) on the fictional corpus.

---

## Wave 4 — Orchestrator closeout (orchestrator-only)

> **🔒 Atomicity invariant (B1):** trunk must NEVER be in a state where the eval's `search` seam is fused but `bench/quality-baseline.json` still holds the FTS-only numbers. `scripts/check.sh` runs `cargo nextest run --workspace`, which executes `quality_baseline.rs` whose baseline gate **panics on `Regressed`** — so a fused `rank_via_search` on trunk against the old armed baseline fails the integrated gate deterministically. Therefore Lane E's `rank_via_search` switch + the `[bench-update]` re-arm + the `ranking_lane` string + `report_is_well_formed` updates land as **ONE atomic trunk commit**, and `scripts/check.sh` runs only AFTER that commit.

1. Cherry-pick **Lane S → Lane R** to trunk in dependency order; resolve `Cargo.lock` (orchestrator-merged; targeted `cargo update -p` + `cargo build --workspace --locked`, never `cargo generate-lockfile`). Run `bash scripts/check.sh` on trunk after this — it is still green because the eval `search` seam is still FTS-only (Lane E's switch is NOT yet on trunk).
2. From **Lane E's branch**, cherry-pick ONLY Task 3.1 (deterministic vector population) and Task 3.2's side-report machinery to trunk — but **hold back the `rank_via_search` fused switch** (Task 3.2's seam change) and Task 3.3's `ranking_lane` + `report_is_well_formed` edits. These three stay off trunk until step 5.
3. **Off-gate real-model measurement.** Run the quality runner with the real Qwen3 provider on (a) the fictional golden corpus and (b) the private corpus via `--corpus-root ~/.memorum/private-golden` (machine-local, NEVER committed, contents never named in any artifact). Use `--output-file` without `--check` — this measures lift, it does not arm anything. Also run the side-report fused metrics from step 2's machinery.
4. **Present numbers to Trey** (fused vs FTS: nDCG@5, trap_rate@5, abstention gap). Trey approves the new numbers before any re-arm.
5. **Single atomic human `[bench-update]` commit** that lands together, in one commit on trunk: (i) Lane E's `rank_via_search` fused switch, (ii) the re-armed `bench/quality-baseline.json` (the approved new `search`-seam numbers), (iii) Task 3.3's `ranking_lane` string, (iv) `report_is_well_formed`. Because all four move together, trunk never holds a fused seam against a stale baseline. `check-baseline-discipline.sh` (`scripts/check.sh:86`, regex `bench/quality-baseline.json`) requires the `[bench-update]` subject tag. **Human-authored only** — the harness never overwrites baselines (CLAUDE.md invariant 7, extended to the quality baseline by the dynamics-eval-hardening plan).
6. Run `bash scripts/check.sh` on trunk — the FIRST integrated run with the fused `search` seam, now green against the re-armed baseline.
7. Promote `bench/stream-e-recall-results.<profile>.json.proposed` → unsuffixed via a human commit if the latency numbers are accepted.
8. Run record in `docs/reviews/2026-06-10-vector-recall-fusion-run-record.md`.

---

## 8. Risk register

1. **Trap-rate regression.** Vector similarity surfaces semantically-adjacent-but-WRONG memories (superseded lookalikes, wrong-project entity collisions); `trap_rate@5` is in the armed gate. *Mitigation:* RRF dampens single-lane spikes (a vector-only false positive needs a high rank in only one list); traps are measured in the side report at every wave step, not just at re-arm. If trap-rate climbs, tune `rrf_k` / `knn_limit` (config, no code) before re-arming. Qwen3 was chosen partly for best trap-rate@5 (dynamics-eval-hardening Task 3.0).
2. **Latency bust.** Query-time embed + KNN may exceed §13 budgets. *Contingency (D7):* measure first (Task 2.6); if busted, skip the vector lane with an `embedding_timeout` degrade marker rather than blow p95, OR renegotiate the budget in the v0.6 amendment with measured numbers. Never a silent bust.
3. **Seam-set gate trip.** Changing the `search` seam's numbers trips `compare_to_baseline` immediately. *Mitigation:* the strict sequencing in Task 3.3 — `ranking_lane` + `report_is_well_formed` + baseline re-arm are ONE commit, landed last (Wave 4). All mid-development measurement is side-report only.
4. **Partial vector coverage.** Pending embedding jobs ⇒ some chunks lack vectors. *Mitigation:* §5 #5 — the hybrid surface tolerates one-lane-only hits; tested in Task 1.1 and 2.7. Eval populates fully (Task 3.1) so the gate isn't confounded by coverage gaps.
5. **Bench no-provider.** The recall bench and eval gate must run without a real model. *Mitigation:* FixtureProvider everywhere in gates/tests (D5); real-model runs are off-gate manual (Wave 4 step 3). `MEMORUM_DISABLE_EMBEDDING_WORKER` path stays exercised (the `None`-provider FTS-only path).
6. **Silent-fallback footgun.** `query_chunks` today silently degrades `vector=Some, triple=None` to FTS. *Mitigation:* the new `query_hybrid_chunks` takes both-or-neither and the recall ladder emits a marker on every degrade (§5 #4); a test asserts no silent FTS fallback without a marker.

---

## 9. Verification

- **Per-lane gates** (worker-run, in-worktree): the exact `cargo test -p <crate> --tests --no-fail-fast` + clippy `-D warnings` + fmt commands listed in each lane brief. Workers never run `scripts/check.sh`.
- **Integrated gate** (orchestrator, trunk-only): `bash scripts/check.sh` after each cherry-pick.
- **Eval sequencing:** (a) mid-development — fused lane measured via side report (`--output-file` without `--check`), armed gate untouched; (b) Lane E's vector population + side-report machinery cherry-pick to trunk WITHOUT the `rank_via_search` fused switch (held on the branch); (c) the fused switch + re-armed `bench/quality-baseline.json` + `report_is_well_formed` + `ranking_lane` land as ONE atomic `[bench-update]` commit (Wave 4 step 5) after Trey approves the numbers, and `scripts/check.sh` runs only after it.
- **Atomicity invariant (B1):** trunk is never in a state where the eval `search` seam is fused but the baseline holds FTS-only numbers — `check.sh`'s `cargo nextest run --workspace` runs `quality_baseline.rs`, which panics on `Regressed`. The four pieces in (c) are inseparable.
- **Re-arm protocol (human-only):** baselines are never overwritten programmatically (invariant 7). The orchestrator presents fused-vs-FTS numbers; Trey approves; one `[bench-update]`-tagged atomic commit carries the fused switch + re-arms `bench/quality-baseline.json`; the `.proposed` recall-bench results are promoted by a separate human commit (Wave 4 step 7).

---

## 10. Out of scope

- **Startup-seam vectors** — startup has no query text to embed; stays structural.
- **Reranker models** — no cross-encoder / second-stage rerank.
- **MMR / diversity / contextual retrieval** — no query rewriting, no diversity penalty.
- **Cross-triple mixing** — triple is identity; one ranked list never spans triples.
- **Dreaming / governance changes** — governance already consumes vectors via its own KNN seam; untouched here.
- **Persistent recall-count / last-recalled mutation** — a separate Stream E deferral (the dynamics plan), not this arc.
- **Dashboard / TUI visualization of vector-recall degradation** — stays deferred (Stream E §15 already defers recall-explanation dashboards). Verified: no `memoryd-tui` or `memoryd-web` code deserializes `DeltaResponse` today, so the additive `vector_recall_degraded` field is consumer-safe — nothing breaks by adding it. A **doctor advisory** for *persistent* vector-recall degradation (e.g. a chronically empty active-triple vec table) is a named follow-up, not in this arc.
