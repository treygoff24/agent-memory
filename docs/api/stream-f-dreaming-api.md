# Stream F Dreaming API

Dreaming uses whichever agent-harness CLI you have installed and authenticated on this device (Claude Code, Codex CLI, Gemini, etc.). Dream prompts are masked through the agent-memory privacy filter before they leave the daemon, but the masked text is processed by the harness CLI's upstream model provider. The data, retention, and training policies of that provider apply. Where this device's selected harness CLI accepts prompts on stdin, the prompt is not visible to other local processes; where it does not, the prompt may be visible via process listing tools (`ps`, `top`, `/proc/<pid>/cmdline`). `memoryd dream status` shows the prompt-transport mode for each installed harness adapter. Substrate fragments written via `memory_observe` are git-synced as low-level durable telemetry; this means the private git repo's raw-observation surface is larger than its canonical-memory surface, even though substrate is not searchable as memory. If you don't want dream content sent to a particular provider, set the per-scope CLI priority to exclude it, or run `memoryd dream disable` on this device.

Stream F adds substrate observation capture, harness-CLI dream passes, lease-elected scheduled runs, candidate review output, cleanup reports, and Stream E pending-attention questions. It does not make dream prose canonical memory automatically.

## Status

Stream F v0.2 is shipped. The final release gate and review reruns are recorded in `docs/reviews/stream-f-final-gate-report.md`.

## Top-level repo paths

Stream F adds valid, noncanonical repo paths on top of Stream A's canonical memory tree:

| Path family                                          | Producer                         | Purpose                                                          | Merge behavior                                                                                                |
| ---------------------------------------------------- | -------------------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `substrate/<device_id>/<YYYY-MM-DD>.jsonl`           | `memory_observe`                 | Plaintext low-level observations, patterns, and signals.         | Append-only JSONL, de-duplicated by canonical row bytes and sorted by `id`.                                   |
| `substrate/archive/<device_id>/<YYYY-MM>.jsonl`      | Cleanup                          | Archived plaintext substrate after the active retention window.  | Same append-only JSONL semantics as daily substrate.                                                          |
| `encrypted/substrate/<device_id>/<YYYY-MM-DD>.jsonl` | `memory_observe` privacy routing | Encrypted substrate fragments for PII/confidential observations. | Append-only JSONL, de-duplicated by canonical row bytes and sorted by `id`; never decrypted by dream passes.  |
| `dreams/journal/<scope_path>/<YYYY-MM-DD>.md`        | Pass 1                           | Masked journal synthesis for a scope/date.                       | One-sided edits fast-path; contested same-scope/date edits quarantine with diagnostics preserving both sides. |
| `dreams/questions/<scope_path>/<YYYY-MM-DD>.jsonl`   | Pass 3                           | Recall-safe questions with explicit `entities`.                  | Append-only JSONL, de-duplicated and sorted by `(scope, ts, id)`.                                             |
| `dreams/cleanup/<device_id>/<YYYY-MM-DD>.json`       | Cleanup                          | Cleanup report for archival/deletion/retry decisions.            | JSON-object last-writer-wins by `(device_id, date)` using report timestamps when present.                     |
| `leases/journal.lease`                               | Scheduler                        | Lease election for daily dreaming.                               | Append-only JSONL lease history, de-duplicated and sorted deterministically.                                  |

These files validate through Stream A tree validators but are excluded from canonical-memory parsing, the SQLite memory index, `query_memory`, `query_recall_index`, and `query_chunks`.

## MCP surface

Stream F adds one agent-facing MCP tool: `memory_observe`. `memory_note` is unchanged and still routes to canonical note handling.

Worked `memory_observe` request:

```json
{
  "tool": "memory_observe",
  "arguments": {
    "text": "Third time debugging JWT rotation here; auth failures cluster around stale JWKS cache.",
    "kind": "pattern",
    "entities": ["ent_auth_flow", "ent_jwks"],
    "cwd": "/Users/treygoff/Code/agent-memory",
    "session_id": "sess_abc123",
    "harness": "codex",
    "harness_version": "0.1.0"
  }
}
```

Worked response:

```json
{
  "fragment_id": "sub_01HWPRZK1SPRAWM6EVQ6Y0XS8R",
  "target": "plaintext_substrate"
}
```

Rules:

