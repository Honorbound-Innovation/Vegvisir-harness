#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Install the full Vegvisir Agent Harness monorepo.

Usage:
  ./install.sh [options]

Common options:
  --prefix <path>                    Install prefix. Default: $HOME/.local
  --complete                         Full bootstrap: system deps, Rust/Cargo bootstrap,
                                      npm audit fix, Playwright browsers, HBSE user broker,
                                      and HBSE vault init with provider auto-detection.
  --no-build                         Reuse existing release artifacts.
  --install-system-deps              Install native build/runtime packages for the full project.
  --bootstrap-toolchains             Install missing Rust/Cargo via rustup when needed. Default: on
  --no-bootstrap-toolchains          Do not install missing Rust/Cargo; fail with instructions.

Component selection:
  --no-agent-admin                   Do not install the standalone agent registry admin binary.
  --no-cms-cli                       Do not install the CMS-v2 CLI.
  --no-hbse                          Do not install HBSE binaries.
  --no-skiller                       Do not install the Skiller CLI.
  --no-usrl                          Do not build/install the USRL CLI wrapper.
  --no-solarium                      Do not install the Solarium component.
  --no-biw                           Do not install Binary Intelligence Workbench.
  --no-ghidra                        Do not install wrappers for an existing Ghidra installation.
  --no-ghidra-headless-mcp           Do not install the Ghidra headless MCP bridge wrapper.
  --no-desktop                       Do not install the desktop component source/web assets.

Node/npm handling:
  --npm-audit <off|check|fix|force>  npm vulnerability handling after npm install. Default: fix
                                      off   = do not run npm audit
                                      check = fail if npm audit reports configured issues
                                      fix   = run npm audit fix; warn if unresolved
                                      force = run npm audit fix --force; warn if unresolved
  --no-npm-audit-fix                 Alias for --npm-audit off.
  --install-playwright-browsers      Install Playwright browser binaries for Solarium. Default: on
  --no-playwright-browsers           Skip Playwright browser downloads.
  --require-playwright-browsers      Fail if Playwright browser installation fails. Default: warn

HBSE setup:
  --hbse-service <none|user|system>  Install HBSE broker service. Default: none
  --enable-hbse-service              Enable HBSE broker service/socket.
  --start-hbse-service               Start HBSE broker service/socket.
  --hbse-vault <path>                HBSE vault path for init/service/model setup.
                                      Default: $HBSE_VAULT_PATH or $HOME/.local/share/hbse/vault.db
  --hbse-socket <path>               HBSE broker socket path for service install.
  --hbse-idle-timeout-seconds <n>    HBSE broker idle timeout. Default: 900
  --hbse-service-user <user>         User for system HBSE service.
  --hbse-init <none|auto|passphrase|tpm2|tpm2-direct|system-fingerprint>
                                      Initialize the HBSE vault if it is not initialized. Default: none
                                      auto prefers tpm2-direct, then tpm2, then system-fingerprint,
                                      then passphrase if the passphrase env var is set.
  --hbse-namespace <name>            HBSE vault namespace. Default: default
  --hbse-tpm-device <path>           TPM device for tpm2 providers. Default: /dev/tpmrm0
  --hbse-passphrase-env <name>       Env var used for passphrase provider. Default: HBSE_PASSPHRASE
  --hbse-run-doctor                  Run hbse doctor after HBSE setup.
  --hbse-readiness-check             Run hbse readiness check after HBSE setup.
  --hbse-model-provider <preset>     Store a model provider credential using hbse model-provider setup.
                                      The credential is read from --hbse-model-api-key-env.
  --hbse-model-api-key-env <name>    Env var containing provider API key for hbse model-provider setup.
  --hbse-model-secret-ref <ref>      secret:// ref for the model credential.
  --hbse-model-consumer <consumer>   HBSE consumer for model policies.
  --hbse-model-purpose <purpose>     Model request purpose. Default: model.chat
  --hbse-model-discovery-purpose <p> Model discovery purpose. Default: model.discovery

Deployment hardening:
  --install-vegvisir-user            Create a low-privilege Vegvisir runtime user and workspace root.
  --vegvisir-service-user <user>     User for hardened Vegvisir deployments. Default: vegvisir-agent
  --workspace-root <path>            Workspace root for hardened deployments. Default: /srv/vegvisir-workspaces
  -h, --help                         Show this help.

