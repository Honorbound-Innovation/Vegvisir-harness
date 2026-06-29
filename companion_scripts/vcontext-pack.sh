#!/usr/bin/env bash
set -euo pipefail

# Gather a lightweight context bundle for a topic.
# Usage: ./vcontext-pack.sh <pattern> [path]

pattern="${1:-}"
root="${2:-.}"

if [[ -z "$pattern" ]]; then
  echo "Usage: $0 <pattern> [path]" >&2
  exit 1
fi

if command -v rg >/dev/null 2>&1; then
  rg --hidden --glob '!.git' --glob '!.vegvisir' -l -- "$pattern" "$root"
else
  grep -RIl --exclude-dir=.git --exclude-dir=.vegvisir -- "$pattern" "$root"
fi
