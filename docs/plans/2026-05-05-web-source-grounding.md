# Web Source Grounding Implementation Plan

**Goal:** Add robust web-link grounding so a memory can cite a locally captured, hashed, excerpt-verified copy of the page content that supported the saved claim.

**Architecture:** Governance must never trust naked URLs. Add a durable `memory-source` crate that captures a URL as accessed, stores verified source artifacts, and exposes `webcap:<artifact_id>#<excerpt_id>` refs. `memoryd` owns capture CLI/MCP/protocol surfaces; `memory-governance` owns the typed grounding decision; `memory-substrate` owns the git tree/merge behavior for the new artifact family.

**Tech Stack:** Rust 2021 workspace, vertical TDD, `tokio`, `serde`, `serde_json`, `chrono`, `reqwest` with rustls, `reqwest::redirect::Policy::none`, `reqwest::ClientBuilder::resolve_to_addrs`, `reqwest::ClientBuilder::no_proxy`, `url`, `scraper`, `encoding_rs`, `sha2`, `zstd`, `memory-privacy`, `memory-governance`, `memoryd` Unix-socket protocol/MCP, source artifact files under `sources/web/**`, and narrow cargo gates before the full `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh` gate.

---

## Source Of Truth And Boundaries

This plan is grounded in the live code as of 2026-05-05, especially:

- `crates/memory-governance/src/grounding.rs` currently accepts explicit user context, absolute `file:` refs, and session-spawn refs only.
- `crates/memory-governance/src/engine.rs` constructs `GroundingVerifier` through a generic session resolver and must change if grounding grows another resolver.
- `crates/memoryd/src/handlers.rs` currently maps `GovernanceSourceKindMeta` into governance sources and builds the `GovernanceEngine` directly.
- `crates/memoryd/tests/protocol_contract.rs` is the existing daemon protocol shape contract and must be updated for new protocol variants.
- `crates/memoryd/src/mcp.rs` currently exposes nine agent-facing tools.
- `crates/memory-substrate/src/tree/layout.rs` owns canonical git tree bootstrap and managed `.gitattributes` patterns.
- `crates/memory-substrate/src/merge/**` must explicitly understand any path family assigned to `merge=memory-merge-driver`.
- `crates/memory-privacy` owns deterministic privacy classification and safe plaintext fragment checks.

### Non-goals for v0.1

- Do **not** accept naked `https://...` URLs as grounding refs.
- Do **not** add browser-rendered capture. Static HTTP capture is v0.1; rendered/browser capture can be v0.2 using the same artifact schema.
- Do **not** index raw or extracted captured pages into passive recall/search.
- Do **not** capture localhost, private network, link-local, loopback, multicast, unspecified, or cloud metadata addresses.
- Do **not** send cookies, bearer tokens, `.netrc`, system proxy credentials, browser state, or ambient auth.
- Do **not** support encrypted source artifacts in v0.1. If the extracted page text or excerpt requires encryption/refusal under `memory-privacy`, the capture fails closed. Encrypted source artifacts can be v0.2.
- Do **not** mutate old spec files unless Trey asks for a spec revision later.

### Product rule

A source ref is grounded only when all of this is true:

1. The repo contains a source artifact manifest for the artifact id.
2. The artifact manifest validates against the extracted text and excerpts hashes.
3. The ref names a specific excerpt id.
4. The excerpt quote is present **exactly** in the extracted page text at the recorded byte range.
5. The excerpt and extracted page text pass `memory-privacy` plaintext storage policy.
6. The capture status is `complete` or `complete_text_only`.
7. The manifest declares `capture_method=http_static_v1`; v0.1 verifies artifact integrity, not cryptographic capture provenance.

`complete_text_only` is allowed because v0.1 grounds claims against exact extracted text and exact excerpts. Full raw response archival is not required for grounding; if raw bytes are stored, they are extra audit evidence and must pass the raw textual privacy check described below.

A URL may be stored as metadata inside the artifact, but never as the grounding proof by itself. v0.1 does not cryptographically prove that an artifact was daemon-generated; it proves only that the local artifact set is internally consistent and explicitly marked as `http_static_v1`. If cryptographic provenance is needed later, add a daemon-signed manifest or event-log-backed generation record in v0.2.

---

## Artifact Layout And Storage Policy

Canonical synced repo paths:

```text
sources/web/YYYY/MM/src_01J0Z7Y8Q9R0ABCDE123456789/
  manifest.json
  extracted.txt
  excerpts.jsonl
  raw.bin.zst              # optional; only when raw textual projection passes privacy checks
```

Future optional paths, explicitly out of scope for v0.1 implementation:

```text
sources/web/YYYY/MM/src_01J0Z7Y8Q9R0ABCDE123456789/rendered.html.zst
sources/web/YYYY/MM/src_01J0Z7Y8Q9R0ABCDE123456789/screenshot.png
sources/web/YYYY/MM/src_01J0Z7Y8Q9R0ABCDE123456789/capture.pdf
```

### Manifest shape

```json
{
  "schema_version": 1,
  "artifact_id": "src_01J0Z7Y8Q9R0ABCDE123456789",
  "kind": "web_capture",
  "original_url": "https://example.com/report",
  "final_url": "https://example.com/report?canonical=1",
  "redirect_chain": [
    {"url": "https://example.com/report", "status": 301, "location": "https://example.com/report?canonical=1"}
  ],
  "captured_at": "2026-05-05T18:00:00Z",
  "capture_method": "http_static_v1",
  "request": {
    "method": "GET",
    "user_agent": "memorum-source-capture/0.1",
    "accept": "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.1"
  },
  "response": {
    "http_status": 200,
    "content_type": "text/html; charset=utf-8",
    "content_encoding": "br",
    "etag": "\"abc123\"",
    "last_modified": "Tue, 05 May 2026 17:00:00 GMT",
    "remote_addr": "93.184.216.34:443"
  },
  "raw_sha256": "sha256:...",
  "raw_zstd_sha256": "sha256:...",
  "raw_storage": "stored",
  "raw_omitted_reason": null,
  "extracted_text_sha256": "sha256:...",
  "excerpts_sha256": "sha256:...",
  "raw_byte_len": 12345,
  "extracted_text_byte_len": 4567,
  "capture_status": "complete",
  "warnings": []
}
```