Examples:
  ./install.sh
  ./install.sh --complete
  ./install.sh --install-system-deps --hbse-init auto --hbse-service user --enable-hbse-service --start-hbse-service
  ./install.sh --hbse-init system-fingerprint --hbse-service user --enable-hbse-service --start-hbse-service
  OPENAI_API_KEY=... ./install.sh --hbse-init auto --hbse-model-provider openai --hbse-model-api-key-env OPENAI_API_KEY
  sudo ./install.sh --install-system-deps --prefix /usr/local --hbse-service system --hbse-service-user hbse --enable-hbse-service
  sudo ./install.sh --install-vegvisir-user --workspace-root /srv/vegvisir-workspaces
USAGE
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
prefix="${VEGVISIR_INSTALL_PREFIX:-$HOME/.local}"
build=1
install_system_deps=0
bootstrap_toolchains=1
install_agent_admin=1
install_cms_cli=1
install_hbse=1
install_skiller=1
install_usrl=1
install_solarium=1
install_biw=1
install_ghidra=1
install_ghidra_headless_mcp=1
install_desktop=1
npm_audit_mode="fix"
install_playwright_browsers=1
require_playwright_browsers=0
hbse_service="none"
enable_hbse_service=0
start_hbse_service=0
hbse_vault="${HBSE_VAULT_PATH:-$HOME/.local/share/hbse/vault.db}"
hbse_socket=""
hbse_idle_timeout_seconds="900"
hbse_service_user=""
hbse_init="none"
hbse_namespace="default"
hbse_tpm_device="/dev/tpmrm0"
hbse_passphrase_env="HBSE_PASSPHRASE"
hbse_run_doctor=0
hbse_readiness_check=0
hbse_model_provider=""
hbse_model_api_key_env=""
hbse_model_secret_ref=""
hbse_model_consumer=""
hbse_model_purpose="model.chat"
hbse_model_discovery_purpose="model.discovery"
install_vegvisir_user=0
vegvisir_service_user="vegvisir-agent"
workspace_root="/srv/vegvisir-workspaces"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)
      prefix="${2:?--prefix requires a path}"
      shift 2
      ;;
    --complete)
      install_system_deps=1
      bootstrap_toolchains=1
      npm_audit_mode="fix"
      install_playwright_browsers=1
      hbse_init="auto"
      hbse_service="user"
      enable_hbse_service=1
      start_hbse_service=1
      hbse_run_doctor=1
      shift
      ;;
    --no-build)
      build=0
      shift
      ;;
    --install-system-deps)
      install_system_deps=1
      shift
      ;;
    --no-agent-admin)
      install_agent_admin=0
      shift
      ;;
    --bootstrap-toolchains)
      bootstrap_toolchains=1
      shift
      ;;
    --no-bootstrap-toolchains)
      bootstrap_toolchains=0
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
    --npm-audit)
      npm_audit_mode="${2:?--npm-audit requires off, check, fix, or force}"
      shift 2
      ;;
    --no-npm-audit-fix)
      npm_audit_mode="off"
      shift
      ;;
    --install-playwright-browsers)
      install_playwright_browsers=1
      shift
      ;;
    --no-playwright-browsers)
      install_playwright_browsers=0
      shift
      ;;
    --require-playwright-browsers)
      require_playwright_browsers=1
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
    --hbse-init)
      hbse_init="${2:?--hbse-init requires none, auto, passphrase, tpm2, tpm2-direct, or system-fingerprint}"
      shift 2
      ;;
    --hbse-namespace)
      hbse_namespace="${2:?--hbse-namespace requires a name}"
      shift 2
      ;;
    --hbse-tpm-device)
      hbse_tpm_device="${2:?--hbse-tpm-device requires a path}"
      shift 2
      ;;
    --hbse-passphrase-env)
      hbse_passphrase_env="${2:?--hbse-passphrase-env requires an environment variable name}"
      shift 2
      ;;
    --hbse-run-doctor)
      hbse_run_doctor=1
      shift
      ;;
    --hbse-readiness-check)
      hbse_readiness_check=1
      shift
      ;;
    --hbse-model-provider)
      hbse_model_provider="${2:?--hbse-model-provider requires a preset}"
      shift 2
      ;;
    --hbse-model-api-key-env)
      hbse_model_api_key_env="${2:?--hbse-model-api-key-env requires an environment variable name}"
      shift 2
      ;;
    --hbse-model-secret-ref)
      hbse_model_secret_ref="${2:?--hbse-model-secret-ref requires a secret:// ref}"
      shift 2
      ;;
    --hbse-model-consumer)
      hbse_model_consumer="${2:?--hbse-model-consumer requires a consumer id}"
      shift 2
      ;;
    --hbse-model-purpose)
      hbse_model_purpose="${2:?--hbse-model-purpose requires a purpose}"
      shift 2
      ;;
    --hbse-model-discovery-purpose)
      hbse_model_discovery_purpose="${2:?--hbse-model-discovery-purpose requires a purpose}"
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

