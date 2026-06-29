#!/usr/bin/env bash
set -euo pipefail

# Show workspace files that mention run, memory, or approval metadata.
# Usage: ./vworkspace-hotspots.sh [path]

root="${1:-.}"
if command -v rg >/dev/null 2>&1; then
  rg --hidden --glob '!.git' --glob '!.vegvisir' -n -i 'run_id|subagent|memory_written|memory_used|approval|verification|context' "$root"
else
  grep -RIn -i --exclude-dir=.git --exclude-dir=.vegvisir -E 'run_id|subagent|memory_written|memory_used|approval|verification|context' "$root"
fi
