#!/usr/bin/env bash
set -euo pipefail

# Search for a pattern in the workspace with useful surrounding context.
# Usage: ./vgrep.sh <pattern> [path] [context]

pattern="${1:-}"
path="${2:-.}"
context="${3:-2}"

if [[ -z "$pattern" ]]; then
  echo "Usage: $0 <pattern> [path] [context]" >&2
  exit 1
fi

if command -v rg >/dev/null 2>&1; then
  rg --hidden --glob '!.git' --glob '!.vegvisir' -n -C "$context" "$pattern" "$path"
else
  grep -RIn -C "$context" --exclude-dir=.git --exclude-dir=.vegvisir "$pattern" "$path"
fi