case "$hbse_init" in
  none|auto|passphrase|tpm2|tpm2-direct|tpm2-esapi|system-fingerprint) ;;
  *)
    echo "--hbse-init must be one of: none, auto, passphrase, tpm2, tpm2-direct, system-fingerprint" >&2
    exit 2
    ;;
esac

case "$npm_audit_mode" in
  off|check|fix|force) ;;
  *)
    echo "--npm-audit must be one of: off, check, fix, force" >&2
    exit 2
    ;;
esac

if [[ "$install_hbse" -eq 0 && ( "$hbse_service" != "none" || "$hbse_init" != "none" || -n "$hbse_model_provider" ) ]]; then
  echo "HBSE setup options require HBSE installation; remove --no-hbse or the HBSE setup flags." >&2
  exit 2
fi

bin_dir="$prefix/bin"
etc_dir="$prefix/etc/vegvisir"
share_dir="$prefix/share/vegvisir"
usrl_share_dir="$share_dir/usrl"
component_share_dir="$share_dir/components"
ghidra_headless_mcp_share_dir="$component_share_dir/ghidra-headless-mcp"
desktop_share_dir="$component_share_dir/desktop"

install_native_system_deps() {
  local args=()
  if [[ "$install_desktop" -eq 0 ]]; then
    args+=(--no-desktop)
  fi
  if [[ "$install_solarium" -eq 0 ]]; then
    args+=(--no-browser)
  fi
  if [[ "$install_ghidra" -eq 0 && "$install_ghidra_headless_mcp" -eq 0 ]]; then
    args+=(--no-ghidra)
  fi
  "$repo_root/scripts/install-system-deps.sh" "${args[@]}"
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

require_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    echo "missing required file: $path" >&2
    exit 1
  fi
}

load_cargo_env() {
  if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
  fi
}

install_rustup_toolchain() {
  if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required to bootstrap Rust with rustup. Install curl or rerun with --install-system-deps." >&2
    exit 1
  fi
  echo "Rust/Cargo not found; installing the stable Rust toolchain with rustup."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
  load_cargo_env
}

ensure_cargo() {
  load_cargo_env
  if command -v cargo >/dev/null 2>&1; then
    return 0
  fi
  if [[ "$bootstrap_toolchains" -eq 1 ]]; then
    install_rustup_toolchain
  fi
  if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo is required. Install Rust with rustup or rerun ./install.sh --install-system-deps." >&2
    exit 1
  fi
}

ensure_node_npm() {
  local component="$1"
  if command -v node >/dev/null 2>&1 && command -v npm >/dev/null 2>&1; then
    return 0
  fi
  echo "node and npm are required for $component." >&2
  echo "Install Node.js/npm or rerun ./install.sh --install-system-deps." >&2
  exit 1
}

run_npm_install_and_audit() {
  local dir="$1"
  local component="$2"
  ensure_node_npm "$component"
  if [[ -f "$dir/package-lock.json" || -f "$dir/npm-shrinkwrap.json" ]]; then
    npm --prefix "$dir" ci
  else
    npm --prefix "$dir" install
  fi
  case "$npm_audit_mode" in
    off)
      ;;
    check)
      npm --prefix "$dir" audit --audit-level=moderate
      ;;
    fix)
      if ! npm --prefix "$dir" audit fix; then
        echo "warning: npm audit fix could not resolve every issue for $component; continuing after reporting audit state." >&2
        npm --prefix "$dir" audit --audit-level=moderate || true
      fi
      ;;
    force)
      if ! npm --prefix "$dir" audit fix --force; then
        echo "warning: npm audit fix --force could not resolve every issue for $component; continuing after reporting audit state." >&2
        npm --prefix "$dir" audit --audit-level=moderate || true
      fi
      ;;
  esac
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