`raw_storage` values:

- `stored`: `raw.bin.zst` exists and hashes verify.
- `omitted_privacy`: raw bytes were omitted because the raw textual projection failed privacy checks while extracted text/excerpts were safe.
- `omitted_unsupported`: raw bytes were omitted because content encoding/type could not be safely audited.

`capture_status` values:

- `complete`: extracted text/excerpts verify and raw is stored.
- `complete_text_only`: extracted text/excerpts verify; raw omitted by policy.
- `partial`: capture happened but cannot ground a source ref.
- `failed`: capture failed and cannot ground a source ref.

### Excerpt record shape

```json
{
  "excerpt_id": "quote_0001",
  "artifact_id": "src_01J0Z7Y8Q9R0ABCDE123456789",
  "quote": "The exact relevant sentence or paragraph that supports the memory claim.",
  "quote_sha256": "sha256:...",
  "locator": {"kind": "byte_range", "start": 1234, "end": 1320},
  "match_kind": "exact",
  "created_at": "2026-05-05T18:00:00Z"
}
```

Source refs consumed by governance:

```text
webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001
```

---

## Orchestrator And Subagent Contract

Mandatory prompt line for every implementation/review/security/docs subagent touching this plan:

```text
Mandatory skills: clean-code, tdd, rust-engineer.
```

Every Rust subagent must load:

```text
/Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md
```

Use vertical TDD: write one failing behavior test, run the narrow RED command, implement the smallest slice, rerun the narrow GREEN command, then refactor only while green.

Full gate policy:

- Worker tasks run only narrow package gates.
- Coordinator runs the full project gate once after integration: `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh`.
- If the full gate fails for an unrelated existing reason, the feature is **not ready without an explicit Trey waiver**; record the waiver in the final gate doc.

---

## Parallelization Map

- Tasks 1-8 are sequential. They touch shared contracts and security-sensitive boundaries; do not parallelize them.
- After Task 8, Tasks 9, 10, 11, and 12 may run in parallel only if each uses its exact owned-file set and the coordinator confirms no overlaps inside that batch.
- Task 13 is final integration and must run after all implementation, docs, eval, and security review tasks.

Batch duplicate check template:

```bash
cat > /tmp/web-source-batch-owned-files.txt <<'LIST'
Task 9: crates/memoryd/src/trust_artifact.rs
Task 10: crates/memorum-eval/src/orchestrator.rs
Task 11: docs/api/web-source-grounding-api.md
Task 12: docs/reviews/2026-05-05-web-source-grounding-security-review.md
LIST
cut -d: -f2- /tmp/web-source-batch-owned-files.txt \
  | tr ',' '\n' \
  | sed 's/`//g' \
  | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' \
  | rg -v '^$' \
  | sort \
  | uniq -d
```

Expected for any parallel batch: no output.

---

## Task 1: Workspace Contract And `memory-source` Crate Skeleton

**Parallel:** no  
**Blocked by:** none  
**Owned files:** `Cargo.toml`, `Cargo.lock`, `crates/memory-source/Cargo.toml`, `crates/memory-source/src/lib.rs`, `crates/memory-source/src/model.rs`, `crates/memory-source/src/error.rs`, `crates/memory-source/src/capture.rs`, `crates/memory-source/src/excerpt.rs`, `crates/memory-source/src/extract.rs`, `crates/memory-source/src/hash.rs`, `crates/memory-source/src/storage.rs`, `crates/memory-source/src/url_safety.rs`, `crates/memory-source/tests/model_contract.rs`

**Invariants:** Do not change existing memory write behavior. Do not wire the crate into `memoryd` yet.  
**Out of scope:** HTTP fetching, extraction, governance integration.

**Files:**

- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/memory-source/Cargo.toml`
- Create: `crates/memory-source/src/lib.rs`
- Create: `crates/memory-source/src/model.rs`
- Create: `crates/memory-source/src/error.rs`
- Create placeholder: `crates/memory-source/src/capture.rs`
- Create placeholder: `crates/memory-source/src/excerpt.rs`
- Create placeholder: `crates/memory-source/src/extract.rs`
- Create placeholder: `crates/memory-source/src/hash.rs`
- Create placeholder: `crates/memory-source/src/storage.rs`
- Create placeholder: `crates/memory-source/src/url_safety.rs`
- Create: `crates/memory-source/tests/model_contract.rs`

**Step 1: Write the failing tests**

Create `crates/memory-source/tests/model_contract.rs` with tests that require:

- `SourceArtifactId::try_new("src_01J0Z7Y8Q9R0ABCDE123456789")` accepts only `src_` plus a 26-character Crockford ULID body.
- `WebCaptureManifest` serializes `capture_status`, `capture_method`, and `raw_storage` as snake_case.
- `WebCaptureSourceRef::parse("webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001")` returns artifact id and excerpt id.
- naked URLs are rejected by the source-ref parser.

**Step 2: Run RED**

```bash
cargo test -p memory-source --test model_contract -- --test-threads=2
```

Expected: fails because package/crate does not exist.

**Step 3: Implement minimal crate and predeclare modules**

Add a new workspace member `crates/memory-source`.

Add dependencies in Task 1 so later sequential tasks do not fight over package metadata:

- `chrono`
- `encoding_rs`
- `memory-privacy`
- `reqwest`
- `scraper`
- `serde`
- `serde_json`
- `sha2`
- `thiserror`
- `tokio`
- `ulid`
- `url`
- `zstd`

`src/lib.rs` must predeclare all planned modules, with empty modules allowed behind stubs where necessary:

```rust
pub mod capture;
pub mod error;
pub mod excerpt;
pub mod extract;
pub mod hash;
pub mod model;
pub mod storage;
pub mod url_safety;
```

