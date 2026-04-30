# Stream D Claude review

Date: 2026-04-29
Reviewer: Claude (Sonnet 4.5)
Scope: full Stream D buildout in working tree (privacy crate, substrate
integration, memoryd handlers/cli/server, tests, docs).

## Verdict

**Don't commit yet.** Three of the regex tier-elevation defaults are wrong as
shipped — they auto-encrypt content that should stay plaintext, in ways that
will gut agent usefulness. Fix is one-line per label. Once those land, ship it.
Everything else is residual hardening on the right side of the v0.1 line.

The substrate integration, the `secret`-refusal boundary, the `encrypted/`
namespace handling, the metadata-minimization for encrypted records, the
fail-closed encrypted supersede/review, the MCP boundary, and the encrypted
forget all check out. Codex's three self-reviews caught the real P0/P1 bugs
(safe-projection-leak, metadata-not-scanned, key-file-permissions, encrypted
lifecycle gaps) and the fixes are present in code with regression tests.

Workspace gate (debug): `cargo clippy --workspace --all-targets --all-features
-- -D warnings`, `cargo fmt --all -- --check`, `cargo test --workspace`,
`RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps` all green. 335
tests pass across 72 suites in 145s.

---

## Blockers (must fix before commit)

### B1 — URL detection forces encryption for any `http(s)://` mention

**Where:** `crates/memory-privacy/src/decision.rs:84-94`,
`crates/memory-privacy/src/regex.rs:52-56`.

**What happens today:** every memory body that contains `https?://anything`
gets a `PrivacyLabel::PrivateUrl` span. `PrivateUrl::implied_tier()` returns
`Personal`. `PrivacyPolicy::resolve_tier` then raises the tier to `Personal`
because labels can only raise. `Personal` is `requires_encryption()`, so the
write goes through `write_encrypted` — body becomes ciphertext-only,
metadata-only in FTS, search returns 0 hits for any term in the body, and
`memory_get` returns `"[encrypted content omitted]"`.

**Why it's wrong:** URLs are *everywhere* in engineering memories. Doc links,
issue trackers, design docs, runbook references, package URLs, stack traces.
A note that says "see https://docs.example.com/foo for the rate-limit
contract" gets pushed to the encrypted tier — agents lose the body, search
loses the content, and the memory becomes effectively useless for recall. This
is a default that breaks the product.

**Why I think Codex shipped it this way:** the spec (§2) lists URL as a
Layer 1 detection category. Codex correctly detects URLs as spans (good — that
data is useful for masking pipelines and for confidential-tier records that
already require encryption). But Codex then conflated *detection* with
*tier elevation* via `implied_tier()`. Detect ≠ elevate.

**Fix (one line):**
```rust
Self::PrivateUrl => PrivacyTier::Internal,
```
URL spans are still recorded (still useful for masking, audit, and elevation
when caller marks `Sensitivity::Personal` explicitly). They no longer force
encryption.

**Test:** add a positive case to `privacy_e2e.rs` — write a project memory
with `body: "see https://docs.example.com/foo"`, no caller sensitivity,
assert `MemoryContent::Plaintext`, assert search hits the URL.

### B2 — Date detection forces encryption for every ISO date

**Where:** `crates/memory-privacy/src/decision.rs:84-94`,
`crates/memory-privacy/src/regex.rs:64-67`.

**What happens today:** `\b\d{4}-\d{2}-\d{2}\b` matches every ISO date.
`PrivateDate::implied_tier()` returns `Personal`. So a memory saying
"shipped on 2026-04-28" or "schema_version 1, captured 2026-04-27" gets
auto-encrypted.

**Why it's wrong:** dates are dense in engineering memories. Commit dates,
release dates, captured-at timestamps, retro dates, milestone dates. The
spec doesn't mandate that *every* date is sensitive — birthdates and medical
dates are, but the regex can't tell those apart from build dates.

**Fix (one line):**
```rust
Self::PrivateDate => PrivacyTier::Internal,
```
Same reasoning as B1. Date spans still recorded, no forced encryption.

**Test:** parallel to B1 — write a project memory with an ISO date in the
body, assert plaintext, assert searchable.

### B3 — Phone regex forces encryption on triple-of-digits patterns

**Where:** `crates/memory-privacy/src/decision.rs:84-94`,
`crates/memory-privacy/src/regex.rs:47-51`.

**What happens today:** any `\d{3}[-.\s]\d{3}[-.\s]\d{4}` pattern with word
boundaries gets `PrivatePhone` → `Personal` → encrypted. Real phone numbers
match. So do version triples like `1.2.3-456-7890`, ticket IDs of similar
shape, and split version-build identifiers.

