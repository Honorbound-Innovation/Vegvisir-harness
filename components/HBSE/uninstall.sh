#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Uninstall HBSE native Rust binaries and optional broker service units.

Usage:
  rust/uninstall.sh [options]

Options:
  --prefix <path>                 Install prefix. Default: $HOME/.local
  --service <none|user|system|both>
                                  Remove broker service units. Default: user
  --keep-binaries                 Do not remove hbse/hbse-broker binaries.
  --purge-vault                   Delete the vault path. Off by default.
  --vault <path>                  Vault path to purge only with --purge-vault.
                                  Default: $HOME/.local/share/hbse/vault.db
  -h, --help                      Show this help.

Examples:
  rust/uninstall.sh
  rust/uninstall.sh --service both
  sudo rust/uninstall.sh --prefix /usr/local --service system
USAGE
}

prefix="${HBSE_INSTALL_PREFIX:-$HOME/.local}"
service="user"
remove_binaries=1
purge_vault=0
vault_path="${HBSE_VAULT_PATH:-$HOME/.local/share/hbse/vault.db}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)
      prefix="${2:?--prefix requires a path}"
      shift 2
      ;;
    --service)
      service="${2:?--service requires none, user, system, or both}"
      shift 2
      ;;
    --keep-binaries)
      remove_binaries=0
      shift
      ;;
    --purge-vault)
      purge_vault=1
      shift
      ;;
    --vault)
      vault_path="${2:?--vault requires a path}"
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

case "$service" in
  none|user|system|both) ;;
  *)
    echo "--service must be one of: none, user, system, both" >&2
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

case "$service" in
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

if [[ "$remove_binaries" -eq 1 ]]; then
  rm -f "$prefix/bin/hbse" "$prefix/bin/hbse-broker"
  echo "Removed binaries from $prefix/bin"
fi

if [[ "$purge_vault" -eq 1 ]]; then
  rm -f "$vault_path"
  echo "Removed vault: $vault_path"
else
  echo "Vault left in place: $vault_path"
fi
