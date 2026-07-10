# API embedding lane (Gemini) — operator runbook

The opt-in `gemini-api` embedding lane replaces the local ~1.27 GB embedding model with Google's
`gemini-embedding-2` API (~10–50 MB daemon footprint). Privacy fence: only plaintext-eligible content
(persisted sensitivity `public`/`internal`) and query text ever transit; `confidential`/`personal` rows —
including masked `safe_body` projections — are held local and never fetched for API embedding.
Spec: the 2026-07-09 amendment to `stream-a-core-substrate-v1.1` (see
`docs/plans/2026-07-09-t33-spec-amendment-draft.md` until ratified). Live dogfood procedure:
`docs/runbooks/api-lane-dogfood-t42.md`.

## Enable

1. **Key** (device-local, never synced): `export MEMORUM_GEMINI_API_KEY=...` for the daemon environment,
   or write the 0600 runtime key file via the CLI. Both resolve env-first.
2. **Switch + consent:** `memoryd config embedding-lane --lane gemini-api`
   - Interactive: shows the consent copy (what transits, what never leaves, Google's no-training +
     55-day-log retention posture) and the cost estimate for re-embedding the current corpus, then asks.
   - Scripted/agent: add `--consent` (refused otherwise). Init-time: `memoryd init --embedding-lane
gemini-api --consent`.
   - Consent is recorded as `api_embedding_consent: true` in synced `config.yaml`. The daemon refuses to
     start the API provider without it, so a hand-edited or merged `active_embedding` cannot silently
     begin sending plaintext.
3. **Restart the daemon** (`launchctl kickstart -k ...` on the live install). The provider is read at
   open; there is no hot swap.
4. The drain worker re-embeds eligible chunks under the new triple. Old vector tables are retained
   (switch-back is instant; drop old triples explicitly when no longer wanted).

## Verify

- `memoryd status` — embedding block shows pending jobs draining and `held_local_jobs` (the
  confidential/personal rows deliberately not sent).
- `memoryd doctor` — API-lane findings:
  | Code | Severity | Meaning |
  | --- | --- | --- |
  | `embedding_api_consent_missing` | Fatal | API triple active, consent not recorded; provider won't start |
  | `embedding_api_key_missing` | Fatal | No env key and no readable runtime key file |
  | `embedding_api_rate_limited` | Advisory | Backlog pending + latest provider error reports 429/rate limit |
  | `embedding_api_offline` | Advisory | Backlog pending + latest provider error indicates network unreachability (no live probe) |
  | `embedding_orphaned_triples` | Advisory | Vector tables remain for non-active triples (by design after a switch) |
  | `embedding_api_lane_held_local` | Advisory | Held-local jobs exist — sensitive content correctly not sent |

## Behavior under failure

- **429 rate limiting:** drain honors `Retry-After` (60s fallback), jobs stay pending, per-job retry
  budget is not charged. Expected during a full-corpus re-embed.
- **Bad key:** Gemini's 400 `API_KEY_INVALID` maps to an auth failure (not endless transport retries);
  doctor points at the key.
- **Offline / slow API:** query-time embeds run under a lane-aware timeout (~750 ms API / ~50 ms local,
  explicit `embed_timeout_ms` config wins) and recall degrades to FTS; drain backs off and resumes.

## Switch back

`memoryd config embedding-lane --lane local` + daemon restart. No consent needed for the local lane; the
old local vector table is still there, so recall is immediately warm.
