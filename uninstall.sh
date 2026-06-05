#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Uninstall the full Vegvisir Agent Harness installed by ./install.sh.

Usage:
  ./uninstall.sh [options]

Options:
  --prefix <path>             Install prefix. Default: $HOME/.local
  --hbse-service <none|user|system|both>
                              Remove HBSE broker service units. Default: user
  --keep-data                 Keep $prefix/share/vegvisir and $prefix/etc/vegvisir.
  --keep-component-data       Keep installed component source/build trees under $prefix/share/vegvisir/components.
  --purge-hbse-vault          Delete the configured HBSE vault path.
  --hbse-vault <path>         HBSE vault path to purge only with --purge-hbse-vault.
                              Default: $HOME/.local/share/hbse/vault.db
  -h, --help                  Show this help.
USAGE
}

prefix="${VEGVISIR_INSTALL_PREFIX:-$HOME/.local}"
hbse_service="user"
keep_data=0
keep_component_data=0
purge_hbse_vault=0
hbse_vault="${HBSE_VAULT_PATH:-$HOME/.local/share/hbse/vault.db}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)
      prefix="${2:?--prefix requires a path}"
      shift 2
      ;;
    --hbse-service)
      hbse_service="${2:?--hbse-service requires none, user, system, or both}"
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
    --purge-hbse-vault)
      purge_hbse_vault=1
      shift
      ;;
    --hbse-vault)
      hbse_vault="${2:?--hbse-vault requires a path}"
      shift 2
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

case "$hbse_service" in
  none|user|system|both) ;;
  *)
    echo "--hbse-service must be one of: none, user, system, both" >&2
    exit 2
    ;;
esac

remove_user_units() {
  systemctl --user stop hbse-broker.service hbse-broker.socket 2>/dev/null || true
  systemctl --user disable hbse-broker.service hbse-broker.socket 2>/dev/null || true
  rm -f "$HOME/.config/systemd/user/hbse-broker.service"
  rm -f "$HOME/.config/systemd/user/hbse-broker.socket"
  systemctl --user daemon-reload 2>/dev/null || true
}

remove_system_units() {
  systemctl stop hbse-broker.service hbse-broker.socket 2>/dev/null || true
  systemctl disable hbse-broker.service hbse-broker.socket 2>/dev/null || true
  rm -f /etc/systemd/system/hbse-broker.service
  rm -f /etc/systemd/system/hbse-broker.socket
  systemctl daemon-reload 2>/dev/null || true
}

case "$hbse_service" in
  user)
    remove_user_units
    ;;
  system)
    remove_system_units
    ;;
  both)
    remove_user_units
    remove_system_units
    ;;
  none)
    ;;
esac

rm -f "$prefix/bin/vegvisir" \
      "$prefix/bin/vegvisir-rust" \
      "$prefix/bin/cms-v2" \
      "$prefix/bin/hbse" \
      "$prefix/bin/hbse-broker" \
      "$prefix/bin/skiller" \
      "$prefix/bin/usrl" \
      "$prefix/bin/solarium" \
      "$prefix/bin/biw" \
      "$prefix/bin/ghidra" \
      "$prefix/bin/analyzeHeadless" \
      "$prefix/bin/ghidra-headless" \
      "$prefix/bin/ghidra-headless-mcp" \
      "$prefix/bin/vegvisir-desktop"

if [[ "$keep_data" -eq 0 ]]; then
  rm -rf "$prefix/share/vegvisir" "$prefix/etc/vegvisir"
elif [[ "$keep_component_data" -eq 0 ]]; then
  rm -rf "$prefix/share/vegvisir/components" \
         "$prefix/share/vegvisir/solarium" \
         "$prefix/share/vegvisir/binary-intelligence-workbench"
fi

if [[ "$purge_hbse_vault" -eq 1 ]]; then
  rm -f "$hbse_vault"
fi

cat <<EOF
Removed Vegvisir Agent Harness binaries from:
  $prefix/bin
EOF
if [[ "$keep_data" -eq 1 ]]; then
  echo "Kept data/config under $prefix/share/vegvisir and $prefix/etc/vegvisir."
  if [[ "$keep_component_data" -eq 0 ]]; then
    echo "Removed installed component source/build trees under $prefix/share/vegvisir."
  fi
fi
if [[ "$purge_hbse_vault" -eq 1 ]]; then
  echo "Removed HBSE vault: $hbse_vault"
else
  echo "HBSE vault left in place: $hbse_vault"
fi

