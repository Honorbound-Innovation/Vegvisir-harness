#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Install the full Vegvisir Agent Harness monorepo.

Usage:
  ./install.sh [options]

Options:
  --prefix <path>                    Install prefix. Default: $HOME/.local
  --no-build                         Reuse existing release artifacts.
  --install-system-deps              Install native build/runtime packages on Debian-like systems.
  --no-cms-cli                       Do not install the CMS-v2 CLI.
  --no-hbse                          Do not install HBSE binaries.
  --no-usrl                          Do not build/install the USRL CLI wrapper.
  --hbse-service <none|user|system>  Install HBSE broker service. Default: none
  --enable-hbse-service              Enable HBSE broker service/socket.
  --start-hbse-service               Start HBSE broker service/socket.
  --hbse-vault <path>                HBSE vault path for service install.
  --hbse-socket <path>               HBSE broker socket path for service install.
  --hbse-idle-timeout-seconds <n>    HBSE broker idle timeout. Default: 900
  --hbse-service-user <user>         User for system HBSE service.
  -h, --help                         Show this help.

Examples:
  ./install.sh
  ./install.sh --prefix "$HOME/.local" --hbse-service user --enable-hbse-service --start-hbse-service
  sudo ./install.sh --install-system-deps --prefix /usr/local --hbse-service system --hbse-service-user hbse --enable-hbse-service
USAGE
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
prefix="${VEGVISIR_INSTALL_PREFIX:-$HOME/.local}"
build=1
install_system_deps=0
install_cms_cli=1
install_hbse=1
install_usrl=1
hbse_service="none"
enable_hbse_service=0
start_hbse_service=0
hbse_vault=""
hbse_socket=""
hbse_idle_timeout_seconds="900"
hbse_service_user=""

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
    --install-system-deps)
      install_system_deps=1
      shift
      ;;
    --no-cms-cli)
      install_cms_cli=0
      shift
      ;;
    --no-hbse)
      install_hbse=0
      shift
      ;;
    --no-usrl)
      install_usrl=0
      shift
      ;;
    --hbse-service)
      hbse_service="${2:?--hbse-service requires none, user, or system}"
      shift 2
      ;;
    --enable-hbse-service)
      enable_hbse_service=1
      shift
      ;;
    --start-hbse-service)
      start_hbse_service=1
      shift
      ;;
    --hbse-vault)
      hbse_vault="${2:?--hbse-vault requires a path}"
      shift 2
      ;;
    --hbse-socket)
      hbse_socket="${2:?--hbse-socket requires a path}"
      shift 2
      ;;
    --hbse-idle-timeout-seconds)
      hbse_idle_timeout_seconds="${2:?--hbse-idle-timeout-seconds requires a number}"
      shift 2
      ;;
    --hbse-service-user)
      hbse_service_user="${2:?--hbse-service-user requires a user}"
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
  none|user|system) ;;
  *)
    echo "--hbse-service must be one of: none, user, system" >&2
    exit 2
    ;;
esac

bin_dir="$prefix/bin"
etc_dir="$prefix/etc/vegvisir"
share_dir="$prefix/share/vegvisir"
usrl_share_dir="$share_dir/usrl"

install_debian_deps() {
  if ! command -v apt-get >/dev/null 2>&1; then
    echo "--install-system-deps currently supports Debian-like systems with apt-get." >&2
    exit 1
  fi
  local apt=(apt-get)
  if [[ "$(id -u)" -ne 0 ]]; then
    apt=(sudo apt-get)
  fi
  "${apt[@]}" update
  "${apt[@]}" install -y \
    build-essential \
    ca-certificates \
    curl \
    nodejs \
    npm \
    pkg-config \
    libtss2-dev
}

require_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    echo "missing required file: $path" >&2
    exit 1
  fi
}

require_file "$repo_root/Cargo.toml"
require_file "$repo_root/vegvisir/Cargo.toml"
require_file "$repo_root/components/cms-v2/Cargo.toml"
require_file "$repo_root/components/HBSE/Cargo.toml"
require_file "$repo_root/components/usrl/package.json"

if [[ "$install_system_deps" -eq 1 ]]; then
  install_debian_deps
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required. Install Rust with rustup or your system package manager." >&2
  exit 1
fi

install -d "$bin_dir" "$etc_dir" "$share_dir"

if [[ "$build" -eq 1 ]]; then
  cargo build --manifest-path "$repo_root/Cargo.toml" --release -p vegvisir-rust
  if [[ "$install_cms_cli" -eq 1 ]]; then
    cargo build --manifest-path "$repo_root/Cargo.toml" --release -p cms-v2 --bin cms
  fi
  if [[ "$install_hbse" -eq 1 ]]; then
    cargo build --manifest-path "$repo_root/Cargo.toml" --release -p hbse --bin hbse
    cargo build --manifest-path "$repo_root/Cargo.toml" --release -p hbse --bin hbse-broker
  fi
