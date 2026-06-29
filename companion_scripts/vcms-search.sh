#!/usr/bin/env bash
set -euo pipefail

# Search CMS memories with content/body fields redacted.
# Usage: ./vcms-search.sh <query> [limit]

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

query="${1:-}"
limit="${2:-10}"
[[ -n "$query" ]] || { echo "Usage: $0 <query> [limit]" >&2; exit 1; }
vs_is_uint "$limit" || { echo "Invalid limit: $limit" >&2; exit 1; }
vs_require_command cms_recall "cms_recall is available inside Vegvisir harness environments."
cms_recall --limit "$limit" --query "$query" | vs_redact_memory_stream
