#!/usr/bin/env bash
set -euo pipefail

# Show untracked files only.
# Usage: ./vuntracked-files.sh

git ls-files --others --exclude-standard
