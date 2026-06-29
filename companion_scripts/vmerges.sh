#!/usr/bin/env bash
set -euo pipefail

# Show recent merge commits.
# Usage: ./vmerges.sh [limit]

limit="${1:-20}"

git log --oneline --decorate --merges -n "$limit"
