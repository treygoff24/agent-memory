# Findings triage — W4-prep enrichment + lane wiring, round 1

Diff: `2171004` in worktree `codex-20260711T094110Z_69b9fc`. Reviewer: Cursor safe (cursor-1, W4prep-worktree registry) + coordinator gate (clippy clean; memorum-eval 135/0 incl. the no-default-features honesty gate after the coordinator's feature-gate fix). Verdict FINDINGS — 6 accepted, 0 rejected.

| # | Sev | Finding → fix contract |
| --- | --- | --- |
| WP-F1 | HIGH | `enrichment.rs:107-129` harness spawn has no timeout/kill — one hung CLI call wedges the whole sweep. **Fix:** per-item timeout (60s, matching production dream harness) with kill + drain; timeout → structural fallback with a distinct disposition key; never block the run. |
| WP-F2 | HIGH | Structural fallback bypasses normalize (only word-count-capped) — an over-120-char abstraction lands in the sidecar, then refuses at ingest as write_error drag, changing the promoted set vs baseline₀. **Fix:** structural output goes through the SAME validate/normalize as harness output; validate failure → skip item (production §B posture), never persist uncapped values. Test: pathological long-token summary → item skipped or capped, never >120 chars in a sidecar. |
| WP-F3 | HIGH | No production-equivalent privacy rebind on enriched writes — a sensitive generated cue refuses/encrypts a write that promoted plaintext in baseline₀, breaking comparability. **Fix:** before sidecar persist (preferred), run the Agent-floor dual-classify drop from production (`generation_privacy_rebind` pattern): fields that make combined stricter than body-only are dropped (disposition `dropped_sensitive`); secret → skip item. Test: sensitive-cue generation → sidecar entry has no cues, body ingestion unchanged. |
| WP-F4 | MEDIUM | Sidecar saved via bare `fs::write` — a mid-write crash tears the JSON and the next run ABORTS on parse, losing all resume state. **Fix:** temp+fsync+rename atomic write; corrupt existing sidecar → quarantine the file aside (`.corrupt-<ts>`) and start fresh rather than aborting. |
| WP-F5 | MEDIUM | Raw `claude`/`codex` spawn instead of the hardened dream harness adapters; dataset text is prompt-interpolated (injection surface: an agentic CLI can treat corpus text as instructions with tool side effects). **Fix:** invoke through the dream harness adapter path (stdin transport, hardened wait) where available; ensure the enrichment prompt frames corpus text as data (delimiters + explicit non-instruction preamble); document the residual (a fully agentic CLI can never be 100% fenced — mitigation is the adapter's sandboxed profile). |
| WP-F6 | LOW | `enriched` counted per attempt including refused writes — dispositions overstate enriched promotions. **Fix:** count enriched-attempted and enriched-promoted separately. |

Cleared: key handling (env→ephemeral tempdir file via the canonical daemon helper, Drop-cleaned), body-hash parity (exact ingest body formats matched), §C meta absent-vs-empty semantics, the coordinator's quality-feature gate (explicitly endorsed), runner defaults/metric formulas untouched.

Round 2: scoped re-review of the fix diff (round 1 of 3 used).
