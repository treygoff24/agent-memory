#!/usr/bin/env bash
set -euo pipefail
task_id="${1:?task id required}"
slug="${2:?branch slug required}"
git diff --quiet && git diff --cached --quiet || { echo "main has uncommitted changes" >&2; exit 1; }
git worktree add -b "stream-a/task-${task_id}-${slug}" "../agent-memory-wt/task-${task_id}" main
