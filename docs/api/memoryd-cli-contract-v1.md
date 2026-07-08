# memoryd CLI agent contract v1

**Status:** v1.0 — the machine contract for the agent-facing `memoryd` subcommands.
**Schema version emitted in every envelope:** `meta.schema_version = "1.0"`.
**Companion:** `memoryd schema --json` emits this contract from the same Rust types that implement it; the two must agree (pinned by `tests/cli_agent_envelope.rs` and the schema round-trip test).

This is the contract every covered command implements against and every test pins. It replaces the raw daemon frame (`{"id":..,"result":{"success":{..}}}`) that covered commands printed before this pass.

## 1. Envelope

Every **covered** command (§4) emits exactly one JSON object and nothing else on the success stream.

**Success** — one object on **stdout**, exit 0:

```json
{
  "ok": true,
  "data": { "...command-specific payload..." },
  "meta": { "schema_version": "1.0", "warnings": [] }
}
```

**Error** — one object on **stderr**, nonzero exit (§2):

```json
{
  "ok": false,
  "error": {
    "code": "not_found",
    "message": "human-readable, names the bad input and why",
    "details": null,
    "retryable": false,
    "suggested_fix": "the exact next command to run, or null"
  },
  "meta": { "schema_version": "1.0", "warnings": [] }
}
```

Rules:

- **stdout carries only the success envelope.** Diagnostics, tracing, and the first-write banner go to **stderr** and are never part of the contract. An agent parses stdout on exit 0 and stderr on nonzero exit.
- `data` is the inner payload DTO serialized directly (e.g. `search` → `{hits,total,guidance}`), never the daemon's `{"search":{...}}` wrapper.
- `meta.warnings` is always present (possibly empty). It carries non-fatal advisories: an empty-result broadening hint, or the DECISION-4 "accepted into review queue" notice.
- `error.retryable` mirrors the daemon's judgment; `error.suggested_fix` names the corrective move where one exists.
- Output for a given input is **byte-stable**: two identical invocations produce byte-identical stdout.

## 2. Exit codes

### 2.1 Enveloped agent commands (this contract)

| Condition | Exit |
| --- | ---: |
| Success — including a valid empty result and `Candidate`/`Quarantined` writes | 0 |
| Usage / argument error (clap) | 2 |
| Invalid input / validation / governance refusal (bad id format, malformed `--meta` JSON, refused write) | 65 |
| Well-formed id that does not exist (`not_found`) | 66 |
| Internal bug / invariant violation | 70 |
| Daemon unreachable / transient failure (retryable) | 75 |
| Client-side gate refusal (`reveal` without `--allow-reveal`) | 77 |
| Config problem detected pre-connect (bad socket path, missing repo/runtime dir) | 78 |

### 2.2 Dual-dictionary rule

The table above applies **only** to the covered commands (§4). Documented exceptions keep their own dictionaries, all published in `schema` output:

- **`doctor`** — linter-style dictionary: `0` healthy, `1` unhealthy. Un-enveloped by decision: it emits the raw daemon frame `{"id":...,"result":{"success":{"doctor":{"healthy":bool,...}}}}`, so read `.result.success.doctor.healthy`, **not** `.ok`/`.data`. A future pass that envelopes it is contract v2.
- **`recall startup-block` / `recall delta-block` / `recall hook`** — the pinned Stream E v0.7 dictionary (`src/cli/exit.rs::recall_exit_code`): `1` invalid/disabled, `2` substrate/recall/dream unavailable, `3` privacy, `4` not-implemented/pass-failed, `5` lease. `recall hook` is fail-open: always exit 0, zero bytes on failure. These emit raw block output, **not** the envelope.
- **`dream *`** — lease/dream dictionary via `LeaseError::cli_exit_code` / `DreamError`.
- **Admin / setup** (`init`, `uninstall`, `export`, `review`, `quarantine`, `peer`, `web`, `reality-check`, `privacy`, `device`, `ui`, `import`) — keep their current codes (typically `1`/`2`) until contract v2.

## 3. Daemon-code → exit crosswalk

