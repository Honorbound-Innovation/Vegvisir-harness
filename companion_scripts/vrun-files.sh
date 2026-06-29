#!/usr/bin/env bash
set -euo pipefail

# List the newest Vegvisir run artifacts by path.
# Usage: ./vrun-files.sh [limit]

limit="${1:-40}"
find .vegvisir/runs -type f 2>/dev/null | sort -r | head -n "$limit"
