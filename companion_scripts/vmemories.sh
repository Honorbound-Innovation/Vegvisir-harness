#!/usr/bin/env bash
set -euo pipefail

# Summarize CMS recent memories with content/body fields redacted.
# Usage: ./vmemories.sh [limit]

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

limit="${1:-10}"
vs_is_uint "$limit" || { echo "Invalid limit: $limit" >&2; exit 1; }
vs_require_command cms_recent "cms_recent is available inside Vegvisir harness environments."
cms_recent --limit "$limit" | vs_redact_memory_stream
