#!/usr/bin/env bash
set -euo pipefail

# Show the current shell environment essentials.
# Usage: ./venv-snapshot.sh

printf 'PWD=%s\n' "$PWD"
printf 'USER=%s\n' "${USER:-}"
printf 'SHELL=%s\n' "${SHELL:-}"
printf 'PATH=%s\n' "${PATH:-}"
