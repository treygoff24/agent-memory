# Gap Worktree Salvage Plan

Date: 2026-05-22

## Run Contract

- Task class: backend/API, Rust refactor, dashboard behavior, source-capture/storage behavior.
- Risk level: high. This touches daemon protocol DTOs, web dashboard mappings, state history, source artifacts, privacy/storage semantics, and test gates.
- Objective: salvage the useful ideas from the abandoned gap worktrees into clean, verified code on a fresh branch from `main`.
- Non-goals: merge old worktrees wholesale, preserve ROI code unless rebuilt correctly, remove old worktrees before the new branch is verified, add new dependencies, change deployment config, or push without explicit follow-up.
- Assumption: `main` at `11c266b` is the integration base.
- Rollback: all work is on a new branch; old worktrees remain untouched as references.

## Acceptance Criteria

- [x] Create a fresh integration branch from clean `main`.
- [x] Implement daemon-backed dashboard status fields from the safe subset of `gap-daemon-dashboard`.
- [x] Fix or avoid brittle `StatusResponse` direct constructors in affected tests.
- [x] Wire `memoryd-web` status mapping to real daemon fields instead of zero/default placeholders.
- [x] Drop or defer the untrustworthy ROI salvage code unless it is rebuilt on typed data and verified.
- [x] Rebuild Reality Check history/progress from the idea, not the dirty branch shape.
- [x] Reality Check history exposes completed sessions with accurate confirmed, corrected, forgotten, not-relevant, skipped/deferred, and remaining/progress semantics.
- [x] Persistence failures in Reality Check completion are surfaced instead of silently ignored.
- [x] Rebuild source-capture privacy artifact support from the useful adapter/encryption ideas.
- [x] Source capture preserves HTTP final URL, redirects, and response metadata through adapter dispatch.
- [x] Encrypted extracted/raw artifacts use correct hash/integrity semantics and existing crypto/base64 APIs.
- [x] Source-capture public struct changes are propagated through daemon callers and tests.
- [x] Add or update targeted tests for daemon status, web status mapping, Reality Check history, and source-capture encrypted artifact behavior.
- [x] Run `cargo fmt --all -- --check`.
- [x] Run targeted cargo checks/tests for changed crates.
- [x] Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`, or record any pre-existing blocker precisely.
- [x] Send changed code for clean-code/Rust subagent review and address actionable findings.
- [x] Final git status and verification evidence are reported.

## Verification Plan

- `cargo fmt --all -- --check`
- `cargo check -p memoryd -p memoryd-web -p memory-source`
- Targeted tests for changed crates/routes/protocol contracts.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Read-only subagent review after local gates.
