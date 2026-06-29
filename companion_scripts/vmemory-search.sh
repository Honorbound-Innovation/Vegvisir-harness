#!/usr/bin/env bash
set -euo pipefail

# Search CMS memories for a query.
# Usage: ./vmemory-search.sh <query> [limit]

query="${1:-}"
limit="${2:-10}"
[[ -n "$query" ]] || { echo "Usage: $0 <query> [limit]" >&2; exit 1; }

cms_recall --limit "$limit" --query "$query"
