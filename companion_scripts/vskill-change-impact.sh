#!/usr/bin/env bash
set -euo pipefail

# Estimate which skills or bundles may be affected by a change by listing touched names.
# Usage: ./vskill-change-impact.sh [path]

root="${1:-.vegvisir}"
find "$root" -type f \( -name '*skill*' -o -name '*manifest*' -o -name '*bundle*' -o -name '*route*' \) -printf '%p\n' 2>/dev/null | sort
