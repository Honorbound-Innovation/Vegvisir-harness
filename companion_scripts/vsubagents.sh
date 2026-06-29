#!/usr/bin/env bash
set -euo pipefail

# Show subagent tasks for the current session.
# Usage: ./vsubagents.sh [status]

status="${1:-}"
if [[ -n "$status" ]]; then
  subagents_list --status "$status" --limit 50 2>/dev/null || subagents_list --limit 50 --status "$status"
else
  subagents_list --limit 50 --status running 2>/dev/null || subagents_list --limit 50 --status all
fi
