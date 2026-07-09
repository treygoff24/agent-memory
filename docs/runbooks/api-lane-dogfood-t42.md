# T4.2 — API-lane live dogfood runbook (STAGED — run only with Trey)

Switch the live `~/memorum` install to the Gemini API embedding lane, verify footprint and recall, switch
back. This is also the first real load test of the T2.2 rate-limit path (full-corpus re-embed hits 429s by
design). Plan: `docs/plans/2026-07-09-api-embedding-lane.md` T4.2. Field notes go to `docs/reviews/`.

## Preconditions

- [ ] T4.1 bake-off done; triple dims ratified (this runbook assumes `gemini-embedding-2` + chosen dims).
- [ ] Gemini paid-tier key in hand; ZDR project-approval mechanics walked (plan Q6).
- [ ] Waves 1–3 merged to main; live daemon rebuilt from main (`cargo install`, then
      `launchctl kickstart -k` — see `memorum-launchd-needs-absolute-binary-path` memory note).
- [ ] `memoryd doctor` healthy on the local lane before switching (clean baseline).

## Steps

1. **Baseline capture:** `footprint -p $(pgrep memoryd)`; `memoryd status` embedding block (pending jobs,
   held_local_jobs); a recall spot-check transcript (3–5 known-answer queries via `memoryd recall`).
2. **Key install:** `MEMORUM_GEMINI_API_KEY` via env for the daemon, or the runtime key file (0600) — use
   the T3.1 CLI surface. Verify the key file perms.
3. **Switch lanes** with the T3.1 CLI (consent prompt + cost estimate will show — record the estimate).
   Restart daemon if T3.1's D4 mechanics require it.
4. **Watch the re-embed drain:** `memoryd status` embedding counts + daemon logs. EXPECT 429s — verify
   Retry-After backoff (log line "embedding drain rate-limited; respecting provider backoff"), jobs stay
   pending, no retry-budget exhaustion spam, drain completes.
5. **Verify the fence live:** `held_local_jobs` in status must equal the confidential/personal chunk count;
   doctor shows the `embedding_api_lane_held_local` advisory. Spot-check: no plaintext of any encrypted
   memory in any outbound request (network-level check optional: proxy or log-level inspection).
6. **Footprint:** `footprint -p` — expect ~10–50 MB (vs ~1.27 GB local-lane warm). Give it an idle window.
7. **Recall quality spot-check:** rerun the step-1 queries; compare.
8. **Switch back** to the local lane (D4: old vec tables untouched, so switch-back should not re-embed).
   Verify recall works immediately and doctor is healthy.

## Abort criteria

- Any evidence of a sensitive chunk in an outbound request → STOP, capture, revert lane, file as a breach.
- Drain wedges (pending count frozen while not rate-limited) → capture logs, revert.

## Record

Cost estimate vs actual token spend; drain wall-clock; 429 count; footprint numbers; recall diffs.
