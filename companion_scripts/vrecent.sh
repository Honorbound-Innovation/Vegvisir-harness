#!/usr/bin/env bash
set -euo pipefail

# Show a compact recent commit summary.
# Usage: ./vrecent.sh [limit]

limit="${1:-10}"
git log --oneline --decorate -n "$limit"
