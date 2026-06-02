#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Install Vegvisir, CMS-v2, and HBSE from a packaged source bundle.

Usage:
  ./install.sh [options]

Options:
  --prefix <path>                    Install prefix. Default: $HOME/.local
  --no-build                         Reuse existing release artifacts in the bundle.
  --online                           Allow Cargo to use the network instead of packaged vendor deps.
  --install-system-deps              Install native build/runtime packages on Debian-like systems.
  --no-cms-cli                       Do not install the CMS-v2 CLI.
  --no-hbse                          Do not install HBSE binaries.
  --no-usrl                          Do not install bundled USRL validator.
  --hbse-service <none|user|system>  Install HBSE broker service. Default: none
  --enable-hbse-service              Enable HBSE broker service/socket.
  --start-hbse-service               Start HBSE broker service/socket.
  --hbse-vault <path>                HBSE vault path.
  --hbse-socket <path>               HBSE broker socket path.
  --hbse-idle-timeout-seconds <n>    HBSE broker idle timeout. Default: 0 (disabled)
  --hbse-service-user <user>         User for system HBSE service.
  --install-vegvisir-user            Create a low-privilege Vegvisir runtime user and workspace root.
  --vegvisir-service-user <user>     User for hardened Vegvisir deployments. Default: vegvisir-agent
  --workspace-root <path>            Workspace root for hardened deployments. Default: /srv/vegvisir-workspaces
  -h, --help                         Show this help.

Examples:
  ./install.sh --prefix "$HOME/.local"
  ./install.sh --prefix "$HOME/.local" --hbse-service user --enable-hbse-service --start-hbse-service
  sudo ./install.sh --install-system-deps --prefix /usr/local --hbse-service system --hbse-service-user hbse --enable-hbse-service
  sudo ./install.sh --install-vegvisir-user --workspace-root /srv/vegvisir-workspaces
USAGE
}

bundle_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
prefix="${VEGVISIR_INSTALL_PREFIX:-$HOME/.local}"
build=1
online=0
install_system_deps=0
install_cms_cli=1
install_hbse=1
install_usrl=1
hbse_service="none"
enable_hbse_service=0
start_hbse_service=0
hbse_vault=""
hbse_socket=""
hbse_idle_timeout_seconds="0"
hbse_service_user=""
install_vegvisir_user=0
vegvisir_service_user="vegvisir-agent"
workspace_root="/srv/vegvisir-workspaces"

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
    --online)
      online=1
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
    --install-vegvisir-user)
      install_vegvisir_user=1
      shift
      ;;
    --vegvisir-service-user)
      vegvisir_service_user="${2:?--vegvisir-service-user requires a user}"
      shift 2
      ;;
    --workspace-root)
      workspace_root="${2:?--workspace-root requires a path}"
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

app_dir="$bundle_root/app"
cms_dir="$bundle_root/third_party/CMS-v2"
hbse_rust_dir="$bundle_root/third_party/HBSE/rust"
usrl_dir="$bundle_root/third_party/USRL"
bin_dir="$prefix/bin"
etc_dir="$prefix/etc/vegvisir"
share_dir="$prefix/share/vegvisir"

if [[ ! -f "$app_dir/Cargo.toml" ]]; then
  echo "missing bundled Vegvisir source at $app_dir" >&2
  exit 1
fi
if [[ ! -f "$cms_dir/Cargo.toml" ]]; then
  echo "missing bundled CMS-v2 source at $cms_dir" >&2
  exit 1
fi
if [[ "$install_hbse" -eq 1 && ! -f "$hbse_rust_dir/Cargo.toml" ]]; then
  echo "missing bundled HBSE source at $hbse_rust_dir" >&2
  exit 1
fi
if [[ "$install_usrl" -eq 1 && ! -f "$usrl_dir/package.json" ]]; then
  echo "missing bundled USRL source at $usrl_dir" >&2
  exit 1
fi

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
    bubblewrap \
    nodejs \
    npm \
    pkg-config \
    libtss2-dev
}

