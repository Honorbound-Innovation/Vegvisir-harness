#!/usr/bin/env bash
set -euo pipefail

# Summarize local skill manifests and indexes in the workspace.
# Usage: ./vskill-index.sh [path]

path="${1:-.vegvisir/compiled/index}"
[[ -d "$path" ]] || { echo "Path not found: $path" >&2; exit 1; }

find "$path" -type f | sort