Create each placeholder module file in Task 1 with no public behavior beyond a private zero-sized marker or comments as needed to keep `cargo test -p memory-source` compiling. Later sequential tasks replace those placeholders.

**Step 4: Implement model/error**

- `model.rs`: `SourceArtifactId`, `WebCaptureManifest`, `RedirectHop`, `CaptureRequestSnapshot`, `CaptureResponseSnapshot`, `CaptureStatus`, `CaptureMethod`, `RawStorage`, `ExcerptRecord`, `ExcerptLocator`, `ExcerptMatchKind`, `WebCaptureSourceRef`.
- `error.rs`: `SourceError` with `InvalidId`, `InvalidSourceRef`, `Io`, `Json`, `Integrity`, `Unsupported`, `UrlSafety`, `CaptureFailed`, `Privacy`, `ExcerptNotFound`.

**Step 5: Run GREEN**

```bash
cargo test -p memory-source --test model_contract -- --test-threads=2
cargo fmt --all -- --check
```

Expected: tests pass and formatting is clean.

---

## Task 2: Artifact Storage And Integrity Verification

**Parallel:** no  
**Blocked by:** Task 1  
**Owned files:** `crates/memory-source/src/storage.rs`, `crates/memory-source/src/hash.rs`, `crates/memory-source/tests/storage_integrity.rs`

**Invariants:** The artifact store must be deterministic and must not trust manifest hashes without re-reading files. Source artifact paths are not `RepoPath`; they are managed by `memory-source::SourceArtifactPath`.  
**Out of scope:** Network fetching and HTML extraction.

**Files:**

- Modify: `crates/memory-source/src/storage.rs`
- Modify: `crates/memory-source/src/hash.rs`
- Create: `crates/memory-source/tests/storage_integrity.rs`

**Step 1: Write failing tests**

Tests should cover:

- `ArtifactStore::write_web_capture` writes `manifest.json`, `extracted.txt`, and `excerpts.jsonl` under `sources/web/YYYY/MM/<artifact_id>/`.
- `raw.bin.zst` is written only when `raw_storage=stored`.
- `ArtifactStore::verify_web_capture` passes immediately after write.
- Mutating `extracted.txt` makes verification fail with an integrity error.
- Mutating `raw.bin.zst` makes verification fail when raw is stored.
- `ArtifactStore::resolve_excerpt_ref("webcap:...#quote_0001")` returns the excerpt record only if the artifact verifies first.
- A manifest with `capture_status=partial` or `failed` is not groundable.

**Step 2: Run RED**

```bash
cargo test -p memory-source --test storage_integrity -- --test-threads=2
```

Expected: fails because `ArtifactStore` does not exist.

**Step 3: Implement storage**

Implement:

- `ArtifactStore::new(repo_root: impl Into<PathBuf>)`.
- `SourceArtifactPath` for safe path construction under `sources/web/YYYY/MM/<artifact_id>/`; no `..`, symlinks, absolute child paths, or path separator input.
- temp-directory write to `sources/web/YYYY/MM/.tmp-<artifact_id>-<pid>` followed by atomic rename into final directory.
- `sha256_hex` / `sha256_prefixed` helpers.
- zstd compression for raw bytes when raw is allowed.
- verification that recomputes hashes for compressed raw, decompressed raw, extracted text, and excerpts JSONL.

**Step 4: Run GREEN**

```bash
cargo test -p memory-source --test storage_integrity -- --test-threads=2
cargo test -p memory-source -- --test-threads=2
```

Expected: all `memory-source` tests pass.

---

## Task 3: URL Safety And Static HTTP Capture

**Parallel:** no  
**Blocked by:** Task 2  
**Owned files:** `crates/memory-source/src/url_safety.rs`, `crates/memory-source/src/capture.rs`, `crates/memory-source/tests/url_safety.rs`, `crates/memory-source/tests/http_capture.rs`

**Invariants:** Capture must fail closed for SSRF-sensitive targets. Redirects must be followed manually. The actual request must be pinned to vetted public socket addresses using `reqwest::ClientBuilder::resolve_to_addrs`, not a separate unchecked DNS lookup. Test-only bypasses must be `#[cfg(test)]` only and impossible to trigger from CLI/MCP.  
**Out of scope:** Browser-rendered capture, authenticated capture, PDF extraction.

**Files:**

- Modify: `crates/memory-source/src/url_safety.rs`
- Modify: `crates/memory-source/src/capture.rs`
- Create: `crates/memory-source/tests/url_safety.rs`
- Create: `crates/memory-source/tests/http_capture.rs`

**Step 1: Write failing URL safety tests**

Cover:

- reject `file://`, `ftp://`, missing scheme, and non-http(s).
- reject localhost names and IPs: `localhost`, `127.0.0.1`, `::1`.
- reject RFC1918, CGNAT, link-local, unique-local, multicast, documentation-only, unspecified, and cloud metadata IPs.
- reject URLs with embedded credentials.
- allow normal public `https://example.com/path` after resolver returns public addrs.
- reject if **any** resolved address for a hostname is private or otherwise disallowed.
- reject redirect to a private address.
- reject redirect chains over 5 hops.

**Step 2: Run RED**

```bash
cargo test -p memory-source --test url_safety -- --test-threads=2
```

Expected: fails because URL safety module does not exist.

**Step 3: Implement URL safety with pinned resolution**

Implement:

- `DnsResolver` trait with production implementation using `tokio::net::lookup_host`.
- `ValidatedHop` containing the URL and the exact public `SocketAddr` list vetted for that host/port.
- IP classification helper that rejects non-public addresses.
- `PinnedReqwestClientFactory` that builds a client per hop with:
  - `redirect(reqwest::redirect::Policy::none())`
  - `no_proxy()`
  - `resolve_to_addrs(host, &validated_addrs)`
  - request timeout 10 seconds
  - connect timeout 5 seconds
  - no cookie store
- after response, validate `response.remote_addr()` is present and belongs to the pinned address set.

Use current reqwest docs behavior: `ClientBuilder::resolve_to_addrs` pins the host to supplied socket addresses; `redirect::Policy::none` disables automatic redirects; `no_proxy` disables system proxy usage.

