#!/usr/bin/env bash
set -euo pipefail

# Show the latest changed files that mention HBSE or approval keywords.
# Usage: ./vhbse-changed.sh

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Not inside a git repository." >&2
  exit 1
fi

git diff --name-only --cached --diff-filter=ACMRTUXB | awk '
  BEGIN { IGNORECASE=1 }
  /hbse|secret|approv|policy|permission/ { print }
'
