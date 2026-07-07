#!/usr/bin/env bash
set -euo pipefail

# Show CMS source artifacts and their ages to help judge staleness.
# Usage: ./vcms-source-audit.sh [path]

root="${1:-.}"
find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( -name 'context-sources.json' -o -name 'context.md' -o -name 'memory-used.json' -o -name 'memory-written.json' \) \
  -printf '%T@ %TY-%Tm-%Td %TH:%TM %p\n' 2>/dev/null | sort -n | awk '{print $2, $3, $4, $5, $6}'
