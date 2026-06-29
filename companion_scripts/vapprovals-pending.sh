#!/usr/bin/env bash
set -euo pipefail

# List pending approval records when an approvals artifact exists.
# Usage: ./vapprovals-pending.sh [path]

root="${1:-.}"
if command -v rg >/dev/null 2>&1; then
  rg --hidden --glob '!.git' --glob '!.vegvisir' -n -i 'pending|requested|awaiting|open' "$root"/*approval* "$root"/*approv* 2>/dev/null || true
else
  grep -RIn -i --exclude-dir=.git --exclude-dir=.vegvisir -E 'pending|requested|awaiting|open' "$root"/*approval* "$root"/*approv* 2>/dev/null || true
fi
