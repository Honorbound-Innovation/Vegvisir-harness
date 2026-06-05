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
  --keep-component-data Keep installed component source/build trees under $prefix/share/vegvisir/components.
  -h, --help            Show this help.
USAGE
}

prefix="${VEGVISIR_INSTALL_PREFIX:-$HOME/.local}"
keep_data=0
keep_component_data=0

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
    --keep-component-data)
      keep_component_data=1
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
      "$prefix/bin/skiller" \
      "$prefix/bin/usrl" \
      "$prefix/bin/solarium" \
      "$prefix/bin/biw" \
      "$prefix/bin/hbse" \
      "$prefix/bin/hbse-broker" \
      "$prefix/bin/ghidra" \
      "$prefix/bin/analyzeHeadless" \
      "$prefix/bin/ghidra-headless" \
      "$prefix/bin/ghidra-headless-mcp" \
      "$prefix/bin/vegvisir-desktop" \
      "$prefix/bin/vegvisir-hbse-provider-onboard"

if [[ "$keep_data" -eq 0 ]]; then
  rm -rf "$prefix/share/vegvisir" "$prefix/etc/vegvisir"
elif [[ "$keep_component_data" -eq 0 ]]; then
  rm -rf "$prefix/share/vegvisir/components" \
         "$prefix/share/vegvisir/solarium" \
         "$prefix/share/vegvisir/binary-intelligence-workbench"
fi

cat <<EOF
Removed Vegvisir binaries from:
  $prefix/bin
EOF
if [[ "$keep_data" -eq 1 ]]; then
  echo "Kept data/config under $prefix/share/vegvisir and $prefix/etc/vegvisir."
  if [[ "$keep_component_data" -eq 0 ]]; then
    echo "Removed installed component source/build trees under $prefix/share/vegvisir."
  fi
fi
