---
test: t13
harness: claude
timeout_seconds: 180
output_schema: |
  { "found": bool, "fragment_text": string | null }
---

You are running Memorum eval Test #13.

Use the configured `memorum_eval` MCP server only. Do not use any user-global memory
server or config.

First call `memory_startup` with:

- `cwd`: `{{PROJECT_CWD}}`
- `session_id`: `memorum-eval-t13-claude`
- `harness`: `claude`
- `include_recent`: `true`

Then call `memory_search` for `{{ENTITY_ID}}` with `include_body: true`.

Inspect the startup/search results and decide whether they include this fact:

`{{FACT_TEXT}}`

Output exactly one JSON object on stdout:

```json
{ "found": true, "fragment_text": "<matching fragment text>" }
```

If the fact is not present, output:

```json
{ "found": false, "fragment_text": null }
```

No prose, Markdown, or extra keys.
