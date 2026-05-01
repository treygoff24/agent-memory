# Stream F Final Security/Privacy Rerun Review

**Verdict: BLOCK.**

The current workspace closes the prior final-review blockers around observe classification-before-write, Pass 1/3 output safety, candidate plaintext validation, file-ref containment, cleanup source refs, and scope `.`/`..` validation. I did not find new risk in the explicitly best-effort benchmark/event paths.

Two S2 privacy/auth-boundary issues remain:

1. the harness subprocess environment allowlist is global, so every selected harness receives unrelated provider credentials/config; and
2. persisted plaintext substrate `privacy_spans` are dropped before prompt masking, so plaintext-allowed private values can reach external dream prompts unmasked.

## Findings by severity

### S2 — Harness subprocesses inherit unrelated provider credentials/config for every adapter

**Files:** `crates/memoryd/src/dream/harness.rs:19-29`, `crates/memoryd/src/dream/harness.rs:68-79`, `crates/memoryd/src/dream/harness.rs:344-379`, `docs/specs/stream-f-dreaming-v0.2.md:480-489`

**What I found:** The spec requires each harness subprocess to run with `PATH`, `HOME`, `TERM=dumb`, and **the harness CLI's own auth env vars** only (`docs/specs/stream-f-dreaming-v0.2.md:480-489`). The implementation instead has one global `DOCUMENTED_ENV_ALLOWLIST` containing Anthropic, Claude config, Codex config, Gemini/Google, and OpenAI variables (`crates/memoryd/src/dream/harness.rs:19-29`). `MinimalEnvironment::from_current` copies every present key from that global list (`crates/memoryd/src/dream/harness.rs:68-79`), and both real prompt execution and auth probes use that same environment for all adapters (`crates/memoryd/src/dream/harness.rs:344-379`).

That means a `claude` subprocess can receive `OPENAI_API_KEY`, `CODEX_HOME`, `GEMINI_API_KEY`, and `GOOGLE_API_KEY`; a `codex` subprocess can receive `ANTHROPIC_API_KEY`, `CLAUDE_CONFIG_DIR`, `GEMINI_API_KEY`, and `GOOGLE_API_KEY`.

**Exploitability:** Medium. A compromised or malicious harness binary on `PATH`, a hijacked CLI plugin/config, or model-mediated tool behavior inside an otherwise legitimate harness can read and exfiltrate credentials unrelated to the selected provider. This is especially sensitive because the dream harness is an explicit external-process trust boundary.

**Impact:** Cross-provider secret/config exposure. The bug violates the spec's least-privilege environment contract and can expose credentials for providers that were not selected for the dream run.

**Minimal remediation:** Make env allowlisting adapter-specific instead of global.

- Add an adapter method such as `auth_env_keys()` / `environment_allowlist()`.
- Keep common keys to `PATH`, `HOME`, and forced `TERM=dumb`.
- Claude should receive only Claude/Anthropic-specific auth/config keys.
- Codex should receive only Codex/OpenAI-specific auth/config keys.
- Gemini/Google keys should not be exposed unless a reviewed Gemini adapter is enabled.
- Add a regression test that sets all provider canaries in the parent env and asserts each adapter's child process sees only its own provider keys plus the common keys.

### S2 — Disk-loaded plaintext substrate privacy spans are discarded before prompt masking

**Files:** `crates/memoryd/src/handlers.rs:439-469`, `crates/memoryd/src/dream/orchestration.rs:402-414`, `crates/memoryd/src/dream/orchestration.rs:432-443`, `crates/memoryd/src/dream/run.rs:276-289`, `docs/specs/stream-f-dreaming-v0.2.md:157-163`

**What I found:** Observe now correctly classifies text before the substrate append and persists `privacy_spans` with plaintext/encrypted substrate records (`crates/memoryd/src/handlers.rs:439-469`). But the dream loader drops those persisted spans: `collect_plaintext_fragments` wraps every parsed plaintext record with `text_spans: Vec::new()` (`crates/memoryd/src/dream/orchestration.rs:402-414`), and `parse_plaintext_fragment` only reads `id`, `kind`, `ts`, `entities`, and `text` (`crates/memoryd/src/dream/orchestration.rs:432-443`). Prompt masking then calls `masking.mask(&fragment.text, &input.text_spans)` (`crates/memoryd/src/dream/run.rs:276-289`), but the span set is empty for real disk-loaded plaintext substrate records.

The spec says Stream F uses one masking session per run and that **every dream prompt input passes through `MaskingSession::mask`** (`docs/specs/stream-f-dreaming-v0.2.md:157-163`). Calling `mask` with an empty span list does not satisfy that for persisted records that already carry classified spans. This affects plaintext-allowed private labels such as private URLs/dates, which may remain on disk by policy but should still be masked before being sent to an external harness prompt.

**Exploitability:** Medium. Any plaintext substrate record with non-empty `privacy_spans` that is within the dream window/scope can be included in a dream prompt with the original value intact. No malicious harness is required; the normal selected harness receives the unmasked prompt input.

