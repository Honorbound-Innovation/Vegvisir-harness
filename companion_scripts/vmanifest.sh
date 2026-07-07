#!/usr/bin/env bash
set -euo pipefail

# Print the companion script manifest.
# Usage: ./vmanifest.sh [--raw|--category <name>|--risk <name>]

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
manifest="$DIR/manifest.tsv"
mode="table"
category=""
risk=""

while (($#)); do
  case "$1" in
    --raw)
      mode="raw"
      ;;
    --category)
      shift
      [[ $# -gt 0 ]] || { echo "Missing value for --category" >&2; exit 1; }
      category="$1"
      ;;
    --category=*)
      category="${1#*=}"
      ;;
    --risk)
      shift
      [[ $# -gt 0 ]] || { echo "Missing value for --risk" >&2; exit 1; }
      risk="$1"
      ;;
    --risk=*)
      risk="${1#*=}"
      ;;
    -h|--help)
      sed -n '2,5p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
  shift || true
done

[[ -f "$manifest" ]] || { echo "Manifest not found: $manifest" >&2; exit 1; }

filter_manifest() {
  awk -F '\t' -v category="$category" -v risk="$risk" '
    NR == 1 { print; next }
    (category == "" || $2 == category) && (risk == "" || $3 == risk) { print }
  ' "$manifest"
}

if [[ "$mode" == "raw" ]]; then
  filter_manifest
elif command -v column >/dev/null 2>&1; then
  filter_manifest | column -t -s $'\t'
else
  filter_manifest
fi
