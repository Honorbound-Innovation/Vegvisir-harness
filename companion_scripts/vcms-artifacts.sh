#!/usr/bin/env bash
set -euo pipefail

# Show CMS-style memory artifacts if present in the current workspace.
# Usage: ./vcms-artifacts.sh [path]

root="${1:-.}"
find "$root" -type f \( -name 'memory-used.json' -o -name 'memory-written.json' -o -name 'context.md' -o -name 'context-sources.json' \) 2>/dev/null | sort
