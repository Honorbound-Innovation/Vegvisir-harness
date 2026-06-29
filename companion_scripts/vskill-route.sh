#!/usr/bin/env bash
set -euo pipefail

# Route a query against a bundle.
# Usage: ./vskill-route.sh <bundle> <query>

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

bundle="${1:-}"
query="${2:-}"
[[ -n "$bundle" && -n "$query" ]] || { echo "Usage: $0 <bundle> <query>" >&2; exit 1; }
vs_require_command skiller_route "skiller_route is available inside Vegvisir harness environments."
skiller_route --bundle "$bundle" --query "$query" --limit 10
