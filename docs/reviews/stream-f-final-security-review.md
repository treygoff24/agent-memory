# Stream F Final Gate E Security/Privacy Review

## Verdict

**Blocked for final security release.** I found **no S1 findings**, but I found **two S2 findings** that should be fixed before Stream F is declared shipped, plus one S3 hardening issue.

Security posture that looks sound in this diff:

- `memory_observe` runs Stream D deterministic privacy classification before substrate writes and refuses secret-like input before fragment creation.
- encrypted substrate records omit a plaintext `text` field.
- shipped v0.2 harness adapters declare `stdin` prompt transport and the harness tests verify prompt bytes do not appear in adapter argv/stderr diagnostics.
- `memoryd dream ...` admin commands are absent from the MCP manifest.
- public docs and human `dream status` include the required provider/privacy disclosure.

## Findings

### S2 - Harness Pass 1/3 output is written to git-synced dream files without output privacy classification

**Evidence:**

- `crates/memoryd/src/dream/pass1.rs:27-47` renders the prompt, receives arbitrary harness stdout, checks only for empty output, then writes the output directly to `dreams/journal/<scope>/<date>.md`.
- `crates/memoryd/src/dream/pass3.rs:75-88` validates JSONL shape, non-empty entities, allowed entity ids, and whether the question contains an exact original private span; it does not run the Stream D classifier over each question before writing.
- `crates/memoryd/src/dream/pass3.rs:95-103` serializes accepted question records directly to `dreams/questions/<scope>/<date>.jsonl`.
- `crates/memoryd/src/recall/dream_questions.rs:122-124` applies `safe_plaintext_fragment` later before recall emission, but by then the question has already been written to the git-synced repo surface.

**Exploitability:** Medium. The harness CLI/model output is an external trust boundary. A malicious, compromised, or prompt-injected harness can return raw PII or secret-looking text in Pass 1 or Pass 3 output. Pass 3 catches only exact original private values already tracked by `MaskingSession`; it does not catch newly generated secrets, missed spans, emails/phones/API-key-shaped text, or secrets copied from a compromised provider/tooling layer.

**Impact:** PII/secret-like content can be persisted into `dreams/journal/**` or `dreams/questions/**`, which are explicitly git-synced Stream F files. Recall emission has a safety filter, but repo persistence and git sync have already happened. This breaks the intended masked-output/privacy boundary for noncanonical dream prose.

**Minimal remediation:**

1. Treat harness output as untrusted before any disk write.
2. For Pass 1, run deterministic Stream D classification or `safe_plaintext_fragment` over the full markdown output before writing. If unsafe, fail Pass 1 closed and write no journal file.
3. For Pass 3, run the same safety classifier on each question before serialization; omit unsafe records and increment `dream_question_omitted_total{reason: unsafe_fragment}` or the local Pass 3 counter equivalent.
4. Add regression tests where EchoCli returns `alice@example.com`, a phone number, and `sk_live_...` in Pass 1/3 output and assert no dream file persists unsafe text.

### S2 - File grounding and cleanup source paths can escape the repo and use local filesystem state as an oracle

**Evidence:**

- `crates/memoryd/src/dream/rehydration.rs:138-155` verifies arbitrary file refs by reading `file_reference_path(...)`; if a quote is supplied, it reads the target file content for drift comparison.
- `crates/memoryd/src/dream/rehydration.rs:309-316` explicitly accepts absolute paths by returning `path.to_path_buf()` for `Path::is_absolute()`, and otherwise joins the raw reference to the repo without rejecting `..` components.
- `crates/memoryd/src/dream/cleanup.rs:241-249` uses `repo.join(source_ref)` and reads file metadata for `observed_at` refresh without constraining `source_ref` to a safe repo-relative path.

**Exploitability:** Medium. A synced or manually inserted dream candidate can cite `file:/absolute/local/path` or `../...` in evidence/source refs. On approval, rehydration can promote/quarantine based on whether that local path exists and whether its contents roughly match the supplied quote. Cleanup can also persist the mtime of an arbitrary local file into frontmatter `observed_at` if a memory source ref points outside the repo.

**Impact:** This breaks the source/evidence validation boundary. It can leak local file existence and metadata into memory state and git history, and it can let a dream candidate claim grounding against files that are not part of the canonical memory repo. The direct content read in rehydration is not printed, but the approve/quarantine result creates a content/existence oracle.

**Minimal remediation:**

