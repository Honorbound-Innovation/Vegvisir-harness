#!/usr/bin/env bash
set -euo pipefail

# Find TODO/FIXME/BUG/HACK notes in the workspace.
# Usage: ./vnotes.sh [path]

root="${1:-.}"

if command -v rg >/dev/null 2>&1; then
  rg --hidden --glob '!.git' --glob '!.vegvisir' -n -i '\b(todo|fixme|bug|hack)\b' "$root"
else
  grep -RIn -i --exclude-dir=.git --exclude-dir=.vegvisir -E '\b(todo|fixme|bug|hack)\b' "$root"
fi
