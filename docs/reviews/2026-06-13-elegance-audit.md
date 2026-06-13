---
date: 2026-06-13
kind: elegance-audit
branch: refactor/desloppify-hardening-elegance
verdict: 4.69/5 (provisional band; uncalibrated), zero hard flags
---

> Read-only elegance audit run as the final phase of the desloppify → hardening → elegance
> sweep on `refactor/desloppify-hardening-elegance`. Three Opus 4.8 judges (architecture,
> readability/maintainability, correctness-risk lenses) scored against the v2 RUBRIC.md +
> Rust red-flag pack, reconciled by a synthesis pass. The backlog below is **not yet actioned** —
> it is residue on an otherwise exemplary base; nothing here blocks merge.

# Memorum Codebase Elegance — Synthesized Report

## 1. Verdict (PROVISIONAL band — uncalibrated)

**Weighted overall: 4.69 / 5 — band "Exemplary" (provisional).** This aggregate is uncalibrated and is the least-trusted output here. Treat it as directional only. The trusted outputs are the hard-flag result (§2) and the backlog (§4).

## 2. HARD-FLAG RESULT (PRIMARY, trusted)

**No blocker. Zero hard flags raised.**

All four trust-breaking dimensions clear the `<= 2` threshold by a wide margin — each scored a unanimous **5.00** across all three lenses:

| Trust-breaking dimension | Lens scores | Mean | Flag |
| --- | --- | --- | --- |
| Contracts & Domain Modeling | 5 / 5 / 5 | 5.00 | clear |
| Error & Invariant Integrity | 5 / 5 / 5 | 5.00 | clear |
| Test & Behavioral Honesty | 5 / 5 / 5 | 5.00 | clear |
| Dependency & Build Hygiene (incl. build/secrets) | 5 / 5 / 5 | 5.00 | clear |

Concretely: secret-never-persisted encoded as a distinct `WriteFailureKind::SecretRefused` before any disk effect; fully-tagged protocol/decision enums with no wildcard arms; ~1720 tests with only justified `#[ignore]`s and a broad real CI gate (clippy `-D warnings`, debug+release, doctests, two-clone-convergence `--full`, durability matrix, bench-regression); committed `Cargo.lock`, no secrets in tree, cargo-audit/deny in the gate. **The codebase is mergeable on trust grounds.**

## 3. Consensus scores per dimension (PRIMARY)

| Dimension | w | Lens A / B / C | Mean | Note |
| --- | --- | --- | --- | --- |
| Module Depth & Information Hiding | 13 | 5 / 4 / 4 | **4.33** | Disagreement (see below) |
| Dependency Shape & Coupling | 11 | 5 / 4 / 5 | **4.67** | |
| Complexity Locality & Special-Case Handling | 9 | 4 / 4 / 4 | **4.00** | Lowest consensus; unanimous |
| Contracts & Domain Modeling | 12 | 5 / 5 / 5 | **5.00** | |
| Naming & Readability | 9 | 5 / 5 / 5 | **5.00** | |
| Right-Sized Abstraction | 11 | 5 / 4 / 4 | **4.33** | |
| Error & Invariant Integrity | 9 | 5 / 5 / 5 | **5.00** | |
| Test & Behavioral Honesty | 12 | 5 / 5 / 5 | **5.00** | |
| Consistency & Idiomatic Coherence | 8 | 5 / 4 / 5 | **4.67** | |
| Dependency & Build Hygiene | 6 | 5 / 5 / 5 | **5.00** | |

**Weighted overall = 4.69 / 5** (Σ weight = 100; PROVISIONAL band, see §1).

### Notable disagreements

- **Module Depth (5 / 4 / 4):** Lens A scored a clean 5 ("no representation leaks"). B and C both docked it to 4 for *concrete* leaks A's sample missed: B flagged `knn_active_memories` duplicated as both a `Substrate` method and an `Index` method (shallow pass-through pair); C flagged web routes reaching into internal `memoryd` modules (`memoryd::policy_editor`, `memoryd::trust_artifact`) instead of going through the protocol seam. The two independent down-votes cite different real evidence, so the 4.33 mean is, if anything, generous — treat Module Depth as the genuine soft spot despite its high weight.
- **Right-Sized Abstraction (5 / 4 / 4):** Same shape. A saw the generics as fully justified (true). B and C agreed on the *inverse* residue — ~26 `too_many_arguments` allow-sites are *missing* parameter-struct abstractions on merge/index hot paths. Not a flaw in existing abstractions; a gap where one is absent.
- **Complexity Locality (4 / 4 / 4):** No disagreement, but it's the lowest consensus score. All three independently converged on the same two causes: the same ~24-26 `too_many_arguments` sites, and a few oversized files (`query.rs` 2985 LoC, `api.rs` 2693). High-confidence signal precisely because it's unanimous and cross-cited.

The four 5.00 dimensions are unanimous across all three lenses — maximally trustworthy.

## 4. Merged refactor backlog (PRIMARY, trusted)

