#!/usr/bin/env bash
set -euo pipefail

# List .vegvisir runs newest first.
# Usage: ./vruns.sh

root=".vegvisir/runs"
[[ -d "$root" ]] || { echo "No .vegvisir/runs directory found." >&2; exit 1; }

find "$root" -mindepth 1 -maxdepth 1 -type d -print0 \
  | xargs -0 stat -c '%Y %n' \
  | sort -nr \
  | awk '{print $2}'