**Why it's wrong:** Trey's call — phone numbers should not be
auto-encrypted by default. False-positive rate is high enough that the
elevation default does more harm than good. Real phone numbers can still be
elevated by caller-supplied `Sensitivity::Personal` when an operator or
classifier knows the field is actually a phone number.

**Fix (one line):**
```rust
Self::PrivatePhone => PrivacyTier::Internal,
```

**Test:** parallel to B1/B2.

### What stays at `Personal` (not blockers, intentional)

- `PrivateEmail` — strong PII signal under GDPR/CCPA, regex precision is high
  enough (`\b...@...\.[A-Za-z]{2,}\b`), real false-positive rate is low.
- `PrivateAddress` — physical address regex requires capitalized street suffix
  word, low false-positive rate.
- `PrivatePerson` — not currently emitted by Layer 1 (no person regex), so
  this only fires from a future model provider; treating model-detected
  person names as Personal is correct.
- `AccountNumber` — not currently emitted by Layer 1 either.
- `Secret` — must stay at `Secret` (refused before disk).

### Decoupling detection from elevation (architectural follow-up)

The deeper structural fix is to stop treating `PrivacyLabel::implied_tier()`
as the single source of truth for tier raising. A cleaner shape:

```rust
pub fn elevation(self) -> Option<PrivacyTier> {
    match self {
        Self::Secret => Some(PrivacyTier::Secret),
        Self::PrivateEmail | Self::PrivateAddress
        | Self::PrivatePerson | Self::AccountNumber => Some(PrivacyTier::Personal),
        // URL, Date, Phone: detect but do not elevate.
        // Caller may still raise via Sensitivity::Personal.
        _ => None,
    }
}
```

`PrivacyPolicy::resolve_tier` then `tier.max(span.label.elevation()
.unwrap_or(tier))`. Same one-line tests.

This is a bigger change than B1-B3 individually, but it's the right model: the
classifier is honest about what it found ("there's a URL here") without
imposing a tier policy that should be the operator/caller's call. The current
`implied_tier()` shape encodes a default that's spec-compliant but
operationally hostile. Recommend doing this in the same commit as the B1-B3
fixes — it's strictly more readable than three separate enum-arm tweaks.

---

## Risks (non-blocking, worth knowing about)

### R1 — Entropy threshold may miss 32-char hex tokens

**Where:** `crates/memory-privacy/src/entropy.rs:4-5`.

