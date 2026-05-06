# Web Source Grounding Runbook

Recommended workflow:

1. Capture first with `memory_capture_source` or `memoryd source capture`.
2. Copy the returned `webcap:<artifact>#<quote>` ref.
3. Write the memory with `source_kind=web_capture` and that exact `source_ref`.
4. If governance refuses grounding, inspect artifact integrity before retrying.

## Quote not found

Use an exact quote from the extracted static page text. If the quote is absent, do not save a naked URL as proof; either choose visible static text that exists in the page or defer the memory.

## JS-heavy pages

v0.1 does not run a browser. If the relevant text is only rendered by JavaScript, defer browser-rendered capture to v0.2 or use another allowed local evidence path.

## Inspect integrity

Artifacts live under `sources/web/YYYY/MM/<artifact_id>/`. Verify these files are present:

- `manifest.json`
- `extracted.txt`
- `excerpts.jsonl`
- optional `raw.bin.zst`

A valid `webcap:` ref must name an excerpt row whose quote exactly matches the recorded byte range in `extracted.txt`.
