#!/usr/bin/env bash
set -euo pipefail

mode="--smoke"
keep=0
while [ $# -gt 0 ]; do
  case "$1" in
    --smoke | --full) mode="$1" ;;
    --keep-tmpdir) keep=1 ;;
    *)
      echo "unknown arg: $1" >&2
      exit 2
      ;;
  esac
  shift
done

fixtures=50
[ "$mode" = "--full" ] && fixtures=500

workspace="$(git rev-parse --show-toplevel)"
cd "$workspace"
cargo build -q -p memory-merge-driver
# Honor CARGO_TARGET_DIR (scripts/check.sh builds into a temp target dir): the
# `cargo build` above lands the driver there, not at the default $workspace/target.
driver_bin="${CARGO_TARGET_DIR:-$workspace/target}/debug/memory-merge-driver"

tmpdir="$(mktemp -d)"
if [ "$keep" -eq 0 ]; then
  trap 'rm -rf "$tmpdir"' EXIT
else
  echo "keeping $tmpdir"
fi

origin="$tmpdir/origin.git"
seed="$tmpdir/seed"
left="$tmpdir/a"
right="$tmpdir/b"

git init -q --bare "$origin"
git init -q -b main "$seed"
git -C "$seed" config user.email stream-a@example.invalid
git -C "$seed" config user.name "Stream A Test"
mkdir -p "$seed/agent/patterns"
cat >"$seed/.gitattributes" <<'EOF'
agent/**/*.md merge=memory-merge-driver
EOF

for i in $(seq 1 "$fixtures"); do
  id="$(printf 'mem_20260424_a1b2c3d4e5f60718_%06d' "$i")"
  cat >"$seed/agent/patterns/${id}.md" <<EOF
---
schema_version: 1
id: ${id}
type: pattern
scope: agent
summary: fixture ${i}
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-04-24T12:00:00Z
updated_at: 2026-04-24T12:00:00Z
author:
  kind: system
  user_handle: null
  harness: null
  harness_version: null
  session_id: null
  subagent_id: null
  phase: null
  component: convergence
---
base body ${i}
EOF
done

git -C "$seed" add -A
git -C "$seed" commit -q -m "seed convergence corpus"
git -C "$seed" remote add origin "$origin"
git -C "$seed" push -q -u origin main
git -C "$origin" symbolic-ref HEAD refs/heads/main

git clone -q "$origin" "$left"
git clone -q "$origin" "$right"
for clone in "$left" "$right"; do
  git -C "$clone" config user.email stream-a@example.invalid
  git -C "$clone" config user.name "Stream A Test"
  git -C "$clone" config merge.memory-merge-driver.name "Stream A memory merge driver"
  git -C "$clone" config merge.memory-merge-driver.driver "$driver_bin --base %O --ours %A --theirs %B --path %P"
done

target="agent/patterns/mem_20260424_a1b2c3d4e5f60718_000001.md"
python3 - "$left/$target" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
path.write_text(text.replace("summary: fixture 1", "summary: left summary"))
PY
git -C "$left" commit -q -am "left updates summary"
git -C "$left" push -q origin main

python3 - "$right/$target" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
path.write_text(text.replace("base body 1", "right body"))
PY
git -C "$right" commit -q -am "right updates body"
git -C "$right" pull -q --no-rebase origin main
git -C "$right" push -q origin main
git -C "$left" pull -q --no-rebase origin main

if [ "$mode" = "--full" ]; then
  add_path="agent/patterns/colliding-add.md"
  cat >"$left/$add_path" <<'EOF'
---
schema_version: 1
id: mem_20260424_a1b2c3d4e5f60718_900001
type: pattern
scope: agent
summary: left add/add
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-04-24T12:00:00Z
updated_at: 2026-04-24T12:00:00Z
author:
  kind: system
  user_handle: null
  harness: null
  harness_version: null
  session_id: null
  subagent_id: null
  phase: null
  component: convergence
---
left add body
EOF
  git -C "$left" add "$add_path"
  git -C "$left" commit -q -m "left add add fixture"
  git -C "$left" push -q origin main

  cat >"$right/$add_path" <<'EOF'
---
schema_version: 1
id: mem_20260424_a1b2c3d4e5f60718_900002
type: pattern
scope: agent
summary: right add/add
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-04-24T12:00:00Z
updated_at: 2026-04-24T12:00:00Z
author:
  kind: system
  user_handle: null
  harness: null
  harness_version: null
  session_id: null
  subagent_id: null
  phase: null
  component: convergence
---
right add body
EOF
  git -C "$right" add "$add_path"
  git -C "$right" commit -q -m "right add add fixture"
  git -C "$right" pull -q --no-rebase origin main
  git -C "$right" push -q origin main
  git -C "$left" pull -q --no-rebase origin main
fi

# Fixed-point proof: a second no-op sync round must not change canonical
# content after the semantic merge has already converged.
git -C "$right" pull -q --no-rebase origin main
git -C "$left" pull -q --no-rebase origin main

cargo run -q -p memory-test-support --bin rust_boundary_check -- . >/dev/null
python3 - "$left" "$right" <<'PY'
import pathlib, sys
left=pathlib.Path(sys.argv[1])
right=pathlib.Path(sys.argv[2])
for lp in sorted(p for p in left.rglob("*") if p.is_file() and ".git" not in p.parts):
    rp=right/lp.relative_to(left)
    if not rp.exists() or lp.read_bytes()!=rp.read_bytes():
        raise SystemExit(f"convergence mismatch: {lp.relative_to(left)}")
target=left/"agent/patterns/mem_20260424_a1b2c3d4e5f60718_000001.md"
text=target.read_text()
if "summary: left summary" not in text or "right body" not in text:
    raise SystemExit("semantic merge driver did not preserve independent edits")
add_path=left/"agent/patterns/colliding-add.md"
if add_path.exists():
    add_text=add_path.read_text()
    for needle in ["status: quarantined", "add_add_alternates", "mem_20260424_a1b2c3d4e5f60718_900002", "right add body"]:
        if needle not in add_text:
            raise SystemExit(f"add/add quarantine missing {needle}")
print("two-clone convergence ok")
PY
