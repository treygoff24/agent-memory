# Governance Review Runbook

This runbook covers the implemented Stream C operator flow for governed writes
that become candidates or quarantines. It does not describe a Stream G UI.

## Start the daemon

From the repository root:

```bash
cargo run --bin memoryd -- serve --repo . --runtime .memoryd --socket .memoryd/memoryd.sock --init
```

Omit `--init` after the substrate has already been initialized.

## Write through governance

Use `write` for structured governed memories:

```bash
cargo run --bin memoryd -- write \
  --socket .memoryd/memoryd.sock \
  --title "Deployment target" \
  --tag deploy \
  --meta '{"namespace":"project","type":"decision","summary":"Deployment target is production","confidence":0.95,"source_kind":"user","explicit_user_context":true}' \
  'The deployment target is production.'
```

The CLI prints the protocol response as pretty JSON. `status: "promoted"` means
an active memory was written. `status: "candidate"` or `status: "quarantined"`
means the memory is visible in the review queue. `status: "refused"` means no
memory was written.

To create a review queue item for operator testing, submit an otherwise grounded
write below the selected policy confidence floor. For the built-in `agent-strict`
policy, `confidence: 0.5` triggers the `low_confidence` gate and writes a
`candidate` item visible in the review queue:

```bash
cargo run --bin memoryd -- write \
  --socket .memoryd/memoryd.sock \
  --meta '{"namespace":"agent","type":"claim","summary":"Needs confidence review","confidence":0.5,"source_kind":"user","explicit_user_context":true}' \
  'This claim is intentionally low confidence and needs review.'
```

For subagent/agent review-path testing, use `source_kind: "subagent"` with a
resolvable `source_ref` tied to the session-spawn proof expected by the local
grounding resolver. Caller-controlled quarantine overrides are intentionally not
part of the implemented metadata contract.

## List the review queue

```bash
cargo run --bin memoryd -- review queue --socket .memoryd/memoryd.sock --limit 20
```

Response shape:

```json
{
  "id": "cli-review-queue",
  "result": {
    "success": {
      "review_queue": {
        "items": [
          {
            "id": "mem_20260429_0123456789abcdef_000003",
            "summary": "Needs confidence review",
            "status": "quarantined",
            "policy_applied": "agent-strict@v3",
            "reason": "governance quarantine",
            "next_actions": ["review_approve", "review_reject"]
          }
        ]
      }
    }
  }
}
```

The queue is derived from canonical memory status/frontmatter. It is not a
separate side database.

## Approve or reject

Approve a queued memory:

```bash
cargo run --bin memoryd -- review approve --socket .memoryd/memoryd.sock mem_20260429_0123456789abcdef_000003
```

Reject a queued memory:

```bash
cargo run --bin memoryd -- review reject \
  --socket .memoryd/memoryd.sock \
  --reason "insufficient evidence" \
  mem_20260429_0123456789abcdef_000003
```

Both commands rewrite the canonical memory through Stream A with
`memoryd-review` event context. The response shape is:

```json
{
  "id": "cli-review-approve",
  "result": {
    "success": {
      "review_approve": {
        "id": "mem_20260429_0123456789abcdef_000003",
        "status": "active",
        "summary": "Needs confidence review"
      }
    }
  }
}
```

Reject returns `review_reject` with the same fields and the post-decision status.

## Supersede and forget

Supersede an existing memory:

```bash
cargo run --bin memoryd -- supersede \
  --socket .memoryd/memoryd.sock \
  --reason "deployment target changed" \
  --meta '{"namespace":"project","type":"decision","summary":"Deployment target is production","confidence":0.95,"source_kind":"user","explicit_user_context":true}' \
  mem_20260429_0123456789abcdef_000001 \
  'The deployment target is production.'
```

Forget a memory:

```bash
cargo run --bin memoryd -- forget \
  --socket .memoryd/memoryd.sock \
  --reason "user requested removal" \
  mem_20260429_0123456789abcdef_000002
```

Forget uses the Stream A tombstone path and removes the tombstoned memory from
daemon search results.

## Policies

Runtime policy files live at:

```text
<repo>/policies/*.yaml
```

If policy YAML exists and loads successfully, responses report
`policy_source: "disk"`. If no YAML exists, responses report
`policy_source: "built_in_fallback"` and use compiled policies:
`me-strict@v1`, `project-standard@v2`, `agent-strict@v3`, and
`dreaming-strict@v1`. Malformed or invalid disk YAML fails closed instead of
falling back to built-ins; repair the policy file and retry.

The implemented dry-run surface is library-only:
`Policy::dry_run(&CandidateContext)`. It reports the selected policy, confidence
floor result, review gates, grounding requirement, and tombstone mode without
writing. There is no `memoryd policy dry-run` CLI in Stream C.

## Refusals and retryability

Governance refusals are successful responses with `status: "refused"`; they do
not create a memory. Stable refusal reasons are `grounding`, `policy`,
`tombstone`, `contradiction`, `privacy`, `superseded`, and `review_required`.

Protocol errors are different:

- `invalid_request`, `retryable: false`: fix the request.
- `substrate_error`, `retryable: true`: retry may be appropriate after the
  substrate/runtime problem is resolved.
- `not_implemented`, `retryable: false`: applies to `memory_startup` only.

## Stream boundary

`memory_startup` is intentionally still Stream E. Stream C does not assemble
startup recall blocks, rank recall candidates, or define a human review UI. It
only governs write/supersede/forget decisions and exposes the CLI review queue.