1. Add one shared `resolve_repo_relative_file_ref(repo, reference) -> Result<RepoPath, ...>` helper.
2. Strip only the optional `file:` prefix and fragment suffix, then reject absolute paths, NULs, empty paths, `.` / `..` components, Windows prefixes, and symlink escapes.
3. Canonicalize the resolved parent/target where it exists and require it to remain under the canonical repo root.
4. Use that helper in both grounding rehydration and cleanup `observed_at` refresh.
5. Add tests for `/etc/passwd`, `file:/Users/...`, `../outside`, and symlink-to-outside refs; all should quarantine/skip without reading external metadata/content.

### S3 - Dream scope IDs allow path-special dot segments before direct dream-file writes

**Evidence:**

- `crates/memoryd/src/dream/scope.rs:65-68` accepts any id made of ASCII alnum plus `_`, `-`, or `.`, which includes `.` and `..`.
- `crates/memoryd/src/dream/pass1.rs:40-47` writes `repo_root.join(relative_path)` directly for the derived journal path.
- `crates/memoryd/src/dream/pass3.rs:45-51` writes `repo_root.join(relative_path)` directly for the derived questions path.

**Exploitability:** Low. The current grammar disallows `/`, so this is not a broad arbitrary path traversal. But `project:..` derives paths like `dreams/journal/project/../<date>.md`, which can write outside the intended `project/<id>/` subtree and produce invalid Stream F layout files.

**Impact:** A malformed manual/admin dream scope can create or overwrite unexpected files under `dreams/journal` or `dreams/questions`, bypassing the intended scope-path shape. Tree validation should later reject the file, but the write has already happened.

**Minimal remediation:**

1. Reject `.` and `..` scope IDs explicitly, and consider rejecting all-dot ids.
2. Build dream output paths through `RepoPath::try_new` or a dedicated Stream F path constructor before writes.
3. Add tests for `project:.`, `project:..`, `org:.`, and `org:..` asserting `invalid_request` and no file creation.

## Required fixes

1. Add output privacy classification before Pass 1 journal and Pass 3 question writes; fail/omit unsafe model output before git-synced persistence.
2. Constrain all file grounding and cleanup source refs to safe repo-relative paths; reject absolute, parent traversal, and symlink-escape refs.
3. Harden dream scope id validation against `.` / `..` segments and validate generated output paths before direct `fs::write` calls.
4. Rerun this security lane after fixes, plus the relevant targeted tests listed below.

## Residual risks

- I did not run the full workspace Gate E (`cargo test --workspace --all-targets --all-features`, clippy workspace, rustdoc workspace). This was a security/privacy review pass with targeted gates.
- Manual `dream now --cli <name>` still selects a known adapter without a prior auth probe in `main.rs`; adapter execution should fail if unauthenticated, but the model-boundary behavior is less explicit than default priority selection.
- Dream candidate persistence is currently abstracted behind `CandidateWriter`; this review focused on the implemented runner and review/rehydration paths. When a real writer replaces `NoopCandidateWriter`, re-audit that restored Pass 2 fields always pass through Stream D/governance before disk effects.

## Commands run

```bash
sed -n '1,260p' docs/plans/2026-04-30-stream-f-dreaming.md
sed -n '1,940p' docs/specs/stream-f-dreaming-v0.2.md
git status --short
git diff --stat
git diff --name-status
rg -n "PromptTransport|stdin|argv|stderr|env|HOME|PATH|spawn|Command|current_dir|scratch|API_KEY|SECRET|log|stderr_tail" crates/memoryd/src crates/memoryd/tests docs/api/stream-f-dreaming-api.md README.md CLAUDE.md
rg -n "MaskingSession|mask\(|restore\(|memory_reveal|privacy|encrypted/substrate|safe_plaintext|Secret|Observe|memory_observe" crates/memoryd/src crates/memory-substrate/src crates/memoryd/tests docs/api
rg -n "dream now|DreamNow|DreamStatus|memoryd dream|MCP|ToolName|dream status|dream review|dream-disabled|privacy disclosure" crates/memoryd/src crates/memoryd/tests docs/api README.md CLAUDE.md
cargo test -p memoryd --test dream_harness_cli --test dream_substrate_fragments --test dream_grounding_rehydration --test dream_recall_integration --test dream_cli --test mcp_manifest
cargo test -p memoryd --test dream_pass_pipeline --test dream_cleanup
cargo fmt --all -- --check
git diff --check
git status --short
```

All cargo test/fmt/diff-check commands above passed.
