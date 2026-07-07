#!/usr/bin/env bash
set -euo pipefail

# Summarize a Vegvisir run/session trace without dumping large or secret values.
# Usage: ./vtrace.sh [latest|<run-dir>] [--lines N]

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

usage() {
  cat <<'USAGE'
Usage: vtrace.sh [latest|<run-dir>] [--lines N]

Shows run metadata, artifact inventory, key excerpts, and failure/tool/subagent
signals. Output is bounded and passed through conservative redaction.
USAGE
}

selector="latest"
lines=80
while [[ $# -gt 0 ]]; do
  case "$1" in
    --lines)
      shift || { echo "--lines requires a number" >&2; exit 1; }
      vs_is_uint "${1:-}" || { echo "--lines requires an unsigned integer" >&2; exit 1; }
      lines="$1"
      ;;
    -h|--help) usage; exit 0 ;;
    *) selector="$1" ;;
  esac
  shift || true
done

resolve_run() {
  local s="$1"
  if [[ "$s" == "latest" ]]; then
    local root=".vegvisir/runs"
    [[ -d "$root" ]] || { echo "No .vegvisir/runs directory found." >&2; return 1; }
    find "$root" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' 2>/dev/null | sort -nr | awk 'NR==1 {print $2}'
  else
    printf '%s\n' "$s"
  fi
}

run_dir="$(resolve_run "$selector")"
[[ -n "$run_dir" && -d "$run_dir" ]] || { echo "Run directory not found: ${run_dir:-$selector}" >&2; exit 1; }

printf 'run_dir=%s\n' "$run_dir"
printf 'mtime=%s\n' "$(stat -c '%y' "$run_dir" 2>/dev/null || true)"
printf '\n== artifact inventory ==\n'
find "$run_dir" -maxdepth 2 -type f -printf '%TY-%Tm-%Td %TH:%TM  %10s  %p\n' 2>/dev/null | sort

print_excerpt() {
  local file="$1"
  local label="$2"
  [[ -f "$file" ]] || return 0
  printf '\n== %s ==\n' "$label"
  sed -n "1,${lines}p" "$file" | vs_redact_memory_stream | vs_redact_secret_stream
}

for name in manifest.json request.json result.md verification.json approvals.json diff.patch; do
  print_excerpt "$run_dir/$name" "$name"
done

printf '\n== failure/tool/subagent signals ==\n'
if command -v rg >/dev/null 2>&1; then
  rg -n -i --hidden --glob '!*.bin' -- 'error|failed|failure|panic|timeout|tool|subagent|approval' "$run_dir" 2>/dev/null \
    | head -n 80 \
    | vs_redact_memory_stream | vs_redact_secret_stream || true
else
  grep -RIn -i -E 'error|failed|failure|panic|timeout|tool|subagent|approval' "$run_dir" 2>/dev/null \
    | head -n 80 \
    | vs_redact_memory_stream | vs_redact_secret_stream || true
fi
