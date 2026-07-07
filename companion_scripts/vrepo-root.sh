#!/usr/bin/env bash
set -euo pipefail

# Print the git repository root, or fail if not inside a repo.
# Usage: ./vrepo-root.sh

git rev-parse --show-toplevel
