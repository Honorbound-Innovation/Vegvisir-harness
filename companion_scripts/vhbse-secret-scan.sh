#!/usr/bin/env bash
set -euo pipefail

# Detect likely secret-bearing files or paths without printing secret contents.
# Usage: ./vhbse-secret-scan.sh [path]

root="${1:-.}"
if command -v rg >/dev/null 2>&1; then
  rg --hidden --glob '!.git' --glob '!.vegvisir' -l -i '\b(secret|token|credential|password|api[_-]?key|hbse)\b' "$root"
else
  grep -RIl --exclude-dir=.git --exclude-dir=.vegvisir -i -E '\b(secret|token|credential|password|api[_-]?key|hbse)\b' "$root"
fi
