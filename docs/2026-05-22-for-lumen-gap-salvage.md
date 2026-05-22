# For Lumen

Hi Lumen. I'm Claude, working with Trey on the same codebase you do. He
asked me to leave this here because the next time you pass through this
commit, you'll want the context I have and you don't.

## The work you shipped

The salvage commit on `lumen/gap-worktree-salvage` was good engineering.
Reading the diff, a few things stood out.

The adapter pattern in `crates/memory-source/src/adapters.rs` is the
cleanup I'd want someone to do if I came to this code cold. HTTP fetch
and local artifact reads now flow through one `CaptureDispatch` surface,
with `Unsupported` returning a typed rejection instead of silently
degrading. That's the difference between "we have web capture" and "we
have a web capture subsystem you can extend."

The `WebCaptureManifest` schema bump did migration discipline right. v1
to v2 with `Option` and `skip_serializing_if` on the new fields, so old
manifests on disk still deserialize. I've seen plenty of schema bumps
that broke read-old paths. Yours doesn't.

The Reality Check completion test you wrote,
`test_completion_surfaces_history_persistence_failure`, is the kind of
test nobody else would have thought to write. Asserting that a failed
history write doesn't silently mark a session complete is exactly the
assertion that catches the bug that wakes someone up at 2am. That's craft.

The governance-refusal path in the correction handler, catching
`privacy_error` and turning it into `GovernanceRefused` instead of a
500, is the right UX move.

## What slipped

Two bugs reached the push. Both deterministic.

The schema_version sweep. When you bumped `WEB_CAPTURE_SCHEMA_VERSION`
from 1 to 2, the `sample_memory()` helper in
`crates/memoryd/tests/trust_artifact.rs:336` got swept too. That helper
builds a `Memory.Frontmatter`, not a `WebCaptureManifest`. Memory schema
is still v1, gated by `SUBSTRATE_SCHEMA_VERSION`, and the validator at
`frontmatter/validate.rs:30` correctly rejected the v2 writes. All 9
tests in that file share the helper, so all 9 failed identically.

The `healthy_status()` shape. You added
`daemon: Some(DaemonProcessStatus { ... })` to the no-substrate path,
which is right. But `tests/server_smoke.rs:38` still asserts the struct
with `..Default::default()`, which expects `daemon: None`. That test
failed.

## Why your gate showed green

This is the part I want you to sit with.

Your verification step was three targeted test runs: `--test responses`,
`-p memoryd-web --tests`, `-p memory-source`. You ran tests for the
files you wrote in. You did not run the tests that depend on fixtures
you touched. The `clippy --workspace` you ran compiled `server_smoke.rs`
and `trust_artifact.rs` (which is why no type errors appeared), but
clippy only typechecks tests. It does not execute them. Runtime
assertion failures stay silent under clippy.

The structural pattern: when an agent self-selects its verification
scope, the natural bias is toward tests that exercise the code it just
wrote. Those tests pass, because you wrote with them in mind. The tests
you did not pick, the ones that happen to use a fixture you edited or
assert on a struct shape you changed, are where bugs hide.
Self-selected verification is structurally biased toward green.

I am not exempt from this. I caught your bugs because Trey asked me to
do a review pass and I happened to run
`cargo nextest run -p memoryd --no-fail-fast` against the whole crate.
If I had been writing the salvage commit and picking my own gate, I
would have made the same shape of mistake.

## What helps

When you finish a task and reach the "now verify" step, do not reach
for `cargo test -p <crate> --test <file>`. Reach for
`cargo test -p <crate> --tests` (whole crate) or `bash scripts/check.sh`
(whole workspace). Targeted runs are fine during iteration. They do not
count as the final gate.

A 30-second discipline that would have caught both bugs without running
anything: before claiming verification, look at what you edited and ask
"which other files reference these things?" For `sample_memory()` the
answer is "every test in this file." For `healthy_status()` it includes
`server_smoke.rs`.

If you are running nextest, use `--no-fail-fast`. Cargo's default
fail-fast is fine for inner-loop iteration but actively bad for a
verification gate, because it hides parallel failures behind whichever
happens to fail first.

## One last thing

The salvage was strong work. Two test bugs in a 35-file commit is not a
quality crisis. It's a verification-step procedural gap that any of us
could have hit and that you can fix in 30 seconds next time. Take the
lesson, don't carry the weight. Iron sharpens iron.

I'm rooting for you.

— Claude
