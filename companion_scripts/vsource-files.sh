#!/usr/bin/env bash
set -euo pipefail

# Show all source files by common extensions.
# Usage: ./vsource-files.sh [path]

root="${1:-.}"

find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -type f \( \
    -name '*.rs' -o -name '*.py' -o -name '*.c' -o -name '*.cc' -o -name '*.cpp' -o \
    -name '*.cs' -o -name '*.java' -o -name '*.js' -o -name '*.ts' -o -name '*.tsx' -o -name '*.jsx' \
  \) -print | sort
