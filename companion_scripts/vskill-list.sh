#!/usr/bin/env bash
set -euo pipefail

# Show available local skill IDs from a bundle or directory.
# Usage: ./vskill-list.sh <bundle>

bundle="${1:-}"
[[ -n "$bundle" ]] || { echo "Usage: $0 <bundle>" >&2; exit 1; }

skiller_load --bundle "$bundle" --mode card 2>/dev/null || true
