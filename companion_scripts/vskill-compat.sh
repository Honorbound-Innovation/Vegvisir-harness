#!/usr/bin/env bash
set -euo pipefail

# Check whether a bundle path exists and list compatible metadata files.
# Usage: ./vskill-compat.sh <bundle-or-path>

path="${1:-}"
[[ -n "$path" ]] || { echo "Usage: $0 <bundle-or-path>" >&2; exit 1; }

if [[ -d "$path" ]]; then
  find "$path" -maxdepth 2 -type f \( -name '*manifest*' -o -name '*bundle*' -o -name '*skill*' \) | sort
elif [[ -f "$path" ]]; then
  printf 'file=%s\n' "$path"
  printf 'size=%s\n' "$(stat -c '%s' "$path")"
else
  echo "Path not found: $path" >&2
  exit 1
fi
