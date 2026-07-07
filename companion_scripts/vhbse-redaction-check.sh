#!/usr/bin/env bash
set -euo pipefail

# Report only redaction-check candidates from files likely to be shown in logs or docs.
# Usage: ./vhbse-redaction-check.sh [path]

root="${1:-.}"
if command -v rg >/dev/null 2>&1; then
  rg --hidden --glob '!.git' --glob '!.vegvisir' -n -i '\b(secret|token|credential|password|api[_-]?key|hbse|authorization|bearer)\b' "$root"
else
  grep -RIn --exclude-dir=.git --exclude-dir=.vegvisir -i -E '\b(secret|token|credential|password|api[_-]?key|hbse|authorization|bearer)\b' "$root"
fi
