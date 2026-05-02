---
test: t13
harness: codex
timeout_seconds: 180
output_schema: |
  { "observed": bool, "fragment_id": string | null }
---

You are running Memorum eval Test #13.

Use the configured `memorum_eval` MCP server only. Do not use any user-global memory
server or config.

Call `memory_observe` exactly once with:

- `text`: `{{FACT_TEXT}}`
- `kind`: `pattern`
- `entities`: [`{{ENTITY_ID}}`]
- `cwd`: `{{PROJECT_CWD}}`
- `session_id`: `memorum-eval-t13-codex`
- `harness`: `codex`

After the tool call returns, output exactly one JSON object on stdout:

```json
{ "observed": true, "fragment_id": "<fragment id from memory_observe>" }
```

No prose, Markdown, or extra keys.
