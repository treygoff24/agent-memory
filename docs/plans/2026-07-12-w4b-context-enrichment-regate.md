# W4b — Context-aware enrichment + four-lane re-gate (overnight autonomous run)

**Status:** r2 — Sol xhigh review findings applied (5 blockers, 4 risks, 1 nit; all accepted, see revision history).
**Executor:** Claude (session coordinator), autonomous overnight run authorized by Trey 2026-07-12 ("I'll leave you to it overnight to run it and fiddle and we'll know for sure by morning").
**Parent arc:** `docs/plans/2026-07-10-memora-lessons-memorum-upgrades.md` (W4 gate escalation), `docs/reviews/memora-arc/w4-eval-gate-results.md`, `docs/reviews/memora-arc/trey-decision-packet-2026-07-12.md`.
**Decision context:** Trey chose W4 disposition (a) — merge dark — *plus* settling the aux-input-quality hypothesis empirically now, rather than deferring to W5.

## The question this answers

W4's honest gate failed: best four-lane config 0.658 vs legacy-fusion-on-identical-vectors 0.700
(dev split, pinned judge). The stated hypothesis was "the aux inputs are the bottleneck, not the
fusion code." **This plan tests that hypothesis to a pre-registered verdict.**

### Why "just re-run the sweep with the dream pipeline" is a placebo

The W4-prep enrichment sweep and the production dream `abstraction_compile` job share the same
single-shot, per-item generation shape (parent plan, W2 deliverable 6 — "single generation
mechanism… production parity by construction"; the prompts differ only in fence and summary
sourcing — compare `crates/memorum-eval/src/enrichment.rs:313` with
`crates/memoryd/src/dream/abstraction_compile.rs:162`). Re-running generation through the dream
code path would reproduce the same quality.

### The real quality levers (what Memora actually does differently)

Memora's reference implementation (parent plan §Memora lessons) mints its retrieval key from
**conversational context** with consolidation (merge-on-write; 344 entries where Mem0 stores 651).
Our sweep abstracts each turn **in isolation**: an 8-word abstraction of `"Speaker A: hello"` is
pure aux-lane noise, and the W4 churn data (+183 gained / −186 lost at top-K) shows aux noise
displacing judge-critical chunk hits. Two levers, both context-shaped:

1. **Context-aware generation** — generate abstraction/cues for a turn *given its session
   transcript*, so the abstraction captures what the turn contributes to the conversation
   (salient fact, stated preference, event + date), not a compression of its surface text.
2. **Selectivity** — low-signal turns (greetings, acks, filler) get **no abstraction at all**
   (`abstraction: null`, no cues). Aux lanes should be sparse and high-precision. The null path
   already exists end-to-end: the sidecar persists explicit nulls (round-2 F4 contract,
   `enrichment.rs:327-332`) and ingestion forwards them as no-abstraction.

W4b = build enrichment **v2** with those two levers, sweep the benchmark corpus, re-run the
pre-registered A/B. Either four-lane earns its flag or the hypothesis is dead and we know.

## Pre-registered decision rule (written before any v2 number exists)

**Definitions.** "Paired delta" = mean per-item judge-score difference over the **identical scored
item set** of the two arms being compared. `judge_mean` silently omits judge failures
(`benchmark.rs:826-828`), so arms may score different item subsets; every comparison below is
recomputed over the intersection of successfully judged items, with the intersection size and
per-arm judge-error counts reported. An arm with >5 judge errors (of 120) is a failed run — rerun
it once; if it fails again, the night ends inconclusive.

