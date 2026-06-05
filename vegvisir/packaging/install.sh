#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Install the full Vegvisir Agent Harness component bundle.

Usage:
  ./install.sh [options]

Options:
  --prefix <path>                    Install prefix. Default: $HOME/.local
  --no-build                         Reuse existing release artifacts in the bundle.
  --online                           Allow Cargo to use the network instead of packaged vendor deps.
  --install-system-deps              Install native build/runtime packages on Debian-like systems.
  --no-cms-cli                       Do not install the CMS-v2 CLI.
  --no-hbse                          Do not install HBSE binaries.
  --no-skiller                       Do not install the Skiller CLI.
  --no-usrl                          Do not install bundled USRL validator.
  --no-solarium                      Do not install the Solarium component.
  --no-biw                           Do not install Binary Intelligence Workbench.
  --no-ghidra                        Do not build/install the vendored Ghidra distribution and runtime wrappers.
  --no-ghidra-headless-mcp           Do not install the Ghidra headless MCP bridge wrapper.
  --no-desktop                       Do not install the desktop component source/web assets.
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
install_skiller=1
install_usrl=1
install_solarium=1
install_biw=1
install_ghidra=1
install_ghidra_headless_mcp=1
install_desktop=1
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
    --no-skiller)
      install_skiller=0
      shift
      ;;
    --no-usrl)
      install_usrl=0
      shift
      ;;
    --no-solarium)
      install_solarium=0
      shift
      ;;
    --no-biw)
      install_biw=0
      shift
      ;;
    --no-ghidra)
      install_ghidra=0
      shift
      ;;
    --no-ghidra-headless-mcp)
      install_ghidra_headless_mcp=0
      shift
      ;;
    --no-desktop)
      install_desktop=0
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
biw_dir="$app_dir/components/binary-intelligence-workbench"
solarium_dir="$app_dir/components/solarium"
ghidra_dir="$app_dir/components/ghidra"
ghidra_headless_mcp_dir="$app_dir/components/ghidra-headless-mcp"
desktop_dir="$app_dir/components/desktop"
bin_dir="$prefix/bin"
etc_dir="$prefix/etc/vegvisir"
share_dir="$prefix/share/vegvisir"
component_share_dir="$share_dir/components"
ghidra_share_dir="$component_share_dir/ghidra"
ghidra_headless_mcp_share_dir="$component_share_dir/ghidra-headless-mcp"
desktop_share_dir="$component_share_dir/desktop"

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
if [[ ! -f "$biw_dir/pyproject.toml" ]]; then
  echo "missing bundled Binary Intelligence Workbench source at $biw_dir" >&2
  exit 1
fi
if [[ ! -f "$solarium_dir/package.json" ]]; then
  echo "missing bundled Solarium source at $solarium_dir" >&2
  exit 1
fi
if [[ "$install_ghidra" -eq 1 && ! -f "$ghidra_dir/gradlew" ]]; then
  echo "missing bundled Ghidra Gradle wrapper at $ghidra_dir" >&2
  exit 1
fi
if [[ "$install_ghidra" -eq 1 && ! -f "$ghidra_dir/build.gradle" ]]; then
  echo "missing bundled Ghidra build file at $ghidra_dir" >&2
  exit 1
fi
if [[ ! -f "$ghidra_headless_mcp_dir/bin/ghidra-headless" ]]; then
  echo "missing bundled Ghidra headless MCP source at $ghidra_headless_mcp_dir" >&2
  exit 1
fi
if [[ ! -f "$desktop_dir/package.json" ]]; then
  echo "missing bundled desktop source at $desktop_dir" >&2
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
    python3 \
    python3-pip \
    python3-venv \
    unzip \
    zip \
    openjdk-21-jdk \
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

install -d "$bin_dir" "$etc_dir" "$share_dir" "$component_share_dir"

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

if [[ "$install_skiller" -eq 1 ]]; then
  install -m 0755 "$app_dir/target/release/skiller" "$bin_dir/skiller"
fi

if [[ "$install_biw" -eq 1 ]]; then
biw_share_dir="$component_share_dir/binary-intelligence-workbench"
rm -rf "$biw_share_dir"
mkdir -p "$biw_share_dir"
tar -C "$biw_dir" \
  --exclude='.git' \
  --exclude='__pycache__' \
  --exclude='*.pyc' \
  --exclude='.pytest_cache' \
  --exclude='.vegvisir' \
  -cf - . | tar -C "$biw_share_dir" -xf -
cat >"$bin_dir/biw" <<EOF
#!/usr/bin/env bash
export PYTHONPATH="$biw_share_dir:\${PYTHONPATH:-}"
if [[ -z "\${BIW_GHIDRA_WRAPPER:-}" && -x "$bin_dir/ghidra-headless" ]]; then
  export BIW_GHIDRA_WRAPPER="$bin_dir/ghidra-headless"
