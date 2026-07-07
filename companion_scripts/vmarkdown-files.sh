#!/usr/bin/env bash
set -euo pipefail

# Show all markdown files in the workspace.
# Usage: ./vmarkdown-files.sh [path]

root="${1:-.}"

find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( -name '*.md' -o -name '*.markdown' \) -print | sort