**Step 4: Write capture tests**

Use a `#[cfg(test)]` local HTTP server helper and a test-only resolver that returns loopback. The production resolver must still reject loopback.

Tests should assert:

- final URL and redirect chain are recorded.
- status/content-type/content-encoding/etag/last-modified/remote_addr are recorded.
- oversized response fails before artifact write.
- HTTP 4xx/5xx returns `capture_status=failed` and does not produce a groundable artifact.
- a resolver that returns public addr first and private addr second is rejected.
- a fake rebinding resolver whose second call changes addresses cannot affect a request already pinned to the first vetted set.

**Step 5: Run GREEN**

```bash
cargo test -p memory-source --test url_safety -- --test-threads=2
cargo test -p memory-source --test http_capture -- --test-threads=2
```

Expected: tests pass.

---

## Task 4: Text Extraction, Privacy Storage Policy, And Exact Excerpt Anchoring

**Parallel:** no  
**Blocked by:** Task 3  
**Owned files:** `crates/memory-source/src/extract.rs`, `crates/memory-source/src/excerpt.rs`, `crates/memory-source/src/capture.rs`, `crates/memory-source/tests/extraction_excerpt.rs`, `crates/memory-source/tests/privacy_storage_policy.rs`

**Invariants:** Governance can only ground refs to exact verified excerpts. The capture must not commit extracted text if `memory-privacy` says it requires encryption or refusal. Raw bytes are stored only when the raw textual projection is also safe; otherwise raw is omitted and status becomes `complete_text_only`.  
**Out of scope:** Semantic claim entailment, browser rendering, screenshot OCR, encrypted source artifacts.

**Files:**

- Modify: `crates/memory-source/src/extract.rs`
- Modify: `crates/memory-source/src/excerpt.rs`
- Modify: `crates/memory-source/src/capture.rs`
- Create: `crates/memory-source/tests/extraction_excerpt.rs`
- Create: `crates/memory-source/tests/privacy_storage_policy.rs`

**Step 1: Write failing extraction/excerpt tests**

Cover:

- HTML extraction with `scraper` removes `script`, `style`, `noscript`, hidden/template content, and preserves visible text in deterministic order.
- malformed HTML still extracts visible text without panic.
- charset handling uses `encoding_rs` when charset is known; invalid bytes are replacement-decoded and warning-recorded.
- plain text content is preserved directly.
- unsupported content type writes a `partial` manifest that cannot ground a source ref.
- exact quote match produces `quote_0001` with byte range locator.
- quote absent returns `ExcerptNotFound` and does not write a groundable artifact.
- quote containing sensitive content fails with a privacy error.

**Step 2: Write failing privacy storage tests**

Cover:

- extracted page text containing a safe quote but surrounding sensitive data refuses capture before artifact write.
- extracted page text that requires encryption refuses capture in v0.1 with `encrypted_source_artifacts_unsupported`.
- raw textual projection that fails privacy while extracted text is safe results in `complete_text_only`, no `raw.bin.zst`, and a `raw_omitted_reason`.
- `complete_text_only` remains groundable because excerpt/extracted hashes verify.

**Step 3: Run RED**

```bash
cargo test -p memory-source --test extraction_excerpt -- --test-threads=2
cargo test -p memory-source --test privacy_storage_policy -- --test-threads=2
```

Expected: fails because extraction/excerpt/privacy policy modules are not implemented.

**Step 4: Implement extraction**

Implement `extract_text(content_type, raw_bytes) -> ExtractedText`:

- `text/plain`: charset-aware bounded text.
- `text/html` / `application/xhtml+xml`: parse with `scraper`; remove `script`, `style`, `noscript`, `template`; collect visible text nodes; normalize runs of whitespace in extraction output only.
- unsupported: explicit unsupported status, no groundable excerpts.
- enforce extracted text cap, default 256 KiB.

**Step 5: Implement exact excerpt anchoring**

Implement `create_excerpt_records(extracted_text, requested_quotes)`:

- at least one quote is required for a groundable capture.
- exact byte-for-byte UTF-8 match only in v0.1.
- quote id format: `quote_0001`, `quote_0002`, stable in request order.
- locator is a byte range in `extracted.txt`.
- verifier re-reads `extracted.txt` and confirms `&text[start..end] == quote`.
- quote text must pass `safe_plaintext_fragment(DeterministicPrivacyClassifier::new(), quote)`.

**Step 6: Implement privacy storage policy**

Before writing any artifact:

- classify extracted text with `DeterministicPrivacyClassifier` in `PrivacyNamespace::Project` unless caller supplies a narrower namespace later through `memoryd`.
- if extracted text storage action is `Refuse`, fail capture.
- if extracted text storage action is `EncryptAtRest`, fail capture in v0.1 with explicit unsupported error.
- classify the raw textual projection when raw is textual; store raw only if safe plaintext. If raw is not safe but extracted text is safe, omit raw and set `complete_text_only`.

**Step 7: Run GREEN**

```bash
cargo test -p memory-source --test extraction_excerpt -- --test-threads=2
cargo test -p memory-source --test privacy_storage_policy -- --test-threads=2
cargo test -p memory-source -- --test-threads=2
```

Expected: all `memory-source` tests pass.

---

## Task 5: Source Artifact Tree Bootstrap And Merge Behavior

**Parallel:** no  
**Blocked by:** Task 4  
**Owned files:** `crates/memory-substrate/src/tree/layout.rs`, `crates/memory-substrate/src/tree/validate.rs`, `crates/memory-substrate/src/merge/mod.rs`, `crates/memory-substrate/src/merge/three_way.rs`, `crates/memory-substrate/src/merge/source_artifact.rs`, `crates/memory-substrate/tests/tree_validation.rs`, `crates/memory-substrate/tests/source_artifact_merge_rules.rs`

**Invariants:** Source artifacts are evidence files, not canonical memory Markdown. They must not be parsed/indexed as memories. Any path assigned to `merge=memory-merge-driver` must have explicit merge-driver support before `.gitattributes` wires it.  
**Out of scope:** Any change to memory frontmatter schema.

