#!/usr/bin/env bash
set -euo pipefail

# Scan files for likely plaintext secrets without printing secret values.
# Usage: ./vsecret-scan.sh [--staged|--changed|--tracked|path...]

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

usage() {
  cat <<'USAGE'
Usage: vsecret-scan.sh [--staged|--changed|--tracked|path...]

Modes:
  --staged    scan staged files only
  --changed   scan modified/untracked workspace files (default in git repos)
  --tracked   scan all tracked files
  path...     scan explicit files/directories

Output is intentionally redacted: file, line number, and rule name only.
USAGE
}

mode="changed"
paths=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --staged|--cached) mode="staged" ;;
    --changed) mode="changed" ;;
    --tracked) mode="tracked" ;;
    -h|--help) usage; exit 0 ;;
    --) shift; paths+=("$@"); break ;;
    *) mode="paths"; paths+=("$1") ;;
  esac
  shift || true
done

is_probably_text() {
  [[ -f "$1" ]] || return 1
  [[ "$1" != .git/* && "$1" != .vegvisir/* && "$1" != */.git/* && "$1" != */.vegvisir/* ]] || return 1
  [[ "$1" != target/* && "$1" != */target/* && "$1" != node_modules/* && "$1" != */node_modules/* ]] || return 1
  grep -Iq . "$1" 2>/dev/null
}

collect_files() {
  case "$mode" in
    staged)
      git diff --cached --name-only --diff-filter=ACMR 2>/dev/null || true
      ;;
    changed)
      if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        { git diff --name-only --diff-filter=ACMR; git diff --cached --name-only --diff-filter=ACMR; git ls-files --others --exclude-standard; } 2>/dev/null | sort -u
      else
        find . -type f \
          -not -path './.git/*' -not -path './.vegvisir/*' \
          -not -path './target/*' -not -path './node_modules/*'
      fi
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

findings=0
while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  is_probably_text "$file" || continue
  line_no=0
  while IFS= read -r line || [[ -n "$line" ]]; do
    line_no=$((line_no + 1))
    rule=""
    lower="${line,,}"
    if [[ "$line" =~ -----BEGIN[[:space:]]+[A-Z0-9[:space:]]*PRIVATE[[:space:]]+KEY----- ]]; then
      rule="private-key-block"
    elif [[ "$line" =~ AKIA[0-9A-Z]{16} ]]; then
      rule="aws-access-key-id"
    elif [[ "$line" =~ (xox[baprs]-[A-Za-z0-9-]{10,}) ]]; then
      rule="slack-token"
    elif [[ "$line" =~ https?://[^/:[:space:]]+:[^@/[:space:]]+@ ]]; then
      rule="credential-url"
    elif [[ "$lower" =~ (api[_-]?key|token|password|passwd|secret|credential)[[:space:]]*[:=][[:space:]]*[\"\']?[^\"\'[:space:]#]{12,} ]]; then
      if [[ ! "$lower" =~ (<redacted>|redacted|example|placeholder|dummy|changeme|your_|\$\{|secret-ref|hbse://|names[[:space:]]+only) ]]; then
        rule="secret-like-assignment"
      fi
    fi
    if [[ -n "$rule" ]]; then
      printf '%s:%s: %s\n' "$file" "$line_no" "$rule"
      findings=$((findings + 1))
    fi
  done < "$file"
done < <(collect_files | sort -u)

if (( findings > 0 )); then
  printf 'vsecret-scan: FAILED with %d possible secret finding(s). Values were not printed.\n' "$findings" >&2
  exit 1
fi
printf 'vsecret-scan: OK, no obvious plaintext secrets found.\n'
