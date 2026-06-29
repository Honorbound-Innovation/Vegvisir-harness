#!/usr/bin/env bash
set -euo pipefail

# Show file sizes for Vegvisir artifacts.
# Usage: ./vartifact-sizes.sh [path]

path="${1:-.vegvisir}"
[[ -d "$path" ]] || { echo "Path not found: $path" >&2; exit 1; }

find "$path" -type f -print0 | xargs -0 stat -c '%s %n' | sort -nr