- `kind` is `observation`, `pattern`, or `signal`.
- `entities` is optional but must contain safe entity ids when present; raw names, emails, secrets, and whitespace-polluted ids are rejected.
- The daemon validates caller binding from `cwd` before disk effects.
- Stream D classification runs before any substrate write.
- Secrets are refused with no fragment written.
- PII/confidential observations route to `encrypted/substrate/...` when key material is available.
- Plaintext substrate fragments written via memory_observe are git-synced and durable, but not searchable as canonical memory.

## Daemon protocol

`memory_observe` forwards to the daemon as:

```rust
RequestPayload::Observe {
    text,
    kind,
    entities,
    cwd,
    session_id,
    harness,
    harness_version,
}
```

The daemon responds with:

```rust
ResponsePayload::Observe(ObserveResponse {
    fragment_id,
    target,
})
```

Dreaming itself is daemon/admin-only and is not exposed through MCP. `memoryd dream now`, `memoryd dream scheduled`, `memoryd dream cleanup`, `memoryd dream status`, `memoryd dream review`, `memoryd dream enable`, and `memoryd dream disable` stay outside the MCP manifest.

## CLI/admin surface

```bash
memoryd dream status --repo . --runtime .memoryd
memoryd dream now --repo . --runtime .memoryd --scope project:agent-memory --cli codex
memoryd dream now --repo . --runtime .memoryd --scope project:agent-memory --cli echo --json
memoryd dream scheduled --repo . --runtime .memoryd --scope project:agent-memory --cli codex --json
memoryd dream cleanup --repo . --runtime .memoryd --json
memoryd dream review --repo . --runtime .memoryd --since 7d
memoryd dream disable --runtime ~/.memoryd
memoryd dream enable --runtime ~/.memoryd
```

Human `memoryd dream status` output begins with the privacy disclosure above as its first line when dreaming is enabled. JSON output is structured and includes CLI inventory, last runs, active leases, local disable state, and each adapter's `prompt_transport`.

The device-local sentinel `~/.memoryd/dream-disabled` disables scheduled and manual dreaming on that device without changing synced per-scope config. `memoryd dream enable` removes the sentinel only after showing the privacy disclosure on first-run confirmation.

`memoryd dream scheduled` is the production entry point for the scheduled lease path. It uses the same dream pipeline as `memoryd dream now`, but calls the scheduled lease wrapper: `lease_held` is recorded as a skip, transient `lease_unavailable` failures retry inside `dreams.dream_retry_window_minutes` using exponential backoff of 1, 2, 4, 8, 16, then capped 32-minute sleeps, and each run writes its per-device summary under `dreams/cleanup/<device_id>/<YYYY-MM-DD>.json`. The harness installer should register the OS scheduler (launchd, systemd, cron, or equivalent) to invoke this command daily at `dreams.cleanup_run_hour_utc`.

`memoryd dream cleanup` is the production entry point for the daily cleanup pass. It opens the local substrate, applies `dreams.fragment_lifetime_days`, `dreams.candidate_stale_days`, and `events.compaction_days`, writes `dreams/cleanup/<device_id>/<YYYY-MM-DD>.json`, and commits cleanup-authored changes when no unrelated user work is dirty. It accepts `--device-id` for explicit scheduler wiring and `--now <RFC3339>` for deterministic tests/replays; by default both values come from `local-device.yaml` and current UTC time. The harness installer should schedule cleanup after the scheduled dream retry window, typically at `dreams.cleanup_run_hour_utc + dreams.dream_retry_window_minutes`.

Exit behavior:

| CLI exit | Protocol code                                           | Meaning                                                                                                                                                        |
| -------: | ------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|        0 | success                                                 | Command completed.                                                                                                                                             |
|        1 | `invalid_request`                                       | Invalid scope, unknown CLI override, malformed `memory_observe` kind, malformed config, or another non-retryable caller/config error.                          |
|        2 | `dream_unavailable` / `recall_unavailable`              | No installed/authenticated harness is eligible for the selected scope, or the daemon/socket is unavailable for CLI client commands.                            |
|        3 | `privacy_error`                                         | Stream D refused fragment write or a privacy/masking diagnostic failed closed.                                                                                 |
|        4 | `dream_pass_failed`                                     | One or more dream passes failed; partial output may be on disk and details are in the per-pass `error_code` fields.                                            |
|        5 | `lease_held` / `lease_unavailable` / `lease_dirty_tree` | Manual dream lease could not be acquired. Scheduled runs treat `lease_held` as a skip and `lease_unavailable` as retryable within the configured retry window. |

`dream_disabled` is a stable non-retryable daemon protocol code for synced
`dreams.enabled = false` or the device-local sentinel. Manual `memoryd dream
now` reports `dream_disabled` on stderr before acquiring a lease and exits 1
via the generic non-retryable caller/config path.

