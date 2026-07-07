#!/usr/bin/env bash
set -euo pipefail

# Print git status and a concise diff summary.
# Usage: ./vgit-status.sh

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Not inside a git repository." >&2
  exit 1
fi

echo '== git status =='
git status --short --branch

echo
if git diff --quiet --ignore-submodules --cached && git diff --quiet --ignore-submodules; then
  echo 'No local changes.'
  exit 0
fi

echo '== diff summary =='
git diff --stat --summary --ignore-submodules
