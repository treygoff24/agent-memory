# Live repair pass — ratified dispositions (Trey, 2026-07-15)

Source: `trey-decision-packet-2026-07-12.md` coordinator recommendations, ratified by Trey 2026-07-15
with two explicit calls on the your-call items: **candidate #17 REJECT** (older twin of active
`mem_20260619_..._000454`, verified in-store 2026-07-15), **quarantine #1 APPROVE**.

Id tails refer to the packet tables; the live pass re-resolves full ids from a fresh export before
applying (rehearsal note: copy queue counts differ by the one probe-consumed quarantine).

## 17 review candidates (me-scope)

| id tail | disposition |
| --- | --- |
| 000001 | REJECT (test artifact) |
| 000688 | REJECT (test artifact) |
| 000701 | APPROVE |
| 000459 | APPROVE |
| 000537 | APPROVE |
| 000541 | APPROVE |
| 000555 | APPROVE |
| 000556 | APPROVE |
| 000559 | APPROVE |
| 000562 | APPROVE |
| 000563 | APPROVE |
| 000565 | APPROVE |
| 000567 | APPROVE |
| 000570 | APPROVE |
| 000574 | APPROVE |
| 000576 | APPROVE |
| 000582 | REJECT (dup of active 000454 — Trey 2026-07-15) |

## 7 governance quarantines

| id tail | disposition |
| --- | --- |
| 000013 | APPROVE |
| 000014 | APPROVE (Trey 2026-07-15) |
| 000015 | APPROVE |
| 000016 | APPROVE |
| 000017 | APPROVE |
| 000018 | APPROVE |
| 000024 | APPROVE |

Plus: 13 dup-rejects drain automatically (rehearsed 13/13).

## Procedure (validated on the 7/11 rehearsal copy)

1. Rebuild + restart live daemon from integrated main (`cargo install`, `launchctl kickstart -k`;
   bootstrap-after-bootout I/O error 5 → retry).
2. Import with `--socket` against the live daemon + `MEMORUM_REPO` set: dry-run → real → re-import
   (stability proof: 0 new / 0 superseded) → doctor after each.
3. Rebuild drain manifest from a fresh live export (summary+namespace twinning).
4. Apply dispositions above (encrypted rows route via the B1 fix, `20ba2ec`; exit codes now honest per B2 fix).
5. Quarantine triage: approve all 7 per table.
6. doctor + reindex; record new corpus counts in BUILD-STATE (was: 786 active / 28 held-local jobs / 31 pending).
