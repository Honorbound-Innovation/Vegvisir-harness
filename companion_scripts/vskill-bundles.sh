#!/usr/bin/env bash
set -euo pipefail

# Show the names of local skill bundles with safe filtering.
# Usage: ./vskill-bundles.sh [path]

root="${1:-.vegvisir}"
find "$root" \
  -path '*/.git' -prune -o \
  -type f \( -name '*.bundle' -o -name '*.json' -o -name '*.yaml' -o -name '*.yml' \) \
  -print | awk 'BEGIN {IGNORECASE=1} /skill|bundle|forge|manifest|route/ { print }' | sort
