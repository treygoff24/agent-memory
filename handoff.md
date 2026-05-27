# Handoff — 2026-05-26 alpha-readiness session

## Current state

`main` at `3e6e0ff`, two commits ahead of the alpha-prep commit `d07641a`. Working tree clean (except local-tooling untracked: `.code-briefcase/`, `.code-briefcaseignore` — not ours, ignore).

**Gates green at `3e6e0ff`:**
- `cargo test --workspace` — 1228/1228 in 257s
- `cargo test -p memorum-eval --test domain -- --test-threads=16` — 10/10 consecutive runs
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean
- `bash scripts/check-dogfood.sh` — clean
- Frontend typecheck/lint/vitest — all clean

**What landed this session** (CLAUDE.md "Current status" has the full record):

- Audit closeout of `docs/plans/2026-05-25-alpha-core-gap-closeout.md` — 8 of 10 tasks Done, 2 Partial. Closeout doc at `docs/reviews/2026-05-26-alpha-gap-audit.md`. Plan's Final alpha acceptance checklist ticked accurately.
- Closed T17 + T18 parallel-execution races. Four distinct bugs fixed durably (substrate gitignore migration, lease dirty-tree filter, atomic-write parent-dir fsync, scaffold socket-path counter) plus `#[serial]` on T13–T18 as defense in depth. Codex (via `delegate codex safe`) caught the scaffold socket-path collision — the only finding I'd never have found alone.
- Stripped "Do not modify Stream A modules" guidance from CLAUDE.md.

---

## Remaining items, prioritized

### 1. Task 4 frontend partials — close the two real gaps

**Status:** Shipped in `038d36c` ("Close Task 4 frontend partials before alpha dogfood"). Both edits below landed; checklist item flipped in the plan + closeout doc.

Both are short, well-scoped frontend edits. From `docs/reviews/2026-05-26-alpha-gap-audit.md`:

- **`crates/memoryd-web/frontend/src/views/entitiesView/EntityTable.tsx`** — `normalizeKind` keyword-classifies entity kind by string-matching `node.kind + node.label` against literals (`acme`, `pnpm`, `rust`, `home`, `office`) with a catch-all default to `'project'`. The plan invariant says daemon mode must prefer "unknown/unavailable" over invented values. Fix: drop the keyword heuristic, default unrecognized kinds to `'unknown'`, render the literal value the daemon supplied.
- **`crates/memoryd-web/frontend/src/views/Peers.tsx`** — `normalizePeer` hardcodes `eventsIn24h: 0`, `eventsOut24h: 0`, `locksPending: 0`, `devicePubkeyShort: 'unknown'` for fields the daemon `PeerSessionStatus` doesn't supply. Rendering `0` reads as a daemon fact and is the same invent-daemon-facts class of issue. Fix: render `null` / "unknown" / em-dash for counters the daemon doesn't supply.

After both ship, the checklist item "Dashboard status, peers, entities, and TUI panels never invent daemon facts" can flip from `[ ]` to `[x]` in `docs/plans/2026-05-25-alpha-core-gap-closeout.md`. Update the closeout review doc to match.

### 2. Task 1 optional TS-side cleanup

**Status:** First bullet shipped in `038d36c` (RoiResponse → DashboardRoiResponse rename done). Second bullet (CaptureSourceMode / SourceCapturePayload TS types) still open — non-blocking for alpha since source capture is CLI/MCP-only.

- **`crates/memoryd-web/frontend/src/api/types.ts`** — `DashboardRoiResponse` is shadowed by the pre-existing `RoiResponse` alias. Either rename `RoiResponse` → `DashboardRoiResponse` for protocol-naming parity (preferred), or amend the plan to document the deliberate divergence.
- **`crates/memoryd-web/frontend/src/api/types.ts` + `mutations.ts`** — `CaptureSourceMode` and `SourceCapturePayload` have no TypeScript counterparts. Source capture is CLI/MCP-only for alpha so it's non-blocking, but the plan called for the type surface to exist. Add the TS types or amend the plan.

### 3. Architecture deviation — decision needed from Trey

**Status:** Resolved in `038d36c` — deviation accepted, plan amended to document the chosen layout. No daemon-side `dashboard/status.rs` / `dashboard/entities.rs` modules; route logic stays in `memoryd-web/src/routes/`.

The 5/25 plan called for `crates/memoryd/src/dashboard/status.rs` and `crates/memoryd/src/dashboard/entities.rs` as daemon-side modules. Neither exists. Route logic lives in `crates/memoryd-web/src/routes/{status,entity_graph,sync_dashboard}.rs` directly. Behavior is correct; module layout diverges from the plan. Three options:

