#!/usr/bin/env bash
set -euo pipefail

# Show subagent tasks for the current session.
# Usage: ./vsubagents.sh [status]

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

status="${1:-}"
vs_require_command subagents_list "subagents_list is available inside Vegvisir harness environments."
if [[ -n "$status" ]]; then
  subagents_list --status "$status" --limit 50 2>/dev/null || subagents_list --limit 50 --status "$status"
else
  subagents_list --limit 50 --status running 2>/dev/null || subagents_list --limit 50 --status all
fi
