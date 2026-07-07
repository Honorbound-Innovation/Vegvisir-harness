#!/usr/bin/env bash
set -euo pipefail

# Inspect HBSE- or secret-related files without printing secret contents.
# Usage: ./vhbse-files.sh [path]

root="${1:-.}"

find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( \
    -name '*hbse*' -o -name '*secret*' -o -name '*cred*' -o -name '*token*' -o -name '*auth*' \
  \) -print | sort
