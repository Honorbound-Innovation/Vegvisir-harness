#!/usr/bin/env bash
set -euo pipefail

# Tail the most recent shell task log.
# Usage: ./vshell-tail.sh [limit]

limit="${1:-100}"
latest="$(find .vegvisir/tasks -type f -name 'shell-*.log' 2>/dev/null | sort -r | head -n 1 || true)"
[[ -n "$latest" ]] || { echo "No shell task logs found." >&2; exit 1; }

tail -n "$limit" "$latest"
