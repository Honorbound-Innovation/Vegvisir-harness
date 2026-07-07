#!/usr/bin/env bash
set -euo pipefail

# Show skill-related local artifacts and manifests.
# Usage: ./vskill-artifacts.sh [path]

path="${1:-.vegvisir}"
[[ -d "$path" ]] || { echo "Path not found: $path" >&2; exit 1; }

find "$path" -type f \( -name '*skill*' -o -name '*manifest*' -o -name '*forge*' -o -name '*bundle*' \) | sort
