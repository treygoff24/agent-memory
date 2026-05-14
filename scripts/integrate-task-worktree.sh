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
  narrow|fast) (cd "$task_worktree" && pnpm run check:fast) ;;
  checkpoint|local) (cd "$task_worktree" && pnpm run check:local) ;;
  full) (cd "$task_worktree" && pnpm run check:full) ;;
  *) echo "unknown gate: $gate" >&2; exit 1 ;;
esac
git merge --ff-only "$branch"
case "$gate" in
  narrow|fast) pnpm run check:fast ;;
  checkpoint|local) pnpm run check:local ;;
  full) pnpm run check:full ;;
esac
git worktree remove "../agent-memory-wt/task-${task_id}"
git branch -d "$branch"
