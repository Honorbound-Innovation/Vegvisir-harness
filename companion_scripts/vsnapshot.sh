#!/usr/bin/env bash
set -euo pipefail

# Print a quick Vegvisir workspace snapshot.
# Usage: ./vsnapshot.sh

printf 'pwd=%s\n' "$PWD"
printf 'git_root=%s\n' "$(git rev-parse --show-toplevel 2>/dev/null || echo '(none)')"
printf 'branch=%s\n' "$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo '(none)')"
printf 'status_entries=%s\n' "$(git status --short 2>/dev/null | wc -l | awk '{print $1}')"
printf 'companion_scripts=%s\n' "$(find companion_scripts -maxdepth 1 -type f -name 'v*.sh' 2>/dev/null | wc -l | awk '{print $1}')"
printf 'vegvisir_runs=%s\n' "$(find .vegvisir/runs -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | awk '{print $1}')"
