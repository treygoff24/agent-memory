# Adversarial Spec Review: Memorum system-v0.2.md

**Reviewer:** plan-reviewer subagent (Claude Sonnet 4.6)
**Date:** 2026-05-01
**Spec reviewed:** `docs/specs/system-v0.2.md`
**Companion specs consulted:** `stream-f-dreaming-v0.2.md`, `stream-g-observability-v0.1.md`, `stream-h-eval-harness-v0.1.md`, `stream-i-cross-session-v0.1.md`, `stream-e-passive-recall-v0.5.md`

---

## Blockers

**Blocker 1: §12.2 describes behavior that is explicitly deferred in the shipped Stream F contract.**

System-v0.2 §12.2 specifies that confidence-graded auto-promotion works as follows: a Pass-2 candidate scoring 0.85 or higher silently promotes to `active` state with the event logged; a candidate scoring 0.65 to 0.85 enters the review queue as `dream_low_confidence`; a candidate below 0.65 is dropped to `dropped_low_confidence`. The thresholds are configurable in `config.yaml` under `dreaming.promotion_thresholds`.

Stream F v0.2 §13 (the explicit deferrals section) says: "Pass 2 auto-promotion. Every Pass-2 candidate goes to the candidate queue under `dreaming-strict`. Auto-promotion bypasses the human/governance gate and is explicitly out of scope."

These two statements are irreconcilable. The shipped contract is the ground truth — system-v0.2's own header says so: "Stream A–F implementation contracts override this document on any conflict." So the behavior described in §12.2 does not exist anywhere in v1. The config keys `dreaming.promotion_thresholds.silent_min` and `dreaming.promotion_thresholds.review_min` are dead config space. The `dropped_low_confidence` field in cleanup reports is not populated because no threshold evaluation runs.

The revision goal entry for item 13 in system-v0.2 describes this addition as a clarification of what Stream F was always going to do. It was not. It specifies a feature that Stream F explicitly refused to implement. The spec needs to be corrected: §12.2 should say that Pass-2 candidates always go to the `candidate` queue under `dreaming-strict`, that auto-promotion is deferred to v1.x or v2, and that the threshold config block should be removed entirely. Until this is corrected, any Stream G or Stream H implementation that touches dream-promotion review paths will be built on a fiction.

**Blocker 2: Stream G §12.3 requires a Stream A schema migration that system-v0.2 §19 does not authorize.**

Stream G v0.1 §12.3 contains this requirement: "The index already carries most of these fields; the provenance source-count is the only field not currently indexed — Stream G must add a `source_count INTEGER NOT NULL DEFAULT 1` column to the `memories` table (additive migration, backward-compatible)."

This is a direct write to Stream A's `memories` table in the SQLite index. System-v0.2 §19 describes Stream G's topology as: owns `crates/memoryd-ui/` (new), CLI command additions, and the localhost HTTP server. No mention of index schema changes.

The issue is not whether the migration is backward-compatible. It is that the system spec's topology section does not record this cross-stream surface touch, and the CLAUDE.md critical invariants establish Stream A as a substrate that other streams cannot touch without explicit authorization. Any executor building from system-v0.2 §19 will not expect Stream G to alter `memories.source_count`. This also creates a test ordering problem: if Stream H runs against a daemon built before Stream G lands the migration, the `cross_source_corroboration` component of drift scoring always returns 0. The formula still computes; it just silently treats every memory as single-source. Reality-check scoring tests pass but measure the wrong thing.

Fix: system-v0.2 §19 Stream G topology must name this migration explicitly. Something like: "Stream G also adds a `source_count INTEGER NOT NULL DEFAULT 1` column to the `memories` SQLite table as the only authorized Stream A index touch in Stream G scope."

**Blocker 3: Stream I's required change to Stream E's config parser is underdescribed in system-v0.2 §19.**

Stream I v0.1 §8.2 specifies that `.memory-project.yaml` must support a `concurrent_session_mode` key with values `minimal`, `default`, and `collaborative`. System-v0.2 §15.2 and §15.7 both reference this key as part of the Stream I contract. System-v0.2 §19 says Stream I "cross-cuts Stream E's recall assembly module (additive surface only — Stream E's contract is preserved)."

The problem is that "additive surface only" does not mean the config parser updates itself. The pattern throughout this codebase is strict deserialization — unknown fields in config structures are rejected, not silently ignored. If Stream I adds `concurrent_session_mode` to `.memory-project.yaml` without explicitly updating Stream E's parser to accept it, every project that sets this key will get a deserialization error on daemon startup when Stream E reads the file.

System-v0.2 §19 needs to name this: Stream I's cross-cut of Stream E includes a specific parser change to accept the `concurrent_session_mode` field. Leaving it as "additive surface only" gives an implementing agent no signal that a parser update is required and sets up a silent runtime failure.

---

## Risks

**Risk 1: The Latin etymology in §22 is wrong, and the error will survive to public release.**

