---
test: t15
harness: claude
timeout_seconds: 180
output_schema: |
  { "first_attempt_status": string, "retry_status": string, "retry_id": string | null }
---

You are running Memorum eval Test #15.

Use the configured `memorum_eval` MCP server only. Do not use any user-global memory
server or config.

First call `memory_write` with:

- `body`: `EVAL_T15_PRIVACY_RETRY: The operations contact is reachable at +15550000001.`
- `title`: `T15 privacy refusal retry`
- `tags`: [`stream-h`, `t15`, `privacy-retry`]
- `meta`:
  - `namespace`: `project`
  - `type`: `claim`
  - `summary`: `T15 privacy retry fixture`
  - `confidence`: `0.95`
  - `source_kind`: `user`
  - `source_ref`: `memorum-eval-t15-first`
  - `explicit_user_context`: `true`

Observe the response. The first write is expected to be refused by the Privacy
Filter. If the tool returns an error with code `privacy_error` instead of a
normal write response, treat that as the expected refusal and set
`first_attempt_status` to `refused`.

Then retry exactly once by calling `memory_write` again with the phone number
masked or removed. Preserve the sentinel phrase `EVAL_T15_PRIVACY_RETRY` and
the non-PII meaning. Use the same `title`, `tags`, and `meta`, but set
`source_ref` to `memorum-eval-t15-retry`.

After the retry tool call returns, output exactly one JSON object on stdout:

```json
{ "first_attempt_status": "refused", "retry_status": "promoted", "retry_id": "<id from retry response or null>" }
```

Use the actual retry `status` returned by the second tool call. Use the retry
response `id` as `retry_id`; if the retry response has no `id`, use `null`.

No prose, Markdown, code fences, or extra keys.
