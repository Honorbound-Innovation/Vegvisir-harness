#!/usr/bin/env bash
set -euo pipefail

# Show a compact summary of all runs with their latest files.
# Usage: ./vrun-artifacts.sh

find .vegvisir/runs -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort | while read -r run; do
  echo "== ${run##*/} =="
  find "$run" -maxdepth 1 -type f | sed 's#^./##' | sort
  echo
 done
