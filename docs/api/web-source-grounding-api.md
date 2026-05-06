# Web Source Grounding API

Web source grounding stores a local, hashed copy of extracted page text plus exact quote anchors. Agents must cite `webcap:<artifact_id>#<quote_id>` refs, not naked URLs.

## MCP tool: `memory_capture_source`

Input:

```json
{
  "url": "https://example.com/report",
  "excerpts": ["exact quote present in extracted page text"],
  "note": "optional safe operator note"
}
```

Output includes:

```json
{
  "artifact_id": "src_01J0Z7Y8Q9R0ABCDE123456789",
  "source_refs": ["webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001"],
  "final_url": "https://example.com/report",
  "captured_at": "2026-05-05T18:00:00Z",
  "capture_status": "complete_text_only",
  "warnings": []
}
```

The schema exposes no local-network, auth, cookie, proxy, or file-capture bypass flags.

## CLI

```bash
memoryd source capture \
  --socket /tmp/memoryd.sock \
  --url https://example.com/report \
  --excerpt 'exact quote present in extracted page text'
```

The CLI is daemon-backed only. It does not perform direct capture outside daemon policy.

## Source ref format

```text
webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001
```

Governance accepts `source_kind=web_capture` only when the local artifact verifies and the quote id resolves to an exact byte range in `extracted.txt`. `https://...` source refs remain ungrounded.

## Artifact layout

```text
sources/web/YYYY/MM/src_01J0Z7Y8Q9R0ABCDE123456789/
  manifest.json
  extracted.txt
  excerpts.jsonl
  raw.bin.zst   # optional, only when raw textual projection is privacy-safe
```

## Safety restrictions

Capture is static HTTP(S) only. It rejects localhost, private/link-local/multicast/unspecified/documentation/metadata addresses, embedded credentials, redirects to unsafe addresses, and redirect chains over five hops. Requests disable automatic redirects and proxies, and pin the request client to vetted DNS socket addresses.

## Privacy and copyright boundaries

Extracted text must be safe for plaintext storage. Text requiring encryption or refusal fails closed in v0.1. Raw bytes are stored only when their textual projection is also safe; otherwise the artifact can still be `complete_text_only` and ground against extracted text plus exact excerpts. Do not quote more page text than needed for the memory claim.

## Examples

Successful write metadata after capture:

```json
{
  "namespace": "project",
  "type": "claim",
  "source_kind": "web_capture",
  "source_ref": "webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001",
  "confidence": 0.95
}
```

Refused grounding examples: naked URL source refs, missing quote id, corrupted `extracted.txt`, partial/failed capture status, or hash mismatch.