**Files:**

- Modify: `crates/memory-substrate/src/tree/layout.rs`
- Modify: `crates/memory-substrate/src/tree/validate.rs`
- Modify: `crates/memory-substrate/src/merge/mod.rs`
- Modify: `crates/memory-substrate/src/merge/three_way.rs`
- Create: `crates/memory-substrate/src/merge/source_artifact.rs`
- Modify: `crates/memory-substrate/tests/tree_validation.rs`
- Create: `crates/memory-substrate/tests/source_artifact_merge_rules.rs`

**Step 1: Write failing tree tests**

Cover:

- init/adoption creates `sources` and `sources/web` directories.
- `sources/web/**` Markdown or JSON files are not returned by `relative_memory_paths`.
- `.gitattributes` includes source artifact rules only after merge support exists.

**Step 2: Write failing merge tests**

Cover:

- identical `manifest.json`, `excerpts.jsonl`, and `extracted.txt` sides merge cleanly.
- one-sided edits against base are accepted.
- divergent `manifest.json` for same artifact returns `MergeResult::Quarantine` with a valid non-groundable partial manifest.
- `excerpts.jsonl` unique-concats by `excerpt_id` and quarantines same-id different-quote conflicts.
- divergent `extracted.txt` for same artifact quarantines rather than picking a side.
- every quarantine output fails `ArtifactStore::verify_web_capture` and therefore cannot ground governance.
- `raw.bin.zst` is marked binary/no custom driver and is not sent through UTF-8 merge tests.

**Step 3: Run RED**

```bash
cargo test -p memory-substrate --test tree_validation -- --test-threads=2
cargo test -p memory-substrate --test source_artifact_merge_rules -- --test-threads=2
```

Expected: fails because `sources/web` and source artifact merge rules do not exist.

**Step 4: Implement source artifact merge support**

Add `merge/source_artifact.rs` with path detectors and merge functions. Quarantine output must be deterministic and must leave the artifact **non-groundable**. The verifier must reject the merged artifact after every quarantine case.

Exact merge behavior:

- `sources/web/**/manifest.json`:
  - identical or one-sided changes merge cleanly.
  - divergent both-sided changes return `MergeResult::Quarantine` whose written text is valid manifest JSON based on the lexicographically smaller side, but with `capture_status="partial"`, `warnings += ["source_artifact_merge_conflict"]`, and a `merge_conflict` object containing bounded SHA256s of base/ours/theirs. Governance must refuse this artifact because status is non-groundable.
- `sources/web/**/excerpts.jsonl`:
  - unique-concat by `excerpt_id` when records do not conflict.
  - same `excerpt_id` with different `quote_sha256` or locator returns `MergeResult::Quarantine` whose written text is deterministic JSONL containing all non-conflicting records plus one `merge_conflict` record. The manifest's `excerpts_sha256` will no longer match, so verification must fail.
- `sources/web/**/extracted.txt`:
  - identical or one-sided changes merge cleanly.
  - divergent both-sided changes return `MergeResult::Quarantine` whose written text is a deterministic bounded conflict report with base/ours/theirs SHA256s and no original full text. The manifest's `extracted_text_sha256` will no longer match, so verification must fail.

Wire path detection in existing merge dispatcher before Markdown/frontmatter merge. Add tests that run the merge and then call `ArtifactStore::verify_web_capture`, expecting refusal for every quarantine output.

**Step 5: Implement tree changes**

Add directories:

- `sources`
- `sources/web`

Add managed `.gitattributes` entries:

```text
sources/web/**/manifest.json merge=memory-merge-driver
sources/web/**/excerpts.jsonl merge=memory-merge-driver
sources/web/**/extracted.txt merge=memory-merge-driver
sources/web/**/raw.bin.zst binary
```

**Step 6: Run GREEN**

```bash
cargo test -p memory-substrate --test tree_validation -- --test-threads=2
cargo test -p memory-substrate --test source_artifact_merge_rules -- --test-threads=2
cargo test -p memory-merge-driver --test merge_driver_cli -- --test-threads=2
```

Expected: tests pass.

---

## Task 6: Governance Web-Capture Grounding Resolver

**Parallel:** no  
**Blocked by:** Task 5  
**Owned files:** `crates/memory-governance/Cargo.toml`, `crates/memory-governance/src/grounding.rs`, `crates/memory-governance/src/engine.rs`, `crates/memory-governance/src/lib.rs`, `crates/memory-governance/tests/grounding_contract.rs`, `crates/memory-governance/tests/governance_matrix.rs`

**Invariants:** A naked URL remains ungrounded. A web capture ref is grounded only by verified local artifact + exact excerpt.  
**Out of scope:** `memoryd` protocol changes.

**Files:**

- Modify: `crates/memory-governance/Cargo.toml`
- Modify: `crates/memory-governance/src/grounding.rs`
- Modify: `crates/memory-governance/src/engine.rs`
- Modify: `crates/memory-governance/src/lib.rs`
- Modify: `crates/memory-governance/tests/grounding_contract.rs`
- Modify: `crates/memory-governance/tests/governance_matrix.rs`

**Step 1: Write failing tests**

Cover:

- `SourceKind::WebCapture` with `webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001` and a resolver returning `Resolved` passes.
- `SourceKind::WebCapture` with `https://example.com` fails.
- missing artifact fails.
- artifact hash mismatch fails.
- `webcap:<id>` without excerpt id fails.
- unsupported/partial capture fails.
- existing user/file/subagent grounding tests still pass unchanged.

**Step 2: Run RED**

```bash
cargo test -p memory-governance --test grounding_contract -- --test-threads=2
cargo test -p memory-governance --test governance_matrix -- --test-threads=2
```

Expected: fails because `WebCapture` source kind/resolver does not exist.

**Step 3: Implement resolver trait and engine generic**

Add:

```rust
pub trait WebCaptureResolver {
    fn resolve_web_capture(&self, source_ref: &str) -> SourceResolution;
}
```

