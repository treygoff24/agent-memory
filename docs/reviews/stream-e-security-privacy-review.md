# Stream E Security/Privacy Review

Date: 2026-04-30
Scope: Review Gate C2 after `startup_recall_privacy` acceptance coverage.

## Verdict

No P0/P1 findings found in the current Stream E privacy surface.

## Checklist

- `memory_startup` and `delta-block` are read-only and do not call `memory_reveal`.
- Recall startup rendering uses Stream A recall-index rows and renders selected safe summaries only; rows with `index_body = false` or non-recall-safe sensitivity are omitted with `encrypted_body_hidden`.
- Candidate and quarantined rows contribute only pending-attention counts; their claim text is not emitted.
- CLI recall success writes XML only to stdout; unavailable/error diagnostics go to stderr with typed exit codes.
- Existing handler privacy descriptors now use the public Stream D `safe_plaintext_fragment` helper after the legacy private helper was renamed to `is_safe_plaintext_for_indexing`.
- XML rendering continues to escape text/attributes through the existing deterministic renderer.

## Verification

Passed locally:

```bash
cargo test -p memoryd --test startup_recall_privacy
cargo test -p memory-privacy --test safe_plaintext_fragment
cargo test -p memoryd --test recall_cli
```

## Residual notes

- The tests model encrypted-body recall exclusion through Stream A's safe recall-index projection (`index_body = false` / safe metadata), which is the Stream E boundary. Stream D remains responsible for actual decrypt/reveal authorization.
