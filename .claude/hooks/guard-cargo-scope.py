#!/usr/bin/env python3
"""PreToolUse guard: block whole-workspace `cargo clippy`/`cargo test` that
spawns the per-crate compiler swarm and roasts syspolicyd on macOS.

Allows anything scoped with -p/--package/--manifest-path, anything that isn't a
direct cargo clippy/test/nextest invocation (so `bash scripts/check.sh` — the
blessed full gate — passes, since its top-level command is the script, not cargo).

Fail-OPEN: any internal error allows the command. A guard bug must never wedge
every Bash call. Run `python3 guard-cargo-scope.py --self-test` to check the matcher.
"""
import json
import re
import sys

SEP = re.compile(r"&&|\|\||;|\n|\|")
LEAD_ENV = re.compile(
    r"^(?:[A-Za-z_][A-Za-z0-9_]*=\S+\s+|env\s+|time\s+|nice\s+(?:-n\s*-?\d+\s+)?|taskpolicy\s+-b\s+)"
)
SCOPE = ("-p", "--package", "--manifest-path")
HEAVY = ("clippy", "test")  # cargo subcommands whose workspace form spawns the swarm

REASON = (
    "Blocked: whole-workspace `cargo {sub}` spawns one compiler process per crate "
    "(12 of them) and roasts syspolicyd on macOS. Scope it: `cargo {sub} -p <crate>`. "
    "The full gate is `bash scripts/check.sh` — trunk-only, end-of-task only. "
    "See the 'Build / lint / test CPU discipline' section in CLAUDE.md / AGENTS.md."
)


def offending_subcommand(command: str):
    """Return the heavy subcommand name if `command` contains an unscoped
    workspace cargo clippy/test/nextest invocation, else None."""
    for raw in SEP.split(command):
        seg = raw.strip()
        # peel leading env-assignments / wrappers (env, time, nice, VAR=val)
        prev = None
        while seg != prev:
            prev = seg
            seg = LEAD_ENV.sub("", seg)
        toks = seg.split()
        if not toks:
            continue
        if toks[0] == "cargo":
            rest = toks[1:]
        elif toks[0] == "cargo-nextest":
            rest = toks  # treat `cargo-nextest run ...` like `cargo nextest run ...`
            rest[0] = "nextest"
        else:
            continue
        # first non-flag/non-toolchain token is the subcommand
        sub = next((t for t in rest if not t.startswith("-") and not t.startswith("+")), None)
        if sub == "nextest":
            after = rest[rest.index("nextest") + 1:]
            run = next((t for t in after if not t.startswith("-")), None)
            if run != "run":
                continue
            sub = "test"
        elif sub not in HEAVY:
            continue
        if is_scoped(rest):
            continue  # scoped → fine
        return sub
    return None


def is_scoped(tokens: list[str]) -> bool:
    for scope in SCOPE:
        if scope in tokens:
            return True
        for token in tokens:
            if token.startswith(f"{scope}=") or (scope == "-p" and token.startswith("-p") and token != "-p"):
                return True
    return False


def _emit_deny(reason: str):
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    }))


def main():
    try:
        data = json.load(sys.stdin)
    except Exception:
        sys.exit(0)  # fail-open
    if data.get("tool_name") != "Bash":
        sys.exit(0)
    command = (data.get("tool_input") or {}).get("command", "")
    if not isinstance(command, str) or not command.strip():
        sys.exit(0)
    try:
        sub = offending_subcommand(command)
    except Exception:
        sys.exit(0)  # fail-open
    if sub:
        _emit_deny(REASON.format(sub=sub))
    sys.exit(0)


def _self_test():
    deny = [
        "cargo clippy --workspace --all-targets --all-features -- -D warnings",
        "cargo clippy",
        "cargo clippy --all",
        "cargo test --workspace",
        "cargo test",
        "cargo test some_filter",
        "cargo nextest run --workspace",
        "cargo-nextest run --workspace",
        "CARGO_TARGET_DIR=/tmp/x cargo clippy --workspace",
        "env RUST_LOG=debug cargo test --workspace",
        "taskpolicy -b cargo test --workspace",
        "cargo +stable test --workspace",
        "cargo +nightly clippy",
        "cd crates && cargo clippy",
        "cargo build --workspace && cargo clippy",
    ]
    allow = [
        "cargo clippy -p memoryd --all-targets -- -D warnings",
        "cargo clippy -p memoryd -p memory-substrate",
        "cargo test -p memoryd -- --test-threads=2",
        "cargo test -pmemoryd",
        "cargo test --package=memoryd",
        "cargo clippy --manifest-path=crates/memoryd/Cargo.toml",
        "cargo check -p memoryd",
        "cargo check --workspace",            # check compiles but execs no swarm; allowed
        "cargo build --workspace --locked",   # docs recommend this for lockfile work
        "cargo fmt --all -- --check",
        "cargo doc --workspace --no-deps",
        "cargo tree -i memory-substrate",
        "bash scripts/check.sh",
        "BENCH_PROFILE=darwin-arm64 ./scripts/check.sh",
        "cargo nextest run -p memoryd",
        "cargo clippy --manifest-path crates/memoryd/Cargo.toml",
        'echo "remember: never run cargo clippy on the whole workspace"',
        "git commit -m 'fix cargo test flakiness'",
    ]
    bad = []
    for c in deny:
        if offending_subcommand(c) is None:
            bad.append(("SHOULD DENY", c))
    for c in allow:
        if offending_subcommand(c) is not None:
            bad.append(("SHOULD ALLOW", c))
    if bad:
        for kind, c in bad:
            print(f"FAIL [{kind}]: {c}")
        sys.exit(1)
    print(f"ok — {len(deny)} deny + {len(allow)} allow cases pass")


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        _self_test()
    else:
        main()
