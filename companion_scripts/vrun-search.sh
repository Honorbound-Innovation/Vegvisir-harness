#!/usr/bin/env bash
set -euo pipefail

# Search run artifacts for a string.
# Usage: ./vrun-search.sh <query>

query="${1:-}"
[[ -n "$query" ]] || { echo "Usage: $0 <query>" >&2; exit 1; }

if command -v rg >/dev/null 2>&1; then
  rg -n --hidden --glob '!.git' -- "$query" .vegvisir/runs
else
  grep -RIn -- "$query" .vegvisir/runs
fi
