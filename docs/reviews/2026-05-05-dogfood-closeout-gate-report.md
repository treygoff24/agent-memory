# Dogfood closeout gate report

Date: 2026-05-05

Branch state during verification: `main...origin/main` with the dogfood-closeout
worktree dirty. Safety snapshots were saved before implementation at
`/tmp/agent-memory-dogfood-closeout-baseline.patch` and
`/tmp/agent-memory-dogfood-closeout-baseline.status`.

## Current dogfood result

The dogfood bar is green for local use.

`./scripts/check-fast.sh` passed in 26s warm-cache after fixes. The final
full dogfood rerun, after the reviewer follow-up fixes, passed in 7m46s:

```text
check-fast passed
check-fast duration: 37s
...
test doctor_unhealthy_exit_is_nonzero_when_no_harness_is_authenticated ... ok
...
check-dogfood passed
check-dogfood duration: 466s
./scripts/check-dogfood.sh  31.91s user 52.03s system 17% cpu 7:46.72 total
```

The final dogfood gate includes the direct unhealthy `memoryd doctor` CLI exit
regression requested by the test/gate review.

## Commands run

```bash
git status --short --branch
git diff --stat
git diff --binary > /tmp/agent-memory-dogfood-closeout-baseline.patch
git status --short --branch > /tmp/agent-memory-dogfood-closeout-baseline.status
cargo fmt --all -- --check
bash -n scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh
scripts/install-memorum.sh --dry-run --repo /tmp/memorum-dry --runtime /tmp/memorum-dry/.memoryd --socket /tmp/memoryd-dry.sock
tmp=$(mktemp -d); mkdir -p "$tmp/runtime"; printf '999999\n' > "$tmp/runtime/memoryd.pid"; scripts/install-memorum.sh --dry-run --repo "$tmp/repo" --runtime "$tmp/runtime" --socket "$tmp/memoryd.sock"; test -f "$tmp/runtime/memoryd.pid"
cargo clippy -p memoryd -p memoryd-tui -p memorum-eval -p memorum-coordination --all-targets -- -D warnings
./scripts/check-fast.sh
./scripts/check-dogfood.sh
cargo run -p memoryd --bin memoryd -- --version
cargo clippy -p memoryd --bin memoryd -- -D warnings
scripts/install-memorum.sh --force-reinstall --repo "$tmp/repo" --runtime "$tmp/runtime" --socket "$sock"
memoryd --version
memoryd status --socket "$sock"
pnpm exec oxfmt --ignore-path .oxfmtignore docs/reviews/2026-05-05-dogfood-security-review.md
```

## Fixes found by gates

The first `check-fast` run failed on a clippy `too_many_arguments` finding in
`crates/memorum-eval/tests/eval/domain/t15_privacy_filter_refusal_retry.rs`.
That was fixed by replacing the loose helper arguments with a small
`PrivacyRetryHarness` config struct.

The next `check-fast` run failed on Markdown formatting in
`docs/api/stream-h-eval-api.md` and
`docs/reviews/2026-05-04-dogfood-readiness-claude-review.md`. `oxfmt` fixed
both.

An early `check-dogfood` run was stopped after 1808s because
`cargo test -p memoryd doctor_health` was enumerating every memoryd integration
binary to run one unit test. The gate script now uses `cargo test -p memoryd
--lib ...` for the two memoryd unit-test filters. The rerun passed in 13m15s,
and the final rerun after later installer/CLI fixes passed in 11m18s.

The first real install smoke started a daemon successfully, but then exposed
that `memoryd --version` was unsupported. That made installer version-skip
untrustworthy. The CLI now exposes Clap's version flag. The installer version
probe also tolerates stale installed binaries whose `--version` exits nonzero.

The independent review loop found no dogfood blockers. Coordinator follow-up
closed the cheap dogfood risks it surfaced:

- Runbook doctor examples now include `--repo ~/memorum --runtime ~/memorum/.memoryd`.
- `check-dogfood.sh` now runs a direct `doctor_unhealthy_exit` CLI regression.
- Installer runtime, log, and PID artifacts are chmodded to `700`/`600`.
- Installer PID reuse validates a numeric PID and expected `memoryd serve`
  command before signaling.
- Doctor missing-CLI output no longer includes the daemon's raw `PATH`.

## Real temp installer smoke

Final smoke command shape:

```bash
tmp=$(mktemp -d)
sock="$tmp/memoryd.sock"
scripts/install-memorum.sh --force-reinstall --repo "$tmp/repo" --runtime "$tmp/runtime" --socket "$sock"
memoryd --version
pid=$(cat "$tmp/runtime/memoryd.pid")
kill -0 "$pid"
memoryd status --socket "$sock"
kill "$pid"
```

Result: pass. `cargo install --path crates/memoryd --locked` replaced the stale
binary, installer readiness succeeded, PID `62736` was live during the smoke,
`memoryd --version` printed `memoryd 0.1.0`, and `memoryd status --socket "$sock"`
returned a JSON `ready` status. The final measured install-smoke duration was
18s. The smoke also verified artifact modes:

```text
700 <tmp>/runtime
600 <tmp>/runtime/memoryd.log
600 <tmp>/runtime/memoryd.pid
```

## Expensive checks not run

These were explicitly outside the dogfood closeout bar and were not run:

- `bash scripts/check.sh` as a full release gate.
- `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh`.
- Workspace-wide nextest/release tests.
- Two-clone convergence.
- Durability matrix.
- Bench smoke/release/regression gates.
- Live paid Claude/Codex eval execution with real provider keys.

## Deferred non-goals

The closeout intentionally leaves these for release/product work: T17 lease
re-entrancy, T18 key rotation, rich Recall protocol fields for score/harness
source/surfaced session, live paid eval CI, full multi-device production
hardening, and making the release gate mandatory for dogfood loops.
