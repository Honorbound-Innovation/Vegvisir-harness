#!/usr/bin/env bash
set -euo pipefail

# List pending approval records when approval-like artifacts exist.
# Usage: ./vapprovals-pending.sh [path]

root="${1:-.}"
[[ -d "$root" || -f "$root" ]] || { echo "Path not found: $root" >&2; exit 1; }

pattern='pending|requested|awaiting|open'

if command -v rg >/dev/null 2>&1; then
  rg --hidden --glob '!.git' --glob '!.vegvisir' -n -i \
    --glob '*approval*' --glob '*approv*' --glob '*policy*' --glob '*permission*' \
    -- "$pattern" "$root" 2>/dev/null || true
else
  find "$root" \
    -path '*/.git' -prune -o \
    -path '*/.vegvisir' -prune -o \
    -type f \( -name '*approval*' -o -name '*approv*' -o -name '*policy*' -o -name '*permission*' \) -print0 \
    | xargs -0 --no-run-if-empty grep -In -i -E -- "$pattern" 2>/dev/null || true
fi
