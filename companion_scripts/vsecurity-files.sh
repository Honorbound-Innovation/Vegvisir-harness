#!/usr/bin/env bash
set -euo pipefail

# Find likely approval, auth, or credential workflow files without reading secrets.
# Usage: ./vsecurity-files.sh [path]

root="${1:-.}"

find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( \
    -name '*approval*' -o -name '*auth*' -o -name '*credential*' -o -name '*policy*' -o -name '*permission*' \
  \) -print | sort
