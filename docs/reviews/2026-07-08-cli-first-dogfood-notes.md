# CLI-first dogfood notes (live ~/memorum re-setup + re-import)

**Date:** 2026-07-08
**Context:** Task 9 of the agent-cli-first-surface plan, run live against `~/memorum`.
Rebuild + redeploy the daemon, full re-import of Claude Code + Codex native
memory, re-wire recall hooks, run the canonical loop. This file captures friction,
bugs, and improvement ideas surfaced by actually using the shipped surface.

## Starting state

- Installed binary: `memoryd 0.1.0` (pre-CLI-first `main` build, `74e3250`).
- Live daemon: `com.memorum.daemon` launchd agent, PID 56042, pre-CLI-first build.
- Local `main`: fast-forwarded to `daf5182` + gate fix `ac8282c`; full `scripts/check.sh` green.
- Hooks/MCP torn off since 2026-06-25.

## Observations

### 1. Redeploy + daemon restart — clean
- `cargo install --path crates/memoryd --locked --force` → release build, replaced `~/.cargo/bin/memoryd`, exit 0 (27.7s).
- `launchctl kickstart -k gui/<uid>/com.memorum.daemon` restarted the daemon in place (old PID 56042 → new 11592) on the new binary, **no** bootstrap-after-bootout I/O error 5. Kickstart-in-place is the clean restart path — prefer it over bootout+bootstrap (which the project memory flags as error-5-prone).
- `status` now emits the v1 envelope live (`ok:true`, `meta.schema_version:"1.0"`, exit 0); `doctor healthy:true`, exit 0 (raw frame, as designed).

### 2. Full re-import — worked, exit 0
Totals: parsed 697 (claude 644 / codex 53), written-new 237, dedup-existing 421, queued-for-review 29 (candidate 11 + quarantined 18), privacy-blocked 10, frontmatter-recovered 3. The corpus grew substantially since the 2026-06-19 rebuild (237 genuinely new memories from ongoing project work). Privacy refusals (donor/personal/financial data) fired as designed.

**Friction / improvement candidates:**

- **Import output is very noisy by default.** After the per-write progress lines, the run prints a long tail of back-edge resolution lines (`claude:.../foo.md → [[target]]`), one per wikilink across the whole corpus — hundreds of lines. This buries the reconciliation summary. Candidates: gate the back-edge lines behind `--verbose`, summarize them as a count by default, or route them to the report only. `--quiet` exists but suppresses the summary too; there's no middle setting.
- **`skipped_idempotent=0` on a re-import.** The import guide + skill claim re-runs "skip unchanged sources by content hash," but this re-import of an already-imported corpus skipped **zero** by idempotency and instead re-parsed all 697 and resolved 421 via `dedup_existing`. Either the source-hash skip path isn't engaging (state file not consulted / reset on the 6-19 rebuild), or dedup-by-content is the *actual* idempotency mechanism and the docs' "skip by hash" framing is misleading. Worth confirming which — if the hash-skip is meant to short-circuit re-parsing, it's not doing so here (re-import did full work).
- The reconciliation summary itself (`queued for review: 29 / privacy-blocked: 10 / frontmatter-recovered: 3` + per-harness counters) is clear and matches the report JSON. Good.

Post-import: active memories 549 → **786** (+237, matches written-new); review queue 15 candidate + 18 quarantined.

### 3. Hook re-wire — worked; MCP left unwired
- `CLAUDE_CONFIG_DIR=~/.claude-personal` — the live Claude config is there, not `~/.claude`. Worth remembering for any hook/config inspection; a naive `~/.claude/settings.json` check reports "no hooks" falsely.
- `memoryd init --non-interactive --harness none --wire-hooks all --wire-mcp none --daemon none` wired 3 recall hooks (SessionStart/UserPromptSubmit/SubagentStart) into both `.claude-personal/settings.json` and `.codex/hooks.json`; `wire_mcp: skipped`, `verify: succeeded`, `restart_required: true`. Real-run message is clear: "Claude: hooks updated (merged ...)".
- MCP confirmed unwired in both harnesses (no `memorum` mcpServers entry in Claude, no `mcp_servers.memorum` in Codex).

**Friction / improvement candidate:**

- **`--print-only` wire_hooks wording is misleading.** In dry-run the step reports `status: "succeeded"` with a message reading `Claude: hooks skipped ({ ...full hook JSON... }); Codex: hooks skipped (...)`. The hooks were genuinely *absent* and *would be written* on a real run — "skipped" reads as "already present, no-op," the opposite of the truth. Other dry-run steps use `status: "expected"` + `"[dry-run] would ..."`; wire_hooks should match that convention (e.g. `status: expected`, message `"[dry-run] would wire Claude + Codex recall hooks"`), rather than `succeeded` + "skipped".

### 4. Daemon footprint after import
- Right after the import, `vmmap --summary` physical footprint = **6.9G** (embedding model loaded in Metal + import working set; `embedding.state: active`, `load_count: 1`). `ps` RSS only 267 MB — Metal memory is invisible to RSS, so footprint is the right metric (matches the daemon-memory-reduction note).
- 6.9G is above the "~2GB loaded" figure from the memory note; the reduction design relies on idle-unload (`idle_unload_secs: 900`) to drop back to ~108MB when embedding goes dormant. **To verify:** re-check footprint after ≥15 min idle to confirm it reclaims (done at the end of this run).

### 5. Recall loop closed live — the headline result
Immediately after wiring the hooks, the next `SessionStart` in *this* Claude session injected a `<memory-recall version="stream-e-v0.7" harness="claude-code">` block into my context — and it surfaced `mem_20260708_40edd13334a43d72_000689` ("Dogfood: using-memorum skill validation"), the exact governed write the canonical-loop subagent had made minutes earlier. So the full runtime loop is closed end-to-end on the CLI-first build: **governed write → commit-on-write → index → passive recall injection**, with the recall block also carrying project resolution (`resolved-via: git_remote`), a `pending-attention` line ("16 memory item(s) require review"), and a budget accounting (`used-tokens: 672 / budget 1900`). This is the whole system working in a real session, not a test harness.

### 6. Canonical-loop findings (fresh agent, skill-only)
Full transcript + findings: `docs/reviews/2026-07-08-canonical-loop-live-transcript.md`. Steps 1–5 were fully covered by the skill alone; step 6 (supersede after a grounding refusal) exposed the gaps. Actionable items fixed in-branch:
- **Code fix:** the `privacy`/`policy`/`review_required` refusal `suggested_fix` referenced a `next_actions` field that refusal envelopes (especially from `supersede`/`forget`) don't carry — a dead-end pointer. Rewrote each per-reason fix to be self-contained (`crates/memoryd/src/cli/output.rs::refusal_suggested_fix`).
- **Skill:** documented `source capture` + the grounding `--meta` key `source_ref` (singular string; `source_refs` plural is rejected), noted that `supersede` runs the full governance gate and can be stricter than the original `write`, and called out that `doctor` differs in output *shape* (raw daemon frame) not just exit codes.
- **Contract:** spelled out doctor's raw shape (`.result.success.doctor.healthy`) so a script author doesn't parse it with `.ok`/`.data`.

Follow-ups (out of scope for this plan, logged for a later arc): the grounding→privacy catch-22 for self-referential claims with no public source; the `summary` field carrying different semantics in `search` (snippet) vs `get` (title); undocumented negative search scores; and the import `skipped_idempotent=0` question from §2.
