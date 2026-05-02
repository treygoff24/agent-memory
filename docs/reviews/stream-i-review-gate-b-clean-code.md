### Verdict

Changes requested

### Intended outcome

Stream I Gate B appears intended to validate the new `memorum-coordination` crate after Tasks 4-8: the workspace skeleton, relevance scoring, `SessionContext` entity/path derivation, and non-blocking recent-query embedding cache. The crate should provide clean boundaries for later memoryd/Stream E integration without a governance dependency, while preserving the exact Stream I score, tier, recency, cap, cooldown, path/entity derivation, and embedding-triple semantics.

### Executive summary

The core score math, Tier 3 short-circuit, local-observed recency check, cap ordering, cooldown, and embedding-triple mismatch behavior are implemented and covered by targeted tests. The crate also has no direct `memory-governance` dependency, and the required test/clippy/fmt gates pass. I would not advance Gate B yet because two public-contract details are likely to break downstream integration: `PeerUpdateEntry.reference` is populated with the repository path instead of the memory id required for `<ref>`, and `ConcurrentSessionMode` names the Level 2 mode `Candidates` even though the project-binding/config contract is `default`. There are also meaningful test gaps in the presence/claim-lock files included in the requested gate, but those appear to be later-task placeholders rather than Gate B logic.

### Findings

[Medium] [API Contract] `PeerUpdateEntry.reference` carries the path, not the memory id

- Evidence: `crates/memorum-coordination/src/gate.rs:163-174` builds `PeerUpdateEntry { reference: candidate.row.path.as_str().to_string(), ... }`; `crates/memorum-coordination/src/protocol.rs:22-34` defines this DTO as the data needed to render `<peer-update>`. The Stream I spec says the `<ref>` child is the memory id (`mem_...`) or substrate fragment id (`sub_...`), while namespace/path is carried separately.
- Why it matters: Stream E will render or consume a `<ref>` that points at `project:.../file.md` instead of `mem_...`, breaking traceability back to the memory id and making downstream claim-lock/contention/audit logic harder or impossible to join correctly.
- Reasoning: The candidate already carries both `memory_id` and `row.path`; the current implementation chooses the namespace path for `reference` and separately sets `namespace`, so the DTO loses the canonical id at the exact boundary where later XML rendering needs it. The tests only recover ids from the synthetic summary text, so they do not catch this contract error.
- Recommendation: Populate `PeerUpdateEntry.reference` from `candidate.memory_id.to_string()` for memory candidates, and add/adjust a Gate B test that asserts `peer_updates[0].reference == <memory_id>` while `namespace` remains the namespace/path context expected by the renderer. If substrate observations need `sub_...`, model that explicitly rather than overloading `row.path`.
- Confidence: High

[Medium] [API Contract] Coordination mode enum does not match the locked `default` value

- Evidence: `crates/memorum-coordination/src/session.rs:6-12` defines `ConcurrentSessionMode::{Minimal, Candidates, Collaborative}`. The Stream I spec and Task 3/8.2 contract use project-binding values `minimal`, `default`, and `collaborative`; the mapping table says `default` maps to Level 2.
- Why it matters: This crate exposes `ProjectBinding.concurrent_session_mode: Option<ConcurrentSessionMode>` as the coordination-side type. If memoryd's parser or future serde boundary maps the locked string `default` to a different enum (`Default`) while this crate expects `Candidates`, integration will require ad-hoc translation or will silently diverge from the public config vocabulary.
- Reasoning: The plan intentionally treats the project-binding field as a cross-stream contract. Naming the Level 2 variant `Candidates` is locally descriptive, but it is not the on-wire/config term and will be easy to misuse when memoryd starts passing effective levels into coordination.
- Recommendation: Rename the variant to `Default` (or introduce an explicit conversion type that keeps the wire/config enum aligned with `minimal | default | collaborative`) and update tests to assert the Level 2/default mapping. Avoid inventing a second term for the same mode at the crate boundary.
- Confidence: High