Section 22 says: "Memorum is Latin for 'of memory' / 'of memories' (genitive plural of `memoria`)." The genitive plural of `memoria` — a first-declension feminine noun — is `memoriarum`, not `memorum`. What `memorum` actually is: the genitive plural of `memor`, a third-declension adjective meaning "mindful of" or "remembering." So the name means "of the mindful ones" or "of those who remember," not "of memory."

The name is fine. The claim is wrong. It will appear in the public README, in documentation, and in any press coverage. The fix is straightforward: change "genitive plural of `memoria`" to "genitive plural of `memor` (an adjective meaning 'mindful,' 'remembering')." The resulting meaning — "of the mindful ones" — is arguably a better framing for an agent-memory tool than "of memory."

**Risk 2: Namespace clearance assertions in §22 are point-in-time claims, not verified facts.**

Section 22 states that as of 2026-05-01, `memorum` is unclaimed on crates.io, has no relevant npm collisions, has one abandoned 2018 PyPI package, and `github.com/memorum` is available. The domain check is explicitly "pending." These were checked once at spec-writing time. They are not a permanent state.

The risk for crates.io is low — preemptive squatting is rare in the Rust ecosystem. The risk for domain names is higher; domain speculators monitor GitHub activity and public spec writing. The spec should stop asserting these as current facts and instead record them as a pre-release checklist item: verify all namespace claims immediately before publishing, not at spec-writing time. The current phrasing will cause an executor to skip re-verification because the spec already says it is clear.

**Risk 3: Dogfood gate §20.5 is self-modifying and the pass criteria are partially subjective.**

Section 20.5 allows spec revisions during the dogfood window when structural problems emerge. Section 20.4 mixes objective criteria (eval harness 18/18 every day, no secret write reaches disk, dream pipeline 5 of 7 nights) with subjective ones ("Trey reports: 'I felt like memory was working, not getting in the way'"; "Cross-session peer-update fires at least once and is correctly framed as third-party"). Combined with the mutable spec, this means the effective gate is "Trey decides when it's done."

This is probably the right engineering call — the dogfood week is explicitly a learning loop, not a fixed regression suite. The risk is that the spec presents it as something more objective than it is. If the subjective items are acknowledged as "informational / influence spec revision, not blocking 1.0.0," the gate criteria become clearer and the mutable-spec clause becomes less alarming.

**Risk 4: Policy "additive only" in §18.4 is schema-additive but not behavior-additive for existing content.**

Section 18.4 says "old memories written under policy v1 read fine under policy v3 because v3 only added new gates, never tightened old ones in a breaking way." This is true from a parse-compatibility standpoint. It is not true from a governance standpoint.

When policy v3 adds a new gate, every memory in the system was promoted without ever passing through it. The `policy migrate --dry-run` command exists explicitly to surface "newly quarantined items." Those are items that were `active` under policy v1 and become `pending` under policy v3. An implementer who reads "additive only" will not expect existing promoted memories to change governance status. The spec should clarify that "additive only" means schema-additive, not outcome-stable for existing content, and that the `--dry-run` migration is load-bearing because behavioral changes for existing content are expected and intended.

**Risk 5: Tier 3 peer-updates effectively do not fire, but §15.2 creates an expectation that they do.**

The peer-update relevance gate in §15.3 scores candidates as: 0.5 × entity overlap + 0.3 × path overlap + 0.2 × topic similarity. The threshold is 0.6. At Tier 3, the daemon has no session context to compute `current_session.salient_entities`, `current_session.salient_paths`, or `current_session.recent_query_embedding`. Without those inputs, all three score components are zero. Nothing surfaces.

System-v0.2 §10.2 does say "Cross-session peer updates: not delivered" for Tier 3, which is accurate. The risk is that §15.2 says Level 2 (writes + candidates + notes via peer-update) is the default for all projects, and a reader encountering §15 before §10 will assume Tier 3 harnesses receive peer updates. The cross-reference from §15 to the Tier 3 exclusion in §10 is implicit. Stream I's implementation needs to detect Tier 3 sessions and skip the scoring gate entirely — not score to zero and log confusion. The spec should make this explicit in §15.2 or §15.7.

---

## Nits

Section 19's topology paragraph closes with: "Trunk gate (`scripts/check.sh`) runs once after all three streams merge, not per-stream-per-task." This is a build process decision, not a system contract. A reader consulting this spec 18 months from now does not need to know how the parallel streams were gated during development. Remove it from the spec or move it to the implementation plan.

Section 21.4's public README shape points to `docs/getting-started.md` as the starting point for documentation. That file is not defined or referenced anywhere in the system spec or any companion spec. Before the public release, it needs to exist or the README's first documentation link is a 404.

Section 20.6's release flow sequences tag → CI → publish public repo → publish crates. The dependency order matters: `Cargo.toml`'s `repository` field should resolve before crates.io publication. This means the public `memorum/memorum` GitHub repo must be created and the mirror must be pushed before `cargo publish` runs. The current sequence has them in the right order, but an executor running step 3 and step 4 in parallel (as the list format might suggest) will get a crates.io warning about an unresolvable `repository` URL.

---

## Cross-spec consistency findings

