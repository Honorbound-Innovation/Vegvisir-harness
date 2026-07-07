#!/usr/bin/env bash
set -euo pipefail

# Show available local skill IDs from a bundle or directory.
# Usage: ./vskill-list.sh <bundle>

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

bundle="${1:-}"
[[ -n "$bundle" ]] || { echo "Usage: $0 <bundle>" >&2; exit 1; }
vs_require_command skiller_load "skiller_load is available inside Vegvisir harness environments."
skiller_load --bundle "$bundle" --mode card
