# W1 + W5 rehearsal findings — copy of ~/memorum, 2026-07-11 night

Rehearsal environment: `~/memorum-rehearsal-20260711` (rsync of the live tree minus models/socket/pid/logs;
index via online `sqlite3 .backup` so the WAL was consistent; models symlinked; live daemon untouched,
HEAD verified stable across the copy). Rehearsal daemon: main build (W1+W2+W3), own socket. All numbers
below are from the copy; nothing live was modified.

## What PASSED

1. **W2 v5→v6 index migration on live-shaped data**: rehearsal daemon start ran the migration cleanly —
   `memory_abstractions` + `memory_cues` tables present, startup reconcile committed, doctor healthy at baseline.
2. **W1 import-repair, the core contract**: dry-run plans → real run: 11 new + 12 superseded + 669
   idempotent skips + 7 ambiguous (report-only) + 11 refused_privacy + 7 quarantined (real governance the
   dry-run can't see — expected). **Double re-import: 0 new / 0 superseded / 699 skips — stable**, the W1
   gate criterion, now verified on the live corpus shape.
3. **Frontmatter already carries W2 `abstraction`/`cues` fields** end-to-end through export.
4. Export of the full copy corpus: 915 memories, 4.6MB, no errors.

## Blockers found (the rehearsal's whole purpose — none of these are live-safe to ignore)

### B1 — Review decisions refuse ALL encrypted memories (blocks the W1 queue drain entirely)
`handlers/review.rs:222` rejects any non-plaintext envelope: *"encrypted review decisions require an
encrypted lifecycle update API."* The drain manifest classified the queue: **13 clear dup-rejects — all 13
encrypted** (118/915 memories are encrypted overall), so the planned drain is a 100% no-op. A review
decision is a frontmatter-only mutation (status/review_state/reason; encryption covers the body), which is
exactly what W3's hardened `update_encrypted_memory_metadata` + lifecycle validator (actor
`memoryd-review`, candidate→active arm already exists) was built to do. Fix direction: route encrypted
envelopes through that path. Governance-critical → needs a review round, not a blind inline patch.

### B2 — `memoryd review reject` exits 0 on an error envelope
The CLI printed the `invalid_request` error envelope and returned exit code 0. My drain script trusted the
exit code and reported "13 ok" until the queue recount caught it. Contract violation (agent envelope v1
promises meaningful exits); a footgun for every scripted caller.

### B3 — W5 backfill is 100% grounding-blocked (design decision needed)
`dream abstraction-compile` applies via **governance supersede**, which re-validates grounding evidence.
Live memories cite import-era `file:` evidence that has drifted (MEMORY.md files change constantly) →
**100/100 probe refusals, reason Grounding** (structural fallback, so it's the supersede wall, not harness
quality). The live corpus will behave identically: W5 cannot backfill anything as shipped. Options:
- (i) a metadata-amendment path for abstraction/cues (no supersede, no grounding re-check — the body is
  unchanged; arguably the semantically correct mechanism, W3 validator gains an actor arm);
- (ii) grounding-exempt supersede class for aux-only amendments (spec change, keeps version history);
- (iii) re-ground everything first (huge, likely impossible for drifted evidence).
Spec-level call — Trey decides. Note W6's memo and the June "grounding→privacy catch-22" follow-up are
adjacent to this same knot.

Also shipped from the rehearsal: the abstraction-compile report now carries the governance refusal reason
(`81928cd`) — B3 took debug archaeology to diagnose because the report dropped it.

## Live-repair runbook implications (for the eventual live pass)

- `sync_blocked` with 7 governance quarantines after import is expected — triage quarantines as a runbook step.
- Drain manifest procedure validated: summary+namespace twinning classifies 13 reject / 17 keep-for-Trey;
  the 17 are genuine me-scope candidates (machine-setup notes, dogfood tests), correctly held by me-strict.
- Import must run with `--socket` against the target daemon and `MEMORUM_REPO` set; dry-run first, real
  run second, re-import third (stability proof), doctor after each.