[Low] [Correctness] XML attribute parsing can match substrings of other attribute names

- Evidence: `crates/memorum-coordination/src/session.rs:165-174` uses `tag.find(&format!("{name}="))` for attributes. `ref_attribute_paths` uses this helper for `ref`, so a tag containing an unrelated attribute such as `data-ref="..."` or `xref="..."` can be treated as a salient path.
- Why it matters: Salient path derivation feeds relevance scoring. False paths from unrelated attributes can make peer updates appear relevant when they are not, especially because path matching is exact and high-signal.
- Reasoning: This is a lightweight parser by design, but substring matching ignores XML attribute boundaries. The current tests only cover clean `ref="..."` attributes, so they would not catch accidental extraction from adjacent or namespaced attributes.
- Recommendation: Require an attribute boundary before the name (`start of tag` or ASCII whitespace) and a direct `=` after the name, or use a tiny purpose-built scanner over whitespace-delimited attributes. Add a regression test with `data-ref`/`xref` that must not populate `salient_paths`.
- Confidence: Medium

### Non-blocking simplifications

- `PeerWriteCandidate.paths` is a `Vec<String>` while the spec describes candidate paths as a set; converting to `HashSet<String>` at the boundary, or normalizing/deduping inside `path_fraction`, would make the scoring invariant clearer and avoid duplicate-path denominator surprises.
- `SessionContext::try_get_embedding` takes `session_id` even though the cache lives inside a single session context and all current callers pass `self.session_id`; if cross-session lookup is not intended, a one-argument `cached_recent_query_embedding(message_hash)` would simplify the abstraction.

### Test gaps

- `crates/memorum-coordination/tests/presence_unit.rs` and `crates/memorum-coordination/tests/claim_lock_unit.rs` still contain only placeholder tests. That is probably acceptable for Gate B if Tasks 9+ own those implementations, but the requested validation command includes them and the passing result gives no confidence in presence or claim-lock behavior.
- No test asserts that `PeerUpdateEntry.reference` is the memory id rather than the repository path; this allowed the main API-contract issue above.
- No test covers `ConcurrentSessionMode` alignment with `minimal/default/collaborative` project-binding vocabulary.
- No test covers attribute-boundary parsing for `ref=` in startup recall XML.

### Questions / uncertainties

- Review was limited to `crates/memorum-coordination/` and workspace/Cargo changes, per the requested Gate B scope. I did not validate memoryd integration because later tasks own that wiring.
- `memory-privacy` is a direct dependency of `memorum-coordination`, but Gate B code does not yet call `safe_plaintext_fragment`; I treated privacy redaction as a later XML-rendering/integration concern rather than a Gate B blocker.
- The repository has a large dirty tree outside this review scope. I did not attempt to attribute or modify unrelated changes.

### Positives

- The score weights, threshold boundary, local-observed recency behavior, cap ordering, cooldown, empty-entity handling, and embedding-triple mismatch behavior are straightforward and covered by focused tests.
- The Tier 3 no-op is implemented at `RelevanceGate::evaluate` entry before per-candidate scoring, which matches the spec's hot-path requirement.
- `cargo tree -p memorum-coordination --depth 1` shows no direct `memory-governance` dependency; the crate boundary is appropriately narrow for this gate.

### Validation run

- `cargo test -p memorum-coordination --test gate_unit --test session_derivation --test presence_unit --test claim_lock_unit` — passed (14 gate tests, 7 session tests, and placeholder presence/claim-lock tests).
- `cargo clippy -p memorum-coordination --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt -p memorum-coordination -- --check` — passed.
- `cargo tree -p memorum-coordination --depth 1` — confirmed direct dependencies are `chrono`, `dashmap`, `memory-privacy`, `memory-substrate`, and `serde`; no direct `memory-governance` dependency.
