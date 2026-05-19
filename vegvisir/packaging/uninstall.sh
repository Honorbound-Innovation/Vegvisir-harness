#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Remove files installed by the Vegvisir system installer.

Usage:
  ./uninstall.sh [options]

Options:
  --prefix <path>       Install prefix. Default: $HOME/.local
  --keep-data           Keep $prefix/share/vegvisir and $prefix/etc/vegvisir.
  -h, --help            Show this help.
USAGE
}

prefix="${VEGVISIR_INSTALL_PREFIX:-$HOME/.local}"
keep_data=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)
      prefix="${2:?--prefix requires a path}"
      shift 2
      ;;
    --keep-data)
      keep_data=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

rm -f "$prefix/bin/vegvisir" \
      "$prefix/bin/vegvisir-rust" \
      "$prefix/bin/cms-v2" \
      "$prefix/bin/usrl"

if [[ "$keep_data" -eq 0 ]]; then
  rm -rf "$prefix/share/vegvisir" "$prefix/etc/vegvisir"
fi

cat <<EOF
Removed Vegvisir binaries from:
  $prefix/bin
EOF
if [[ "$keep_data" -eq 1 ]]; then
  echo "Kept data/config under $prefix/share/vegvisir and $prefix/etc/vegvisir."
fi
