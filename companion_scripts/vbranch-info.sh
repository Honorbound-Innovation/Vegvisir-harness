#!/usr/bin/env bash
set -euo pipefail

# Show the current branch and its upstream tracking info.
# Usage: ./vbranch-info.sh

git branch --show-current
printf 'Upstream: '
git rev-parse --abbrev-ref --symbolic-full-name @{u} 2>/dev/null || echo '(none)'
