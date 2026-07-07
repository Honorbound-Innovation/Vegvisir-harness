#!/usr/bin/env bash
set -euo pipefail

# Show skill or bundle metadata without executing it.
# Usage: ./vskill-meta.sh <bundle>

bundle="${1:-}"
[[ -n "$bundle" ]] || { echo "Usage: $0 <bundle>" >&2; exit 1; }

if [[ -f "$bundle" ]]; then
  sed -n '1,200p' "$bundle"
elif [[ -d "$bundle" ]]; then
  find "$bundle" -maxdepth 2 -type f | sort
else
  echo "Bundle not found: $bundle" >&2
  exit 1
fi
