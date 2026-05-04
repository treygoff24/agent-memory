#!/usr/bin/env bash
set -euo pipefail

commit_msg="${COMMIT_MSG:-}"
changed_files="${CHANGED_FILES:-}"

if [ -z "$commit_msg" ]; then
  commit_msg=$(git log -1 --pretty=%B 2>/dev/null || true)
fi
if [ -z "$changed_files" ]; then
  changed_files=$(git diff-tree --no-commit-id --name-only -r HEAD 2>/dev/null || true)
fi

canonical=$(printf '%s\n' "$changed_files" | grep -E '^bench/(baseline\..*\.json|.*-results\..*\.json)$' || true)
proposed=$(printf '%s\n' "$changed_files" | grep -E '^bench/.*\.proposed$' || true)

if [ -n "$canonical" ] && ! printf '%s' "$commit_msg" | grep -q '\[bench-update\]'; then
  echo "canonical bench files require [bench-update] in the commit message:" >&2
  printf '%s\n' "$canonical" >&2
  exit 1
fi

if [ -n "$canonical" ] && [ -n "$proposed" ]; then
  echo "do not commit canonical bench files and .proposed bench files together" >&2
  printf 'canonical:\n%s\nproposed:\n%s\n' "$canonical" "$proposed" >&2
  exit 1
fi