**Finding A: system-v0.2 §12.2 vs. stream-f-dreaming-v0.2 §13 — direct contradiction (see Blocker 1).** The most significant gap. System-v0.2 describes auto-promotion behavior that Stream F explicitly defers. The system spec is wrong on this point and must be updated before Stream G or Stream H build against it.

**Finding B: system-v0.2 §19 Stream G topology vs. stream-g-observability-v0.1 §12.3 — undocumented Stream A schema touch (see Blocker 2).** The stream spec requires a `source_count` column addition to the `memories` table. The system spec does not authorize this. Either the system spec must be updated to authorize it, or the stream spec must be revised to avoid it.

**Finding C: system-v0.2 §19 Stream I cross-cut description vs. stream-i-cross-session-v0.1 §8.2 — parser change not named (see Blocker 3).** System-v0.2 says "additive surface only." The stream spec requires a concrete parser update. These are not the same thing.

**Finding D: system-v0.2 §16.4 drift formula vs. stream-g-observability-v0.1 §5.1 — consistent.** Both documents define `cross_source_corroboration` as distinct `(harness, session_id)` pairs in the provenance chain, binary 0 or 1, threshold at 2. Formula weights are identical (0.35/0.20/0.20/0.15/0.10). No discrepancy.

**Finding E: system-v0.2 §14.1 nine-tool freeze vs. stream-g-observability-v0.1 §1.2 — consistent.** Stream G explicitly states "The nine tools are frozen for v1" and "does not add MCP tools." No conflict.

**Finding F: Stream H §10.3 open question on drift scoring observability creates a sequencing dependency on Stream G.** The eval harness spec notes that test 16 (reality-check drift scoring sanity) depends on Stream G's scoring response shape. With Streams G and H declared as parallel tracks in §19, test 16's implementation will need to be written twice — once against a mock, and once against the actual Stream G API after Stream G merges. The spec should acknowledge this dependency rather than implying complete independence.

**Finding G: system-v0.2 §15.5 presence heartbeat semantics vs. stream-i-cross-session-v0.1 — consistent.** Both specs agree on in-memory-only presence, no persistence across daemon restart, 5-minute staleness TTL, and 60-second heartbeat interval. No gap.

---

## Things checked and found correct

**MCP tool count and per-stream attribution.** The 9-tool table in §14.1 is accurate. Stream A/B contributed search, get, note; Stream C contributed write, supersede, forget; Stream E added startup as the 7th; Stream D added reveal as the 8th; Stream F added observe as the 9th. The per-stream attribution matches the shipping history in CLAUDE.md. The freeze contract ("new tools require v2.0.0") is unambiguous.

**`memory_subscribe` removal is clean.** The removal is stated in §1.2 (non-purpose item 6), §1.3 (anti-feature item 1), §14.2 (explicit removal with rationale), and §15 (replaced architecture). No leakage anywhere in the document. The rationale is correct: LLM agents only think inside the generation loop; mid-generation event delivery has no architectural surface in any current harness. The Level 3 presence heartbeats in §15.5 are not a violation of the anti-streaming anti-feature — they are request-response over the existing delta-block hook, not long-lived connections.

**Drift-risk formula ordering.** The weights sum to 1.0. The formula is monotone in the correct direction for all five components: higher staleness, lower recall frequency, lower corroboration, higher confidence decay, and higher sensitivity all increase the score. A 60-day-old, never-recalled, single-source memory scores approximately 0.63, which correctly ranks higher drift risk than a fresh, frequently-recalled, two-source memory scoring near zero.

**Tier 1 / Tier 2 / Tier 3 classification.** The tier policy is internally consistent. Tier 1 is Claude Code and Codex CLI, both of which ship session-start and pre-turn hook surfaces. Tier 2 (Factory, OpenCode, Cursor, OpenClaw) is correctly deferred to v2 with the right rationale. Tier 3 is correctly defined as raw MCP with no hooks, and its limitations are stated clearly in §10.2.

**Open threads resolution.** All six v0.1 open threads are closed without ambiguity: prospective memory deferred to v2 with the path reserved; eval harness is Stream H in v1; bootstrap UX fully specified with interactive-by-default wizard; policy versioning has the additive-only constraint; multi-user is post-v2; naming is closed. None of the six are left as TBD.

**Revision goal substantiation.** All 15 revision goal items in the v0.1 to v0.2 header are substantiated in the spec body. Each item that references a section resolves to content that matches the claim. This is better discipline than most system-level documents maintain.

**Apache 2.0 patent grant rationale.** The rationale in §21.1 is sound. The specific surfaces named as potentially patentable — masked synthesis with grounding rehydration, drift-risk scoring with cross-source corroboration weights, peer-update relevance gating with the entity/path/topic blend — are plausible candidates. The Apache 2.0 explicit patent grant defangs them on day one. MIT's absence of a patent grant is accurately noted.

---

_End of review. Three blockers require spec changes before Streams G/H/I begin implementation. The §12.2 auto-promotion contradiction is the most urgent — it will silently invalidate Stream G and Stream H work on dream-promotion review paths if not corrected first._
