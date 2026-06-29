#!/usr/bin/env bash
set -euo pipefail

# Show the current Vegvisir workspace snapshot and run artifacts.
# Usage: ./vworkspace-map.sh

printf 'Workspace: %s\n' "$(pwd)"
echo

echo '== scripts =='
find scripts -maxdepth 1 -type f | sed 's#^./##' | sort

echo
echo '== vegvisir runs =='
find .vegvisir/runs -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort || true
