# Final security audit — Streams F/G/H/I dogfood readiness

Date: 2026-05-04
Verdict: no Blocker findings found in the touched dogfood-readiness surfaces.

## Scope reviewed

- Stream F EncryptAtRest Pass 2 refusal flow (`crates/memoryd/src/dream/orchestration.rs`, `docs/specs/stream-f-dreaming-v0.3.md`)
- Stream F auth probe diagnostics (`crates/memoryd/src/dream/harness.rs`, `crates/memoryd/src/dream/status.rs`, `crates/memoryd/src/handlers.rs`)
- Install/scheduling scripts (`scripts/install-memorum.sh`, `scripts/install-launchd.sh`)
- TUI Recall panel (`crates/memoryd-tui/src/panels/recall.rs` and wiring)

## Findings

None at Blocker severity.

### Low — auth probe diagnostics include daemon PATH

The plan explicitly required `CliMissing` diagnostics to show the daemon PATH. This can disclose local install paths in operator-facing output. That is acceptable for `memoryd doctor`/`dream status` local diagnostics, but these strings should not be forwarded into MCP responses or external notifications. Current implementation emits them through local daemon status/doctor surfaces only.

### Low — install scripts trust caller-provided paths

The launchd and install scripts interpolate caller-supplied repo/runtime paths. Arguments are quoted in shell execution, and the plist render is intended for local operator use. Avoid running these scripts with untrusted path input.

## Positive checks

- EncryptAtRest candidates return only `privacy_required_encryption`; the candidate content is not embedded in the refusal reason.
- Harness subprocess execution still uses stdin transport and environment allowlists.
- Doctor harness warnings are non-fatal for otherwise healthy substrate state.
- TUI Recall displays daemon-provided metadata summaries only; it does not introduce a new persistence or reveal path.