**Impact:** Unmasked private values can cross the local-to-harness privacy boundary despite the masked-synthesis contract. This also weakens Pass 2 restoration safety because the model may learn/echo original private values that should have been represented only by stable mask labels.

**Minimal remediation:** Hydrate persisted `privacy_spans` when loading plaintext substrate fragments.

- Parse `privacy_spans` from JSONL into `Vec<PrivacySpan>` and store it in `DreamSubstrateFragmentInput.text_spans`.
- Fail closed or omit the fragment if a persisted span is malformed rather than silently treating it as public.
- Add a disk-backed regression test: write/observe a plaintext-allowed private value with a privacy span, run/build the dream prompt from disk-loaded substrate, and assert the original value is absent while a mask label is present.
- Keep encrypted descriptor behavior separate; descriptors may remain spanless if they are already safe summaries.

## Closed prior blocker confirmations

- **Privacy classification before disk effects:** `memory_observe` validates input/binding, runs `classify_privacy`, refuses storage on secrets, chooses encrypted vs plaintext payload, and only then appends the substrate fragment (`crates/memoryd/src/handlers.rs:423-473`). Existing tests cover secret/email/phone-sensitive metadata rejection and PII encryption before plaintext leak.
- **Pass 1 output safety:** Pass 1 rejects empty or unsafe harness output before journal path creation/write (`crates/memoryd/src/dream/pass1.rs:34-54`).
- **Pass 3 output safety:** Pass 3 parses each JSONL line, rejects malformed/unknown/original-private/unsafe questions, and writes only filtered records (`crates/memoryd/src/dream/pass3.rs:44-60`, `crates/memoryd/src/dream/pass3.rs:78-104`).
- **Candidate writer plaintext safety:** Pass 2 restores masked fields immediately before candidate write (`crates/memoryd/src/dream/pass2.rs:62-72`), and `SubstrateCandidateWriter` rejects unsafe claim/rationale/evidence plaintext before allocating an id or calling `write_memory` (`crates/memoryd/src/dream/orchestration.rs:148-179`, `crates/memoryd/src/dream/orchestration.rs:316-330`).
- **File-ref containment:** Grounding rehydration resolves file refs through a repo-relative helper that rejects empty/NUL/backslash refs, absolute paths, `.`/`..` components, and canonical escapes/symlinks (`crates/memoryd/src/dream/rehydration.rs:141-159`, `crates/memoryd/src/dream/rehydration.rs:312-345`).
- **Cleanup source refs:** Cleanup `observed_at` refresh uses the same contained file-ref resolver and skips invalid/escaping refs (`crates/memoryd/src/dream/cleanup.rs:230-266`).
- **Scope validation:** scope ids now reject empty, `.`/`..`, all-dot ids, overlong ids, and non-ASCII path-ish bytes (`crates/memoryd/src/dream/scope.rs:65-78`).
- **Prompt transport safety:** v0.2 real adapters use stdin transport (`crates/memoryd/src/dream/harness.rs:200-205`, `crates/memoryd/src/dream/harness.rs:275-282`); hardened execution writes stdin, not argv, and redacts captured diagnostics (`crates/memoryd/src/dream/harness.rs:395-445`). This is separate from the env least-privilege blocker above.
- **Best-effort benchmark/event paths:** benchmark output requires explicit `--write-output` and refuses to write failing baselines (`crates/memoryd/src/bin/stream_f_dream_bench.rs:136-156`, `crates/memoryd/src/bin/stream_f_dream_bench.rs:474-480`). Best-effort event append is only used when the substrate durability tier is explicitly `BestEffort`; full durability continues through the fsyncing append path (`crates/memory-substrate/src/api.rs:1298-1315`, `crates/memory-substrate/src/events/log.rs:172-183`). JSONL fsyncs also remain gated on full durability (`crates/memory-substrate/src/api.rs:1407-1444`).

## Validation run

Passed focused security/privacy tests:

```text
cargo test -p memoryd --test dream_pass_pipeline --test dream_grounding_rehydration --test dream_cleanup --test dream_scope_and_prompts --test dream_harness_cli --test dream_substrate_fragments --test dream_cli
# dream_cleanup: 11 passed
# dream_cli: 7 passed
# dream_grounding_rehydration: 13 passed
# dream_harness_cli: 8 passed
# dream_pass_pipeline: 16 passed
# dream_scope_and_prompts: 3 passed
# dream_substrate_fragments: 13 passed

cargo test -p memory-substrate --test dream_substrate_primitives --test dream_canonical_isolation --test event_kind_schema
# dream_canonical_isolation: 8 passed
# dream_substrate_primitives: 5 passed
# event_kind_schema: 1 passed
```

These tests are useful regression coverage for the previously blocked areas, but they do not cover the two remaining issues above: per-adapter env separation and disk-hydrated privacy-span masking.

## Residual risk and confidence

Residual risk is moderate until the two S2s are fixed and tested. The file containment, scope validation, pass output filtering, and candidate writer safety areas are now high-confidence based on code inspection plus focused tests. Confidence in this BLOCK verdict is high because both findings are direct source/spec mismatches with clear privacy or credential-boundary impact.
