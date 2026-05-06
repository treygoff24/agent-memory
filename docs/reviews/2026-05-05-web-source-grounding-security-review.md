# Web Source Grounding Security Review

## Findings by severity

No open P0/P1 findings after implementation review. Residual risks are listed below.

## SSRF guard coverage

`memory-source` rejects non-HTTP(S), embedded credentials, localhost, private, CGNAT, link-local, unique-local, multicast, unspecified, documentation, and cloud metadata addresses. Tests cover representative unsafe addresses and mixed public/private DNS results.

## DNS pinning and redirects

Capture resolves each hop through the resolver, rejects any unsafe address, builds a per-hop reqwest client with `resolve_to_addrs`, disables automatic redirects, and validates `remote_addr` against the pinned set. Redirects are manually revalidated and capped at five hops.

## Size/time limits

HTTP clients use connect/request timeouts, and response bodies over the raw cap fail before artifact write. Excerpt count and per-excerpt size are bounded in `memoryd`.

## Privacy and indexing boundaries

Extracted text requiring encryption/refusal fails closed. Raw bytes are omitted unless raw textual projection is safe. Source artifacts are under `sources/web/**`, excluded from canonical memory Markdown enumeration, and are not passive-recall indexed.

## MCP/CLI attack surface

MCP exposes only `url`, `excerpts`, and optional `note`; no test bypass, auth, proxy, local file, or local-network flag is exposed. CLI capture is daemon-backed only.

## Source artifact merge behavior

Text artifact paths are explicitly handled by the merge driver. Divergent manifest/excerpt/extracted merges quarantine deterministically and leave artifacts non-groundable. `raw.bin.zst` is marked binary.

## Residual risks

- Static capture misses browser-rendered text.
- v0.1 proves excerpt existence and artifact integrity, not semantic entailment.
- v0.1 does not cryptographically prove daemon provenance of artifact creation.
