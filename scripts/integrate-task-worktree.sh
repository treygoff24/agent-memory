#!/usr/bin/env bash
set -euo pipefail
task_id="${1:?task id required}"
gate="narrow"
if [ "${2:-}" = "--gate" ]; then gate="${3:?gate required}"; fi
if [ -n "$(git status --porcelain=v1 --untracked-files=all)" ]; then
  echo "main has uncommitted changes" >&2
  exit 1
fi
branch="$(git -C "../agent-memory-wt/task-${task_id}" branch --show-current)"
git -C "../agent-memory-wt/task-${task_id}" rebase main
task_worktree="../agent-memory-wt/task-${task_id}"
case "$gate" in
  narrow) (cd "$task_worktree" && cargo test --workspace) ;;
  checkpoint|full) (cd "$task_worktree" && bash scripts/check.sh) ;;
  *) echo "unknown gate: $gate" >&2; exit 1 ;;
esac
git merge --ff-only "$branch"
case "$gate" in
  narrow) cargo test --workspace ;;
  checkpoint|full) bash scripts/check.sh ;;
esac
git worktree remove "../agent-memory-wt/task-${task_id}"
git branch -d "$branch"
