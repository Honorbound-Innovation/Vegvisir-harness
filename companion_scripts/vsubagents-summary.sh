#!/usr/bin/env bash
set -euo pipefail

# Summarize current subagents with a focus on task state and names only.
# Usage: ./vsubagents-summary.sh [status]

status="${1:-all}"
subagents_list --status "$status" --limit 50
