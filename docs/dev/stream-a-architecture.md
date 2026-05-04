# Stream A architecture notes

## Specgate ownership doctor note

The installed Specgate CLI identifies itself as "Machine-checkable architectural intent for TypeScript projects" (`specgate --help`, tool version `0.3.1`). In this Rust workspace, `specgate validate` and `specgate check --output-mode deterministic` are meaningful green gates for config/policy, but `specgate doctor ownership --project-root . --format json` discovers only TypeScript/JavaScript-style source files.

Earlier Stream A Rust ownership stubs under `modules/stream-a-*.spec.yml` therefore appeared as `orphaned_specs` even though the Rust paths existed. Those stubs were removed during the 2026-05-04 dogfood-readiness pass so the canonical check gate stays warning-free. Rust ownership remains covered by `scripts/rust-boundary-check.sh`, crate boundaries, and targeted Rust tests.

If Specgate later gains Rust source discovery, reintroduce Rust module specs with a tool version note and promote ownership doctor back to a substantive Rust ownership gate.
