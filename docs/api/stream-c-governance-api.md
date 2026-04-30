# Stream C Governance API

This document describes the implemented Stream C governance surface in `memoryd`.
It is not a Stream G UI spec.

## Transport

The daemon protocol is newline-delimited JSON over the `memoryd` Unix socket. Each
request frame has an `id` and a single externally tagged `request` variant. Each
response preserves the `id` and returns either `result.success` or
`result.error`.

Protocol errors have this shape:

```json
{
  "id": "req-bad",
  "result": {
    "error": {
      "code": "invalid_request",
      "message": "memory body must not be empty",
      "retryable": false
    }
  }
}
```

Current daemon error codes:

| Code              | Retryable | Meaning                                                                                           |
| ----------------- | --------- | ------------------------------------------------------------------------------------------------- |
| `invalid_request` | `false`   | Bad request shape, invalid memory id, empty body/query/note, or invalid governance metadata JSON. |
| `substrate_error` | `true`    | Stream A substrate read/write/index operation failed.                                             |
| `not_implemented` | `false`   | Reserved for explicitly deferred features such as non-null Stream E `since_event_id`.             |

Governance refusals are not protocol errors. They are successful governance
responses with `status: "refused"`, no memory id, and a stable `reason` code.

## Governance metadata

CLI `--meta` JSON maps to daemon `meta`. MCP `memory_write` and
`memory_supersede` also accept and forward `meta` to the daemon. `memory_forget`
has no metadata in either the MCP or daemon request shape.

Default metadata:

```json
{
  "namespace": "project",
  "type": "project",
  "summary": null,
  "confidence": 0.85,
  "sensitivity": null,
  "source_kind": "user",
  "source_ref": null,
  "explicit_user_context": false
}
```

Supported `namespace` values are `me`, `user`, `project`, and `agent` (`user`
maps to `me`). Supported `type` values are `project`, `claim`, `decision`,
`pattern`, `playbook`, `procedure`, and `artifact`. Supported `source_kind`
values are `user`, `agent_primary`, `subagent`, and `file`. Unknown metadata
fields, unsupported enum values, and non-finite or out-of-range confidence values
return `invalid_request`; caller-controlled quarantine overrides are not part of
the implemented metadata contract.

## `memory_write`

MCP tool arguments:

```json
{
  "body": "The deployment target is production.",
  "title": "Deployment target",
  "tags": ["deploy"]
}
```

Daemon request with metadata:

```json
{
  "id": "req-write-memory",
  "request": {
    "write_memory": {
      "body": "The deployment target is production.",
      "title": "Deployment target",
      "tags": ["deploy"],
      "meta": {
        "namespace": "project",
        "type": "decision",
        "summary": "Deployment target is production",
        "confidence": 0.95,
        "source_kind": "user",
        "explicit_user_context": true
      }
    }
  }
}
```

Promoted response:

```json
{
  "id": "req-write-memory",
  "result": {
    "success": {
      "governance_write": {
        "status": "promoted",
        "id": "mem_20260429_0123456789abcdef_000001",
        "namespace": "project",
        "reason": null,
        "next_actions": [],
        "policy_applied": "project-standard@v2",
        "policy_source": "built_in_fallback",
        "existing_id": null
      }
    }
  }
}
```

Other implemented write statuses:

- `candidate`: memory was written with candidate lifecycle state; `next_actions`
  names the policy gate or follow-up action.
- `quarantined`: memory was written with quarantined lifecycle state;
  `next_actions` names the quarantine/review reason. Use the daemon/CLI review
  queue commands to approve or reject queued items.
- `refused`: no memory was written; `reason` is one of the refusal reasons below.
- Duplicate/refinement decisions are represented as `status: "promoted"` with
  `existing_id` set and no second active memory created.
- Supersession suggestions are represented as `status: "candidate"`,
  `existing_id` set, and `next_actions: ["memory_supersede"]`.

## `memory_supersede`

MCP tool arguments:

```json
{
  "old_id": "mem_20260429_0123456789abcdef_000001",
  "new_body": "The deployment target is production.",
  "reason": "deployment target changed",
  "meta": {
    "namespace": "project",
    "type": "decision",
    "summary": "Deployment target is production",
    "confidence": 0.95,
    "sensitivity": "internal",
    "source_kind": "user",
    "explicit_user_context": true
  }
}
```

