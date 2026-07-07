#!/usr/bin/env bash
set -euo pipefail

# Show error lines from Vegvisir run artifacts.
# Usage: ./vrun-errors.sh [path]

path="${1:-.vegvisir/runs}"

if command -v rg >/dev/null 2>&1; then
  rg -n -i --hidden -- 'error|exception|traceback|failed|fatal' "$path"
else
  grep -RIn -i -- 'error\|exception\|traceback\|failed\|fatal' "$path"
fi
