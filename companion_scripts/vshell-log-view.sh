#!/usr/bin/env bash
set -euo pipefail

# Inspect a Vegvisir shell task log.
# Usage: ./vshell-log-view.sh <path>

path="${1:-}"
[[ -n "$path" ]] || { echo "Usage: $0 <path>" >&2; exit 1; }
[[ -f "$path" ]] || { echo "Log not found: $path" >&2; exit 1; }

sed -n '1,200p' "$path"