Daemon request with metadata:

```json
{
  "id": "req-supersede",
  "request": {
    "supersede": {
      "old_id": "mem_20260429_0123456789abcdef_000001",
      "content": "The deployment target is production.",
      "reason": "deployment target changed",
      "meta": {
        "namespace": "project",
        "type": "decision",
        "summary": "Deployment target is production",
        "confidence": 0.95,
        "source_kind": "user",
        "explicit_user_context": true
      }
    }
  }
}
```

Successful response:

```json
{
  "id": "req-supersede",
  "result": {
    "success": {
      "governance_supersede": {
        "status": "promoted",
        "new_id": "mem_20260429_0123456789abcdef_000002",
        "old_id": "mem_20260429_0123456789abcdef_000001",
        "reason": null,
        "chain": {
          "supersedes": ["mem_20260429_0123456789abcdef_000001"]
        },
        "policy_applied": "project-standard@v2",
        "policy_source": "built_in_fallback"
      }
    }
  }
}
```

If governance refuses the replacement content, the response is
`governance_supersede` with `status: "refused"`, `new_id: null`, `old_id` set,
and a refusal `reason`.

## `memory_forget`

MCP tool arguments and daemon request both carry an id and reason:

```json
{
  "id": "req-forget",
  "request": {
    "forget": {
      "id": "mem_20260429_0123456789abcdef_000002",
      "reason": "user requested removal"
    }
  }
}
```

Successful response:

```json
{
  "id": "req-forget",
  "result": {
    "success": {
      "governance_forget": {
        "status": "tombstoned",
        "id": "mem_20260429_0123456789abcdef_000002",
        "tombstone_ref": "tombstone:stream-a",
        "reason": null
      }
    }
  }
}
```

`memory_forget` calls the Stream A tombstone path. Tombstoned memories are not
returned by the daemon search path.

## Refusal reasons

Stable refusal reason codes are serialized as snake case:

| Reason            | Current source                                                                                    |
| ----------------- | ------------------------------------------------------------------------------------------------- |
| `grounding`       | Missing or insufficient local grounding for a governed write.                                     |
| `policy`          | Policy disallows promotion.                                                                       |
| `tombstone`       | Candidate matches an active tombstone rule.                                                       |
| `contradiction`   | Contradiction handling requires a non-promotion path.                                             |
| `privacy`         | Metadata requests sensitive/confidential/personal/secret handling that Stream D has not supplied. |
| `superseded`      | Candidate has already been superseded.                                                            |
| `review_required` | Human review is required before any write.                                                        |

A `privacy` refusal currently returns `next_actions:
["run_stream_d_privacy_classification"]`. It is non-retryable as-is unless the
caller can provide the missing Stream D classification path in a future stream.

## Policy loading and dry run

At runtime, `memoryd` looks for YAML files in:

```text
<repo>/policies/*.yaml
```

If the directory contains YAML files, `PolicySet::load_from_dir` is attempted. A
successful disk load records `policy_source: "disk"`. Malformed YAML, unknown
fields, invalid values, or missing required policy coverage fail closed: governed
writes/supersedes return refusal responses, or malformed metadata returns
`invalid_request`. If no policy YAML exists, compiled built-ins are used and
responses record `policy_source: "built_in_fallback"`.

Built-in policy markers are:

- `me-strict@v1`
- `project-standard@v2`
- `agent-strict@v3`
- `dreaming-strict@v1`

The governance crate has a `Policy::dry_run(&CandidateContext)` helper. It does
not mutate the substrate. It reports selected policy, policy source, candidate
confidence, confidence floor pass/fail, triggered review gates, grounding
requirement/satisfaction, and tombstone enforcement mode. There is no public
operator CLI for policy dry-run in Stream C; daemon responses expose the applied
policy and source after evaluation.

## Stream E startup recall

`memory_startup` is implemented by Stream E, not Stream C. Governance remains the authority for write/review lifecycle, while passive recall reads only governed Stream A index projections and excludes candidate/quarantine claims from factual output.
