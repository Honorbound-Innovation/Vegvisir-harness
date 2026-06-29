#!/usr/bin/env bash
set -euo pipefail

# Group recent run failures by normalized error signature using filenames only.
# Usage: ./vrun-failure-cluster.sh [path]

root="${1:-.vegvisir/runs}"
if command -v rg >/dev/null 2>&1; then
  rg -n -i --hidden --glob '!.git' 'error|exception|traceback|failed|fatal' "$root" \
    | sed -E 's/[0-9]{4}-[0-9]{2}-[0-9]{2}T[^ ]+/TIMESTAMP/g'
else
  grep -RIn -i --exclude-dir=.git -E 'error|exception|traceback|failed|fatal' "$root" \
    | sed -E 's/[0-9]{4}-[0-9]{2}-[0-9]{2}T[^ ]+/TIMESTAMP/g'
fi