`MIN_ENTROPY_BITS_PER_CHAR = 4.2`. Hex max entropy is ~4.0 bits/char for a
random hex string. So a 32-char hex API key (without vendor prefix) will fall
just short of the entropy gate and not get flagged as `Secret` by the entropy
fallback. Vendor-specific regexes still catch AWS/GitHub/Stripe tokens with
their characteristic prefixes; the gap is custom hex tokens (e.g. an internal
service's API key).

**My take:** not a v0.1 blocker. Defense-in-depth would add a lower threshold
for hex-only tokens (~3.8 bits/char), or a separate hex-shape regex with
prefix exclusions. Punt.

### R2 — `MaskingSession::restore` does sequential `str::replace`

**Where:** `crates/memory-privacy/src/masking.rs:54-63`.

```rust
let mut restored = text.to_string();
for (token, original) in self.replacements.iter().rev() {
    restored = restored.replace(token, original);
}
```

If a restored value contains a substring that matches a later token (e.g. a
restored person name happens to contain `Person_B`), the second replace pass
will corrupt it. Spec §5 requires reclassification before write, which limits
the blast radius, but this is brittle.

**Fix:** single-pass walker that scans for token occurrences and substitutes
without re-scanning the substituted output. Or, more pragmatic, restrict
tokens to a uniquely-prefixed namespace (e.g. `__MASK__Person_A__`) the
restored values cannot contain.

**My take:** worth fixing in v0.1 because masking is a privacy primitive and
"restore corrupts text" is a sharp edge that could be hit by demos. Low effort.

### R3 — `PrivacyEncryptor` reloads file-backed key per write

**Where:** `crates/memory-privacy/src/crypto.rs:28-29, 43-44`.

`fs::metadata` + `read_to_string` + `serde_json::from_str` + `age::Identity::
parse` per encrypted write. For a low-volume daemon this is fine. For higher
volume — say, dreaming pipelines or a future bulk import — it's wasteful.

**My take:** Codex's perf review already lists this as a residual. Concur on
deferring. The right shape is a daemon-level cached `Arc<KeyMaterial>` with
an explicit invalidation path on rotation, plumbed through a daemon state
struct. Stream D v0.1 doesn't have a daemon-state object yet; building one
just for this is over-investment.

### R4 — TOCTOU on key file load

**Where:** `crates/memory-privacy/src/keys.rs:113-122`.

Three separate syscalls between symlink check, permission check, and file
read. An attacker with same-user privileges could swap files in between. Spec
§4 documents the file provider as a "development/onboarding boundary," so
the threat model excludes hostile same-user processes. For a production
keychain provider, the right shape is `open()` + `fstat()` on the same fd,
then `read()`.

**My take:** acceptable as documented. A comment at the top of `FileKeyProvider`
saying "TOCTOU-safe variant requires fd-pinned stat; this provider is the
documented dev boundary per spec §4" would help the next person.

### R5 — `repo_contains` test helper falls open on `rg` internal error

**Where:** `crates/memoryd/tests/privacy_e2e.rs:372-376`.

```rust
let output = std::process::Command::new("rg").arg("--fixed-strings")
    .arg(needle).arg(root).output().expect("run rg");
output.status.success()
```

`rg` exits 0 on match, 1 on no-match, 2 on error. `output.status.success()`
is `false` for both no-match and error. The canary test
`assert!(!repo_contains(...), "secret canary must not be written")` is `true`
in both no-match and error cases — so an `rg` internal error makes the
canary check pass silently.

**My take:** rare in practice but a real correctness gap in security-critical
canary tests. Two ways to fix:

1. Replace shell-out with a Rust file-walker (e.g. `walkdir` + memchr-based
   `bstr::ByteSlice::contains_str`).
2. Assert exit code is exactly 0 or 1, panic otherwise.

I'd do (1) — it kills the runtime dependency on `rg` being on PATH and is
trivial to write. ~15 lines. Adds robustness to a test boundary that is
specifically tasked with proving a secret didn't leak.

### R6 — `PrivacyFilterProvider` trait is sync

**Where:** `crates/memory-privacy/src/privacy_filter.rs:5-11`.

When the real OpenAI provider lands, it'll be a network call that needs
`async fn detect`. Today's sync trait works for `DisabledPrivacyFilter` and
`FixturePrivacyFilter`. Will need refactor.

**My take:** defer to whenever the real provider is wired up. The trait
boundary will rev anyway. Document.

### R7 — Privacy refusals don't emit structured audit records

**Where:** `crates/memoryd/src/handlers.rs:498-500, 1076-1090`.

Privacy refusals return `privacy_error` envelopes / `GovernanceWriteResponse
{ status: Refused, reason: Privacy }` but don't emit anything to the substrate
event log. Successful writes get `WriteCommitted` / `EncryptedWriteCommitted`
events; refused writes are invisible to operators.

**My take:** Stream G dashboard will need this. Spec doesn't currently mandate
it for Stream D. Worth a TODO comment near the refusal sites pointing at
"emit `PrivacyRefused` event when Stream G defines the schema." Low priority
for now.

### R8 — `PrivacyDecision::scan.ran_at: Utc::now()` baked into persisted metadata

**Where:** `crates/memory-privacy/src/decision.rs:163`,
`crates/memoryd/src/handlers.rs:549-554`.

Every decision creates a fresh timestamp that gets serialized into
`frontmatter.extras["privacy_scan"]`. Content-equal writes produce
non-content-equal frontmatter. This is fine today because the substrate's
canonical hash is over the body, not the frontmatter extras — but if anything
ever starts hashing the full frontmatter for dedup, this breaks idempotence.

**My take:** worth noting but not fixing. The audit value of `ran_at` is real.
If dedup-on-frontmatter-hash ever becomes a thing, exclude `extras` from the
hash (which Stream A almost certainly already does).

---

## Nits

### N1 — Strong inline comment at `safe_index_projection: None`

**Where:** `crates/memoryd/src/handlers.rs:511`.

This is the exact site of Codex's P0 security fix (the safe-projection-leak).
A future contributor will reach for `Some(...)` to make encrypted records
searchable and accidentally re-introduce the leak. Add:

```rust
// Stream D §4: encrypted records are metadata-only in indexes by default.
// Do NOT supply a projection without proving non-sensitivity at the
// projector boundary — see docs/reviews/stream-d-security-review.md P0.
safe_index_projection: None,
```

Cheap tripwire. Worth it.

### N2 — `PrivacyError` variants are stringly-typed

**Where:** `crates/memory-privacy/src/error.rs`.

`PrivacyFilterUnavailable(String)`, `KeyUnavailable(String)`, `Crypto(String)`
flatten provider/path/operation context into a single message. Structured
fields (`provider_id`, `path`, `op`) would make wiring Stream G dashboards
easier. Cosmetic for v0.1.

### N3 — Privacy Filter trait is sync (duplicate of R6 for the nit list)

See R6.

### N4 — `MaskingSessionId(String)` accepts any string

**Where:** `crates/memory-privacy/src/masking.rs:8-15`.

No structural validation. UUID v4 would be more disciplined. Trivial.

### N5 — Address regex is US-centric

**Where:** `crates/memory-privacy/src/regex.rs:58-62`.

`(?:St|Street|Ave|Avenue|Rd|Road|Blvd|Lane|Ln|Drive|Dr)` — missing Way, Court,
Ct, Place, Pl, Highway, Hwy, Boulevard (Blvd is there but not the long form),
and zero international coverage. Fine for v0.1.

### N6 — `CallerSensitivity::Sensitive` is a compatibility alias

**Where:** `crates/memory-privacy/src/policy.rs:18-22, 30`.

Maps to `Confidential`. Worth a comment explaining what it's compatible *with*
— I assume Stream A's `Sensitivity::Sensitive`, but nothing in Stream D's code
or spec connects those dots.

---

## Verification ledger (CLAUDE.md critical invariants)

| # | Invariant | Status | Where |
|---|-----------|--------|-------|
| 1 | `secret` never persisted to disk | ✓ | `handlers.rs:498` early-return; `api.rs:467-469` `SecretRefused`; `privacy_e2e_secret_governed_write_is_refused_before_disk_effects` |
| 2 | Every write carries `ClassificationOutcome` | ✓ | `write_privacy_memory` sets `RequiresEncryption` for encrypted, `tier.classification()` for plaintext, `Trusted` for review decisions |
| 3 | Embedding triple is identity | n/a | Stream D doesn't touch embedding triples |
| 4 | Device IDs in runtime only | ✓ | Unchanged |
| 5 | `MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION` | ✓ | Unchanged |
| 6 | Two-clone convergence | n/a | Stream D doesn't change merge |
| 7 | Bench baselines human-only | ✓ | Unchanged |

Spec-mandated boundaries:
- ✓ Encrypted records land at `encrypted/<original-path>` (`api.rs:1182`).
- ✓ Body overwritten with base64 ciphertext before serialization (`api.rs:511`).
- ✓ `indexed_memory.body.clear()` before FTS upsert when no safe projection
  (`api.rs:542`).
- ✓ Encrypted record summary becomes `"encrypted memory"`, tags zeroed,
  source ref replaced (`handlers.rs:1174-1188, 1247-1267`).
- ✓ Encrypted forget tombstones without decrypting (`handlers.rs:346-369`).
- ✓ Encrypted supersede + encrypted review fail closed
  (`handlers.rs:280-286, 317-323, 602-606`).
- ✓ MCP manifest pinned to 7 agent-facing tools; admin tool names rejected
  (`mcp_manifest.rs:24-46`).

## What Codex's reviews already covered (independently verified)

- ✓ Encrypted forget no longer plaintext-only — verified `handlers.rs:353-358`
  summary-fallback for ciphertext.
- ✓ Encrypted supersession bypass removed — verified `handlers.rs:317-323`
  fail-closed + `handlers.rs:280-286` for old-side.
- ✓ MCP admin names pinned out — verified in `mcp_manifest.rs:27-41`.
- ✓ `safe_index_projection: None` default — verified at `handlers.rs:511`
  (the N1 site).
- ✓ Classifier scans full persisted envelope — verified
  `privacy_scan_text()` joins body, title, summary, source_ref, tags.
- ✓ Key file permissions (0700/0600, symlink reject) — verified
  `keys.rs:124-178`.
- ✓ Encrypted review fails closed — verified `handlers.rs:602-606`.

## Recommended commit shape

1. **B1+B2+B3 in one commit**, ideally with the architectural refactor
   (separate `elevation()` helper) so the change reads as a single coherent
   "decouple detection from elevation" rather than three enum-arm tweaks.
   Add three positive e2e tests (URL, date, phone — each a project memory
   stays plaintext and is searchable).
2. **R2 (`MaskingSession::restore` single-pass)** in a small follow-up commit,
   or fold into the same commit if convenient.
3. **R5 (`repo_contains` Rust file-walker)** in a small follow-up commit.
4. **N1 (the tripwire comment)** anywhere — it's a one-liner.

Everything else can ship as-is. The Codex reviews stand; they caught the
interesting bugs and the fixes are correct.