Add `SourceKind::WebCapture`.

Change `GroundingVerifier` to be generic over both `SessionSpawnResolver` and `WebCaptureResolver`, or introduce a composite resolver that preserves clean constructor ergonomics. Because `GovernanceEngine` owns a `GroundingVerifier`, update `crates/memory-governance/src/engine.rs` constructors and type parameters explicitly. **Compatibility requirement:** the existing `GovernanceEngine::new(...)` constructor must keep compiling and must internally use `NeverResolveWebCapture`; add a new constructor such as `GovernanceEngine::new_with_web_capture_resolver(...)` for Task 8. This preserves all current `memoryd` call sites until web capture is wired.

Add deterministic test resolvers:

- `AlwaysResolveWebCapture`
- `NeverResolveWebCapture`

Run `cargo check -p memoryd` before ending Task 6 to prove the compatibility constructor preserves downstream compilation.

**Step 4: Run GREEN**

```bash
cargo test -p memory-governance --test grounding_contract -- --test-threads=2
cargo test -p memory-governance --test governance_matrix -- --test-threads=2
cargo test -p memory-governance -- --test-threads=2
cargo check -p memoryd
```

Expected: tests pass.

---

## Task 7: `memoryd` Capture Protocol, CLI, And MCP Tool

**Parallel:** no  
**Blocked by:** Task 6  
**Owned files:** `crates/memoryd/Cargo.toml`, `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/cli.rs`, `crates/memoryd/src/main.rs`, `crates/memoryd/src/mcp.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/tests/protocol_contract.rs`, `crates/memoryd/tests/source_capture_contract.rs`, `crates/memoryd/tests/mcp_contract.rs`, `crates/memoryd/tests/cli_contract.rs`

**Invariants:** Existing nine MCP tools keep backward-compatible schemas. The new `memory_capture_source` tool must not expose test-only URL bypasses, auth, local file capture, or admin-only behavior.  
**Out of scope:** Governance write acceptance of web refs; Task 8 owns that.

**Files:**

- Modify: `crates/memoryd/Cargo.toml`
- Modify: `crates/memoryd/src/protocol.rs`
- Modify: `crates/memoryd/src/cli.rs`
- Modify: `crates/memoryd/src/main.rs`
- Modify: `crates/memoryd/src/mcp.rs`
- Modify: `crates/memoryd/src/handlers.rs`
- Modify: `crates/memoryd/tests/protocol_contract.rs`
- Create: `crates/memoryd/tests/source_capture_contract.rs`
- Create or modify: `crates/memoryd/tests/mcp_contract.rs`
- Modify: `crates/memoryd/tests/cli_contract.rs`

**Step 1: Write failing protocol tests**

Update `crates/memoryd/tests/protocol_contract.rs` for JSON round trip and backwards-compatible shapes:

```rust
RequestPayload::CaptureSource {
    url: String,
    excerpts: Vec<String>,
    note: Option<String>,
}

ResponsePayload::CaptureSource(CaptureSourceResponse {
    artifact_id: String,
    source_refs: Vec<String>,
    final_url: String,
    captured_at: DateTime<Utc>,
    capture_status: String,
    warnings: Vec<String>,
})
```

**Step 2: Run RED**

```bash
cargo test -p memoryd --test protocol_contract -- capture_source --test-threads=2
```

Expected: fails because protocol variant does not exist.

**Step 3: Implement daemon handler**

Add handler path:

- Validate `excerpts` is non-empty and bounded, e.g. max 8 excerpts, max 2 KiB each.
- Validate `note` is bounded and safe plaintext if present.
- Call `memory_source::capture_web_source` with repo root.
- Return artifact id and refs.
- Map SSRF/unsupported/excerpt-not-found/privacy errors to structured `invalid_request` or `source_capture_failed` codes.

**Step 4: Add CLI**

CLI surface:

```bash
memoryd source capture --socket /tmp/memoryd.sock --url https://example.com/report --excerpt 'exact relevant quote'
```

No direct non-daemon capture mode in v0.1. Keeping capture daemon-backed prevents CLI drift from handler safety policy.

**Step 5: Add MCP tool**

Add `memory_capture_source` tool to MCP manifest.

Input:

```json
{"url":"https://example.com/report","excerpts":["exact quote"],"note":"optional operator note"}
```

Output: daemon response envelope with `source_refs`.

MCP schema must not expose any test bypass or local-network flag.

**Step 6: Run GREEN**

```bash
cargo test -p memoryd --test protocol_contract -- capture_source --test-threads=2
cargo test -p memoryd --test source_capture_contract -- --test-threads=2
cargo test -p memoryd --test mcp_contract -- --test-threads=2
cargo test -p memoryd --test cli_contract -- --test-threads=2
```

Expected: capture protocol, MCP manifest, and CLI tests pass.

---

## Task 8: Governed Writes Accept `web_capture` Source Refs

**Parallel:** no  
**Blocked by:** Task 7  
**Owned files:** `crates/memoryd/src/handlers.rs`, `crates/memoryd/tests/governance_web_capture.rs`, `crates/memory-governance/tests/governance_matrix.rs`

**Invariants:** `source_ref=https://...` must still fail grounding. `source_kind=web_capture` must require `webcap:<id>#quote_id`.  
**Out of scope:** Capture tool implementation.

**Files:**

- Modify: `crates/memoryd/src/handlers.rs`
- Create: `crates/memoryd/tests/governance_web_capture.rs`
- Modify: `crates/memory-governance/tests/governance_matrix.rs` only if needed for integration coverage.

**Step 1: Write failing integration tests**

Cover:

1. Build a verified source artifact with `ArtifactStore` fixture, not external network.
2. Write a memory with:

```json
{
  "namespace": "project",
  "source_kind": "web_capture",
  "source_ref": "webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001",
  "explicit_user_context": false,
  "confidence": 0.9
}
```

Expected: governance can promote or candidate-route according to policy, not refuse for grounding.

