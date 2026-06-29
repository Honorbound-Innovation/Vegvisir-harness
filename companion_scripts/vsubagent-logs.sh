#!/usr/bin/env bash
set -euo pipefail

# Summarize subagent logs from the current session workspace.
# Usage: ./vsubagent-logs.sh [limit]

limit="${1:-20}"
find .vegvisir -type f \( -name 'subagents.json' -o -name 'tool-events.jsonl' -o -name 'runtime-events.jsonl' \) 2>/dev/null | sort | head -n "$limit"
