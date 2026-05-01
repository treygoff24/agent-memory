# Stream F Final Gate Report

Date: 2026-05-01
Spec: `docs/specs/stream-f-dreaming-v0.2.md`
Plan: `docs/plans/2026-04-30-stream-f-dreaming.md`

## Verdict

PASS. Stream F v0.2 is build-ready/shipped from the implementation plan. All final review lanes reran after blocker fixes and returned PASS, and the explicit final gate commands passed from the current workspace.

## Final reviewer reruns

| Lane               | Final verdict | Notes                                                                                                                                                                        |
| ------------------ | ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Clean code         | PASS          | Pass 3 omission classification is explicit; no remaining clean-code blockers.                                                                                                |
| API / contract     | PASS          | `dream_disabled` is a stable non-retryable daemon code; manual disabled CLI exits before lease/output; Stream F exit docs match v0.2.                                        |
| Security / privacy | PASS          | Provider-specific harness env allowlists, stdin prompt transport, output safety, scope validation, file-ref containment, and disk-loaded substrate span masking are covered. |
| Performance        | PASS          | Stream F bench assert passed; cleanup remains host-noise-sensitive but under the v0.2 budget on the final run.                                                               |
| Test hardening     | PASS          | Full test/doc/lint/bench gate passed; no remaining test/gate blockers.                                                                                                       |

## Commands run

### Stream F acceptance slice

```bash
cargo test -p memory-substrate --test dream_canonical_isolation --test dream_substrate_primitives
cargo test -p memory-substrate --test dream_merge_rules
cargo test -p memoryd --test dream_canonical_isolation
cargo test -p memoryd --test dream_substrate_fragments --test dream_lease_election --test dream_lease_scheduled_retry --test dream_pass_pipeline --test dream_grounding_rehydration --test dream_harness_cli --test dream_cleanup --test dream_recall_integration --test dream_cli
```

Result: PASS.

### Full workspace gate

```bash
cargo test --workspace --all-targets --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
./scripts/rust-boundary-check.sh
pnpm exec oxfmt --check .
pnpm exec oxlint .
git diff --check
```

Result: PASS.

### Stream F benchmark assert

```bash
cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json
```

Result: PASS on final run.

Final measured values:

| Measurement                                       |   Final p95 |    Budget | Result |
| ------------------------------------------------- | ----------: | --------: | ------ |
| lease acquisition                                 |    88.257ms |  < 2000ms | PASS   |
| substrate fragment write / `memory_observe`       |     0.307ms |     < 5ms | PASS   |
| cleanup full pass representative                  | 33317.577ms | < 60000ms | PASS   |
| Stream E pending-attention question read overhead |     3.065ms |    <= 5ms | PASS   |

## Not run

`./scripts/check.sh` was not run as a monolithic command. The final gate was run as the explicit constituent commands from the Stream F plan to avoid unrelated release/smoke modes rewriting non-Stream-F benchmark artifacts. The release-gate contract test that checks the script contents passed inside `cargo test --workspace --all-targets --all-features`.

## Residual risks

- Cleanup benchmark p95 is a single-sample representative measurement and can be noisy under host contention. The final run was under budget.
- The `memory_observe` benchmark fixture uses the explicit best-effort fixture profile; full-durability repositories still fsync append and event records and are not certified by that latency number.
- `dream_disabled` has a stable daemon protocol code and manual CLI stderr code. Because v0.2 has no dedicated disabled CLI exit row, manual disabled dreams exit 1 via the generic non-retryable caller/config path before acquiring a lease.

No remaining blocker is known.