The daemon emits free-form error-code strings inside a `Success`-shaped or `Error`-shaped frame. The covered commands translate each **daemon error code** to a contract exit code. The canonical vocabulary is registered in `crates/memoryd/src/handlers/error.rs::DAEMON_ERROR_CODES`; the crosswalk test enumerates it and fails on any code that falls through to the internal-error default — so vocabulary drift is caught at the gate.

| Daemon error code | Exit | retryable | Notes |
| --- | ---: | --- | --- |
| `invalid_request` | 65 | false | bad id format, empty text, bad `--meta` |
| `not_found` | 66 | false | well-formed id, no such memory (Task 2 split from `substrate_error`) |
| `substrate_error` | 75 | true | genuine transient substrate fault |
| `privacy_error` | 65 | false | classification unavailable / refused |
| `unsupported` | 65 | false | unsupported source-capture mode |
| `source_capture_failed` | 75 | false | fetch / integrity / IO failure during capture; the operation, not the input, failed |
| `trust_artifact_error` | 75 | true | trust-artifact read fault |
| `grounding_rehydration_failed` | 65 | false | review-time refusal (not reachable by covered commands; mapped for completeness) |
| `embedding_backlog` | 75 | true | embedding worker catching up |
| `embedding_worker_idle` | 75 | true | worker not yet spun up |
| `embedding_retry_budget_exhausted` | 75 | true | transient, budget reset on next cycle |
| `embedding_model_load_failed` | 70 | false | config/model fault, not transient |
| `embedding_provider_unsupported` | 70 | false | invariant 3: unknown embedding triple |
| `recall_unavailable` | 75 | true | (exception command; mapped for completeness) |
| `not_implemented` | 70 | false | |
| `dream_unavailable` | 75 | true | (exception command) |
| `dream_disabled` | 65 | false | (exception command) |
| `web_unavailable` | 75 | true | (web admin command) |
| `port_in_use` | 65 | false | (web admin command) |
| `method_not_allowed_on_mcp` | 70 | false | never reaches the CLI transport |

**Client-synthesized codes** (no daemon origin):

- Pre-connect config validation (bad socket path, missing repo/runtime) → **78**.
- `ECONNREFUSED`-class daemon-unreachable → **75**.
- `reveal` refused for missing `--allow-reveal`, before any socket connection → **77**.
- Auth/permission refusals stay **65** (the daemon has no auth taxonomy; 77 is client-only).

## 4. Covered commands

Side-effect class: `read_only` (no mutation), `mutating` (creates/updates memory through governance/substrate), `destructive` (tombstones). Idempotency noted where meaningful.

### `search` — `read_only`

`memoryd search <QUERY> [--limit N=10] [--include-body] [--socket PATH]`
`data`: `{ "hits": [SearchHit], "total": int, "guidance": string }`. Empty result → exit 0, `data.hits: []`, `meta.warnings` carries a broadening hint. `--include-body` opts into full bodies (default: bounded summaries). Byte-stable across identical calls.

### `get` — `read_only`

`memoryd get <ID> [--include-provenance] [--socket PATH]`
`data`: `{ "id", "summary", "body", "truncated", "provenance"?, "guidance" }`. Bodies are bounded **server-side** to 4096 bytes; `truncated: true` marks a cut body. There is no `--include-body` on `get`. Bad id format → 65; well-formed-but-missing id → 66 (`not_found`), `suggested_fix` points at `search`.

### `write-note` — `mutating`

`memoryd write-note <TEXT> [--socket PATH]`
Low-friction substrate note; lands immediately (no governance candidate step). `data`: `{ "id", "summary" }`. Empty text → 65. A secret-classified note is refused before disk effects → 65 (`privacy_error` / `invalid_request`).

### `write` — `mutating`

`memoryd write <BODY> [--title T] [--tag TAG]... [--meta JSON] [--socket PATH]`
Governed structured write. `data`: the `GovernanceWriteResponse` with **mandatory `status`** (`promoted`/`candidate`/`quarantined`; never `refused` in a success envelope). See §5 for the status→outcome mapping. Malformed `--meta` JSON → 65 with a minimal valid example in `suggested_fix`.

### `supersede` — `mutating`

`memoryd supersede <OLD_ID> <CONTENT> --reason R [--meta JSON] [--socket PATH]`
`data`: the `GovernanceSupersedeResponse` with mandatory `status`. Same status mapping as `write`.

