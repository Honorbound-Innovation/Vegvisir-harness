#!/usr/bin/env bash
set -euo pipefail

# Show branches sorted by most recent commit.
# Usage: ./vbranches-recent.sh [limit]

limit="${1:-20}"

git for-each-ref --sort=-committerdate --format='%(committerdate:short) %(refname:short)' refs/heads | head -n "$limit"
