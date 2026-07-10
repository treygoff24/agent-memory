# W6 spike memo — abstraction-only API-lane transit for sensitive memories

Status: memo only (plan §W6, r5). No code; no fence changes without a ratified spec + Trey sign-off. Author: Fable coordinator; grounding research by a native Opus subagent (citations spot-checked against source); Sol xhigh read to follow.

## Question

Can a Stream-D-classified, transit-safe **abstraction** (W2's ≤8-word/≤120-char label) of a `confidential`/`personal` memory embed via the Gemini API lane while the memory body stays local — and should it?

## The one structural insight

**Vector opacity buys nothing.** A 768-dim embedding of a ≤120-char string is approximately invertible (vec2text-class attacks), and the provider retains logs up to 55 days absent ZDR (`crates/memoryd/src/cli/config.rs:64`). So "we only send the vector's source text" and "we send the text" are the same threat, and three of the five leakage classes (restatement, inversion recovery, provider retention) collapse into a single question: **is the abstraction text itself safe to send in clear?** That is a text-classification problem, answerable by Stream D. What no content gate can close is **traffic analysis** — the existence, count, and timing of API embeds for sensitive memories reveals *that* they exist. That residual survives every design below except "never" and "local-only."

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
- **W4 recall is the real value, and it is API-lane-conditional.** Under the local lane, all tiers get abstraction/cue vectors and nothing is excluded. Only when the API lane is active (the 11–17 MB deployment we actually want to run) do sensitive memories become invisible to W4's two new vector lanes, surfacing only via BM25/metadata. Live scale: **28 held-local memories** out of 786 active.

So the honest statement of the prize: *prevent the low-footprint API lane from making ~3.6% of the corpus second-class in recall.* Real, but modest.

## Options

**Option 1 — Never (status quo; W2 §A3 as drafted).** Sensitive memories get no aux vectors under the API lane. Zero new surface, zero consent change, zero traffic exposure. Cost: the documented W4 recall gap, which exists only under the API lane.

**Option 2 — Opt-in transit with an abstraction-only classification gate.** New consent scope (`api_abstraction_transit_consent` — the existing consent copy makes a flat promise that this feature would *break*, so it is a consent **correction**, never an overload of the existing key). New third classification pass — W2 §A4's dual pass classifies combined and body-only payloads, **neither proves the abstraction alone is safe**; the gate is abstraction-only ∈ api-eligible while the body is sensitive. Mandatory NEW revocation trigger: W2 §A3's upgrade-revocation fires only when sensitivity *rises into* sensitive — already-sensitive memories never trip it, so consent withdrawal and unsafe re-mints need their own deletion path. Accepts the traffic-analysis residual permanently. Highest value, highest surface.

**Option 3 — Local micro-lane for sensitive-tier abstractions.** Keep the API lane for eligible tiers; embed sensitive memories' abstraction/cue rows with a small local model. Bodies and labels never leave; W4 still sees every memory; no consent change; no traffic residual. On the privacy axis this **dominates Option 2**. Hidden cost: W2 models one active triple across all row kinds (§A3 triple-switch language) — a split lane needs **per-row-kind active triples**, an unmodeled substrate change; and mixing embedding spaces across row kinds is exactly what RRF fusion tolerates (rank-space, not score-space), so W4 survives it — but that claim needs a bake-off before anyone builds it.

## Recommendation

**Option 1 now; Option 3 as the design direction if sensitive-recall parity ever matters; reject Option 2 outright.** Reasons: (a) Option 2's entire benefit is 28 memories' membership in two of four RRF lanes, purchased with a consent-promise correction, a new classification pass, a new revocation subsystem, and a permanent traffic side-channel — the surface-to-value ratio is wrong by an order of magnitude; (b) Option 3 delivers the same recall parity with strictly less exposure and no consent change, and its prerequisite (per-row-kind triples) is honest substrate work that also unblocks future model-per-kind tuning; (c) the gap Option 1 leaves is self-documenting — doctor already counts held-local jobs per kind (W2 §A3), so the cost stays visible rather than silent.

One cheap W2-time action is worth taking regardless: have doctor's per-kind counts explicitly label the aux held-local number as "excluded from abstraction/cue recall lanes" so the Option-1 gap is legible to an operator, not just to us.

## Interactions recorded

- The grounding→privacy catch-22 (docs/issues.md) is a **promotion**-side problem; abstraction transit neither relieves nor worsens it. Cross-referenced, not solved (plan language stands).
- W2 task 3's hardcoded-`Trusted` audit (`handlers/quarantine.rs:61-80`) is adjacent write-side privacy-composition work in the same blast radius; unchanged by this memo.
- Any future Option-2/3 spec must thread both enqueue and fetch enforcement layers and bump the Stream A spec (aux eligibility is now spec'd in W2 §A3).
