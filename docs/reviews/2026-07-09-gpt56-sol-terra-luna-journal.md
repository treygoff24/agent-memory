# GPT-5.6 lane journal — Sol / Terra / Luna field notes (API embedding lane build)

Live evaluation journal for the new Codex GPT-5.6 aliases (`sol`, `terra`, `luna`) exercised as delegate
lanes during the Memorum API-embedding-lane build, 2026-07-09. Feeds back into the `delegate-agent` skill
roster. Prior baseline: GPT-5.5 Codex = "the AUTHOR" (strong coherent builds, author-blindness failure
mode).

## Routing plan for this run

| Task | Model | Effort | Rationale |
| --- | --- | --- | --- |
| T2.2 rate-limit + credential hardening | sol | high | Trust-adjacent, decision-dense (error-classification refactor, credential file security) |
| T3.1 init/config CLI + consent | terra | medium | Everyday implementation, well-specified brief |
| T3.2 doctor findings | luna | medium | Bounded, mechanical-ish, follows existing idiom |
| Wave-3 review gate | luna high vs terra medium (TBD) | — | Probe review quality at the fast tier |

Installed delegate supports `--model` + `--reasoning-effort`; NO `--fast` flag in this build (service tier
inherits Codex config). Luna caps at `max` (no `ultra`); Sol/Terra `ultra` = nested multi-agent mode, not
used this run.

## Observations

### T2.2 — sol, effort high (run group `apilane-t22`, launched ~this session)

- Brief: `/tmp/memorum-briefs/T2.2-ratelimit-hardening.md` — 3 security findings (credential write-window +
  O_NOFOLLOW, 400-INVALID_ARGUMENT→Auth, stranded blocking threads), drain-loop rate-limit backoff with
  typed error classification, microbatch sizing. Owned: `api_provider.rs`, `worker.rs` (backoff only, fence
  frozen).
- Attempt 1 (codex-66, personal auth): KILLED mid-run by the personal Codex usage-limit reset — no
  model-quality signal, not a lane failure. Delegate config rerouted (codex.authProfile → work,
  fallbackProfile → work, until 6:30pm CDT) but the work OAuth token was revoked (stale since 6/30);
  blocked on Trey re-login before relaunch.
- **Plot twist:** the "dead" run had already FINISHED the implementation before the usage limit killed it —
  it died during its own verification gate. All three deliverables complete + tested on disk:
  - R1 credential hardening is *better than briefed*: symlink reject pre-open, dev/ino TOCTOU verify
    post-open, chmod-on-fd BEFORE `set_len(0)` + write (no world-readable window), symlink-target
    non-clobber test.
  - R2 400→Auth body sniff (`API_KEY_INVALID`/`PERMISSION_DENIED`/message match) with mock test.
  - R3 timeouts 30s → 2s connect / 8s total, named consts.
  - Backoff: typed `DrainError` enum (no string parsing), RateLimit propagated from batch AND per-job
    paths before any budget charge, run loop honors Retry-After (60s fallback) without growing the
    generic backoff. Test proves budget uncharged + jobs stay pending + Retry-After=7s honored.
  - Microbatch cap 100 (named const, T4.1-validation comment) with 101-doc order-preservation test.
- Scope discipline: PERFECT — exactly the two owned files, fence code untouched, brief's "frozen" lines
  respected. One acceptable design nit: mid-loop 429 in `embed_jobs_individually` discards
  already-embedded survivors (re-billed next tick) rather than writing them first.
- Wall clock: ~35 min to full implementation + tests before the auth kill (limit was usage, not model).
- **Verdict: sol×high = GPT-5.5-Codex quality or better on trust-adjacent hardening.** Notably it
  *upgraded* the brief's security spec (fd-based TOCTOU verify wasn't asked for). Gate run by
  orchestrator post-mortem since the lane died pre-gate.

### sol×high postscript — the one defect

Sol's single miss: its new async test built (and dropped) the reqwest *blocking* client inside the tokio
test runtime — panics with "Cannot drop a runtime in a context where blocking is not allowed". It
half-knew (the test's last line spawn_blocks the *drop*) but missed that *construction* also enters the
runtime (`reqwest::blocking::wait::enter` in `ClientHandle::new`). Orchestrator fixed by building under
`spawn_blocking` too. Classic author-blindness residue, though milder than GPT-5.5's (it died pre-gate,
so the lane may well have caught this itself). Final: clippy clean, 398/398, committed `493980e`.

### T3.1 — terra, effort medium (group `apilane-t31`, work auth)

- Brief: `/tmp/memorum-briefs/T3.1-init-config-consent.md` — init/config lane selection, Q5 consent
  prompt, R4 cost estimate, D4 switch mechanics. Bigger + more open-ended than T2.2 (CLI surface design
  judgment required) — good probe of terra at its recommended default effort.
- Status: IN FLIGHT.

## Matrix scorecard (fill as evidence lands)

| Model × effort | Task type | Wall clock | Scope discipline | Quality | Notes |
| --- | --- | --- | --- | --- | --- |
| sol × high | hardening (T2.2) | — | — | — | — |
| terra × medium | impl (T3.1) | — | — | — | — |
| luna × medium | impl (T3.2) | — | — | — | — |

## Standing comparisons

- GPT-5.5 Codex baseline this run: T1.1/T1.2/T2.0/T2.1a/T2.1b all landed near-right first try; one latent
  clippy failure slipped through MY gate (not the lane's fault — grep-based exit detection). cursor/Grok 4.5
  showed scope creep into `doctor.rs` on T2.3.
