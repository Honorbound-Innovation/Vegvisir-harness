#!/usr/bin/env bash
set -euo pipefail

# Show a condensed view of CMS-related queries in recent memory artifacts.
# Usage: ./vcms-query-summary.sh [path]

root="${1:-.}"
if command -v rg >/dev/null 2>&1; then
  rg -n -i --hidden --glob '!.git' --glob '!.vegvisir' 'query|memory|recall|remember|context' "$root" \
    | sed -E 's/(content:|body:).*/\1 <redacted>/g'
else
  grep -RIn -i --exclude-dir=.git --exclude-dir=.vegvisir -E 'query|memory|recall|remember|context' "$root" \
    | sed -E 's/(content:|body:).*/\1 <redacted>/g'
fi
