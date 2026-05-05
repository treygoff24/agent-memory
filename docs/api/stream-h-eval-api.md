# Stream H Eval API

`memorum-eval` is the Stream H command-line orchestrator for the Memorum eval harness. It lists a 19-test catalog: 17 active tests plus 2 explicitly deferred tracking tests (#17/#18), plus future regression tests under `crates/memorum-eval/tests/eval/regression/`.

## CLI

```text
memorum-eval [OPTIONS]

Options:
  --harness <MODE>       Harness for real-harness tests: claude, codex, all, or mock.
                         Default: mock.
  --filter <PATTERN>     Run tests matching a number, name, group/name, or glob-like pattern.
                         Examples: t01, #13, handbook/*, domain/t15.
  --output <FORMAT>      Output format: json or text. Defaults to text on a TTY and json otherwise.
  --output-file <PATH>   Write the JSON report to a file in addition to stdout.
  --timeout <SECONDS>    Per-test timeout override. Use 0 only to verify timeout handling.
  --workers <N>          Parallel worker count for parallel-group simulator tests. Default: 4.
  --no-cleanup           Preserve temporary test trees for debugging.
  --list                 Print catalog entries and exit 0.
  -v, --verbose          Print per-test progress diagnostics to stderr.
```

## Harness modes

| Mode     | Behavior                                                                                                                                                           |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `mock`   | Default local/CI mode. Simulator tests run; real-harness tests (#13, #15, #19) skip with `SKIP_NO_AUTH`. The run is `partial: true` and exits 0 if nothing failed. |
| `claude` | Real-harness tests requiring Claude use the authenticated Claude CLI. Missing `MEMORUM_EVAL_CLAUDE_KEY` produces `SKIP_NO_AUTH` and exits 1.                       |
| `codex`  | Real-harness tests requiring Codex use the authenticated Codex CLI. Missing `MEMORUM_EVAL_CODEX_KEY` produces `SKIP_NO_AUTH` and exits 1.                          |
| `all`    | Full dogfood gate. Requires both `MEMORUM_EVAL_CLAUDE_KEY` and `MEMORUM_EVAL_CODEX_KEY`; any auth skip exits 1.                                                    |

## JSON report format

`--output json` writes a single JSON object:

```json
{
  "run_id": "eval-<monotonic-id>",
  "started_at": "unix-ms:<milliseconds>",
  "finished_at": "unix-ms:<milliseconds>",
  "harness_mode": "mock",
  "total": 19,
  "passed": 16,
  "failed": 0,
  "skipped": 3,
  "partial": true,
  "missing_credentials": ["MEMORUM_EVAL_CLAUDE_KEY", "MEMORUM_EVAL_CODEX_KEY"],
  "tests": [
    {
      "number": 1,
      "name": "exact_identifier_recall",
      "group": "handbook",
      "mode": "simulator",
      "deferred": false,
      "status": "passed",
      "duration_ms": 3,
      "assertions": 1,
      "assertions_passed": 1,
      "assertions_failed": 0,
      "failure_detail": null,
      "skip_reason": null,
      "skip_kind": null
    }
  ]
}
```

Field notes:

- `partial` is true when any test is skipped.
- `missing_credentials` is populated only for partial runs.
- Each test entry always includes `deferred`, `failure_detail`, `skip_reason`, and `skip_kind`; absent optional values are JSON `null`.
- `skip_kind` is `auth_missing`, `feature_deferred`, or `runtime_self_skip`. T17/T18 use `feature_deferred` in v1 because their upstream Stream F/D contracts are not shipped.
- Test statuses are `passed`, `failed`, or `skipped`.
- Test modes are `simulator` or `real_harness`.
- Test groups are `handbook`, `domain`, or `regression`.

## Exit codes

| Code | Meaning                                                                                                                                               |
| ---: | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
|    0 | All selected tests passed, or only real-harness tests were skipped in `--harness mock` mode.                                                          |
|    1 | One or more tests failed, or a non-mock full run skipped because required auth was missing.                                                           |
|    2 | Internal orchestrator error, such as invalid worker count, no tests matching `--filter`, socket/scaffold setup failure, or output-file write failure. |
|    3 | One or more tests exceeded the configured timeout.                                                                                                    |

## Test catalog

|   # | Name                                 | Group      | Mode         | Deferred | Execution |
| --: | ------------------------------------ | ---------- | ------------ | -------- | --------- |
|  01 | `exact_identifier_recall`            | handbook   | simulator    | no       | parallel  |
|  02 | `superseded_fact_handling`           | handbook   | simulator    | no       | parallel  |
|  03 | `cross_project_entity_collision`     | handbook   | simulator    | no       | parallel  |
|  04 | `abstention`                         | handbook   | simulator    | no       | parallel  |
|  05 | `poisoned_candidate`                 | handbook   | simulator    | no       | parallel  |
|  06 | `tool_output_preservation`           | handbook   | simulator    | no       | parallel  |
|  07 | `subagent_writeback`                 | handbook   | simulator    | no       | parallel  |
|  08 | `deletion_and_tombstone`             | handbook   | simulator    | no       | parallel  |
|  09 | `recall_budget_pressure`             | handbook   | simulator    | no       | parallel  |
|  10 | `compaction_resumption`              | handbook   | simulator    | no       | parallel  |
|  11 | `self_poisoning`                     | handbook   | simulator    | no       | parallel  |
|  12 | `temporal_validity`                  | handbook   | simulator    | no       | parallel  |
|  13 | `cross_harness_substrate_sharing`    | domain     | real_harness | no       | serial    |
|  14 | `merge_driver_semantic_correctness`  | domain     | simulator    | no       | serial    |
|  15 | `privacy_filter_refusal_retry`       | domain     | real_harness | no       | serial    |
|  16 | `reality_check_drift_scoring_sanity` | domain     | simulator    | no       | parallel  |
|  17 | `lease_contention_resolution`        | domain     | simulator    | yes      | serial    |
|  18 | `encrypted_tier_key_rotation`        | domain     | simulator    | yes      | serial    |
|  19 | `peer_update_framing_correctness`    | regression | real_harness | no       | serial    |

See `docs/runbooks/eval-real-harness-ci.md` for wiring real-harness secrets in GitHub Actions.

## Regression metadata contract

Every file matching `crates/memorum-eval/tests/eval/regression/t<NN>_*.rs` must start with a `//!` doc-comment block containing:

- test number;
- incident date;
- production failure description;
- root cause;
- fix commit;
- what the test asserts.

`cargo test -p memorum-eval --test regression_meta` enforces the directory path and metadata contract.