3. Same write with `source_ref="https://example.com/report"` refuses for grounding.
4. Same write after corrupting `extracted.txt` refuses for grounding.
5. Same write with `webcap:<id>` missing quote refuses for grounding.
6. Same write with `capture_status=partial` refuses for grounding.

**Step 2: Run RED**

```bash
cargo test -p memoryd --test governance_web_capture -- --test-threads=2
```

Expected: fails because `web_capture` source kind is unsupported in `memoryd`.

**Step 3: Implement metadata support**

Add `GovernanceSourceKindMeta::WebCapture` with serde name `web_capture`.

Map to:

- governance `SourceKind::WebCapture`
- substrate `SourceKind::Web` with `reference=webcap:<id>#quote`

Update `governance_engine(...)` construction so `MemorydSessionResolver` is paired with a repo-root-backed web capture resolver using `memory_source::ArtifactStore`.

**Step 4: Run GREEN**

```bash
cargo test -p memoryd --test governance_web_capture -- --test-threads=2
cargo test -p memory-governance --test grounding_contract -- --test-threads=2
```

Expected: tests pass.

---

## Task 9: Trust Artifact And Review Visibility For Web Evidence

**Parallel:** yes, after Task 8  
**Blocked by:** Task 8  
**Owned files:** `crates/memoryd/src/trust_artifact.rs`, `crates/memoryd-tui/src/widgets/trust_artifact.rs`, `crates/memoryd-web/src/routes/audit.rs`, `crates/memoryd/tests/trust_artifact.rs`, `crates/memoryd-tui/tests/trust_artifact.rs`, `crates/memoryd-web/tests/api_contract.rs`

**Invariants:** Do not expose full raw captured page content in trust artifacts. Show source metadata and the specific verified excerpt only.  
**Out of scope:** Entity graph and ROI dashboard changes.

**Files:**

- Modify: `crates/memoryd/src/trust_artifact.rs`
- Modify: `crates/memoryd-tui/src/widgets/trust_artifact.rs`
- Modify: `crates/memoryd-web/src/routes/audit.rs`
- Modify/create: `crates/memoryd/tests/trust_artifact.rs`
- Modify: `crates/memoryd-tui/tests/trust_artifact.rs`
- Modify: `crates/memoryd-web/tests/api_contract.rs`

**Step 1: Write failing tests**

Cover:

- trust artifact for a web-grounded memory shows `source.kind=web`, original/final URL, captured_at, artifact id, excerpt id, and bounded quote.
- raw page body is not included.
- missing/corrupt artifact marks source evidence as unavailable, not silently trusted.

**Step 2: Run RED**

```bash
cargo test -p memoryd --test trust_artifact -- web_source --test-threads=2
```

Expected: fails because trust artifact does not resolve `webcap:` refs.

**Step 3: Implement display projection**

Add a safe evidence projection to trust artifacts. Keep quote bounded, e.g. 500 bytes.

**Step 4: Run GREEN**

```bash
cargo test -p memoryd --test trust_artifact -- web_source --test-threads=2
cargo test -p memoryd-tui --test trust_artifact -- --test-threads=2
cargo test -p memoryd-web --test api_contract -- --test-threads=2
```

Expected: tests pass.

---

## Task 10: Eval Harness Coverage For Web-Source Grounding

**Parallel:** yes, after Task 8  
**Blocked by:** Task 8  
**Owned files:** `crates/memorum-eval/src/orchestrator.rs`, `crates/memorum-eval/src/harness_runner.rs`, `crates/memorum-eval/tests/eval/domain/t20_web_source_grounding.rs`, `crates/memorum-eval/tests/domain.rs`, `crates/memorum-eval/tests/orchestrator_integration.rs`

**Invariants:** New eval must run in mock/simulator mode without external network.  
**Out of scope:** Live LLM web browsing tests.

**Files:**

- Modify: `crates/memorum-eval/src/orchestrator.rs`
- Modify: `crates/memorum-eval/src/harness_runner.rs` if dispatch table needs the new test.
- Create: `crates/memorum-eval/tests/eval/domain/t20_web_source_grounding.rs`
- Modify: `crates/memorum-eval/tests/domain.rs`
- Modify: `crates/memorum-eval/tests/orchestrator_integration.rs`

**Step 1: Write failing eval test**

Test #20 should:

1. Start daemon scaffold.
2. Create a deterministic verified source artifact fixture.
3. Write a memory grounded by the returned `webcap:` ref.
4. Assert the write is not refused for grounding.
5. Corrupt the artifact.
6. Assert a second write using the same ref is refused for grounding.

**Step 2: Run RED**

```bash
cargo test -p memorum-eval --test domain -- t20_web_source_grounding --test-threads=2
```

Expected: fails because test and catalog entry do not exist.

**Step 3: Implement eval catalog addition**

Add catalog entry:

```text
20 web_source_grounding simulator/domain non-deferred
```

**Step 4: Run GREEN**

```bash
cargo test -p memorum-eval --test domain -- t20_web_source_grounding --test-threads=2
cargo test -p memorum-eval --test orchestrator_integration -- --test-threads=2
```

Expected: tests pass and catalog reports 20 tests.

---

## Task 11: Docs And Operator Runbook

**Parallel:** yes, after Task 8  
**Blocked by:** Task 8  
**Owned files:** `docs/api/web-source-grounding-api.md`, `docs/runbooks/web-source-grounding.md`, `README.md`

**Invariants:** Do not claim browser-rendered capture exists in v0.1. Do not tell agents that naked URLs are grounded. Do not update `CLAUDE.md` unless Trey separately asks to make this durable repo guidance after implementation.  
**Out of scope:** Spec rewrite unless Trey requests one.

**Files:**

- Create: `docs/api/web-source-grounding-api.md`
- Create: `docs/runbooks/web-source-grounding.md`
- Modify: `README.md` only for a short pointer.

**Step 1: Write docs after implementation behavior is green**

API doc must include:

- `memory_capture_source` MCP schema.
- `memoryd source capture` CLI examples.
- `webcap:<artifact>#<quote>` source-ref format.
- artifact layout.
- URL safety restrictions.
- privacy/copyright caveats.
- examples for successful and refused grounding.

