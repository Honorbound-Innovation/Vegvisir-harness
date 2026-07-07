#!/usr/bin/env bash
set -euo pipefail

# Show concise local status plus ahead/behind counts.
# Usage: ./vbranch-drift.sh

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Not inside a git repository." >&2
  exit 1
fi

branch="$(git branch --show-current)"
upstream="$(git rev-parse --abbrev-ref --symbolic-full-name @{u} 2>/dev/null || true)"

echo "Branch: ${branch:-DETACHED}"
echo "Upstream: ${upstream:-(none)}"

if [[ -n "$upstream" ]]; then
  git rev-list --left-right --count "${branch}...${upstream}"
else
  echo 'No upstream configured.'
fi
