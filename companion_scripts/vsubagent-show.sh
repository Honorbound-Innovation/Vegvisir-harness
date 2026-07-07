#!/usr/bin/env bash
set -euo pipefail

# Show details for one subagent task.
# Usage: ./vsubagent-show.sh <id-or-name>

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

id_or_name="${1:-}"
[[ -n "$id_or_name" ]] || { echo "Usage: $0 <id-or-name>" >&2; exit 1; }
vs_require_command subagents_show "subagents_show is available inside Vegvisir harness environments."
subagents_show "$id_or_name"