- **Backfill the modules** as the plan called for. Architecturally cleaner (daemon owns query construction, routes are thin adapters), more work.
- **Accept the deviation, update the plan** to document the chosen layout. Lowest-effort path.
- **Defer to a post-alpha refactor.** Note it in the closeout doc and move on.

Trey said "I'm not yet sure how to handle that" — decision lives with him.

### 4. Codex review follow-up tests (post-alpha, non-blocking)

From Codex's review of the race-fix diff. None block alpha; all close a regression gap.

- **`is_substrate_managed_path` unit test** — a real temp git repo with `.memorum/substrate`, `.memoryd/local-device.yaml`, `events/dev.jsonl`, and `leases/journal.lease` should produce an empty dirty-paths list; `agent/patterns/foo.md`, `me/user-work.md`, `substrate/foo.jsonl` should appear in the list and block. Lives best in `crates/memoryd/tests/dream_lease_election.rs` or a new sibling.
- **Gitignore reconciliation test** — start with an existing `.gitignore` containing only `/.memoryd/`, run `bootstrap_repo_layout`, assert `/.memorum/` is now present and pre-existing entries are preserved.
- **Scaffold socket uniqueness stress test** — spawn many `DaemonScaffold::fresh()`/`two_device()` concurrently, assert each socket path is unique and each daemon attribution is correct.

### 5. Live dogfood smoke (Task 10 Step 3)

The structural audit + dogfood gate exercise the surfaces but don't run the full operator install flow. The plan's Task 10 Step 3 spells it out:

```bash
export MEMORUM_REPO="$(mktemp -d)/memorum"
export MEMORUM_RUNTIME="$MEMORUM_REPO/.memoryd"
export MEMORUM_SOCKET="$MEMORUM_RUNTIME/memoryd.sock"
bash scripts/install-memorum.sh --force-reinstall --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"
memoryd serve --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME" --socket "$MEMORUM_SOCKET" --init --force-unsafe-durability &
# ... smoke memory_write→memory_search via MCP stdio, web /api/status + /api/roi + /api/notifications/stream + /api/policy-editor, TUI launch
```

Best done on a clean machine if possible; otherwise the temp-repo flow above approximates it.

### 6. Minor housekeeping (optional)

- **`crates/memoryd-web/src/routes/policy_editor.rs`** — `FIXTURE_POLICY_YAML` constant in the fixture-mode path is inconsistent with the daemon's builtin policy-set shape. Fixture-mode-only, non-blocking. Could be extracted to a test-only module or aligned with the daemon shape.

---

## Pointers

- Project context + stream model: `CLAUDE.md`
- Latest plan: `docs/plans/2026-05-25-alpha-core-gap-closeout.md` (now carries status banner pointing to the closeout doc)
- Latest closeout review: `docs/reviews/2026-05-26-alpha-gap-audit.md`
- Lessons-learned style file: `CLAUDE.md` § "Lessons from the 2026-05-22 gap-worktree salvage closeout" (self-selected-verification bias, `.oxfmtignore` carve-outs, peer-note pattern)
- Codex delegation pattern: `delegate codex safe --prompt-file <path>` for read-only reviews; `delegate runs --recent --harness codex --limit 3` to find the alias; `delegate run-output <alias> --raw` to read the JSONL transcript (look for the final `agent_message` item)

## Working notes from this session

- The `agent/patterns/t14-merge-driver.md` cross-test pollution mystery: Codex's socket-path-collision theory in `daemon_scaffold.rs::short_socket_path` (no atomic counter, just pid+nanos) is the most plausible mechanism — explains how T14's daemon could serve T17's `dream_now` call. Fixed in the counter patch but the mechanism itself is worth remembering: if you ever see "this file shouldn't be in this temp dir," check whether two scaffolds could be sharing a daemon.
- The CLAUDE.md 5/22 lesson on self-selected verification bias hit again in d07641a — author added `lease.rs` re-entrancy guard, picked tests that exercise it, missed the older `dream_lease_election.rs::forced_takeover_makes_forced_holder_active_and_ignores_stale_prior_holder` that depended on the old contract. Verification gate at the workspace level catches these; targeted `--test <file>` doesn't.
- Codex's gitignore-migration catch is the kind of thing a structural audit would never find. `write_if_missing` is a fresh-repo-only pattern; for any managed file the substrate owns going forward, prefer `reconcile_*` patterns that migrate existing repos.
