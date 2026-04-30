# Stream E Review Gate A - Stream D `safe_plaintext_fragment` Security Review

**Review date:** 2026-04-30
**Scope:** Current uncommitted Task 3 changes for `memory_privacy::safe_plaintext_fragment` only.
**Changed path from this review:** `docs/reviews/stream-e-safe-fragment-security-review.md`

## Verdict

No P0 or P1 security findings.

The helper is isolated from reveal/decrypt functionality, explicitly classifies under `PrivacyNamespace::Me`, fails closed on classifier errors/refuse/secret labels, distinguishes private review-pending fragments from hard-hidden secret fragments, and passed the required narrow test and clippy gates.

One P2 contract/docs ambiguity remains around how to describe `PrivacyStorageAction::EncryptAtRest` when it is caused only by the required `PrivacyNamespace::Me` default rather than by a private/account span.

## Findings

### P0 - None

No reachable reveal/decrypt path, plaintext secret emission, or authorization-boundary break found in the reviewed helper.

### P1 - None

No direct bypass found for secret/high-risk fragments in the shipped deterministic classifier path.

### P2 - Contract ambiguity: `final EncryptAtRest` wording conflicts with the tested Allow path for benign/URL/date fragments

**Severity:** P2
**Exploitability:** Low in the current implementation. The deterministic classifier path covered by the Task 3 tests still hides secret/refused fragments and review-pends private/account fragments. This is mainly a contract drift risk for the next Stream E integration worker.
**Impact:** A later worker could read the Stream E spec literally and change/consume the helper as if any final `PrivacyStorageAction::EncryptAtRest` must become `OmitReviewPending`, which would conflict with the current intended/tested `Allow` result for benign, URL-only, date-only, or no-span fragments classified under `PrivacyNamespace::Me`.

**Evidence:**

- The active Stream E spec says no spans, URL-only spans, and date-only spans may be `Allow`, but also says caller confidential/personal tier or final `PrivacyStorageAction::EncryptAtRest` maps to `OmitReviewPending`: `docs/specs/stream-e-passive-recall-v0.5.md:140-147`.
- The implementation does not map final `EncryptAtRest` by itself to `OmitReviewPending`; it hard-hides only final refuse or explicit secret labels, review-pends only private/account labels, then falls through to `Allow`: `crates/memory-privacy/src/decision.rs:103-117` and `crates/memory-privacy/src/decision.rs:120-129`.
- The Stream D API doc matches the implementation's safer operational distinction by saying `Allow` covers plaintext, URL-only, date-only, or no-span fragments, while `OmitReviewPending` covers encrypted-at-rest private or account-like fragments: `docs/api/stream-d-privacy-api.md:31-44`.
- The tests assert benign and URL/date fragments are allowed, private fragments are review-pending, mixed private+secret is hard-hidden, and namespace is `Me`: `crates/memory-privacy/tests/safe_plaintext_fragment.rs:9-18`, `crates/memory-privacy/tests/safe_plaintext_fragment.rs:41-68`, and `crates/memory-privacy/tests/safe_plaintext_fragment.rs:71-83`.
- `PrivacyNamespace::Me` is intentionally stricter than `Project`/`Agent` and defaults to `Personal`; taken literally, that default can make final storage routing `EncryptAtRest` even for no-span fragments: `crates/memory-privacy/src/policy.rs:50-56` and `crates/memory-privacy/src/policy.rs:93-98`.

**Minimal remediation:** Clarify the active Stream E contract before later Stream E consumers wire this helper. Either:

1. state that `Allow` takes precedence for no-span, URL-only, and date-only fragments when `EncryptAtRest` is produced only by the `PrivacyNamespace::Me` default; or
2. if literal final `EncryptAtRest` should always omit, add a failing behavior test for an `EncryptAtRest` decision with no private/account/secret span and change the helper accordingly.

Given the existing tests and Stream D API doc, option 1 appears to match the implemented intent.

## Checklist review

- **No reveal/decrypt path is reachable:** Pass. The helper imports only `PrivacyClassifier` and calls `classifier.classify`; the helper body has no `PrivacyEncryptor`, `KeyProvider`, `MaskingSession`, `decrypt`, or `memory_reveal` call path (`crates/memory-privacy/src/decision.rs:5`, `crates/memory-privacy/src/decision.rs:103-117`). A targeted search of the reviewed helper path found no reveal/decrypt references outside public re-exports in `lib.rs`.
- **Classification uses `PrivacyNamespace::Me`:** Pass. The call is explicit at `crates/memory-privacy/src/decision.rs:104`, and the recording test asserts two calls captured `[PrivacyNamespace::Me, PrivacyNamespace::Me]` at `crates/memory-privacy/tests/safe_plaintext_fragment.rs:71-83`.
- **Secret/high-risk fragments never become `Allow`:** Pass for the current deterministic path. Refuse/secret decisions return `OmitEncryptedBodyHidden` at `crates/memory-privacy/src/decision.rs:108-111`; the deterministic secret/JWT/private-key test covers hard-hidden output at `crates/memory-privacy/tests/safe_plaintext_fragment.rs:20-39`; explicit secret-label test covers inconsistent label/action inputs at `crates/memory-privacy/tests/safe_plaintext_fragment.rs:142-152`.
- **Review-pending private fragments are distinguishable from hard-hidden secrets:** Pass. Private/account labels map to `OmitReviewPending` through `label_requires_review` at `crates/memory-privacy/src/decision.rs:120-129`; tests cover private email/phone/address and account/person labels at `crates/memory-privacy/tests/safe_plaintext_fragment.rs:41-56` and `crates/memory-privacy/tests/safe_plaintext_fragment.rs:128-139`.
- **Helper cannot panic on arbitrary UTF-8:** Pass by code inspection for the helper and deterministic classifier path. The helper does no string slicing or unwrap/expect (`crates/memory-privacy/src/decision.rs:103-117`). The deterministic classifier delegates to regex matching and entropy scanning (`crates/memory-privacy/src/classifier.rs:44-56`); regex spans use safe `find_iter` offsets and entropy uses token boundaries from `split_whitespace` before slicing (`crates/memory-privacy/src/regex.rs:76-88`, `crates/memory-privacy/src/entropy.rs:7-21`). The only `expect` calls in this path are static regex literal initialization (`crates/memory-privacy/src/regex.rs:12-73`, `crates/memory-privacy/src/regex.rs:91-93`), not input-dependent panics.
- **Docs match implementation:** Mostly pass for `docs/api/stream-d-privacy-api.md:31-44`; P2 above tracks the remaining mismatch/ambiguity with the active Stream E spec wording.

## Evidence commands

```bash
cargo test -p memory-privacy --test safe_plaintext_fragment
```

Result: passed. Output reported 8 tests run, 8 passed, 0 failed.

```bash
cargo clippy -p memory-privacy --all-targets --all-features -- -D warnings
```

Result: passed. Output reported `Finished dev profile` with no warnings.

```bash
git diff --check -- crates/memory-privacy/src/decision.rs crates/memory-privacy/src/lib.rs docs/api/stream-d-privacy-api.md
git diff --check -- crates/memory-privacy/tests/safe_plaintext_fragment.rs
```

Result: passed with no whitespace diagnostics.

## Residual risk

This review did not modify code or add new tests because Task 5 is a read-only review gate and the only owned file is this report. Residual risk is limited to the P2 contract ambiguity above and to behavior of non-deterministic/custom `PrivacyClassifier` implementations outside the current deterministic Stream D path. Confidence is high for the current Task 3 implementation and medium for future Stream E consumers until the `EncryptAtRest` wording is clarified.
