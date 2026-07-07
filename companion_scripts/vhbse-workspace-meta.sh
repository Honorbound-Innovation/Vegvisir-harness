#!/usr/bin/env bash
set -euo pipefail

# Show safe workspace metadata plus HBSE-related markers.
# Usage: ./vhbse-workspace-meta.sh

printf 'PWD=%s\n' "$PWD"
printf 'git_root=%s\n' "$(git rev-parse --show-toplevel 2>/dev/null || echo '(none)')"
printf '.vegvisir_exists=%s\n' "$( [[ -d .vegvisir ]] && echo yes || echo no )"
printf 'hbse_marker_count=%s\n' "$(grep -RIn -i -E 'hbse|secret[_-]?ref' . 2>/dev/null | wc -l || true)"
