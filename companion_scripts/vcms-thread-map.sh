#!/usr/bin/env bash
set -euo pipefail

# Summarize CMS memory threads by filename and timestamp without reading payloads.
# Usage: ./vcms-thread-map.sh [path]

root="${1:-.}"
find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( -name 'memory-used.json' -o -name 'memory-written.json' -o -name 'context-sources.json' \) \
  -printf '%TY-%Tm-%Td %TH:%TM %p\n' 2>/dev/null | sort
