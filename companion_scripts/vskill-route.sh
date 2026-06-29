#!/usr/bin/env bash
set -euo pipefail

# Show skill routing matches for a query against a bundle.
# Usage: ./vskill-route.sh <bundle> <query>

bundle="${1:-}"
query="${2:-}"
[[ -n "$bundle" && -n "$query" ]] || { echo "Usage: $0 <bundle> <query>" >&2; exit 1; }

skiller_route --bundle "$bundle" --query "$query" --limit 10
