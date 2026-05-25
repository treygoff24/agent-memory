# Stream B Daemon MCP API

This document lists the agent-facing MCP tools exported by `memoryd`. Admin and operator commands such as `memoryd dream ...`, privacy controls, device onboarding, review commands, and doctor/status repair flows are not MCP tools.

## Tool manifest

The MCP manifest exposes ten tools:

1. `memory_search`
2. `memory_get`
3. `memory_write`
4. `memory_supersede`
5. `memory_forget`
6. `memory_reveal`
7. `memory_startup`
8. `memory_note`
9. `memory_observe`
10. `memory_capture_source`

## `memory_capture_source`

`memory_capture_source` captures a daemon-mediated source as a local verified `webcap:` artifact before the agent writes a grounded memory. Alpha supports deterministic-first capture only:

- `http_static`: public HTTP(S) text/HTML fetched by the daemon.
- `local_artifact`: an operator-visible local file captured by path.

The schema also names `pdf_text`, `browser_rendered`, `screenshot`, and `authenticated` so clients can request them explicitly, but those modes fail closed with typed unsupported errors in this alpha. No browser cookies, screenshots, OCR, authenticated browser state, model privacy classifier toggles, key paths, raw key material, or privacy-bypass fields are accepted from MCP clients.

Input:

```json
{
  "source": "https://example.com/report",
  "mode": "http_static",
  "excerpts": ["exact quote present on the page"],
  "note": "optional operator context"
}
```

Local artifact input:

```json
{
  "source": "local-alpha-runbook",
  "mode": "local_artifact",
  "local_path": "/absolute/path/to/runbook.md",
  "excerpts": ["exact quote present in the file"]
}
```

Output includes `artifact_id`, one or more `source_refs`, the resolved capture `mode`, the redacted `final_url`, capture timestamp, capture status, and warnings.

Rules:

- URL fetching is daemon-mediated only.
- `url` remains accepted as a backward-compatible alias for `source`, but new clients should send `source`.
- Local/private network targets, embedded credentials, unsafe schemes, oversized bodies, unsupported content, and path traversal fail closed.
- `local_artifact` requires `local_path`; HTTP modes must not send `local_path`.
- Sensitive query parameters and redirect `Location` values are redacted before URLs are persisted.
- Additional fields are rejected by the MCP parser.
- `memory_reveal` is part of the v1 MCP contract, but the stdio bridge hides it by default; expose it only with `memoryd mcp --socket <path> --allow-reveal`.

## `memory_note`

`memory_note` is unchanged by Stream F. It accepts only canonical note text and forwards to `RequestPayload::WriteNote`.

Input:

```json
{
  "text": "A concise note that may become canonical memory."
}
```

Rules:

- `text` is required.
- No `kind`, `entities`, dream controls, or admin fields are accepted.
- The storage route is canonical memory handling, not Stream F substrate fragment handling.

## `memory_observe`

`memory_observe` is the Stream F substrate-fragment write surface. It captures low-level durable telemetry that may inform later dream passes but does not itself create a canonical memory.

Input:

```json
{
  "text": "Third time investigating JWT validation in this repo - pattern emerging around key rotation.",
  "kind": "pattern",
  "entities": ["ent_auth_flow", "ent_jwt"],
  "cwd": "/Users/treygoff/Code/agent-memory",
  "session_id": "sess_abc123",
  "harness": "codex",
  "harness_version": "0.1.0"
}
```

Schema:

- `text`: required string observation text, non-empty after trim and at most 16 KiB.
- `kind`: required string enum: `observation`, `pattern`, or `signal`.
- `entities`: optional array of entity id strings, defaulting to `[]` when omitted. Each id must match `^ent_[A-Za-z0-9_.:-]{1,124}$`; free-form emails, secrets, names, whitespace-polluted ids, or sensitive-looking `ent_...` ids are rejected before storage.
- `cwd`: required absolute caller working directory. The handler canonicalizes it and resolves `.memory-project.yaml` / git-remote project binding through the Stream E binding path.
- `session_id`: required caller session id, exact trimmed, at most 128 bytes, using only `[A-Za-z0-9_.:-]+`, and rejected if it contains secret/PII-looking material.
- `harness`: required caller harness name, exact trimmed, at most 128 bytes, using only `[A-Za-z0-9_.:-]+`, and rejected if it contains secret/PII-looking material.
- `harness_version`: optional caller harness version; when present, it follows the same safe metadata rule as `session_id` and `harness`.
- Additional properties are rejected.

Output:

```json
{
  "fragment_id": "sub_01HWPRZK1SPRAWM6EVQ6Y0XS8R",
  "target": "plaintext_substrate"
}
```

`target` is `plaintext_substrate` or `encrypted_substrate`.

Forwarding contract:

- MCP DTO: `ToolRequest::MemoryObserve`.
- Daemon protocol: `RequestPayload::Observe { text, kind, entities, cwd, session_id, harness, harness_version }`.
- Storage handler: implemented in `memoryd`; it validates caller binding before disk effects and appends a Stream F substrate fragment only after Stream D privacy routing succeeds.

Privacy and storage notes:

- `memory_observe` uses the Stream D privacy path before substrate disk effects.
- Plaintext substrate fragments are git-synced low-level telemetry under Stream F substrate paths; encrypted fragments are written under `encrypted/substrate/`.
- Secret/refused observations and invalid binding/entity requests fail closed with no substrate fragment.
- Scope is derived from validated binding: `project:<canonical_id>` when project binding exists, otherwise `agent`.

## Non-tools

The following remain CLI/admin surfaces and must not appear in the MCP manifest:

- `memoryd dream now`
- `memoryd dream status`
- `memoryd dream enable`
- `memoryd dream disable`
- privacy filter install/enable/disable commands
- device onboarding, key rotation, and revocation commands
- review queue approval/rejection commands
- doctor and repair commands