Runbook must include:

- recommended agent workflow: capture first, then write memory citing the returned ref.
- what to do when quote is not found.
- what to do when page is JS-heavy: save local file or defer browser capture v0.2.
- how to inspect artifact integrity.

**Step 2: Verify docs links**

```bash
rg "web-source-grounding|memory_capture_source|webcap:" README.md docs/api docs/runbooks
```

Expected: pointers exist.

---

## Task 12: Security Review And Hardening Pass

**Parallel:** yes, after Task 8  
**Blocked by:** Task 8  
**Owned files:** `docs/reviews/2026-05-05-web-source-grounding-security-review.md`

**Read-only review scope:** `crates/memory-source/**`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/mcp.rs`, `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/cli.rs`, `crates/memoryd/src/main.rs`, `crates/memory-governance/src/grounding.rs`, `crates/memory-governance/src/engine.rs`, `crates/memory-substrate/src/merge/source_artifact.rs`, `crates/memory-substrate/src/tree/layout.rs`.

**Invariants:** Review must specifically look for SSRF, DNS rebinding/TOCTOU, credential leakage, proxy leakage, oversized response DoS, unsafe HTML/script handling, privacy leakage, raw artifact leakage, and MCP schema abuse.  
**Out of scope:** Feature implementation except tiny documentation corrections after review.

**Files:**

- Create: `docs/reviews/2026-05-05-web-source-grounding-security-review.md`

**Step 1: Confirm baked-in security tests pass first**

```bash
cargo test -p memory-source --test url_safety -- --test-threads=2
cargo test -p memory-source --test http_capture -- --test-threads=2
cargo test -p memory-source --test privacy_storage_policy -- --test-threads=2
cargo test -p memoryd --test source_capture_contract -- --test-threads=2
cargo test -p memoryd --test governance_web_capture -- --test-threads=2
```

Expected: all pass.

**Step 2: Perform review**

Produce a review doc with sections:

- Findings by severity.
- SSRF guard coverage.
- DNS pinning and redirect handling.
- Size/time limits.
- Privacy/indexing boundaries.
- MCP/CLI attack surface.
- Source artifact merge behavior.
- Residual risks.

**Step 3: Fix any P0/P1 before final gate**

Do not proceed to Task 13 with open P0/P1 findings.

---

## Task 13: Final Integration Gate

**Parallel:** no  
**Blocked by:** Tasks 1-12  
**Owned files:** `docs/reviews/2026-05-05-web-source-grounding-final-gate.md`

**Invariants:** Full gate runs once on integrated main worktree after narrow package gates pass.  
**Out of scope:** Unrelated cleanup.

**Files:**

- Create: `docs/reviews/2026-05-05-web-source-grounding-final-gate.md`

**Step 1: Narrow package gates**

```bash
cargo fmt --all -- --check
cargo clippy -p memory-source --all-targets --all-features -- -D warnings
cargo test -p memory-source -- --test-threads=2
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
cargo test -p memory-substrate --test source_artifact_merge_rules -- --test-threads=2
cargo clippy -p memory-merge-driver --all-targets --all-features -- -D warnings
cargo test -p memory-merge-driver --test merge_driver_cli -- --test-threads=2
cargo clippy -p memory-governance --all-targets --all-features -- -D warnings
cargo test -p memory-governance -- --test-threads=2
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
cargo test -p memoryd -- --test-threads=2
cargo test -p memorum-eval --test domain -- t20_web_source_grounding --test-threads=2
```

Expected: all pass.

**Step 2: Full gate**

```bash
BENCH_PROFILE=darwin-arm64 bash scripts/check.sh
```

Expected: pass. If it fails, record exact failure. The feature is not ready unless the failure is fixed or Trey explicitly grants a ship waiver for a proven-unrelated pre-existing failure.

**Step 3: Final report**

Write `docs/reviews/2026-05-05-web-source-grounding-final-gate.md` with:

- commands run
- pass/fail status
- any skipped checks and why
- unresolved risks
- exact artifact source refs used in test fixtures

---

## Acceptance Criteria

Implementation is ready when all are true:

1. `memory_capture_source` captures a public HTTP(S) URL as local source artifact files through the daemon.
2. The capture records original URL, final URL, redirect chain, timestamp, method, bounded request headers, status, selected response headers, remote address, extracted text hash, excerpt hash, and raw hash when raw is stored.
3. Requests are DNS-pinned to vetted public socket addresses with automatic redirects/proxies disabled.
4. `webcap:<artifact>#<quote>` refs are accepted by governance only when artifact integrity and exact excerpt verification pass.
5. Naked URLs still fail grounding.
6. Captured raw/extracted page content is not indexed into passive recall/search.
7. Extracted page text requiring encryption/refusal causes capture refusal in v0.1; raw bytes are omitted unless raw textual projection is safe.
8. SSRF-sensitive targets and redirects are rejected by default, with tests.
9. MCP/CLI surfaces are documented and tested and expose no test bypass.
10. Trust artifacts expose bounded evidence metadata/excerpt without full-page leakage.
11. Eval harness includes a deterministic domain test for web-source grounding.
12. Full project gate passes, or Trey grants an explicit waiver for a proven-unrelated pre-existing failure.

---

## Known Risks And Decisions For Trey

1. **v0.1 refuses some useful pages.** If a page contains surrounding PII or content requiring encryption, capture fails even if the desired quote is safe. This is intentional until encrypted source artifacts exist.
2. **Static HTTP capture misses JS-rendered content.** v0.1 marks such captures as partial unless the exact quote appears in extracted text. Browser-rendered capture should be v0.2.
3. **No semantic entailment.** v0.1 proves the excerpt existed; it does not prove the claim logically follows from the excerpt. Human/agent still bears that judgment.
4. **Raw response archival is conditional.** The ground truth for v0.1 is exact extracted text plus exact quote. Raw response storage is allowed only when privacy-safe.
5. **Subagent grounding remains separately weak.** This plan does not fix the current `MemorydSessionResolver` behavior for `session-spawn:` refs.
