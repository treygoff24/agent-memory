# Run record — recall-quality autoresearch loop (Codex × Claude controller)

**Date:** 2026-06-11. **Harness archive:** `~/Code/agent-memory-autoresearch/runs/2026-06-11-recall-quality/` (constitution, controller, scorer, full ledger, orchestrator journal, per-iteration logs). **Architecture:** an external bash controller (`loop.sh`) spawned one fresh `codex exec` per iteration against a sandboxed worktree; each iteration proposed ONE change to seven allowed recall files; a clean-room scorer (`score.sh`: tamper check → allowed-path patch → capacity guards → contract test suites → real-Qwen3 fused eval on the golden fictional corpus) decided keep/revert on a ratchet. Claude orchestrated between iterations via a `DIRECTIVE:` line in the constitution, a 30-min monitor cron, and held-out checkpoints on a private corpus the agent could never see.

## Outcome

**36 iterations, 5 keeps. J = nDCG@5 − 0.5·trap@5: 0.6456 → 0.6757 (+0.030) visible; 0.5839 → 0.6150 (+0.031) held-out.** Both held-out checkpoints passed (nDCG trend divergence ≈0.006 vs the 0.05 overfit-revert threshold). Merged to trunk as commits `757ab84..df65f56` (fast-forward, 2026-06-11 evening).

| Iter | Commit | Keep | Effect |
|---|---|---|---|
| 3 | 757ab84 | Qwen query instruction → stored-memory recall framing | nDCG +0.005, recall@5 +0.027 |
| 11 | ece74cb | Query instruction: lookalike disambiguation (exact identifiers/dates over topical adjacency) | nDCG +0.002 |
| 21 | 634bc2a | Query instruction: evidence-grade exact-answer preference | trap@5 0.20 → 0.18 |
| 32 | 9e60829 | Recency tie-break: 2.5e-4 RRF epsilon window, newer `mem_YYYYMMDD` wins near-ties | trap@5 0.18 → 0.16 (nDCG −0.008 cost) |
| 35 | df65f56 | Bounded relaxed OR-term BM25 fallback for underfull lexical lanes | nDCG 0.742 → 0.776, recall@5 0.79; best held-out generalizer (+0.031 J, trap flat OOS) |

Final visible metrics: nDCG@5 0.776, recall@5 0.790, MRR 0.860, trap@5 0.200 (re-risen — see pending).

## Research findings (negative results included — they're most of the value)

1. **Fusion-level re-weighting is saturated.** Eight experiments (cosine floors, lane weights, verifier multipliers, rrf_k 60→10, lane-limit 20→32, fanout 8→16) produced bit-identical metrics. Re-ranking the existing candidate pool cannot move this eval; only representation changes (query instruction, lane composition) ever moved metrics.
2. **Traps are stale, not semantic.** The constitution's prior ("the vector lane surfaces traps") was empirically wrong. Per-case analysis showed traps are BM25-retrieved superseded lookalikes sitting in zero-gain tails (e.g. q21: trap dated 2025-09-21 at rank 1 vs the true answer dated 2026-02-04). That diagnosis produced the recency tie-break keep, and the mechanism generalized OOS (held-out trap −0.022).
3. **Query-side instruction tuning went 3-for-3 on keeps** but held-out suggests those gains are partially visible-corpus-specific; the mechanism keeps (recency, OR-fallback) generalized best.
4. **Doc-side embedding changes are fragile in both directions:** chunk-context enrichment broke the governance contradiction-detection e2e (document similarity also feeds governance), and a lighter variant tanked nDCG. Treat document-embedding inputs as a coupled contract.
5. **Orchestrator directive scorecard: ~0-for-5 on mechanism guesses until grounded in per-case data;** Codex free-choice earned 3 of the 5 keeps. Steering earned its keep only after the per-case dump analysis.

## Process notes

- The eval is deterministic, so the initial keep threshold (ΔJ ≥ 0.003) discarded a real +0.0022 improvement; a mid-run pit-stop lowered it to 0.0005 and re-ran the lost experiment (it kept).
- Directives must self-expire: iterations (~10 min) outpace 30-min monitor checks, and a stale directive burned 4 iterations on re-runs before the "if any ledger row matches the directive, it's consumed" rule was added to the constitution.
- One false-negative crash: substrate watcher tests hung to timeout on an environment flake, mis-charging an innocent patch.
- `memorum-eval` lacks fused per-case dump support; the trap analysis required local uncommitted instrumentation (reverted). Backlog: add `--dump-cases` to the fused seam.
- **Stop cause was environmental, not convergence:** macOS rewrote the "Codex Safe Storage" keychain items mid-run, wedging every new `codex` exec on a SecurityAgent prompt. Clean KILL at iter 36 with 4 iterations left of the 40 cap.

## Pending

1. **Trap re-suppression (staged, resumable):** iter 35's fuller lexical lanes re-admitted stale lookalikes (trap 0.16 → 0.20 visible; flat OOS). The archived `program.md` carries the staged directive — re-tune the recency window on the new baseline, target J ≈ 0.70. Resume = keychain repair (`fix-mcp-keychain-acl.zsh`) + `rm KILL` + relaunch `loop.sh`.
2. This run informs the fusion-arc **D1 arm-vs-tune decision** (trap-rate mitigation now has demonstrated config/code knobs).
3. `memorum-eval` fused `--dump-cases` support.
