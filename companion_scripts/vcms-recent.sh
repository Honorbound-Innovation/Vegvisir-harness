#!/usr/bin/env bash
set -euo pipefail

# Show memory titles and types only; do not print full sensitive content.
# Usage: ./vcms-recent.sh [limit]

limit="${1:-10}"
command -v cms_recent >/dev/null 2>&1 || { echo "cms_recent command not available in this environment." >&2; exit 1; }
cms_recent --limit "$limit" | sed -E 's/(content:|body:).*/\1 <redacted>/g'
