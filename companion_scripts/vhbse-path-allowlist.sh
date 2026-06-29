#!/usr/bin/env bash
set -euo pipefail

# Show HBSE path allowlist candidates by directory name only.
# Usage: ./vhbse-path-allowlist.sh [path]

root="${1:-.}"
find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type d \( -name 'hbse' -o -name 'secrets' -o -name 'credentials' -o -name 'auth' -o -name 'approval' \) -print | sort