fi

install -m 0755 "$repo_root/target/release/vegvisir-rust" "$bin_dir/vegvisir-rust"
ln -sfn "vegvisir-rust" "$bin_dir/vegvisir"

if [[ "$install_cms_cli" -eq 1 ]]; then
  install -m 0755 "$repo_root/target/release/cms" "$bin_dir/cms-v2"
fi

if [[ "$install_hbse" -eq 1 ]]; then
  install -m 0755 "$repo_root/target/release/hbse" "$bin_dir/hbse"
  install -m 0755 "$repo_root/target/release/hbse-broker" "$bin_dir/hbse-broker"
  "$bin_dir/hbse" --help >/dev/null
  "$bin_dir/hbse-broker" --help >/dev/null

  if [[ "$hbse_service" != "none" ]]; then
    hbse_cmd=("$bin_dir/hbse")
    if [[ -n "$hbse_vault" ]]; then
      hbse_cmd+=(--vault "$hbse_vault")
    fi
    service_args=(
      broker
      install-service
      --scope "$hbse_service"
      --broker-executable "$bin_dir/hbse-broker"
    )
    if [[ -n "$hbse_socket" ]]; then
      service_args+=(--socket "$hbse_socket")
    fi
    if [[ -n "$hbse_service_user" ]]; then
      service_args+=(--service-user "$hbse_service_user")
    fi
    if [[ "$enable_hbse_service" -eq 1 ]]; then
      service_args+=(--enable)
    fi
    if [[ "$start_hbse_service" -eq 1 ]]; then
      service_args+=(--start)
    fi
    if [[ "$hbse_idle_timeout_seconds" != "900" ]]; then
      service_args+=(--idle-timeout-seconds "$hbse_idle_timeout_seconds")
    fi
    "${hbse_cmd[@]}" "${service_args[@]}"
  fi
fi

if [[ "$install_usrl" -eq 1 ]]; then
  if ! command -v node >/dev/null 2>&1 || ! command -v npm >/dev/null 2>&1; then
    echo "node and npm are required for USRL. Install nodejs/npm or rerun with --no-usrl." >&2
    exit 1
  fi
  rm -rf "$usrl_share_dir"
  mkdir -p "$usrl_share_dir"
  tar -C "$repo_root/components/usrl" \
    --exclude='.git' \
    --exclude='node_modules' \
    --exclude='dist' \
    -cf - . | tar -C "$usrl_share_dir" -xf -
  npm --prefix "$usrl_share_dir" ci
  npm --prefix "$usrl_share_dir" run build
  cat >"$bin_dir/usrl" <<EOF
#!/usr/bin/env bash
if [[ "\${1:-}" == "--help" || "\${1:-}" == "-h" ]]; then
  node "$usrl_share_dir/dist/src/cli.js" || true
  exit 0
fi
exec node "$usrl_share_dir/dist/src/cli.js" "\$@"
EOF
  chmod 0755 "$bin_dir/usrl"
fi

cat >"$etc_dir/vegvisir.env.example" <<'ENV'
# Vegvisir runtime data root.
export VEGVISIR_HOME="${XDG_DATA_HOME:-$HOME/.local/share}/vegvisir"

# Production mode blocks direct provider API-key fallbacks.
export VEGVISIR_PRODUCTION=1
ENV

if [[ "$install_hbse" -eq 1 ]]; then
  cat >>"$etc_dir/vegvisir.env.example" <<'ENV'

# Optional HBSE configuration.
# export HBSE_VAULT_PATH="$HOME/.local/share/hbse/vault.db"
# export HBSE_BROKER_SOCKET="${XDG_RUNTIME_DIR:-$HOME/.local/share}/hbse/broker.sock"
ENV
fi

if [[ "$install_usrl" -eq 1 ]]; then
  cat >>"$etc_dir/vegvisir.env.example" <<EOF

# Bundled USRL validator location.
export VEGVISIR_USRL_VALIDATOR_ROOT="$usrl_share_dir"
EOF
fi

"$bin_dir/vegvisir" verify runtime --workspace "$repo_root" >/dev/null

cat <<EOF
Installed Vegvisir Agent Harness:
  $bin_dir/vegvisir
  $bin_dir/vegvisir-rust
EOF
if [[ "$install_cms_cli" -eq 1 ]]; then
  echo "  $bin_dir/cms-v2"
fi
if [[ "$install_hbse" -eq 1 ]]; then
  echo "  $bin_dir/hbse"
  echo "  $bin_dir/hbse-broker"
fi
if [[ "$install_usrl" -eq 1 ]]; then
  echo "  $bin_dir/usrl"
fi
cat <<EOF

Environment example:
  $etc_dir/vegvisir.env.example

Try:
  $bin_dir/vegvisir verify all --workspace "$repo_root"
  $bin_dir/vegvisir
EOF