fi
exec python3 -m biw.cli "\$@"
EOF
chmod 0755 "$bin_dir/biw"
fi

if [[ "$install_solarium" -eq 1 ]]; then
solarium_share_dir="$component_share_dir/solarium"
rm -rf "$solarium_share_dir"
mkdir -p "$solarium_share_dir"
tar -C "$solarium_dir" \
  --exclude='.git' \
  --exclude='node_modules' \
  --exclude='dist' \
  --exclude='.solarium' \
  --exclude='.vegvisir' \
  -cf - . | tar -C "$solarium_share_dir" -xf -
if ! command -v node >/dev/null 2>&1 || ! command -v npm >/dev/null 2>&1; then
  echo "node and npm are required for Solarium. Install nodejs/npm or rerun with --install-system-deps." >&2
  exit 1
fi
npm --prefix "$solarium_share_dir" ci
npm --prefix "$solarium_share_dir" run build
cat >"$bin_dir/solarium" <<EOF
#!/usr/bin/env bash
exec node "$solarium_share_dir/dist/cli/index.js" "\$@"
EOF
chmod 0755 "$bin_dir/solarium"
fi



build_and_install_ghidra() {
  local src_dir="$1"
  local dst_dir="$2"

  if ! command -v java >/dev/null 2>&1; then
    echo "java is required to build Ghidra. Install JDK 21 or rerun with --install-system-deps." >&2
    exit 1
  fi
  if ! command -v unzip >/dev/null 2>&1; then
    echo "unzip is required to install the built Ghidra distribution. Install unzip or rerun with --install-system-deps." >&2
    exit 1
  fi

  chmod +x "$src_dir/gradlew"
  if [[ ! -d "$src_dir/dependencies" ]]; then
    echo "Fetching Ghidra Gradle build dependencies..."
    (cd "$src_dir" && ./gradlew -I gradle/support/fetchDependencies.gradle)
  fi

  echo "Building Ghidra distribution with Gradle buildGhidra..."
  (cd "$src_dir" && ./gradlew buildGhidra)

  local dist_zip
  dist_zip="$(find "$src_dir/build/dist" -maxdepth 1 -type f -name 'ghidra_*_PUBLIC_*.zip' -print | sort | tail -n 1)"
  if [[ -z "$dist_zip" ]]; then
    dist_zip="$(find "$src_dir/build/dist" -maxdepth 1 -type f -name 'ghidra*.zip' -print | sort | tail -n 1)"
  fi
  if [[ -z "$dist_zip" ]]; then
    echo "Ghidra build completed but no distribution zip was found under $src_dir/build/dist" >&2
    exit 1
  fi

  rm -rf "$dst_dir"
  mkdir -p "$dst_dir"
  unzip -q "$dist_zip" -d "$dst_dir"

  local unpacked_dir
  unpacked_dir="$(find "$dst_dir" -mindepth 1 -maxdepth 1 -type d -name 'ghidra*' -print | sort | tail -n 1)"
  if [[ -z "$unpacked_dir" ]]; then
    echo "Ghidra distribution did not unpack to a ghidra* directory under $dst_dir" >&2
    exit 1
  fi

  rm -rf "$dst_dir/current"
  ln -s "$(basename "$unpacked_dir")" "$dst_dir/current"

  if [[ ! -f "$dst_dir/current/ghidraRun" ]]; then
    echo "Ghidra distribution is missing ghidraRun: $dst_dir/current/ghidraRun" >&2
    exit 1
  fi
  if [[ ! -f "$dst_dir/current/support/analyzeHeadless" ]]; then
    echo "Ghidra distribution is missing analyzeHeadless: $dst_dir/current/support/analyzeHeadless" >&2
    exit 1
  fi
}

