# Canonical-loop live validation — fresh agent, skill-only, live ~/memorum

**Date:** 2026-07-08
**Context:** Task 9 (live validation gate) + Task 7 manual half. A fresh-context
subagent, briefed **only** with `skills/using-memorum/SKILL.md`, drove the live
`~/memorum` daemon through the canonical loop (orient → search → get → note →
governed write → supersede). This is the verbatim finding set; the surviving,
actionable items were folded back into the skill, the CLI contract, and the
envelope layer in the same branch (see the follow-up commit).

## Transcript

| # | Step | Command (abbrev.) | Exit | Outcome |
|---|------|-------------------|-----:|---------|
| 1a | Orient | `memoryd status --socket …` | 0 | `ok:true`, `state:"ready"`, 786 active; `review_queue_counts {candidate:15, quarantined:18}`, `conflicts_count:18`. |
| 1b | Orient | `memoryd doctor --repo …` | 1 | `healthy:false`, finding `sync_blocked` (18 quarantined block sync). Skill predicted the 0/1 exit. |
| 1c | Orient | `memoryd schema --json` | 0 | Full machine contract. |
| 2 | Search | `memoryd search "delegate" --limit 5 …` | 0 | `ok:true`, `total:20`, 5 hits. |
| 3 | Get | `memoryd get mem_…_000120 …` | 0 | `ok:true`, full body, `truncated:false`. |
| 4 | Note | `memoryd write-note "DOGFOOD TEST 2026-07-08 …" …` | 0 | Immediate → `mem_20260708_40edd13334a43d72_000688`; no `status` (correct for notes). |
| 5 | Governed write | `memoryd write … --title … --tag dogfood --meta '{"namespace":"project","type":"claim","confidence":0.80}' …` | 0 | `data.status:"promoted"` → `mem_20260708_40edd13334a43d72_000689`, live. |
| 6a | Supersede | `memoryd supersede …_000689 "…" --reason … …` | 65 | Refused, `error.code:"grounding"`. |
| 6b | Recover | `memoryd source capture --file <SKILL.md> --mode local-artifact --excerpt "…" …` | 0 | `capture_status:"complete"`, `source_refs:["webcap:src_…#quote_0001"]`. |
| 6c | Supersede | `--meta '{"source_refs":[…]}'` | 65 | `invalid_request` — unknown field `source_refs`; error listed valid meta fields. |
| 6d | Supersede | `--meta '{"evidence":[…]}'` | 65 | `invalid_request` — `evidence` expects a struct, not a string. |
| 6e | Supersede | `--meta '{"source_ref":"webcap:…"}'` | 65 | Grounding passed; refused for `error.code:"privacy"`. |
| 6f | Supersede | innocuous content + `source_ref` | 65 | Still `privacy` — trigger is the cited artifact, not the wording. |

Original promoted write `000689` stays active (supersede never succeeded).

## Was the skill sufficient?

- **Steps 1–5: fully.** Envelope contract, exit-code table, `data.status` reactions, note-vs-governed-write — all matched reality with no guessing.
- **Step 6 (supersede after a grounding refusal): no.** Gaps below.

## Findings (raw)

1. **`source capture` undocumented in the skill.** The grounding refusal's `suggested_fix` points at it, but the skill only listed `source` as a `--socket` command with no interface. Recovered via `memoryd source --help`. → **fixed:** skill now documents `source capture` + the grounding `--meta` key.
2. **Grounding `--meta` key undocumented.** `source_refs` (plural) wrong; `source_ref` (singular string) is correct. Two failed guesses. → **fixed:** skill documents `source_ref`.
3. **Supersede is stricter than write.** The original `write` promoted with no grounding; superseding the same claim demanded grounding. The skill's supersede examples carry no `--meta`, implying a light text-replace. → **fixed:** skill notes supersede runs the full grounding/privacy gate.
4. **`doctor` breaks the envelope *shape*, not just its exit codes.** Output is `{"id":…,"result":{"success":{"doctor":{…}}}}`. The skill/contract flagged doctor's 0/1 exit exception but not that a `.data`/`.ok` parser breaks on it. → **fixed:** skill + contract call out the shape difference explicitly.
5. **Two `suggested_fix` dead-ends:**
   - `evidence` struct error → suggested_fix said `memoryd schema commands --json` "for the exact argument shape," but `EvidenceMeta`'s shape is not in schema output. → **partly addressed:** this is the daemon's own `invalid_request`; the daemon message already lists valid fields. Left the daemon message as the fix; noted the schema-doesn't-carry-meta-DTO-shapes limitation as a follow-up.
   - `privacy`/`policy`/`review_required` refusal → suggested_fix said "the daemon `next_actions` name the corrective step," but refusal envelopes (especially from `supersede`/`forget`) have no `next_actions` field. → **fixed:** rewrote the per-reason refusal suggested_fix to be self-contained and stop referencing a field that isn't there.
6. **Grounding→privacy catch-22 (product limitation, follow-up).** The only local artifact to ground a self-referential claim was SKILL.md, whose `/Users/treygoff` paths the privacy classifier flags → citing it propagates a privacy descriptor that disallows a plaintext `project` write, and there's no public URL. The documented recovery loop can't complete for this class of write. Not a skill bug; a governance/privacy interaction to revisit in a later arc.
7. **`summary` field overloaded (follow-up).** `search` hit `summary` = body snippet; `get` `summary` = title. Same field, different semantics across commands. Stream A/B DTO inconsistency; noted for a later cleanup.
8. **Negative search scores** (e.g. `-3.74`) undocumented — harmless, follow-up.
9. Could not exercise `candidate`/`quarantined` warnings or the empty-search broadening hint live — the store's `project-standard@v2` policy promoted the write outright.

## Memories created (dogfood, cleanupable)

- `mem_20260708_40edd13334a43d72_000688` — the note
- `mem_20260708_40edd13334a43d72_000689` — the promoted governed write (still active)
- `src_01KX223HSTFWG5Q2BFEKR0KDN8` — a source-capture artifact of SKILL.md

All memories are prefixed `DOGFOOD TEST 2026-07-08` and tagged `dogfood`.
