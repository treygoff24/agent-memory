# Import hardening plan — 2026-06-19

Implements the 7 findings from `docs/2026-06-18-import-dogfood-log.md` plus a first-class agent flow. Branch: `import-hardening`. Owner: Claude (autonomous), rust-engineer discipline, subagent-implemented, Codex-reviewed, then shipped + re-dogfooded.

## Locked design decisions

1. **Non-git-cwd default = `me`, never skip.** Memories are always saved and placed; `me` keeps side effects inside the Memorum repo (no `.memory-project.yaml` littering `~/` or `~/Code`). `generate` stays opt-in. The non-interactive default flips from `Skip` → `Me` (`cli/init/agent.rs:86`).

2. **Non-git memories land as active, derived-project-scoped memories by default** (REVISED — see pivot below). The original plan was to auto-activate me-scope candidates via bulk `ReviewApprove`. That is **structurally blocked**: `review_decision_response` refuses encrypted memories (`handlers/review.rs:126` — "encrypted review decisions require an encrypted lifecycle update API"), and me-scope imports are encrypted-at-rest. Rather than build a new encrypted-promote API (Stream-D-adjacent, risky to do autonomously), we fix the dormancy **at write time**: route non-git cwds to a project namespace derived deterministically from the cwd path via the existing `derive_canonical_id_for_dir` (`import/project_map.rs:333`), with `project_yaml: None` (no file written). Project scope uses the default policy (no 0.85 floor) → memories land **Active** at the importer's existing 0.7 confidence, recall-visible immediately, no me-strict review gating, no encryption-by-me-default. The privacy classifier still runs per-write, so genuinely sensitive content is still encrypted (and surfaces via masked recall) — we only bypass the me-strict *review-queue* gating, not privacy classification. **No spec/policy change.** `me`, `generate`, `skip` remain available; `generate` still writes the persistent `.memory-project.yaml`. Bulk-approve and an encrypted-promote API are deferred (logged as follow-ups).

3. **Multi-profile union by default.** When `--from-claude` is omitted, discover the union of all existing `~/.claude*/projects` roots (precedence root + auto-detected siblings), not just the `CLAUDE_CONFIG_DIR` one. `--from-claude` may be passed multiple times to pin exact roots. Within-run dedup by `source_key` (cross-root duplicates collapse). Codex unchanged.

4. **Lenient frontmatter recovery.** On `serde_yaml` failure, fall back to a line-scan for `name`/`description`/`type` and import the body anyway. Recovered files are counted and surfaced, never silently dropped. Only genuinely unreadable files (non-UTF8, unterminated frontmatter) remain hard errors.

5. **Reconciliation report.** Both the CLI and the `init` wizard print a closing reconciliation: active / queued-for-review / privacy-blocked / parse-recovered / dropped counts, with the exact next-action command. Machine-readable via `--report <json>` (already exists) extended with `candidates[]` / `quarantined[]` source lists.

6. **First-class agent flow.** Ship a skill (`skills/using-memorum/SKILL.md`) + `docs/agent-import-guide.md` teaching an agent to drive import non-interactively: one-command union import, reading the JSON reconciliation, activating/forgetting, recovering parse errors. Good machine defaults + stable exit codes.

7. **CLI consistency.** `doctor` accepts-and-ignores `--socket` (mirrors the existing "Accepted for command symmetry" pattern at `cli/mod.rs:771`).

## Invariant guards (must hold)
- No change to me-strict policy or any spec/plan version (CLAUDE.md §critical-invariants, §what-not-to-do).
- Bulk-approve goes through `ReviewDecision::apply`; every write keeps a `ClassificationOutcome`; `secret` never persisted.
- `scripts/check.sh` is the gate, run at coordinator on the integrated branch — never per-subagent.

## Work breakdown (file ownership)

| ID | Change | Primary files | Wave |
|----|--------|---------------|------|
| F2 | Lenient YAML + dotfile decode | `import/sources/claude.rs` (+ tests); expose `recovered: Vec<String>` on `ClaudeParseOutput` | 1 (parallel) |
| F7 | Agent skill + guide | `skills/using-memorum/SKILL.md`, `docs/agent-import-guide.md` (new) | 1 (parallel) |
| F1 | Multi-profile union discovery | `import/discovery.rs`, `import/pipeline.rs` (parse loop + dedup), `cli/import.rs`, `cli/mod.rs` (ImportArgs) | 2a |
| F4 | Reconciliation report (+ wire F2 recovered count) | `import/report.rs`, `import/pipeline.rs`, `cli/import.rs` | 2b |
| F5 | Bulk-approve protocol/handler/CLI + importer auto-activate | `protocol.rs`, `handlers/review.rs`, `handlers/mod.rs`, `cli/review.rs`, `cli/mod.rs`, `import/pipeline.rs` | 2c |
| F3+F6+init | non-git default=me, doctor `--socket`, init wizard union + epilogue | `cli/init/agent.rs`, `cli/mod.rs` (DoctorArgs), `setup/{detect,steps}.rs`, `cli/init/interactive.rs` | 2d |

Coupled files (`pipeline.rs`, `report.rs`, `cli/mod.rs`, `cli/import.rs`) → waves 2a–2d run sequentially, coordinator builds + integrates between each.

## Execution tail
1. `bash scripts/check.sh` green on `import-hardening`.
2. `/delegate-agent` Codex code review → fix findings.
3. Fast-forward merge to `main`.
4. Rebuild + reinstall `memoryd` to `~/.cargo/bin`; restart daemon (launchctl).
5. `memoryd uninstall` → reinstall → reimport (full union) → verify all 7 findings fixed in the live store.
6. Update `docs/2026-06-18-import-dogfood-log.md` with the re-dogfood results; update `docs/importer.md` for new defaults.
