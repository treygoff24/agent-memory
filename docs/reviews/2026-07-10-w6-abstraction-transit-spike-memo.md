# W6 spike memo — abstraction-only API-lane transit for sensitive memories

Status: memo only (plan §W6, r5), **revision 2**. No code; no fence changes without a ratified spec + Trey sign-off. Author: Fable coordinator; grounding research by a native Opus subagent (citations spot-checked against source). The plan-mandated Sol xhigh read (codex-80) returned a grounded DISSENT on r1; its corrections are folded in below and marked. Net effect: the case for the recommendation got *stronger*, but for partly different reasons than r1 argued.

## Question

Can a Stream-D-classified, transit-safe **abstraction** (W2's ≤8-word/≤120-char label) of a `confidential`/`personal` memory embed via the Gemini API lane while the memory body stays local — and should it?

## The two structural insights

**1. Vector opacity buys nothing — because the text transits in clear anyway.** (Corrected per Sol: r1 argued this via embedding inversion; that was uncited and unnecessary.) The embedding API receives the abstraction as **plaintext in the request body** (`crates/memoryd/src/embedding/api_provider.rs`), and the provider retains logs up to 55 days absent ZDR (`crates/memoryd/src/cli/config.rs:64`). So restatement, recovery, and retention collapse into one question: **is the abstraction text itself safe to send in clear?** What no content gate can close is **traffic analysis** — existence, count, and timing of API embeds for sensitive memories reveal *that* they exist. That residual survives every design below except "never" and "local-only."

**2. "Safe to send" is not a classification question Stream D is allowed to answer laxly.** (Sol's catch, accepted.) Stream D's posture is raise-only: caller metadata may raise but never lower sensitivity, and the classifier is expressly not an anonymization or compliance guarantee (`docs/specs/stream-d-privacy-v0.1.md`). An abstraction derived from a `personal` memory cannot silently become `internal` because its eight words contain no detectable PII — that is an **unauthorized declassification**. Any transit design therefore needs an explicit, separately ratified declassification/release policy (who authorizes, whether source namespace/sensitivity is inherited). Without one, either inherited sensitivity keeps every sensitive abstraction ineligible (defeating the feature), or standalone classification quietly downgrades (violating Stream D). This is the strongest single argument against Option 2 — stronger than anything in r1.

## The fence as shipped (what any change would touch)

Three enforcement layers plus a consent gate, all fail-closed:

1. Tier allowlist: `Sensitivity::api_lane_eligible()` = `Public | Internal` only (`crates/memory-substrate/src/model.rs:169-171`), from which the SQL predicates derive.
2. Enqueue fence: `reconcile_missing` holds unknown/missing/ineligible tiers local under `PlaintextOnly` (`crates/memory-substrate/src/index/vector.rs:98-109`).
3. Fetch-time check: the fence is enforced again at job fetch (T4.2 dogfood, `docs/reviews/2026-07-09-t42-api-lane-dogfood-notes.md:48`) — any relaxation must thread **both** layers or they disagree.
4. Consent: daemon refuses to start an API-lane worker without `api_embedding_consent` (`crates/memoryd/src/server.rs:180-190`); the consent copy **explicitly promises** "confidential/personal/encrypted content never leaves this machine" (`cli/config.rs:64`).

W2 §A3 applies the identical fence to the new abstraction/cue aux rows: under the API lane, sensitive memories get **no vectors of any kind**.

## Value — corrected from the plan's framing

The plan hypothesized a "possible privacy unlock" benefiting W3 merges and W4 fusion. The research corrects half of that:

- **W3 merge value is moot.** W3's candidate fence already excludes encrypted-tier memories structurally (no atomic encrypted-supersession primitive in Stream D v0.1; W3 spec §2). Sensitive memories are never merge candidates regardless of whether their abstractions embed. Abstraction transit buys W3 nothing.
- **W4 recall is the residual value, and it is smaller and more conditional than r1 claimed.** (Corrected per Sol.) Three compounding reductions: (a) the "28" figure is **held-local chunk jobs**, not unique memories — the unique-memory count is unmeasured; (b) sensitive records default `retrieval_policy.index_embeddings = false` (Stream A §6.2), so under W2 they get **no aux rows even on the local lane** — r1's "local lane gives all tiers aux vectors" claim was wrong except for explicit operator overrides; (c) `abstraction_compile` writes through governed supersession, which Stream D **refuses for encrypted records** — so most sensitive memories cannot even *acquire* an abstraction under current contracts.

So the honest statement of the prize: under today's ratified contracts, the recall gap Option 2/3 would close is **approximately zero by default**, becoming nonzero only for sensitive memories with explicit embedding-eligibility overrides once encrypted-supersession exists. The right denominator is post-W5 measured: unique sensitive memories that actually hold safe abstractions with eligibility overrides, weighted by query-level recall loss — not corpus share.

## Options

**Option 1 — Never (status quo; W2 §A3 as drafted).** Sensitive memories get no aux vectors under the API lane (and, by the `index_embeddings=false` default, none under the local lane either without an operator override). Zero new surface, zero consent change, zero traffic exposure. Cost: a recall gap that is approximately zero today and measurable after W5.

**Option 2 — Opt-in transit with an abstraction-only classification gate.** Requires, cumulatively: a new consent scope (`api_abstraction_transit_consent` — the existing consent copy makes a flat promise this feature would *break*: a consent **correction**, never an overload of the existing key); a **ratified declassification/release policy** (insight 2 — without it the abstraction-only classification is an unauthorized downgrade); a new third classification pass (W2 §A4's dual pass proves neither payload-alone safety); a NEW revocation trigger (W2's upgrade-revocation fires only on *rising* sensitivity — already-sensitive memories never trip it); and permanent acceptance of the traffic-analysis residual. Highest surface in the set.

**Option 3 — Local micro-lane for sensitive-tier abstractions.** Keep the API lane for eligible tiers; embed sensitive memories' abstraction/cue rows with a small local model. Bodies and labels never leave; no consent change; no traffic residual. On the **privacy axis** this dominates Option 2. (Qualified per Sol:) as a *system design* it is unproven — it needs per-row-kind active triples (unmodeled in W2), **a second query embedding and a second provider lifecycle** (W4's contract is one query embedding reused across all vector lanes, and the second local model attacks the API lane's core 11–17 MB footprint win), plus retrieval-quality parity from a micro-model. RRF's rank-space fusion makes the experiment *legal*, not successful — a footprint/latency/recall bake-off is a precondition, not a follow-up.

## Recommendation (revised with Sol's dissent folded in)

**Option 1 now; instrument through W5/W4; revisit only against post-W5 evidence.** Reasons: (a) under current contracts the recall gap any transit design would close is approximately zero by default (value section above) — there is nothing to buy yet; (b) Option 2 is **not viable as sketched** because it requires an artifact-declassification authority that contradicts Stream D's raise-only posture — it comes back on the table only if Trey ever ratifies an explicit release policy AND post-W5 measurement shows material query-weighted recall loss (Sol's point that a second local model could break the footprint objective, making transit comparatively attractive, is recorded and fair — hence "not viable as sketched," not "rejected forever"); (c) Option 3 remains the privacy-dominant direction but is an unproven two-provider/two-query-embedding design that attacks the W4 contract and the 11–17 MB win — bake-off before any spec.

Two cheap actions worth taking regardless:
1. W2-time: doctor's per-kind counts explicitly label the aux held-local number as "excluded from abstraction/cue recall lanes," so the Option-1 gap is operator-legible.
2. W5-time: record the actual unique-sensitive-memory aux exclusion count and (in W4 eval) sensitive-query recall deltas — the evidence this memo's revisit clause depends on.

## Fail-closed obligations for any future transit/micro-lane spec (expanded per Sol)

Artifact-level declassification authority (inheritance + who authorizes); row-kind-scoped eligibility persisted with content hash, classifier version, and consent version — never relax the global `Sensitivity::api_lane_eligible()` allowlist; audit **all four** provider ingress paths named in the API-lane plan (not just enqueue + fetch); revocation breadth: consent withdrawal, field re-mint, body/source reclassification, namespace/scope change, classifier-policy upgrade, lane switch, orphaned triples, queued work, in-flight batches; race guarantee: once revocation begins, no queued or concurrent request transits the old text; per-field batching: one unsafe cue must not leak via partial batches; Option-3 fallback: local-model failure degrades, never falls back to the API for sensitive rows; zero-request tests for every uncertainty state (classifier unavailable, consent absent/withdrawn, stale hash, unknown tier, revocation race).

## Interactions recorded

- The grounding→privacy catch-22 (docs/issues.md) is a **promotion**-side problem; abstraction transit neither relieves nor worsens it. Cross-referenced, not solved (plan language stands).
- W2 task 3's hardcoded-`Trusted` audit (`handlers/quarantine.rs:61-80`) is adjacent write-side privacy-composition work in the same blast radius; unchanged by this memo.
- Any future Option-2/3 spec must thread both enqueue and fetch enforcement layers and bump the Stream A spec (aux eligibility is now spec'd in W2 §A3).