1. **Legacy invariance control (corpus attribution).** v2 enrichment can change the *corpus*, not
   just the aux lanes: abstraction/cues participate in write-time privacy classification
   (`handlers/governance/meta.rs:370-381`), so different generated content can shift refusals,
   encryption tier, and promotion. Before any treatment arm:
   - Run arm L (gemini-api + legacy fusion, dev) on the **v2** corpus.
   - Compare against the v1 arm-L reference on: dataset sha256s + selected item IDs (must be
     identical), governance/ingestion disposition counts, and per-item retrieved-context sets.
     If `/tmp/memora-eval/dev-legacy-api.json` (v1 artifact) no longer exists, re-run arm L on
     the v1 sidecars first — same night, before anything else.
   - **Retrieved-context sets identical** → attribution holds; proceed. `judge_mean` delta vs the
     v1 reference acts only as a secondary drift alarm (|Δ| > 0.02 → note it; judge drift affects
     both arms of tonight's A/B equally, so proceed, but flag it in the results doc).
   - **Retrieved-context sets differ** → STOP. v2 changed the effective legacy corpus; any
     four-lane comparison would be confounded. Diagnose, report inconclusive; no treatment arms.
2. **Dev gate:** best four-lane dev config must beat the same-night arm-L-on-v2 control by a
   **paired delta ≥ +0.01**. Sweep budget: **≤3 configs** — Memora defaults (1.0/1.0/2.0/1.0)
   first, then at most 2 informed adjustments. Holdout untouched during the sweep.
   - Paired delta in (0, +0.01) → **ambiguous**: flag stays dark, result goes to Trey in the
     morning summary. No further runs.
   - Paired delta ≤ 0 → hypothesis fails; skip to T5.
3. **Holdout freeze (only on an unambiguous dev win):** freeze the winning config; enrich holdout
   blind (T2 step 5 — prompt already frozen); run holdout once per arm (frozen four-lane, arm L).
   Four-lane passes iff holdout **paired delta ≥ 0** and neither dataset (LoCoMo, LongMemEval)
   regresses by more than 0.05 vs arm L on holdout (collapse guard, not a noise gate — at n=60
   per dataset, 0.05 ≈ 3 items flipping; anything bigger is structural).
4. **Outcomes:**
   - **Pass** → flip `four_lane_enabled` default to `true`, update the pinning test, integrate to
     main, record results.
   - **Fail or ambiguous** → flag stays dark (already merged that way by T0), write the closeout
     with numbers. A negative verdict is a fully acceptable outcome — the arc's chunk-vector win
     (arm L, +0.087 over FTS) is already banked.
5. No fourth config, no post-hoc metric switches, no holdout re-rolls, no reusing tonight's
   numbers to justify a different rule. Anything the rule doesn't cover goes to Trey in the
   morning summary, not into another eval run.

**Holdout framing (honest version):** holdout has been *scored* before (baseline₀ covered both
splits) but never used to select a config. Tonight it stays that way: no holdout entry is read,
enriched under a tunable prompt, or scored until the dev gate passes with the prompt frozen.

## Tasks

### T0 — Integrate W4 dark onto main

The W4 worktree branch `delegate/codex-20260711T155101Z_6f69ac` already carries the staged
dark-merge commit (`6843708` — `four_lane_enabled` default `false`) plus the `--w-*` runner
overrides (`47b945e`). Merge-base is `2a2fa43`; main has since advanced (docs + `e341a62`, which
the branch carries as cherry-pick `6c36e15` — rebase will drop the duplicate).

1. Rebase the branch onto `main` (in the worktree; never force-push anything).
2. Verify the pinning test asserts default-off; verify `memoryd recall`/search behavior is
   byte-identical to legacy with the flag off (the branch's own W4 review covered this; spot-check
   the flag plumbing after rebase).
3. Fast-forward `main`, then run the one blessed full gate: `bash scripts/check.sh` on the
   integrated trunk. Known-flaky bench-regression stage: apply the 3-run evidence rule from
   project memory before treating a trip as real.
4. Commit checkpoint. **Gate: check.sh green (modulo the documented flaky stage).**

### T1 — Enrichment v2: context-aware + selective generation

All changes scoped to `crates/memorum-eval` (leaf crate; inner-loop gates are crate-scoped).
No spec surface: the sidecar is eval-harness-internal; the abstraction/cues frontmatter contract
(lengths, counts, privacy composition) is unchanged.

1. **Generation switch.** A `Generation::{V1, V2}` enum threaded through `EnrichmentOptions` and
   `BenchmarkConfig`, exposed as `--generation v1|v2` on both `enrich_runner` and
   `benchmark_runner` (default `v1`). It selects prompt, keying, sidecar path, output validation,
   and failure semantics together — one switch, no mixed states. The benchmark report stamps the
   generation into `split_config`.
2. **Context enumeration.** Extend corpus enumeration (`benchmark.rs:sampled_corpus_bodies`)
   with a v2 variant yielding `(dataset, corpus_instance_id, session_id, target_ordinal,
   session_turns, body)` — sessions are already in hand at enumeration time (`locomo_sessions` /
   `longmem_sessions`). `corpus_instance_id` = LoCoMo `sample_id` / LongMemEval `question_id`.
3. **v2 sidecar keying.** v1 keys (`sha256(body)`) collide across contexts: LoCoMo reuses
   `session_1…` labels across conversations, LongMemEval builds sessions per question item, and
   repeated turn text ("hello") is common. v2 keys are `sha256` over the **length-prefixed**
   tuple `(corpus_instance_id, session_id, target_ordinal, body)` — computed identically by the
   producer and by ingestion (`benchmark.rs:ingest_sessions` has all four in scope; thread them
   to the per-turn lookup at `benchmark.rs:515`). The enumeration deduplicates pending work by
   key before scheduling, so async completion order can never race two writers of one key.
   Tests: reused `session_1` labels across conversations get distinct keys; duplicate bodies
   within one session at different ordinals get distinct keys; producer/consumer key parity.
4. **v2 sidecar provenance.** v2 writes `<dataset>.enrichment.v2.json` (v1 sidecars never
   touched — the invariance control needs them) with a header block:
   `{generation: "v2", prompt_sha256, window_policy: "w4b-r2", dataset_sha256, entries: {…}}`.
   Resume **refuses** on any header mismatch — a prompt edit can never silently mix generations
   in one sidecar. Benchmark artifacts record the same header values.
5. **Deterministic context windowing** (golden-tested, no judgment calls at runtime):
   - Render the session transcript as `speaker: text` lines in turn order.
   - If the full transcript ≤ 6,000 chars (`str::len` bytes on the rendered UTF-8, cheap and
     deterministic), use it whole.
   - Else: start from the target turn (always included whole, even if it alone exceeds the cap)
     and add neighboring turns alternately before/after, whole-turn granularity, until adding the
     next turn would exceed 6,000 chars.
   - Prompt layout: `BEGIN_CONTEXT … END_CONTEXT` (the window) and `BEGIN_TARGET … END_TARGET`
     (the target turn, repeated verbatim) — both fenced as data; the v1 injection-fence
     discipline extends to the transcript.
   - **Session-date bodies** (`benchmark.rs:907` — synthetic "Dataset session X occurred at…"
     sentences) are enumerated but not conversational turns: v2 assigns them a deterministic
     null entry (`abstraction: null`, cues `[]`, disposition `date_metadata`) with no harness
     call. Chunk/BM25 lanes already index them; dates are metadata, not salient facts —
     pre-registered here so it can't become a post-hoc knob.
6. **v2 prompt.** Instruct:
   - The context is the conversation; the TARGET turn is the item to enrich.
   - Abstraction (≤8 words): the durable fact/preference/event this turn contributes to the
     conversation — entity-anchored, not a paraphrase of the turn's surface text.
   - Cues (0–3, 2–4 words, `[Main Entity] + [Key Aspect]`): phrases a future question about this
     fact would plausibly contain.
   - **If the turn contributes no durably recallable content (greeting, ack, filler,
     conversational glue): return `{"abstraction": null, "cues": []}`.**
7. **v2 output validation + failure semantics.** v2 gets its own output type accepting
   `abstraction: null` (with `cues == []` **required** when null → disposition
   `skipped_low_signal`); the v1 required-string validator is untouched. **v2 has no structural
   fallback** — structural output is exactly the isolated-surface-compression noise the
   experiment removes; letting timeouts mint it would reintroduce the mechanism under test and
   make a negative verdict uninterpretable. Timeout/auth/exit failures leave the item pending
   (retried on resume). Circuit breaker: ≥50% failures across 3 consecutive batches → abort the
   sweep run (resumable), log, re-check codex health. **Eval eligibility bar: 100% of enumerated
   items resolve to validated harness output or a deliberate null (`skipped_low_signal` /
   `date_metadata`). Anything less → the night is inconclusive; no gate runs.**
8. **Tests** (crate-scoped): keying (per step 3); null persisted + forwarded to ingestion as
   no-abstraction; null-with-cues rejected; windowing goldens (small session whole, large session
   window, oversized target); date-body null disposition; provenance-mismatch resume refusal;
   v1 paths byte-identical (v1 sidecar untouched by a v2 run; v1 validator still rejects null).
9. **Gates:** `cargo check -p memorum-eval`, `cargo clippy -p memorum-eval --all-targets -- -D
   warnings`, `cargo test -p memorum-eval -- --test-threads=2`. Commit checkpoint.

### T2 — v2 enrichment sweep (dev first; holdout only behind the dev gate)

`enrich_runner` gains `--split dev|holdout|both` (enrichment currently hardcodes both —
`enrichment.rs:143-146`). Holdout enrichment happens **only** after the prompt is frozen and
**only** if the dev gate passes — this both prevents adaptive holdout leakage and saves ~3h when
the dev gate fails.

1. Preflight (also run before Trey leaves — see Runtime prerequisites).
2. **Dev pilot (~200 items):** run `--generation v2 --split dev --limit 200` under `caffeinate
   -imsu`, log to `/tmp/w4b-enrich-v2.log`. Inspect ~20 random entries: null rate expected
   roughly 20–60% on chit-chat-heavy LoCoMo; **null rate <5% or >80% → fix the prompt, trash the
   entire partial v2 sidecar set (provenance header makes a stale mix impossible to miss, but
   don't rely on it), restart the pilot.** Also check abstractions are entity-anchored facts,
   not turn paraphrases — a v2 that reads like v1 is not worth 6 more hours.
3. **Freeze:** record the prompt sha256 + window policy in the results doc. After this point the
   prompt does not change; a post-freeze prompt edit ends the night inconclusive (rule 5).
4. **Full dev sweep:** `--generation v2 --split dev` (~4.3k items; v1 did 8.6k in ~4h at 8-wide,
   v2 prompts are larger — budget 3–5h for dev). Existing batching (8-wide, per-batch atomic
   persistence) makes this resumable. On transient codex outages (`harness:exit_1` clusters —
   seen in W4-prep run 3): wait, verify codex is alive, relaunch; resume is free and retries
   pending items (no structural pollution, per T1 step 7).
5. **Holdout sweep (deferred):** runs inside T4 step 1, blind — no holdout entry is read or
   inspected; only the aggregate disposition counts are checked against the eligibility bar.
6. Record disposition counts + null rate per split. Eligibility bar per T1 step 7 before any
   eval run consumes a split's sidecar.

### T3 — Re-gate (dev)

All runs: pinned judge (`scripts/eval/pinned-judge.sh` via `--judge-command`, frozen 2026-07-10 —
**do not modify**), `--locomo-qa-per-conversation 12 --longmemeval-per-split 60`, `--split dev`,
`--generation v2`, artifacts to `/tmp/memora-eval/` (~40 min each):

1. **Legacy invariance control** per decision rule 1 (arm L on v2 corpus; v1 reference from the
   existing artifact or a same-night v1 re-run if `/tmp` was cleared). STOP on retrieved-context
   divergence.
2. **Arm F-v2:** `--fusion four-lane` at Memora defaults → `dev-fourlane-v2.json`.
3. **≤2 informed weight adjustments** via `--w-chunk/--w-bm25/--w-abstraction/--w-cue`, chosen
   from the per-item win/loss/churn breakdown of run 2 (e.g., if abstraction-lane precision is
   now high but the cue lane still churns, drop cue weight). Every config's weights are stamped
   by the runner (`split_config.fusion_weights`).
4. Apply decision rule 2 (paired-delta over identical item sets, ambiguity band). Log per-dataset
   means and paired W/L/T vs the control in the results doc as they land.

### T4 — Holdout freeze (only on an unambiguous dev win) + disposition

1. Enrich holdout blind (T2 step 5; eligibility bar applies).
2. `--split holdout` for the frozen four-lane config and arm L (2 runs). Apply decision rule 3.
3. **Pass:** flip `four_lane_enabled` default to `true` + update the pinning test + targeted
   crate gates + `bash scripts/check.sh` on main + commit.
4. **Fail/ambiguous:** no code change (flag already dark on main from T0).

### T5 — Closeout (all outcomes, including inconclusive)

1. `docs/reviews/memora-arc/w4b-results.md`: protocol, prompt sha256 + window policy,
   disposition/null-rate stats per split, every arm's numbers with intersection sizes and
   judge-error counts, the pre-registered rule applied verbatim, verdict, and the fold-back
   note — **if v2 context+selectivity won, the same strategy must be folded into the production
   `abstraction_compile` job** (production parity cuts both ways; that's a follow-up wave tied to
   the B3 decision, not this one).
2. Update BUILD-STATE task ledger + CLAUDE.md current-status pointer; write the memory note;
   append the model-performance journal entry for the sweep's codex usage if anything notable.
3. Commit everything (docs + artifacts summary; raw 6MB JSONs stay in /tmp, summarized like W4).
4. Morning summary for Trey per the final-message readability rule: verdict first, numbers,
   anything ambiguous flagged for his call. **No pushes** regardless of outcome.

## Overnight schedule + hard cutoff

Priority order is the task order; the verdict-critical path is T0 → T1 → T2(dev) → T3. Rough
budget: T0 ~1h (check.sh dominates) · T1 ~1.5–2h · T2 dev ~3–5h · T3 ~2–2.5h → the dev verdict
lands within ~8–10h even at the slow end. T4 (+~5h) only exists on a dev win and may finish
after morning — that's fine; the dev verdict is the night's deliverable.

**Hard cutoff: 07:30 local.** At cutoff, stop launching new work, leave everything resumable
(sidecars + artifacts are), and write the morning summary from whatever stage was reached.
Never: compress or skip a control, raise concurrency beyond 8, inspect holdout entries, or relax
the eligibility bar to force a verdict before cutoff. A truthful "inconclusive, resumable at
step X" beats a gamed number.

## Runtime prerequisites (verify while Trey is still here)

- `codex` CLI authenticated (enrichment generator + pinned judge both ride it).
- `GEMINI_API_KEY` in the session environment (vector arms; scaffolds write it via
  `write_gemini_api_key`).
- `datasets/` populated (gitignored — cannot be re-cloned); v1 sidecars present.
- `/tmp/memora-eval/dev-legacy-api.json` still present (v1 arm-L reference) — if missing, add
  one v1 arm-L re-run to the T3 budget.
- ≥20 GB free disk (scaffold daemons + artifacts), machine on AC power, `caffeinate` available.

## Safety rails (standing orders, restated for the overnight run)

- **Never touch live `~/memorum`** — this plan is benchmark-corpus-only. The live repair pass and
  B3 are separate decisions and explicitly out of scope tonight.
- **No pushes, no PRs, no spec version bumps.** Local commits liberal, at every checkpoint.
- CPU discipline per CLAUDE.md: crate-scoped gates inner-loop; `scripts/check.sh` only on
  integrated main at T0 and T4, never mid-task.
- Pinned judge and `bench/baseline.*.json` are frozen artifacts — read-only.
- Blocked >30 min on one root cause → write
  `docs/plans/2026-07-12-w4b-context-enrichment-regate-execution-log.md` (blocker, tried,
  would-unblock) before any further retry. Stuck twice on the same problem → stop grinding,
  leave it for the morning summary; do not improvise around a wall at 4am.
- Holdout runs only as specified in T4.

## Explicit non-goals

- No changes to production recall, dream, or substrate code beyond T0's already-reviewed merge
  and T4's flag flip. Enrichment v2 lives in the eval crate.
- No B3 mechanism work, no live backfill, no live repair pass.
- No holdout-informed weight or prompt tuning, ever.
- No attempt to also fold v2 generation into `abstraction_compile` tonight — that's a reviewed
  follow-up wave if v2 wins.

## Plan revision history

- r1 (2026-07-12): initial draft.
- r2 (2026-07-12): Sol xhigh review (codex-87, safe, 5m31s) — all 10 findings accepted.
  Blockers: (1) v2 key now length-prefixed `(corpus_instance_id, session_id, target_ordinal,
  body)` with pending-work dedup — LoCoMo reuses session labels across conversations;
  (2) holdout-leak fix — `--split` on enrich_runner, dev-only pilot, prompt frozen + hashed
  before blind holdout enrichment, holdout deferred behind the dev gate; (3) v2 structural
  fallback removed — failures stay pending, circuit breaker, 100% eligibility bar or
  inconclusive; (4) attribution control upgraded from score-tolerance to corpus-invariance
  (retrieved-context set identity + governance dispositions; enrichment feeds write-time privacy
  classification); (5) decision rule hardened — paired deltas over identical item sets, ±0.01
  ambiguity band, holdout delta ≥ 0 required, judge-error caps. Risks: Generation enum with
  separate v2 validator; sidecar provenance header with resume refusal; deterministic windowing
  spec + date-body policy; hard 07:30 cutoff with priority order. Nit: "same generation
  mechanism" → "same single-shot, per-item generation shape".
