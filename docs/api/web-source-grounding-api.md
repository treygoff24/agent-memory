# Web Source Grounding API

Web source grounding stores a local, hashed copy of extracted text plus exact
quote anchors. Agents must cite `webcap:<artifact_id>#<quote_id>` refs, not
naked URLs.

## Alpha support matrix

Supported alpha modes:

| Mode             | Input                                                  | Capture method      | Notes                                                                                                                                                            |
| ---------------- | ------------------------------------------------------ | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `http_static`    | public `http://` or `https://` URL plus exact excerpts | `http_static_v1`    | Static text/HTML fetch only. No cookies, browser session, proxy, localhost/private-network target, or authenticated fetch.                                       |
| `local_artifact` | local text/HTML artifact path plus exact excerpts      | `local_artifact_v1` | Use this for exported pages, local notes, and browser-saved text/HTML artifacts. The daemon validates excerpts and stores only privacy-safe plaintext artifacts. |

Explicitly unsupported alpha modes: browser-rendered capture is unsupported;
screenshots, OCR, authenticated browser/cookie capture, and client-supplied
privacy bypass/key paths are unsupported. PDF capture is unsupported unless a
code lane has landed a deterministic `pdf_text` adapter; until then export PDF
text/HTML and import it as `local_artifact`.

The model privacy filter remains unsupported for alpha. Source capture uses the
deterministic privacy checks available in the daemon/runtime path and fails
closed rather than storing unsafe plaintext.

## MCP tool: `memory_capture_source`

Input:

```json
{
  "source": "https://example.com/report",
  "mode": "http_static",
  "excerpts": ["exact quote present in extracted page text"],
  "note": "optional safe operator note"
}
```

Local artifact input:

```json
{
  "source": "file:///absolute/path/to/exported-report.html",
  "mode": "local_artifact",
  "local_path": "/absolute/path/to/exported-report.html",
  "excerpts": ["exact quote present in the exported artifact"],
  "note": "optional safe operator note"
}
```

Output includes:

```json
{
  "artifact_id": "src_01J0Z7Y8Q9R0ABCDE123456789",
  "source_refs": ["webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001"],
  "mode": "http_static",
  "final_url": "https://example.com/report",
  "captured_at": "2026-05-05T18:00:00Z",
  "capture_status": "complete_text_only",
  "warnings": []
}
```

The schema exposes no local-network, auth, cookie, proxy, `key_path`, raw key
material, or privacy-bypass flags.

## CLI

```bash
memoryd source capture \
  --socket <runtime>/memoryd.sock \
  --url https://example.com/report \
  --excerpt 'exact quote present in extracted page text'
```

```bash
memoryd source capture \
  --socket <runtime>/memoryd.sock \
  --file /absolute/path/to/exported-report.html \
  --mode local-artifact \
  --excerpt 'exact quote present in the exported artifact'
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

HTTP capture is static HTTP(S) only. It rejects localhost,
private/link-local/multicast/unspecified/documentation/metadata addresses,
embedded credentials, redirects to unsafe addresses, and redirect chains over
five hops. Requests disable automatic redirects and proxies, and pin the
request client to vetted DNS socket addresses.

Local artifact capture reads a user-selected local text/HTML export through the
daemon and records it as `local_artifact_v1`. Browser-rendered capture is
unsupported here, and local artifact capture does not preserve authenticated
session state.

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
