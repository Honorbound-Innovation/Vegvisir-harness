#!/usr/bin/env bash
set -euo pipefail

# Show a compact list of likely workflow entry files relevant to Vegvisir automation.
# Usage: ./vworkflow-files.sh [path]

root="${1:-.}"
find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( -name '*.md' -o -name '*.json' -o -name '*.sh' -o -name '*.yaml' -o -name '*.yml' \) -print | awk 'BEGIN {IGNORECASE=1} /workflow|runbook|approval|skill|hbse|memory|subagent|context/ { print }' | sort
