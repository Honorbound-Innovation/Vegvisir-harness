#!/usr/bin/env bash
set -euo pipefail

# Display skill dependency hints from manifest files without executing anything.
# Usage: ./vskill-deps.sh [path]

root="${1:-.vegvisir}"
if command -v rg >/dev/null 2>&1; then
  rg --hidden --glob '!.git' --glob '!.vegvisir' -n -i 'depends|dependency|requires|imports|uses|extends' "$root"
else
  grep -RIn -i --exclude-dir=.git --exclude-dir=.vegvisir -E 'depends|dependency|requires|imports|uses|extends' "$root"
fi
