#!/usr/bin/env bash
set -euo pipefail

# Show all HBSE-related secret reference mentions without expanding values.
# Usage: ./vhbse-secret-refs.sh [path]

root="${1:-.}"

if command -v rg >/dev/null 2>&1; then
  rg --hidden --glob '!.git' --glob '!.vegvisir' -n -i '\b(secret[_-]?ref|hbse://|hbse[_-]?ref)\b' "$root"
else
  grep -RIn -i --exclude-dir=.git --exclude-dir=.vegvisir -E '\b(secret[_-]?ref|hbse://|hbse[_-]?ref)\b' "$root"
fi
