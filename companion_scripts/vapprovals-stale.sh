#!/usr/bin/env bash
set -euo pipefail

# Highlight stale approval artifacts by file age, not content.
# Usage: ./vapprovals-stale.sh [path]

root="${1:-.}"
find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( -name '*approval*' -o -name '*approv*' \) \
  -printf '%TY-%Tm-%Td %TH:%TM %p\n' 2>/dev/null | sort
