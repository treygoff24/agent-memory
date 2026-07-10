# T3.3 spec amendment DRAFT — APPLIED 2026-07-09 (Trey: "spec amendment approved")

> Applied to `docs/specs/stream-a-core-substrate-v1.1.md` (Amendments section) and pointed to from `docs/specs/system-v0.3.md` (embedding worker bullet). Kept for provenance.

Per repo rules ("Don't bump spec or plan versions without Trey's explicit ask"), this is the STAGED
amendment text. On approval, the block below is appended as a dated amendment to
`docs/specs/stream-a-core-substrate-v1.1.md` (following the 2026-06-09 amendment precedent at §20) and a
one-paragraph pointer is added to `docs/specs/system-v0.3.md`'s embedding section. No version bump: the
change is additive surface (a second registered provider string + two additive config keys + an additive
eligibility parameter); triple identity, no-silent-fallback, and all §10.2.2 behavior are unchanged.

---

## Amendment (2026-07-09): `gemini-api` embedding provider + API-lane privacy fence

**Touches:** §10.2.2 (registered provider strings), §20 #2 (embedding default unchanged — this adds an
opt-in alternative), config.yaml key registry.

1. **New provider string.** `gemini-api` joins `fastembed-candle` as a registered `provider` value in the
   embedding triple. Default API triple: `("gemini-api", "gemini-embedding-2", 768)` (dimension subject to
   the T4.1 bake-off; the triple literal, not this amendment, changes if it moves). Triple identity and
   typed-mismatch rules (§10.2.2 #6/#9) apply unchanged.
2. **Plaintext-eligibility fence (Stream A surface).** Embedding job fetch/count/reconcile surfaces take an
   `EmbeddingLaneEligibility` parameter: `AllTiers` (local providers) or `PlaintextOnly` (API providers).
   `PlaintextOnly` restricts to persisted sensitivity `public`/`internal` — `confidential`/`personal`
   rows (including masked `safe_body` projections, which keep their source tier) are never fetched for
   embedding and are reported separately as held-local jobs. Fail-closed: unknown tiers are held.
3. **Consent key.** Synced `config.yaml` gains optional `api_embedding_consent: bool` (absent = false).
   The daemon MUST NOT start an API embedding provider unless it is `true`; the CLI consent ceremony is
   the only writer. Unknown-key tolerance in `SyncedConfig` loading is load-bearing and now contractual.
4. **Credentials.** API keys live in device-local runtime state (env `MEMORUM_GEMINI_API_KEY` or 0600 key
   file), never in synced config — same rationale as device IDs (invariant 4).
5. **Non-goals.** No hot lane-swap (restart required); no cross-triple vector reuse; old triple tables
   remain until explicitly dropped (§10.2.2 unchanged).

---

Also staged under T3.3 (committed as regular docs, not spec-gated): `docs/runbooks/api-embedding-lane.md`
operator runbook and a `skills/using-memorum/SKILL.md` note covering the lane-switch command + consent.
