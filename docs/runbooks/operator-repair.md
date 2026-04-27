# Operator repair runbook

## Durability unsupported

If `Substrate::open` returns `DurabilityUnsupported`, inspect filesystem support for parent-directory fsync. `force_unsafe_durability` is for tests/CI only and must be surfaced in every write outcome.

## Merge driver missing or stale

Run `git_preflight` with the expected merge-driver binary path. Repair by reinstalling the binary and updating local git config for `merge.memory-merge-driver.driver`.

## Startup quarantine

If startup reconciliation emits `OperatorRepairRequired`, pause writes, inspect the quarantine/runtime marker, repair or remove invalid memory files, then reopen the substrate.

## Event log recovery

A single malformed trailing JSONL frame is truncated. Non-final malformed lines require operator review because audit ordering may be compromised.

## Pending queues

Pending index/event queue replay is idempotent. If replay fails repeatedly, preserve the queue files and run a full reindex after resolving the underlying file or SQLite error.
