#!/usr/bin/env bash
set -euo pipefail

# Show a minimal, safe summary of approval-related files in the workspace.
# Usage: ./vapprovals-files.sh [path]

root="${1:-.}"
find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( -name '*approval*' -o -name '*approv*' -o -name '*policy*' -o -name '*permission*' \) -print | sort
