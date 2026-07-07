#!/usr/bin/env bash
set -euo pipefail

# Show the current git commit and short description.
# Usage: ./vcommit-info.sh

git log -1 --stat --decorate --oneline
