#!/usr/bin/env bash
set -euo pipefail

# Count HBSE and secret-reference mentions without expanding any secret values.
# Usage: ./vhbse-ref-count.sh [path]

root="${1:-.}"

if command -v rg >/dev/null 2>&1; then
  total=$(rg --hidden --glob '!.git' --glob '!.vegvisir' -n -i -o '\b(secret[_-]?ref|hbse|hbse://)\b' "$root" | wc -l | awk '{print $1}')
  printf 'HBSE/secret-ref matches: %s\n' "${total:-0}"
else
  total=$(grep -RIn -i -E '\b(secret[_-]?ref|hbse|hbse://)\b' "$root" 2>/dev/null | wc -l | awk '{print $1}')
  printf 'HBSE/secret-ref matches: %s\n' "${total:-0}"
fi
