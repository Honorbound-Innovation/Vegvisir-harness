#!/usr/bin/env bash
set -euo pipefail

# Run a fast, sensible test command based on the repository contents.
# Usage: ./vtest.sh

if [[ -f package.json ]]; then
  if command -v npm >/dev/null 2>&1; then
    npm test -- --runInBand
  else
    echo "npm not available" >&2
    exit 1
  fi
elif [[ -f Cargo.toml ]]; then
  cargo test
elif [[ -f pyproject.toml || -f requirements.txt || -d tests ]]; then
  if command -v pytest >/dev/null 2>&1; then
    pytest -q
  else
    python -m pytest -q
  fi
else
  echo "No recognized test entrypoint found." >&2
  exit 1
fi