Nine raw items deduped to **six**. The largest cluster (the `too_many_arguments` items) appeared in all three lenses and is merged into one. Impact is uniformly **low** and effort uniformly **low/med** — this is residue on an otherwise exemplary base, not load-bearing debt. None blocks merge.

### 4a. Design residue → behavior-preserving refactor pass

**D1. Group 6+ argument substrate/merge/index APIs into parameter structs** *(impact: low · effort: med)*
Merged from all three lenses (Backlog A-item-1, B-item-1, plus C's complexity note). ~24–26 `#[allow(clippy::too_many_arguments)]` sites total. Hotspots: `merge/three_way.rs:474` (`finalize_merge`), `index/query.rs:1447` (`upsert_vector_payload`), `merge/field_rules.rs:211/248/299`, `api.rs:1491/1913`.
*Risk:* touches the convergence-critical merge driver — each change must keep two-clone-convergence green.
*First step:* pilot a `FinalizeMergeInputs` struct for `finalize_merge` (mirroring the existing `SearchResponseRequest`/`ObserveRequestFields` idiom); run `two-clone-convergence.sh` after.

**D2. Split the file_too_long-suppressed / oversized substrate files** *(impact: low · effort: med)*
Merged from A-item-2 and C-item-2. `api.rs` (2693), `model.rs` (1660), `query.rs` (2985), `import/pipeline.rs` (1947).
*Risk:* these are deliberately centralized (header comments call out the centralized-DTO seam); split into submodules re-exported from the facade, not a true decomposition — a bad cut fragments a cohesive boundary.
*First step:* extract `api.rs` read-path methods (`read_memory*`, `read_path_envelope`) into an `api/read.rs` submodule behind the same `impl Substrate`; confirm rustdoc/public surface unchanged.

**D3. Route web routes through the protocol seam, not internal memoryd modules** *(impact: low · effort: med)*
From C-item-1 (corroborates B's and C's Module-Depth down-vote). `memoryd-web/src/routes/policy_editor.rs:8` imports `memoryd::policy_editor`; `routes/audit.rs:7` imports `memoryd::trust_artifact`.
*First step:* inventory the non-protocol `memoryd::` symbols `memoryd-web` imports; per symbol, decide protocol DTO vs shared DTO crate.

**D4. Collapse the duplicated `knn_active_memories` surface** *(impact: low · effort: low)*
From B-item-4. `api.rs:1492` (`Substrate::knn_active_memories`) is a thin forward to `query.rs:837` (`Index::knn_active_memories`).
*First step:* diff the two bodies; if substrate only locks+forwards, document it as the public seam or inline at the single consumer. Confirm it isn't load-bearing (triple-resolution/locking) before removing.

**D5. Unify the error idiom in `memorum-eval`** *(impact: low · effort: med)*
From B-item-2. Three competing idioms in one crate: `daemon_scaffold.rs:292` returns `Result<(), String>`, `harness_runner.rs:44` defines typed `HarnessRunnerError`, other harness fns return bare `io::Result`.
*Risk:* test-only crate, contained blast radius.
*First step:* extend `HarnessRunnerError` with a `Scaffold(...)` variant; convert the `Result<(), String>` signatures.

**D6. Typed errors for the few public stringly-typed lib APIs** *(impact: low · effort: low)*
From B-item-3. `model.rs:1033` `RepoPath::try_new -> Result<Self, String>` and `config/privacy.rs:36/43/54` (`from_yaml`/`from_env`/`validate`) return raw `String` while sibling newtypes (`MemoryId`/`DeviceId`) return typed `ValidationError`.
*Risk:* public-signature change on `RepoPath::try_new`; most callers discard the error via `?` into `ValidationError::Other`, so ripple is small — but note the serde `TryFrom<String>` impl (`model.rs:1084`) where `String` is the contract.
*First step:* change `RepoPath::try_new` to return `ValidationError`; update the handful of call sites.

### 4b. Mechanical residue → route back to desloppify

**M1. Internal path validators return `Result<(), String>`** *(impact: low · effort: low)*
From C-item-3 (the only item any lens tagged `mechanical`). `model.rs:1106` (`validate_repo_relative_path`), `:1154`, `:1163`, `api.rs:2090` (`validate_substrate_fragment_append`).
*Note:* these are crate-internal and feed `RepoPath::try_new` / serde `try_from`, whose public error contract is already `String`. **Sequence after D6** — if D6 tightens `RepoPath::try_new` to a typed error, these become a coherent follow-on; if D6 is deferred, confirm none escape the crate boundary and either leave as-is or introduce a private `PathValidationError` used only inside `model.rs`.

---

**Bottom line:** No blocker, no hard flag — the four trust-breaking dimensions are unanimous 5.00s and the codebase is mergeable on trust grounds. The provisional 4.69/5 band is uncalibrated and should not be over-read. The real (low-impact) residue clusters on three things all three lenses converged on: missing parameter-struct abstractions on the `too_many_arguments` sites, a handful of oversized centralized files, and a couple of stringly-typed seams — none of which threaten correctness or trust.