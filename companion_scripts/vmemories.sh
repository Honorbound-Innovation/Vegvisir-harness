#!/usr/bin/env bash
set -euo pipefail

# Summarize CMS recent memories.
# Usage: ./vmemories.sh [limit]

limit="${1:-10}"
cms_recent --limit "$limit"
