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
- Attempt 1 (codex-67): **blocked in 53s, correctly.** Explored the dispatch surface, found my brief's
  owned-files list couldn't wire a new CLI verb (main.rs unlisted) or compute the R4 estimate (no
  substrate corpus-stats API), changed NOTHING, and asked for minimal additional ownership. That's the
  classic Codex failure-reporting virtue intact in terra at medium effort — 53s to a precise, actionable
  block beats 20min of scope creep (contrast: Grok-4.5/cursor creeping into doctor.rs on T2.3).
- Attempt 2 (codex-68): **blocked again, deeper and again correctly** (~2min). Found that NO public
  config-mutation API exists — `config.yaml` is written only by substrate bootstrap/init internals — and
  refused to build the "explicitly prohibited second config writer" in memoryd. Asked for a narrowly
  scoped substrate config-mutation surface. Both blocks are really *plan defects* (v0.2's T3.1 says
  "triple write to synced config.yaml" as if the surface existed); terra surfaced them instead of
  papering over. Two-for-two on the stop-boundary contract.
- Attempt 3 (codex-69): implemented the whole surface (config CLI, consent, cost estimate, substrate
  mutation + stats API) but its run window expired mid-`cargo check`. Honest report: flagged its own two
  follow-ups (envelope non-conformance, duplicated triple consts) unprompted.
- Attempt 4 / finish pass (codex-70): fixed both follow-ups + a gate-found type mismatch; window expired
  again during compilation, gates unrun. Orchestrator ran gates, fixed 2 clippy lints (items-after-test-module
  — really a boundary-check collision; collapsible_if), corrected the price const ($0.15 → plan-ratified
  $0.20/M), and added the daemon-side consent enforcement + tests. Committed `310fd3b`.
- **Verdict: terra×medium = excellent judgment, short legs.** Two-for-two on correct scope-blocks
  (both were genuine plan defects), clean idiomatic implementation (`Value`-based YAML RMW preserving
  operator keys was exactly right), honest self-reporting of unfinished follow-ups. Weakness: its run
  windows kept expiring during Rust compilation — on this workspace terra needs tasks sized so the gate
  fits, or the orchestrator should own gates by default (codex windows ≪ Rust cold-compile times).
- **Cross-cutting lesson (all GPT-5.6 lanes):** crate-scoped clippy/test does NOT include this repo's
  release-gate validators (rust_boundary_check bans raw unwrap/expect in substrate src — even in
  #[cfg(test)] modules — without `expect-justified:` annotations). Lane briefs for memory-substrate work
  should name that rule; the full-suite run at the orchestrator caught it.

### T3.2 — luna, effort medium (codex-71, work auth)

- Single-file diff exactly as briefed, 5 findings + tests, ran its own clippy gate to completion (unlike
  terra it POLLED the compile with `ps` loops instead of dying — the fast tier's longer effective window
  or better process discipline, worth watching). Disclosed its interrupted final test rerun honestly.
- One quality nit found later by its own sibling reviewer: the rate-limit/offline advisories keyed off a
  signal (`lifecycle.last_error`) that drain errors never populate. The brief allowed "closest honest
  signal" and it disclosed the telemetry gap in the finding message, so partial credit — but a stronger
  run would have flagged that the signal was structurally dead.
- **Verdict: luna×medium = ideal for bounded follow-the-idiom work.** Fast, scope-perfect, honest.

### Wave-3 review round — luna×high (safe) vs native Opus subagent, in parallel

- **Native Opus:** 9 findings. Unique catches: doctor key-check Err-arm false-fire on env-only setups;
  the rate-limit substring mismatch ("rate limit" vs the actual "rate-limited" Display); the hardcoded
  2-param IN clause; schema COVERED gap (also found by luna). One finding REFUTED by code
  (claimed pending count is always 0 at switch — Substrate::open runs reconcile, so it's real).
- **luna×high:** 8 findings, zero refuted. Unique catches: `init --print-only` still mutates config (real
  dry-run violation both the author AND the native reviewer missed); the structural version of the
  rate-limit issue (the signal is never populated at all — deeper than the substring bug); cost-estimate
  overstatement on switch-back. Also independently confirmed the consent gate + fence clean, including
  the crash-ordering analysis.
- Overlap (both): atomic-write blocker on config.yaml, YAML comment loss, schema gap, init-TTY
  consent-mode bug.
- **Verdict: luna×high is a legitimate adversarial reviewer** — on this round it matched or beat the
  native Opus review on depth (print_only + dead-signal) while Opus was stronger on string/API-level
  precision. Same-family review (GPT reviewing GPT) did NOT show author-blindness here, but keep
  cross-family for trust-critical gates.

## Matrix scorecard (fill as evidence lands)

| Model × effort | Task type | Wall clock | Scope discipline | Quality | Notes |
| --- | --- | --- | --- | --- | --- |
| sol × high | trust-adjacent hardening (T2.2) | ~35 min | Perfect (fence untouched) | Exceeded brief (fd TOCTOU verify unasked) | 1 async-test defect; died mid-gate to usage limit, not model |
| terra × medium | open-ended CLI impl (T3.1) | 4 runs | Perfect × 2 scope-blocks (both real plan defects) | Clean, idiomatic YAML RMW | Run windows too short for Rust gates; orchestrator finished |
| luna × medium | bounded findings impl (T3.2) | 1 run | Perfect (single file) | Good; one structurally-dead signal | Polled compiles instead of dying; honest disclosure |
| luna × high (safe) | adversarial review (W3) | 1 run | n/a | 8 findings, 0 refuted; beat native Opus on depth | Caught print_only + dead-signal; keep cross-family for sacred gates anyway |

**Bottom line for the delegate-agent skill:** Sol = author for trust-critical/hard work (high effort);
Terra = judgment-dense everyday impl but orchestrator owns cargo gates (window-limited); Luna = bounded
impl AND a shockingly strong cheap reviewer at high effort. All three inherit Codex's scope-discipline +
honest-failure-reporting virtues; none showed cursor-style scope creep.

## Standing comparisons

- GPT-5.5 Codex baseline this run: T1.1/T1.2/T2.0/T2.1a/T2.1b all landed near-right first try; one latent
  clippy failure slipped through MY gate (not the lane's fault — grep-based exit detection). cursor/Grok 4.5
  showed scope creep into `doctor.rs` on T2.3.