### `forget` — `destructive`

`memoryd forget <ID> --reason R [--socket PATH]`
Tombstones a memory through governance. `data`: `{ "status", "id", "tombstone_ref"?, "reason"? }`. A refused forget → 65.

### `source capture` — `mutating`

`memoryd source capture (--url URL | --file PATH) [--mode MODE=http-static] [--excerpt Q]... [--note N] [--socket PATH]`
Captures source artifacts for grounded writes. `data`: the `CaptureSourceResponse`. `--url` requires `--mode http-static`; `--file` requires a local mode. Unsupported mode → 65 (`unsupported`); capture failure → 75 (`source_capture_failed`).

### `reveal` — `read_only` (audited)

`memoryd reveal <ID> --reason R --allow-reveal [--socket PATH]`
Audited unmask of encrypted content (Stream D). **Client-side gate:** without `--allow-reveal`, the CLI refuses **before any daemon request** → exit 77, `suggested_fix` names the flag and warns that a successful reveal writes an `EncryptedContentRevealed` audit event. With the flag: `data`: `{ "id", "summary", "body", "truncated", "guidance" }`. Non-encrypted target → 65. Empty/oversize reason → 65.

### `observe` — `mutating`

`memoryd observe <TEXT> --kind (observation|pattern|signal) [--entity ENT]... [--session-id S] [--harness H] [--socket PATH]`
Records a Stream F substrate observation. `--kind` is required. `text` is bounded to 16 KiB (65 on overflow); up to 32 entities, each ≤128 bytes, `ent_*`-validated (65 on violation). `--session-id` / `--harness` default from the environment (documented, not the protocol's synthetic constants) and can be overridden. `data`: `{ "fragment_id", "target" }`.

### `status` — `read_only`

`memoryd status [--socket PATH]`
`data`: the `StatusResponse` fields. Daemon down → 75.

### `schema` — `read_only`, local

`memoryd schema [all|commands|envelope|exit-codes] --json`
Prints this contract from the implementing Rust types. Never contacts the daemon. See Task 3.

## 5. Governance write-status mapping (DECISION-4)

A governed `write` / `supersede` / `forget` returns a `GovernanceStatus` inside a daemon `Success` payload. The CLI inspects it and maps:

| `status` | Envelope | Exit | `meta.warnings` | Notes |
| --- | --- | ---: | --- | --- |
| `promoted` | `ok:true`, `data.status="promoted"` | 0 | — | live and active |
| `tombstoned` | `ok:true`, `data.status="tombstoned"` | 0 | — | `forget` success |
| `candidate` | `ok:true`, `data.status="candidate"` | 0 | "accepted into review queue; not yet active; check `memoryd review queue`" | queued, not active |
| `quarantined` | `ok:true`, `data.status="quarantined"` | 0 | "quarantined for review; not yet active; check `memoryd review queue`" | queued, not active |
| `refused` | `ok:false`, `error.code = refusal reason` | 65 | — | not written; `suggested_fix` names the next move |

`data.status` is **mandatory** in every governance-write success envelope — an agent must never read a queued write as a completed one.

For `refused`, `error.code` is the `GovernanceRefusalReason` (snake_case): `grounding`, `policy`, `tombstone`, `contradiction`, `privacy`, `superseded`, `review_required`. `error.retryable = false`. `suggested_fix` by reason:

- `contradiction` → "run `memoryd search <topic>` to find the conflicting memory, then `memoryd supersede <old-id> ...` instead of a fresh write."
- `tombstone` → "this content was previously forgotten; `memoryd search` for the tombstone before rewriting."
- `superseded` → "the target was already superseded; `memoryd get <existing-id>` for the current version."
- `policy` / `grounding` / `privacy` / `review_required` → name the failing gate and the corrective step.

An unmapped `GovernanceStatus` is a hard failure in the envelope layer (test cell), never a silent `ok:true`.

## 6. Environment

- `MEMORUM_REPO` — canonical repo root (default `~/memorum`).
- `MEMORUM_SOCKET` — daemon socket path override.

Both are surfaced in `schema` output.
