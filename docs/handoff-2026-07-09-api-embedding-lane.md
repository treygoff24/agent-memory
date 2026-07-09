# Handoff 2026-07-09 — API embedding lane (post-restart pickup)

**For:** next Claude session in this repo, picking up with Trey after a computer restart.

## Where we are

- **The plan is drafted and Trey has ratified the vendor: `gemini-embedding-2` (NOT Voyage).** Plan: `docs/plans/2026-07-09-api-embedding-lane.md` (read it first — it is self-contained: architecture audit, decisions D1–D8, resolved audits Q1/Q2/Q6-partial, scout research tables, 4-wave task graph). Commits `e72bece..` on `main` today, ending with the ratification note.
- Decision rationale: cost gap is ~$0.45/mo at Trey's scale (823 memories, ~430k tokens corpus, ~2–3M tokens/mo flow); Gemini's verified no-training paid tier beats Voyage's trains-by-default; native multimodality = future image/video memories for free (explicitly NOT in scope now).
- The D7 ship gate is NOT waived: gemini-embedding-2 must match/beat local Qwen3-0.6B on trap-rate@5 + abstention gap on `fixtures/golden/_embed_bench/` (T4.1) or we fall back to voyage-4-lite / jina-v5.
- Scout research (sonnet exa agent + codex delegate lane) is folded into the plan's Model candidates section; raw codex report is in this session's transcript only — the plan captures everything load-bearing.

## Not yet done / next actions

1. Trey to ratify decisions D1–D8 (vendor now decided; the rest were drafted for his review — walk him through them or just ask "any objections to D1–D8?").
2. Pre-build unknowns: Gemini rate limits for a background daemon (Q4), consent-prompt copy (Q5), ZDR approval mechanics, output dims choice (768 vs 1536 vs 3072 — bake-off should test at least two).
3. Then execute the task graph (Waves 1–4), implementation delegated per repo convention.

## Unrelated open threads in this repo (context, don't lose)

- **Memory footprint lab is DONE and live** (6.9GB → 1.27GB warm; journal `docs/perf/2026-07-08-memory-footprint-lab.md`). Its branch `delegate/codex-20260709T010123Z_ae2406` (commits `07f802f`, `6786014`) is **unmerged**: needs full `scripts/check.sh` on trunk (the one blessed heavy command; bench-regression stage is known-flaky), removal of the untracked duplicate copies of the journal + `scripts/perf/memlab-embed-footprint.sh` in the main checkout first, and Trey's packaging call on vendored candle (`third_party/` in-repo vs GitHub fork pin). Cleanup of that worktree ONLY via `delegate worktree remove`.
- Visual explainer of the lab lives at `~/.agent/diagrams/memoryd-footprint-lab.html`.
- Pre-existing, untouched: doctor reports `sync_blocked`, 18 quarantined memories awaiting `memoryd review`.
- Pushes remain gated on Trey per-push; local commits ungated.
