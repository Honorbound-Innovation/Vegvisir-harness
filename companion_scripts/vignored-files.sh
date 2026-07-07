#!/usr/bin/env bash
set -euo pipefail

# Show ignored files that git is ignoring.
# Usage: ./vignored-files.sh

git ls-files --others -i --exclude-standard
