#!/usr/bin/env bash
set -euo pipefail

# Show files that likely define HBSE-safe secret references by name only.
# Usage: ./vhbse-manifest-files.sh [path]

root="${1:-.}"
find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( -name '*manifest*' -o -name '*ref*' -o -name '*secret*' -o -name '*hbse*' \) -print | sort
