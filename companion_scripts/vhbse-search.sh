#!/usr/bin/env bash
set -euo pipefail

# Search for HBSE references in the workspace while avoiding secret content dumps.
# Usage: ./vhbse-search.sh <query> [path]

query="${1:-}"
root="${2:-.}"
[[ -n "$query" ]] || { echo "Usage: $0 <query> [path]" >&2; exit 1; }

if command -v rg >/dev/null 2>&1; then
  rg --hidden --glob '!.git' --glob '!.vegvisir' -n -i -- "$query" "$root"
else
  grep -RIn -i --exclude-dir=.git --exclude-dir=.vegvisir -- "$query" "$root"
fi
