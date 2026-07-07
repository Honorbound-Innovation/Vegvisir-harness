#!/usr/bin/env bash
set -euo pipefail

# Show recent commits with date, author, decoration, and subject.
# Usage: ./vrecent-commits.sh [limit]

limit="${1:-15}"
git log --date=short --pretty=format:'%C(auto)%h%Creset %ad %C(cyan)%an%Creset %C(auto)%d%Creset %s' -n "$limit"
echo
