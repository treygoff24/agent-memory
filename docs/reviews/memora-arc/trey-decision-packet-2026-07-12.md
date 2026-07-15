# Memora arc — Trey decision packet (2026-07-12)

Everything below is pre-staged so each decision is one word. Sources: `w4-eval-gate-results.md`,
`rehearsal-findings-2026-07-11.md`, BUILD-STATE. Nothing here has touched live `~/memorum`.

## Decision 1 — W4 merge disposition

Dev A/B (pinned judge): legacy fusion + Gemini vectors **0.700** · best four-lane config **0.658**
(3-config sweep, cap reached). Four-lane beats the FTS-only baseline but loses to legacy on the same
vectors — the aux inputs (one-shot enrichment abstractions) are the bottleneck, not the fusion code
(11 findings closed, gates green).

- **(a) Merge dark** — flip `four_lane_enabled` default to `false` + pinning test, merge the reviewed
  code, latency counters, and A/B tooling; re-run the eval gate after W5 lands dream-quality
  abstractions on the live corpus. **← coordinator recommendation**
- (b) Merge with cue-only weights enabled — knowingly worse than legacy; not defensible.
- (c) Drop the wave — the branch dies, sweep tooling and counters die with it.

## Decision 2 — B3: W5 backfill mechanism

The backfill is 100% grounding-refused as shipped (supersede re-validates drifted import-era
evidence; 100/100 probe). Options:

- **(i) Metadata-amendment path** — abstraction/cues are frontmatter-only; add a validator-gated
  actor arm (like `memoryd-review`/`memoryd-reality-check`) and skip supersede+grounding entirely.
  The body never changes, so grounding re-verification is checking the wrong thing.
  **← coordinator recommendation** (small spec amendment to Stream A/W2; code is a bounded wave)
- (ii) Grounding-exempt supersede class for aux-only amendments — keeps version-per-change history,
  bigger spec surface, same outcome.
- (iii) Re-ground the corpus first — drifted `file:` evidence makes this largely impossible.

## Decision 3 — live repair pass: per-item dispositions (rehearsed, ready to execute)

Mechanics fully verified on the copy (13/13 dup drain, queue 30→17). The live pass = import →
rebuild drain manifest from a live export → apply your dispositions below → quarantine triage →
`memoryd doctor --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME" --reindex` (rebuilds the SQLite event-log mirror from canonical JSONL). The 13 dup-rejects need no decision (older twins of live active memories).

**17 candidates** (me-scope, held by me-strict policy; ENC = encrypted at rest):

| # | id tail | enc | summary | coordinator rec |
|---|---|---|---|---|
| 1 | 000001 | pt | daemon memory acceptance probe — lifecycle validation write | **REJECT** (test artifact) |
| 2 | 000688 | pt | DOGFOOD TEST 2026-07-08: CLI-first canonical loop run | **REJECT** (test artifact) |
| 3 | 000701 | pt | Karlyn devbox session — remote-control Claude session notes | **APPROVE** (real ops memory) |
| 4 | 000459 | ENC | Claude Code local profile and model configuration | **APPROVE** |
| 5 | 000537 | ENC | local macOS machine-health triage and process cleanup | **APPROVE** |
| 6 | 000541 | ENC | machine_setup / local secrets and account-routing audits | **APPROVE** |
| 7 | 000555 | ENC | macOS system health and cleanup triage | **APPROVE** |
| 8 | 000556 | ENC | macOS Spotlight/CoreSpotlight troubleshooting | **APPROVE** |
| 9 | 000559 | ENC | local Claude Code hook docs + subagent-model guard hardening | **APPROVE** |
| 10 | 000562 | ENC | local Claude voice-notification disable | **APPROVE** |
| 11 | 000563 | ENC | Delegate/Droid Fireworks fast-router wiring + skill surfaces | **APPROVE** |
| 12 | 000565 | ENC | local Factory/Droid hook inspection and disable | **APPROVE** |
| 13 | 000567 | ENC | Claude Code + Codex local profile, auth, model configuration | **APPROVE** |
| 14 | 000570 | ENC | skill-surface hygiene + cmux hook/notification wiring | **APPROVE** |
| 15 | 000574 | ENC | local VoiceInk / Qwen3-ASR service recovery | **APPROVE** |
| 16 | 000576 | ENC | Devin CLI skill, autonomy smoke tests, Delegate boundary | **APPROVE** |
| 17 | 000582 | ENC | 2026-05-21 azc-cold-email-subagent-preference | **YOUR CALL** (stale? single-purpose) |

**6 quarantines** (governance quarantines from the fixed import; approving promotes to active —
these look like real project memories the old import missed):

| # | id tail | namespace | summary | coordinator rec |
|---|---|---|---|---|
| 1 | 000014 | Policy | witness-facing-doc-register | **YOUR CALL** (policy-scoped; you know the register) |
| 2 | 000015 | delegate-agent | issue sweep, stacked PRs, local-work preservation | **APPROVE** |
| 3 | 000016 | exa-agent-cli | deferred-work closeout on a local-only branch | **APPROVE** |
| 4 | 000017 | atlasos | original-vision audit and trust-gap roadmap | **APPROVE** |
| 5 | 000018 | probita | dogfood readiness, LegiScan policy, harness-first plan | **APPROVE** |
| 6 | 000024 | delegate-agent | Codex GPT-5.6 routing and fast-mode control | **APPROVE** |

(A 7th quarantine was consumed as the rehearsal mechanics probe on the copy; it will still be
present live: `000013` "v3.8 staged collateral quarantine" — rec **APPROVE**, it's a real Policy memory.)

Reply shape that unblocks everything: **"W4: a · B3: i · items: as recommended (or list exceptions) · go live"** —
or any subset; each decision unblocks its own track independently.