run_as_root() {
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo "$@"
  else
    echo "root privileges or sudo are required for: $*" >&2
    exit 1
  fi
}

install_vegvisir_service_user() {
  if [[ ! "$vegvisir_service_user" =~ ^[a-z_][a-z0-9_-]*$ ]]; then
    echo "--vegvisir-service-user must be a valid Unix user name" >&2
    exit 2
  fi
  if [[ "$workspace_root" != /* ]]; then
    echo "--workspace-root must be an absolute path" >&2
    exit 2
  fi
  if id -u "$vegvisir_service_user" >/dev/null 2>&1; then
    echo "Vegvisir runtime user already exists: $vegvisir_service_user"
  else
    run_as_root useradd \
      --system \
      --create-home \
      --home-dir "$workspace_root" \
      --shell /usr/sbin/nologin \
      "$vegvisir_service_user"
  fi
  run_as_root install -d -m 0750 "$workspace_root"
  run_as_root chown "$vegvisir_service_user:" "$workspace_root"
}

if [[ "$install_system_deps" -eq 1 ]]; then
  install_debian_deps
fi

if [[ "$install_vegvisir_user" -eq 1 ]]; then
  install_vegvisir_service_user
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required. Install Rust with rustup or your system package manager." >&2
  exit 1
fi

if [[ "$online" -eq 0 && -d "$bundle_root/vendor" ]]; then
  export CARGO_NET_OFFLINE=true
fi

install -d "$bin_dir" "$etc_dir" "$share_dir"

if [[ "$build" -eq 1 ]]; then
  cargo build --manifest-path "$app_dir/Cargo.toml" --release
  if [[ "$install_cms_cli" -eq 1 ]]; then
    cargo build --manifest-path "$cms_dir/Cargo.toml" --release --bin cms
  fi
fi

install -m 0755 "$app_dir/target/release/vegvisir-rust" "$bin_dir/vegvisir-rust"
ln -sfn "vegvisir-rust" "$bin_dir/vegvisir"
if [[ -f "$app_dir/scripts/hbse-provider-onboard.sh" ]]; then
  install -m 0755 "$app_dir/scripts/hbse-provider-onboard.sh" "$bin_dir/vegvisir-hbse-provider-onboard"
fi

if [[ "$install_cms_cli" -eq 1 ]]; then
  install -m 0755 "$cms_dir/target/release/cms" "$bin_dir/cms-v2"
fi

if [[ "$install_usrl" -eq 1 ]]; then
  if ! command -v node >/dev/null 2>&1; then
    echo "node is required for the bundled USRL validator. Install nodejs or rerun with --install-system-deps." >&2
    exit 1
  fi
  rm -rf "$share_dir/usrl"
  mkdir -p "$share_dir/usrl"
  tar -C "$usrl_dir" \
    --exclude='.git' \
    --exclude='.claude' \
    --exclude='.codex' \
    -cf - . | tar -C "$share_dir/usrl" -xf -
  if [[ "$build" -eq 1 && ! -f "$share_dir/usrl/dist/src/cli.js" ]]; then
    if [[ -d "$share_dir/usrl/node_modules" ]]; then
      npm --prefix "$share_dir/usrl" run build
    else
      npm --prefix "$share_dir/usrl" ci
      npm --prefix "$share_dir/usrl" run build
    fi
  fi
  cat >"$bin_dir/usrl" <<EOF
#!/usr/bin/env bash
if [[ "\${1:-}" == "--help" || "\${1:-}" == "-h" ]]; then
  node "$share_dir/usrl/dist/src/cli.js" || true
  exit 0
fi
exec node "$share_dir/usrl/dist/src/cli.js" "\$@"
EOF
  chmod 0755 "$bin_dir/usrl"
fi

cat >"$etc_dir/vegvisir.env.example" <<'ENV'
# Copy to a shell profile, service environment, or local env file as needed.
# Vegvisir stores sessions, CMS-v2 data, agents, MCP config, approvals, and traces here.
export VEGVISIR_HOME="${XDG_DATA_HOME:-$HOME/.local/share}/vegvisir"

# Production mode blocks direct provider API-key fallbacks.
export VEGVISIR_PRODUCTION=1

ENV

if [[ "$install_vegvisir_user" -eq 1 ]]; then
  cat >>"$etc_dir/vegvisir.env.example" <<EOF

# Hardened deployment account and workspace root.
# Run headless workers as $vegvisir_service_user and keep workspaces below this path.
export VEGVISIR_WORKSPACE_ROOT="$workspace_root"
EOF
fi

if [[ "$install_hbse" -eq 1 ]]; then
  cat >>"$etc_dir/vegvisir.env.example" <<'ENV'

# Optional explicit HBSE vault and broker socket. The broker service must be
# installed with the same vault path that you use when adding secrets/policies.
ENV
  if [[ -n "$hbse_vault" ]]; then
    cat >>"$etc_dir/vegvisir.env.example" <<EOF
export HBSE_VAULT_PATH="$hbse_vault"
EOF
  else
    cat >>"$etc_dir/vegvisir.env.example" <<'ENV'
# export HBSE_VAULT_PATH="$HOME/.local/share/hbse/vault.db"
ENV
  fi
  if [[ -n "$hbse_socket" ]]; then
    cat >>"$etc_dir/vegvisir.env.example" <<EOF
export HBSE_BROKER_SOCKET="$hbse_socket"
EOF
  else
    cat >>"$etc_dir/vegvisir.env.example" <<'ENV'
# export HBSE_BROKER_SOCKET="${XDG_RUNTIME_DIR:-$HOME/.local/share}/hbse/broker.sock"
ENV
  fi
fi

if [[ "$install_usrl" -eq 1 ]]; then
  cat >>"$etc_dir/vegvisir.env.example" <<EOF

# Authoritative bundled USRL validator used by CMS-v2/Vegvisir when loading .usrl skills.
export VEGVISIR_USRL_VALIDATOR_ROOT="$share_dir/usrl"
EOF
fi

if [[ "$install_hbse" -eq 1 ]]; then
  if [[ "$build" -eq 1 ]]; then
    cargo build --manifest-path "$hbse_rust_dir/Cargo.toml" --release --bin hbse
    cargo build --manifest-path "$hbse_rust_dir/Cargo.toml" --release --bin hbse-broker
  fi

  install -m 0755 "$hbse_rust_dir/target/release/hbse" "$bin_dir/hbse"
  install -m 0755 "$hbse_rust_dir/target/release/hbse-broker" "$bin_dir/hbse-broker"
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
      --idle-timeout-seconds "$hbse_idle_timeout_seconds"
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
    "${hbse_cmd[@]}" "${service_args[@]}"
  fi
fi

"$bin_dir/vegvisir-rust" verify runtime --workspace "$PWD" >/dev/null

cat <<EOF
Installed Vegvisir:
  $bin_dir/vegvisir-rust
  $bin_dir/vegvisir -> vegvisir-rust
EOF
if [[ "$install_cms_cli" -eq 1 ]]; then
  echo "  $bin_dir/cms-v2"
fi
if [[ "$install_hbse" -eq 1 ]]; then
  echo "  $bin_dir/hbse"
  echo "  $bin_dir/hbse-broker"
  if [[ -f "$bin_dir/vegvisir-hbse-provider-onboard" ]]; then
    echo "  $bin_dir/vegvisir-hbse-provider-onboard"
  fi
fi
if [[ "$install_usrl" -eq 1 ]]; then
  echo "  $bin_dir/usrl"
  echo "  $share_dir/usrl"
fi
if [[ "$install_vegvisir_user" -eq 1 ]]; then
  echo "  runtime user: $vegvisir_service_user"
  echo "  workspace root: $workspace_root"
fi
cat <<EOF

Environment example:
  $etc_dir/vegvisir.env.example

Next checks:
  $bin_dir/vegvisir verify all --workspace /path/to/project
  $bin_dir/vegvisir tui
EOF
