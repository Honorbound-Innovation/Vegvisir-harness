#!/usr/bin/env bash
set -euo pipefail

# Show a focused summary of repository statistics.
# Usage: ./vrepo-stats.sh

printf 'Tracked files: '
git ls-files | wc -l
printf 'Branches: '
git for-each-ref --format='%(refname:short)' refs/heads | wc -l
printf 'Remotes: '
git remote | wc -l