hbse_vault_initialized() {
  "$bin_dir/hbse" --vault "$hbse_vault" vault status >/dev/null 2>&1
}

candidate_hbse_providers() {
  if [[ "$hbse_init" != "auto" ]]; then
    printf '%s\n' "$hbse_init"
    return 0
  fi
  # Prefer hardware-backed providers, then system fingerprint, then passphrase when configured.
  printf '%s\n' tpm2-direct tpm2 system-fingerprint
  if [[ -n "${!hbse_passphrase_env:-}" ]]; then
    printf '%s\n' passphrase
  fi
}

hbse_provider_probe_available() {
  local provider="$1"
  case "$provider" in
    passphrase)
      [[ -n "${!hbse_passphrase_env:-}" ]]
      ;;
    tpm2)
      "$bin_dir/hbse" provider test-tpm2 --device "$hbse_tpm_device" >/dev/null 2>&1
      ;;
    tpm2-direct|tpm2-esapi)
      "$bin_dir/hbse" provider test-tpm2-direct --device "$hbse_tpm_device" >/dev/null 2>&1
      ;;
    system-fingerprint)
      "$bin_dir/hbse" provider test-system-fingerprint >/dev/null 2>&1
      ;;
    *)
      return 1
      ;;
  esac
}

hbse_init_provider() {
  local provider="$1"
  case "$provider" in
    passphrase)
      if [[ -z "${!hbse_passphrase_env:-}" ]]; then
        echo "Passphrase provider selected but $hbse_passphrase_env is not set." >&2
        echo "Set $hbse_passphrase_env in the local environment; do not paste it into chat." >&2
        return 1
      fi
      env HBSE_PASSPHRASE="${!hbse_passphrase_env}" "$bin_dir/hbse" --vault "$hbse_vault" vault init \
        --namespace "$hbse_namespace" \
        --provider passphrase
      ;;
    tpm2)
      "$bin_dir/hbse" --vault "$hbse_vault" vault init \
        --namespace "$hbse_namespace" \
        --provider tpm2 \
        --tpm-device "$hbse_tpm_device"
      ;;
    tpm2-direct|tpm2-esapi)
      "$bin_dir/hbse" --vault "$hbse_vault" vault init \
        --namespace "$hbse_namespace" \
        --provider tpm2-direct \
        --tpm-device "$hbse_tpm_device"
      ;;
    system-fingerprint)
      "$bin_dir/hbse" --vault "$hbse_vault" vault init \
        --namespace "$hbse_namespace" \
        --provider system-fingerprint
      ;;
    *)
      echo "unsupported HBSE provider selected: $provider" >&2
      return 2
      ;;
  esac
}

setup_hbse_vault_and_provider() {
  mkdir -p "$(dirname "$hbse_vault")"
  if [[ "$hbse_init" != "none" ]]; then
    if hbse_vault_initialized; then
      echo "HBSE vault already initialized: $hbse_vault"
    else
      local provider initialized=0
      while IFS= read -r provider; do
        [[ -n "$provider" ]] || continue
        if ! hbse_provider_probe_available "$provider"; then
          echo "HBSE provider unavailable, skipping: $provider" >&2
          continue
        fi
        echo "Initializing HBSE vault with provider: $provider"
        if hbse_init_provider "$provider"; then
          initialized=1
          break
        fi
        if [[ "$hbse_init" != "auto" ]]; then
          exit 1
        fi
        echo "warning: HBSE provider '$provider' probed available but initialization failed; trying next provider." >&2
        rm -f "$hbse_vault" "$hbse_vault-shm" "$hbse_vault-wal"
      done < <(candidate_hbse_providers)
      if [[ "$initialized" -ne 1 ]]; then
        echo "Could not initialize HBSE vault automatically." >&2
        echo "Set $hbse_passphrase_env for passphrase fallback or run hbse setup manually." >&2
        exit 1
      fi
    fi
  fi

  if [[ -n "$hbse_model_provider" ]]; then
    if [[ -z "$hbse_model_api_key_env" ]]; then
      echo "--hbse-model-provider requires --hbse-model-api-key-env so the credential is read from the local environment." >&2
      exit 2
    fi
    if [[ -z "${!hbse_model_api_key_env:-}" ]]; then
      echo "$hbse_model_api_key_env is not set; cannot store HBSE model-provider credential." >&2
      exit 1
    fi
    local model_args=(model-provider setup "$hbse_model_provider" --api-key-env "$hbse_model_api_key_env" --purpose "$hbse_model_purpose" --model-discovery-purpose "$hbse_model_discovery_purpose")
    if [[ -n "$hbse_model_secret_ref" ]]; then
      model_args+=(--secret-ref "$hbse_model_secret_ref")
    fi
    if [[ -n "$hbse_model_consumer" ]]; then
      model_args+=(--consumer "$hbse_model_consumer")
    fi
    "$bin_dir/hbse" --vault "$hbse_vault" "${model_args[@]}"
  fi
}

