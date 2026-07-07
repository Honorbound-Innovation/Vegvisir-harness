#!/usr/bin/env bash
set -euo pipefail

# Summarize current subagents with a focus on task state and names only.
# Usage: ./vsubagents-summary.sh [status]

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

status="${1:-all}"
vs_require_command subagents_list "subagents_list is available inside Vegvisir harness environments."
subagents_list --status "$status" --limit 50
