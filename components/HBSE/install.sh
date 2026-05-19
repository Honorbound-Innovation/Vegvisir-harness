#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Install HBSE native Rust binaries.

Usage:
  rust/install.sh [options]

Options:
  --prefix <path>                 Install prefix. Default: $HOME/.local
  --no-build                      Do not run cargo build --release first.
  --service <none|user|system>    Install broker service. Default: none
  --enable-service                Enable broker service/socket after install.
  --start-service                 Start broker service after install.
  --vault <path>                  Vault path for broker service.
                                  Default: $HOME/.local/share/hbse/vault.db
  --socket <path>                 Broker socket path for service.
  --idle-timeout-seconds <n>      Broker idle timeout. Default: 900
  --service-user <user>           User for system service.
  -h, --help                      Show this help.

Examples:
  rust/install.sh
  rust/install.sh --service user --enable-service --start-service
  sudo rust/install.sh --prefix /usr/local --service system --service-user hbse --enable-service
USAGE
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
prefix="${HBSE_INSTALL_PREFIX:-$HOME/.local}"
build=1
service="none"
enable_service=0
start_service=0
vault_path="${HBSE_VAULT_PATH:-$HOME/.local/share/hbse/vault.db}"
socket_path=""
idle_timeout_seconds="900"
service_user=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)
      prefix="${2:?--prefix requires a path}"
      shift 2
      ;;
    --no-build)
      build=0
      shift
      ;;
    --service)
      service="${2:?--service requires none, user, or system}"
      shift 2
      ;;
    --enable-service)
      enable_service=1
      shift
      ;;
    --start-service)
      start_service=1
      shift
      ;;
    --vault)
      vault_path="${2:?--vault requires a path}"
      shift 2
      ;;
    --socket)
      socket_path="${2:?--socket requires a path}"
      shift 2
      ;;
    --idle-timeout-seconds)
      idle_timeout_seconds="${2:?--idle-timeout-seconds requires a number}"
      shift 2
      ;;
    --service-user)
      service_user="${2:?--service-user requires a user}"
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
  none|user|system) ;;
  *)
    echo "--service must be one of: none, user, system" >&2
    exit 2
    ;;
esac

bin_dir="$prefix/bin"
if [[ "$build" -eq 1 ]]; then
  cargo build --manifest-path "$repo_root/rust/Cargo.toml" --release
fi

install -d "$bin_dir"
install -m 0755 "$repo_root/rust/target/release/hbse" "$bin_dir/hbse"
install -m 0755 "$repo_root/rust/target/release/hbse-broker" "$bin_dir/hbse-broker"

"$bin_dir/hbse" --help >/dev/null
"$bin_dir/hbse-broker" --help >/dev/null

echo "Installed:"
echo "  $bin_dir/hbse"
echo "  $bin_dir/hbse-broker"

if [[ "$service" != "none" ]]; then
  service_args=(
    --scope "$service"
    --broker-executable "$bin_dir/hbse-broker"
    --vault "$vault_path"
    --idle-timeout-seconds "$idle_timeout_seconds"
  )
  if [[ -n "$socket_path" ]]; then
    service_args+=(--socket "$socket_path")
  fi
  if [[ -n "$service_user" ]]; then
    service_args+=(--service-user "$service_user")
  fi
  if [[ "$enable_service" -eq 1 ]]; then
    service_args+=(--enable)
  fi
  if [[ "$start_service" -eq 1 ]]; then
    service_args+=(--start)
  fi

  "$bin_dir/hbse" broker install-service "${service_args[@]}"
fi

if [[ ":$PATH:" != *":$bin_dir:"* ]]; then
  echo "Note: $bin_dir is not currently on PATH."
fi