install_hbse_service() {
  if [[ "$hbse_service" == "none" ]]; then
    return 0
  fi
  local service_args=(
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
  "$bin_dir/hbse" --vault "$hbse_vault" "${service_args[@]}"
}

run_hbse_post_checks() {
  if [[ "$hbse_run_doctor" -eq 1 ]]; then
    "$bin_dir/hbse" --vault "$hbse_vault" doctor
  fi
  if [[ "$hbse_readiness_check" -eq 1 ]]; then
    if ! "$bin_dir/hbse" --vault "$hbse_vault" readiness check --verify-audit; then
      echo "warning: HBSE readiness check with audit verification failed; retrying without --verify-audit." >&2
      "$bin_dir/hbse" --vault "$hbse_vault" readiness check || true
    fi
  fi
}

require_file "$repo_root/Cargo.toml"
require_file "$repo_root/vegvisir/Cargo.toml"
require_file "$repo_root/components/cms-v2/Cargo.toml"
if [[ "$install_hbse" -eq 1 ]]; then
  require_file "$repo_root/components/HBSE/hbse/Cargo.toml"
fi
if [[ "$install_skiller" -eq 1 ]]; then
  require_file "$repo_root/components/skiller/Cargo.toml"
fi
if [[ "$install_usrl" -eq 1 ]]; then
  require_file "$repo_root/components/usrl/package.json"
fi
if [[ "$install_solarium" -eq 1 ]]; then
  require_file "$repo_root/components/solarium/package.json"
fi
if [[ "$install_biw" -eq 1 ]]; then
  require_file "$repo_root/components/binary-intelligence-workbench/pyproject.toml"
fi
if [[ "$install_ghidra_headless_mcp" -eq 1 ]]; then
  require_file "$repo_root/components/ghidra-headless-mcp/bin/ghidra-headless"
  require_file "$repo_root/components/ghidra-headless-mcp/bridge_mcp_ghidra_headless.py"
fi
if [[ "$install_desktop" -eq 1 ]]; then
  require_file "$repo_root/components/desktop/package.json"
fi

if [[ "$install_system_deps" -eq 1 ]]; then
  install_native_system_deps
fi

if [[ "$install_vegvisir_user" -eq 1 ]]; then
  install_vegvisir_service_user
fi

ensure_cargo

install -d "$bin_dir" "$etc_dir" "$share_dir" "$component_share_dir"

if [[ "$build" -eq 1 ]]; then
  cargo build --manifest-path "$repo_root/Cargo.toml" --release -p vegvisir-rust
  if [[ "$install_agent_admin" -eq 1 ]]; then
    cargo build --manifest-path "$repo_root/Cargo.toml" --release -p vegvisir-rust --bin vegvisir-agent-admin
  fi
  if [[ "$install_cms_cli" -eq 1 ]]; then
    cargo build --manifest-path "$repo_root/Cargo.toml" --release -p cms-v2 --bin cms
  fi
  if [[ "$install_hbse" -eq 1 ]]; then
    cargo build --manifest-path "$repo_root/Cargo.toml" --release -p hbse --bin hbse
    cargo build --manifest-path "$repo_root/Cargo.toml" --release -p hbse --bin hbse-broker
  fi
  if [[ "$install_skiller" -eq 1 ]]; then
    cargo build --manifest-path "$repo_root/Cargo.toml" --release -p skiller --bin skiller
  fi
fi

install -m 0755 "$repo_root/target/release/vegvisir-rust" "$bin_dir/vegvisir-rust"
ln -sfn "vegvisir-rust" "$bin_dir/vegvisir"

if [[ "$install_agent_admin" -eq 1 ]]; then
  install -m 0755 "$repo_root/target/release/vegvisir-agent-admin" "$bin_dir/vegvisir-agent-admin"
fi

if [[ "$install_cms_cli" -eq 1 ]]; then
  install -m 0755 "$repo_root/target/release/cms" "$bin_dir/cms-v2"
fi

if [[ "$install_skiller" -eq 1 ]]; then
  install -m 0755 "$repo_root/target/release/skiller" "$bin_dir/skiller"
fi

if [[ "$install_hbse" -eq 1 ]]; then
  install -m 0755 "$repo_root/target/release/hbse" "$bin_dir/hbse"
  install -m 0755 "$repo_root/target/release/hbse-broker" "$bin_dir/hbse-broker"
  "$bin_dir/hbse" --help >/dev/null
  "$bin_dir/hbse-broker" --help >/dev/null
  setup_hbse_vault_and_provider
  install_hbse_service
  run_hbse_post_checks
fi

if [[ "$install_biw" -eq 1 ]]; then
biw_share_dir="$component_share_dir/binary-intelligence-workbench"
rm -rf "$biw_share_dir"
mkdir -p "$biw_share_dir"
tar -C "$repo_root/components/binary-intelligence-workbench" \
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
solarium_share_dir="$share_dir/solarium"
rm -rf "$solarium_share_dir"
mkdir -p "$solarium_share_dir"
tar -C "$repo_root/components/solarium" \
  --exclude='.git' \
  --exclude='node_modules' \
  --exclude='dist' \
  --exclude='.solarium' \
  --exclude='.vegvisir' \
  -cf - . | tar -C "$solarium_share_dir" -xf -
run_npm_install_and_audit "$solarium_share_dir" "Solarium"
npm --prefix "$solarium_share_dir" run build
if [[ "$install_playwright_browsers" -eq 1 ]]; then
  if ! npm --prefix "$solarium_share_dir" run install:browsers; then
    if [[ "$require_playwright_browsers" -eq 1 ]]; then
      echo "Playwright browser installation failed and --require-playwright-browsers was set." >&2
      exit 1
    fi
    echo "warning: Playwright browser installation failed; Solarium may need 'npm --prefix $solarium_share_dir run install:browsers' later." >&2
  fi
fi
cat >"$bin_dir/solarium" <<EOF
#!/usr/bin/env bash
exec node "$solarium_share_dir/dist/cli/index.js" "\$@"
EOF
chmod 0755 "$bin_dir/solarium"
fi


discover_ghidra_install() {
  GHIDRA_RUN_PATH=""
  GHIDRA_HEADLESS_PATH=""

  if [[ -n "${GHIDRA_HOME:-}" ]]; then
    if [[ -x "$GHIDRA_HOME/ghidraRun" ]]; then
      GHIDRA_RUN_PATH="$GHIDRA_HOME/ghidraRun"
    fi
    if [[ -x "$GHIDRA_HOME/support/analyzeHeadless" ]]; then
      GHIDRA_HEADLESS_PATH="$GHIDRA_HOME/support/analyzeHeadless"
    fi
  fi

  if [[ -z "$GHIDRA_HEADLESS_PATH" && -n "${GHIDRA_HEADLESS:-}" && -x "$GHIDRA_HEADLESS" ]]; then
    GHIDRA_HEADLESS_PATH="$GHIDRA_HEADLESS"
    local inferred_home
    inferred_home="$(cd "$(dirname "$GHIDRA_HEADLESS")/.." && pwd)"
    if [[ -z "$GHIDRA_RUN_PATH" && -x "$inferred_home/ghidraRun" ]]; then
      GHIDRA_RUN_PATH="$inferred_home/ghidraRun"
    fi
  fi

  if [[ -z "$GHIDRA_HEADLESS_PATH" ]] && command -v analyzeHeadless >/dev/null 2>&1; then
    GHIDRA_HEADLESS_PATH="$(command -v analyzeHeadless)"
  fi
  if [[ -z "$GHIDRA_RUN_PATH" ]] && command -v ghidraRun >/dev/null 2>&1; then
    GHIDRA_RUN_PATH="$(command -v ghidraRun)"
  fi
  if [[ -z "$GHIDRA_HEADLESS_PATH" ]]; then
    echo "Ghidra analyzeHeadless was not found." >&2
    echo "Install Ghidra separately and set GHIDRA_HOME=/path/to/ghidra_<version> or GHIDRA_HEADLESS=/path/to/support/analyzeHeadless." >&2
    exit 1
  fi
}

if [[ "$install_ghidra" -eq 1 ]]; then
  discover_ghidra_install
  if [[ -n "$GHIDRA_RUN_PATH" ]]; then
    cat >"$bin_dir/ghidra" <<EOF
#!/usr/bin/env bash
exec "$GHIDRA_RUN_PATH" "\$@"
EOF
    chmod 0755 "$bin_dir/ghidra"
  fi
  cat >"$bin_dir/analyzeHeadless" <<EOF
#!/usr/bin/env bash
exec "$GHIDRA_HEADLESS_PATH" "\$@"
EOF
  chmod 0755 "$bin_dir/analyzeHeadless"
fi

if [[ "$install_ghidra_headless_mcp" -eq 1 ]]; then
  rm -rf "$ghidra_headless_mcp_share_dir"
  mkdir -p "$ghidra_headless_mcp_share_dir"
  tar -C "$repo_root/components/ghidra-headless-mcp" \
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
  tar -C "$repo_root/components/desktop" \
    --exclude='.git' \
    --exclude='node_modules' \
    --exclude='dist' \
    --exclude='src-tauri/target' \
    -cf - . | tar -C "$desktop_share_dir" -xf -
  run_npm_install_and_audit "$desktop_share_dir" "desktop component"
  npm --prefix "$desktop_share_dir" run web:build
  cat >"$bin_dir/vegvisir-desktop" <<EOF
#!/usr/bin/env bash
export VEGVISIR_DESKTOP_BRIDGE="\${VEGVISIR_DESKTOP_BRIDGE:-$bin_dir/vegvisir}"
exec npm --prefix "$desktop_share_dir" run dev -- "\$@"
EOF
  chmod 0755 "$bin_dir/vegvisir-desktop"
fi

if [[ "$install_usrl" -eq 1 ]]; then
  rm -rf "$usrl_share_dir"
  mkdir -p "$usrl_share_dir"
  tar -C "$repo_root/components/usrl" \
    --exclude='.git' \
    --exclude='node_modules' \
    --exclude='dist' \
    -cf - . | tar -C "$usrl_share_dir" -xf -
  run_npm_install_and_audit "$usrl_share_dir" "USRL"
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

# Optional installed Ghidra discovery for wrappers and Ghidra headless MCP.
# export GHIDRA_HOME="/path/to/ghidra_<version>"
# export GHIDRA_HEADLESS="$GHIDRA_HOME/support/analyzeHeadless"
ENV

if [[ "$install_vegvisir_user" -eq 1 ]]; then
  cat >>"$etc_dir/vegvisir.env.example" <<EOF

# Hardened deployment account and workspace root.
# Run headless workers as $vegvisir_service_user and keep workspaces below this path.
export VEGVISIR_WORKSPACE_ROOT="$workspace_root"
EOF
fi

if [[ "$install_hbse" -eq 1 ]]; then
  cat >>"$etc_dir/vegvisir.env.example" <<EOF

# HBSE configuration.
export HBSE_VAULT_PATH="$hbse_vault"
# export HBSE_BROKER_SOCKET="\${XDG_RUNTIME_DIR:-\$HOME/.local/share}/hbse/broker.sock"
EOF
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
if [[ "$install_agent_admin" -eq 1 ]]; then
  echo "  $bin_dir/vegvisir-agent-admin"
fi
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
  echo "  HBSE vault: $hbse_vault"
fi
if [[ "$install_biw" -eq 1 ]]; then
  echo "  $bin_dir/biw"
fi
if [[ "$install_usrl" -eq 1 ]]; then
  echo "  $bin_dir/usrl"
fi
if [[ "$install_ghidra" -eq 1 ]]; then
  if [[ -n "${GHIDRA_RUN_PATH:-}" ]]; then
    echo "  $bin_dir/ghidra"
  fi
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

Try:
  $bin_dir/vegvisir verify all --workspace "$repo_root"
  $bin_dir/vegvisir
EOF
