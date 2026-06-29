#!/usr/bin/env bash
set -euo pipefail

# Show a concise workspace metadata block for Vegvisir.
# Usage: ./vworkspace-meta.sh

printf 'PWD=%s\n' "$PWD"
printf 'git_root=%s\n' "$(git rev-parse --show-toplevel 2>/dev/null || echo '(none)')"
printf '.vegvisir_exists=%s\n' "$( [[ -d .vegvisir ]] && echo yes || echo no )"
printf 'scripts_count=%s\n' "$(find scripts -maxdepth 1 -type f 2>/dev/null | wc -l)"
