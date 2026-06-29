#!/usr/bin/env bash
set -euo pipefail

# Estimate context size for files/paths without printing file contents.
# Usage: ./vcontext-budget.sh [--changed|--staged|--tracked|path...] [--top N]

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

usage() {
  cat <<'USAGE'
Usage: vcontext-budget.sh [--changed|--staged|--tracked|path...] [--top N]

Estimates bytes and rough tokens for candidate context files. Generated/heavy
folders such as .git, .vegvisir, target, and node_modules are excluded.
USAGE
}

mode="paths"
top_n=20
paths=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --changed) mode="changed" ;;
    --staged|--cached) mode="staged" ;;
    --tracked) mode="tracked" ;;
    --top)
      shift || { echo "--top requires a number" >&2; exit 1; }
      vs_is_uint "${1:-}" || { echo "--top requires an unsigned integer" >&2; exit 1; }
      top_n="$1"
      ;;
    -h|--help) usage; exit 0 ;;
    --) shift; paths+=("$@"); break ;;
    *) paths+=("$1") ;;
  esac
  shift || true
done

if [[ "$mode" == "paths" && ${#paths[@]} -eq 0 ]]; then
  paths=(.)
fi

collect_files() {
  case "$mode" in
    changed)
      { git diff --name-only --diff-filter=ACMR; git diff --cached --name-only --diff-filter=ACMR; git ls-files --others --exclude-standard; } 2>/dev/null | sort -u
      ;;
    staged)
      git diff --cached --name-only --diff-filter=ACMR 2>/dev/null || true
      ;;
    tracked)
      git ls-files 2>/dev/null || true
      ;;
    paths)
      for p in "${paths[@]}"; do
        if [[ -d "$p" ]]; then
          find "$p" -type f \
            -not -path '*/.git/*' -not -path '*/.vegvisir/*' \
            -not -path '*/target/*' -not -path '*/node_modules/*'
        elif [[ -f "$p" ]]; then
          printf '%s\n' "$p"
        fi
      done
      ;;
  esac
}

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

while IFS= read -r file; do
  [[ -n "$file" && -f "$file" ]] || continue
  [[ "$file" != .git/* && "$file" != .vegvisir/* && "$file" != */.git/* && "$file" != */.vegvisir/* ]] || continue
  [[ "$file" != target/* && "$file" != */target/* && "$file" != node_modules/* && "$file" != */node_modules/* ]] || continue
  size="$(wc -c < "$file" | tr -d ' ')"
  printf '%s\t%s\n' "$size" "$file" >> "$tmp"
done < <(collect_files | sort -u)

if [[ ! -s "$tmp" ]]; then
  echo "No files selected."
  exit 0
fi

total_bytes="$(awk -F '\t' '{sum += $1} END {print sum + 0}' "$tmp")"
file_count="$(wc -l < "$tmp" | tr -d ' ')"
token_est=$(( (total_bytes + 3) / 4 ))

printf 'files=%s\n' "$file_count"
printf 'bytes=%s\n' "$total_bytes"
printf 'human_bytes=%s\n' "$(vs_human_bytes "$total_bytes")"
printf 'rough_tokens=%s\n' "$token_est"
printf 'rough_tokens_note=%s\n' 'very approximate: bytes/4, not provider-tokenizer exact'
printf '\n== largest files ==\n'
sort -nr "$tmp" | head -n "$top_n" | awk -F '\t' '{ printf "%10d  %s\n", $1, $2 }'