install_python_venv() {
  local venv_dir="$1"
  shift
  if ! command -v python3 >/dev/null 2>&1; then
    echo "python3 is required for Python component wrappers. Install python3 or rerun with the related --no-* option." >&2
    exit 1
  fi
  python3 -m venv "$venv_dir"
  "$venv_dir/bin/python" -m pip install --upgrade pip >/dev/null
  if [[ $# -gt 0 ]]; then
    "$venv_dir/bin/python" -m pip install "$@"
  fi
}

if [[ "$install_ghidra" -eq 1 ]]; then
  if [[ "$build" -eq 1 ]]; then
    build_and_install_ghidra "$ghidra_dir" "$ghidra_share_dir"
  elif [[ ! -f "$ghidra_share_dir/current/ghidraRun" || ! -f "$ghidra_share_dir/current/support/analyzeHeadless" ]]; then
    echo "--no-build requires an existing installed Ghidra distribution at $ghidra_share_dir/current" >&2
    exit 1
  fi
  cat >"$bin_dir/ghidra" <<EOF
#!/usr/bin/env bash
exec "$ghidra_share_dir/current/ghidraRun" "\$@"
EOF
  chmod 0755 "$bin_dir/ghidra"
  cat >"$bin_dir/analyzeHeadless" <<EOF
#!/usr/bin/env bash
exec "$ghidra_share_dir/current/support/analyzeHeadless" "\$@"
EOF
  chmod 0755 "$bin_dir/analyzeHeadless"
fi

if [[ "$install_ghidra_headless_mcp" -eq 1 ]]; then
  rm -rf "$ghidra_headless_mcp_share_dir"
  mkdir -p "$ghidra_headless_mcp_share_dir"
  tar -C "$ghidra_headless_mcp_dir" \
    --exclude='.git' \
    --exclude='.venv' \
    --exclude='__pycache__' \
    --exclude='*.pyc' \
    -cf - . | tar -C "$ghidra_headless_mcp_share_dir" -xf -
  install_python_venv "$ghidra_headless_mcp_share_dir/.venv" "mcp==1.5.0"
  cat >"$bin_dir/ghidra-headless" <<EOF
#!/usr/bin/env bash
export GHIDRA_HEADLESS="\${GHIDRA_HEADLESS:-$bin_dir/analyzeHeadless}"
exec "$ghidra_headless_mcp_share_dir/bin/ghidra-headless" "\$@"
EOF
  chmod 0755 "$bin_dir/ghidra-headless"
  cat >"$bin_dir/ghidra-headless-mcp" <<EOF
#!/usr/bin/env bash
export GHIDRA_HEADLESS="\${GHIDRA_HEADLESS:-$bin_dir/analyzeHeadless}"
exec "$ghidra_headless_mcp_share_dir/.venv/bin/python" "$ghidra_headless_mcp_share_dir/bridge_mcp_ghidra_headless.py" "\$@"
EOF
  chmod 0755 "$bin_dir/ghidra-headless-mcp"
fi


if [[ "$install_desktop" -eq 1 ]]; then
  rm -rf "$desktop_share_dir"
  mkdir -p "$desktop_share_dir"
  tar -C "$desktop_dir" \
    --exclude='.git' \
    --exclude='node_modules' \
    --exclude='dist' \
    --exclude='src-tauri/target' \
    -cf - . | tar -C "$desktop_share_dir" -xf -
  if ! command -v node >/dev/null 2>&1 || ! command -v npm >/dev/null 2>&1; then
    echo "node and npm are required for the desktop component. Install nodejs/npm or rerun with --no-desktop." >&2
    exit 1
  fi
  npm --prefix "$desktop_share_dir" ci
  npm --prefix "$desktop_share_dir" run web:build
  cat >"$bin_dir/vegvisir-desktop" <<EOF
#!/usr/bin/env bash
export VEGVISIR_DESKTOP_BRIDGE="\${VEGVISIR_DESKTOP_BRIDGE:-$bin_dir/vegvisir}"
exec npm --prefix "$desktop_share_dir" run dev -- "\$@"
EOF
  chmod 0755 "$bin_dir/vegvisir-desktop"
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
if [[ "$install_skiller" -eq 1 ]]; then
  echo "  $bin_dir/skiller"
fi
if [[ "$install_solarium" -eq 1 ]]; then
  echo "  $bin_dir/solarium"
fi
if [[ "$install_hbse" -eq 1 ]]; then
  echo "  $bin_dir/hbse"
  echo "  $bin_dir/hbse-broker"
  if [[ -f "$bin_dir/vegvisir-hbse-provider-onboard" ]]; then
    echo "  $bin_dir/vegvisir-hbse-provider-onboard"
  fi
fi
if [[ "$install_biw" -eq 1 ]]; then
  echo "  $bin_dir/biw"
fi
if [[ "$install_usrl" -eq 1 ]]; then
  echo "  $bin_dir/usrl"
  echo "  $share_dir/usrl"
fi
if [[ "$install_ghidra" -eq 1 ]]; then
  echo "  $bin_dir/ghidra"
  echo "  $bin_dir/analyzeHeadless"
fi
if [[ "$install_ghidra_headless_mcp" -eq 1 ]]; then
  echo "  $bin_dir/ghidra-headless"
  echo "  $bin_dir/ghidra-headless-mcp"
fi
if [[ "$install_desktop" -eq 1 ]]; then
  echo "  $bin_dir/vegvisir-desktop"
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
