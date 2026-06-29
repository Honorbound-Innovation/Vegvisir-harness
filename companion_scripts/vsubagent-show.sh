#!/usr/bin/env bash
set -euo pipefail

# Show details for one subagent task.
# Usage: ./vsubagent-show.sh <id-or-name>

id_or_name="${1:-}"
[[ -n "$id_or_name" ]] || { echo "Usage: $0 <id-or-name>" >&2; exit 1; }

subagents_show "$id_or_name"
