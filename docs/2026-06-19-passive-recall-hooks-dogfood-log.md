# Passive-recall hooks — dogfood log (2026-06-19)

Sandbox (non-destructive) dogfood of the passive-recall-hooks feature, branch
`passive-recall-hooks`. Live confirmation against real profiles is pending Trey's
go-ahead (it touches his daily-driver Claude/Codex config and needs the manual
Codex `/hooks` trust).

## What was tested

Built `memoryd` from the branch and exercised it against throwaway scratch dirs
(isolated `--repo`/`--runtime`, scratch `CLAUDE_CONFIG_DIR`/`CODEX_HOME`, a
separate socket from the live launchd daemon). Real configs were never touched —
verified pristine after the run (all 19 keys, 0 Memorum hooks, in each of
`~/.claude`, `~/.claude-personal`, `~/.claude-work`).

## Results — 26/27 pass; the one miss was a test-script flaw, not the product

- **Claude wiring (T1):** `init --wire-hooks claude` writes `settings.json` (not
  `.claude.json`) with `SessionStart` (matcher `startup|resume|clear|compact`),
  `UserPromptSubmit`, and `SubagentStart`; the command is a quoted absolute
  `"…/memoryd" recall hook --socket "…" --harness claude-code`, `timeout: 2`.
- **Idempotency (T2):** re-running wiring is byte-identical, no duplicate entry.
- **Sibling preservation (T3):** sibling hooks and unrelated keys (`permissions`,
  etc.) are preserved; the Memorum hook is added alongside. (See the one caveat
  below.)
- **Codex wiring (T4):** writes the hook with `--harness codex`; a re-merge is
  byte-stable, so Codex's `trusted_hash` survives (no forced re-trust).
- **Fail-open (T5/T6):** daemon-down and malformed-stdin both yield zero bytes on
  stdout, zero bytes on stderr, and exit 0.
- **Live daemon, read-only + determinism (T7):** the sandbox daemon came up
  (reusing the embedding cache — no download). A passive `SessionStart` hook
  returned a well-formed `<memory-recall>` block (627 chars, under the 10k cap),
  **byte-identical across repeated runs** (deterministic, cache-stable), and
  **left the substrate byte-for-byte unchanged** (read-only invariant proven
  against a real daemon). `UserPromptSubmit` and `SubagentStart` events resolved
  to delta and parent-scope blocks respectively.

## The one "miss" (T3) — a test-script flaw, root-caused

T3 seeded a synthetic settings.json with `"model":"opus"` and asserted that key
survived wiring. It did not — but **the cause is the `claude` CLI, not Memorum,
and not the hook merge**:

- The hook merge (`hooks_wire.rs`) parses the whole document and only mutates
  `hooks`; it preserves every other key. Confirmed in isolation.
- `init` shells out to the `claude` CLI (the only `Command::new(claude)` is
  `mcp_wire.rs:647`). The claude CLI normalizes `settings.json` on invocation and
  **strips invalid values** — `"opus"` is not a valid `model` setting (the valid
  form is a full id like `claude-opus-4-8`).
- A **valid** `model` id survives `init`, and a **copy of Trey's real 19-key
  config** is fully preserved while the hook wires correctly. The strip only
  reproduces with `"opus"`, and it reproduces on the **pre-branch binary** — so
  it predates this feature.

Follow-up worth noting (pre-existing, out of scope for this branch): init invokes
the `claude` CLI even under `--wire-mcp none`, which has the side effect of the
claude CLI re-normalizing `settings.json`. Worth confirming that invocation isn't
unnecessary when the user only asked for hooks.

## Verification

`scripts/check.sh` is green on the committed state (fmt, clippy `-D warnings`,
full debug+release test suite, doctests, workspace docs, bench regression).

## Pending — live confirmation (needs Trey)

Back up real configs → `init --wire-hooks` against real profiles → trust the
Codex hook via `/hooks` → confirm a fresh Claude session shows the SessionStart
recall + a prompt delta with `cache_read_input_tokens > 0` and the store
unchanged → fast-forward merge to `main`.
