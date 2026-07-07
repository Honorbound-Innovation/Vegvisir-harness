#!/usr/bin/env bash
set -euo pipefail

# Show all shell scripts in the workspace.
# Usage: ./vshell-files.sh [path]

root="${1:-.}"

find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( -name '*.sh' -o -name '*.bash' \) -print | sort