## Harness CLI providers and prompt transport

Dreaming does not build a generic provider. It shells out to installed harness CLIs selected by synced per-scope priority and local availability.

| Adapter              | CLI                                                      | prompt_transport  | Provider disclosure                                                                                                                                                               |
| -------------------- | -------------------------------------------------------- | ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ClaudeCodeCli`      | `claude --print`                                         | `stdin`           | Uses Claude Code's configured upstream account/provider; upstream data policy applies.                                                                                            |
| `CodexCli`           | `codex exec -` or `codex exec --json -`                  | `stdin`           | Uses Codex CLI's configured upstream account/provider; upstream data policy applies.                                                                                              |
| Gemini / `GeminiCli` | deferred/disabled in v0.2 unless stdin support is proven | disabled/deferred | Disabled/deferred Gemini support is not accepted in `dreams.default_cli_priority` or per-scope overrides until prompt_transport and upstream data policy disclosure are reviewed. |

Adapters must declare `prompt_transport` truthfully. v0.2 ships only stdin-supporting adapters unless Trey approves an argv fallback; argv fallback would be disclosed in `memoryd dream status` because prompts could appear in process listings.

## Dream pass semantics

A manual or scheduled dream run:

1. Acquires or observes the `leases/journal.lease` election state.
2. Selects substrate fragments for the requested scope/window.
3. Masks prompt text with Stream D `MaskingSession`.
4. Runs the selected `HarnessCli` through its declared `prompt_transport`.
5. Restores masked output through the same session and drops the session afterward.
6. Writes Pass 1 journal output under `dreams/journal/...`.
7. Builds an evidence catalog and writes plaintext-safe Pass 2 candidates to the canonical candidate queue under `dreaming-strict` policy.
8. Writes Pass 3 recall questions under `dreams/questions/...` with explicit `entities` arrays.
9. Emits status counters and per-run reports.

Pass 2 never auto-promotes canonical memory. Candidates require normal governance/human review and may later be approved with review tooling. Restored candidates are re-classified through Stream D before any canonical write; `EncryptAtRest` candidates are intentionally refused with `encrypt_at_rest_candidate_refused` in v0.2 rather than written as encrypted dream candidates.

## Review, cleanup, and status

`memoryd dream review --since 7d` lists journal files, question files, and candidate queue entries for operator inspection. It does not approve candidates.

Cleanup archives or removes eligible substrate and dream artifacts according to retention rules and records reports under `dreams/cleanup/<device_id>/<YYYY-MM-DD>.json`. Cleanup commits are daemon-authored, separate from human-authored memory commits, and safe to retry.

Status surfaces include:

- selected per-scope CLI priority and local availability;
- prompt_transport per adapter;
- whether `dream-disabled` is active;
- active lease holder and expiry;
- last scheduled/manual run status;
- privacy disclosure and provider disclosure text;
- Stream E dream-question omission counters.

## Privacy, masking, and encrypted substrate

- `memory_observe` uses the same deterministic privacy classification boundary as canonical daemon writes.
- `MaskingSession::new`, `mask`, `restore`, and Drop-based teardown are the only masking lifecycle used by dream prompts and outputs.
- Journal and question passes use plaintext substrate plus content-aware safe descriptors for encrypted substrate; they never decrypt encrypted substrate fragments. The encrypted-fragment descriptor removes Stream D-detected private spans, emits only `safe_plaintext_fragment`-allowed summary/tags, and falls back to a generic encrypted-fragment descriptor if no safe signal remains.
- Prompt text is not logged, echoed, or passed through argv for shipped v0.2 adapters.
- Refused/secret content causes fail-closed behavior with no substrate fragment and no dream output commit.
- Provider disclosure is required because masked prompt text still leaves the daemon through the selected harness CLI and that provider's upstream data policy applies.

## Benchmarks and release gates

Stream F budgets from the v0.2 spec:

- `memory_observe` substrate write p95: `< 5ms`.
- Startup recall overhead from dream questions: `< 2ms` p95 over Stream E baseline.
- Daily dream run: bounded by configured per-pass harness timeout; subprocess time is not charged to substrate write latency.
- Cleanup: linear in eligible artifact count and idempotent on rerun.

Benchmark fixtures write proposed results rather than overwriting baselines. Stream F v0.2 shipped after the final release gate, including docs checks, `git diff --check`, Rust format/clippy/tests, and benchmark review, passed.
