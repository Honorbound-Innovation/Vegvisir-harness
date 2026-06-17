#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Install native/system dependencies for the full Vegvisir monorepo.

Usage:
  sudo bash scripts/install-system-deps.sh [options]
  bash scripts/install-system-deps.sh [options]   # re-execs through sudo when needed

Options:
  --core-only              Install only dependencies needed for the Rust harness and core CLIs.
  --no-rustup              Do not install the optional rustup package.
  --no-desktop             Skip Tauri/WebKit/GTK desktop build dependencies.
  --no-browser             Skip Playwright/Solarium browser runtime dependencies.
  --no-ghidra              Skip Java/Ghidra runtime support dependencies.
  --no-optional-tools      Skip optional debugging/reverse-engineering convenience tools.
  --dry-run                Print the package-manager commands without running them.
  -h, --help               Show this help.

Supported package managers:
  apt-get, dnf, pacman

Notes:
  - This script installs OS packages only. It does not install provider credentials.
  - Rust/Cargo are bootstrapped by ./install.sh with rustup if still missing.
  - Rust crates, npm packages, npm audit repair, and Python venv packages are still handled by ./install.sh.
  - Ghidra itself is not redistributed here; this installs Java/runtime prerequisites and wrappers support.
USAGE
}

original_args=("$@")

install_desktop=1
install_browser=1
install_ghidra=1
install_optional_tools=1
dry_run=0
install_rustup=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --core-only)
      install_desktop=0
      install_browser=0
      install_ghidra=0
      install_optional_tools=0
      shift
      ;;
    --no-rustup)
      install_rustup=0
      shift
      ;;
    --no-desktop)
      install_desktop=0
      shift
      ;;
    --no-browser)
      install_browser=0
      shift
      ;;
    --no-ghidra)
      install_ghidra=0
      shift
      ;;
    --no-optional-tools)
      install_optional_tools=0
      shift
      ;;
    --dry-run)
      dry_run=1
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

run() {
  if [[ "$dry_run" -eq 1 ]]; then
    printf '+ '
    printf '%q ' "$@"
    printf '\n'
  else
    "$@"
  fi
}

need_root_or_sudo() {
  if [[ "$dry_run" -eq 1 ]]; then
    return 0
  fi
  if [[ "$(id -u)" -eq 0 ]]; then
    return 0
  fi
  if command -v sudo >/dev/null 2>&1; then
    exec sudo bash "$0" "${original_args[@]}"
  fi
  echo "root privileges are required. Re-run with sudo." >&2
  exit 1
}

append_if_available_apt() {
  local package
  for package in "$@"; do
    if apt-cache show "$package" >/dev/null 2>&1; then
      APT_PACKAGES+=("$package")
    else
      echo "Skipping unavailable apt package: $package" >&2
    fi
  done
}

append_first_available_apt() {
  local package
  for package in "$@"; do
    if apt-cache show "$package" >/dev/null 2>&1; then
      APT_PACKAGES+=("$package")
      return 0
    fi
  done
  echo "Skipping unavailable apt package group: $*" >&2
}

install_with_apt() {
  local -a APT_PACKAGES=(
    build-essential
    ca-certificates
    curl
    wget
    git
    pkg-config
    cmake
    clang
    lld
    libssl-dev
    libsqlite3-dev
    libdbus-1-dev
    libtss2-dev
    bubblewrap
    rustc
    cargo
    nodejs
    npm
    python3
    python3-pip
    python3-venv
  )

  if [[ "$install_rustup" -eq 1 ]]; then
    append_if_available_apt rustup
  fi

  if [[ "$install_ghidra" -eq 1 ]]; then
    append_first_available_apt openjdk-21-jdk openjdk-17-jdk default-jdk
  fi

  if [[ "$install_desktop" -eq 1 ]]; then
    APT_PACKAGES+=(
      libglib2.0-dev
      libgtk-3-dev
      librsvg2-dev
      patchelf
      desktop-file-utils
      xdg-utils
    )
    append_if_available_apt \
      libwebkit2gtk-4.1-dev \
      libjavascriptcoregtk-4.1-dev \
      libsoup-3.0-dev \
      libayatana-appindicator3-dev
  fi

  if [[ "$install_browser" -eq 1 ]]; then
    append_if_available_apt \
      libnss3 \
      libnspr4 \
      libatk1.0-0 \
      libatk-bridge2.0-0 \
      libcups2 \
      libdrm2 \
      libxkbcommon0 \
      libxcomposite1 \
      libxdamage1 \
      libxfixes3 \
      libxrandr2 \
      libgbm1 \
      libpango-1.0-0 \
      libcairo2
    append_first_available_apt libasound2t64 libasound2
  fi

  if [[ "$install_optional_tools" -eq 1 ]]; then
    append_if_available_apt \
      file \
      binutils \
      gdb \
      lldb \
      strace \
      jq \
      ripgrep \
      fd-find \
      shellcheck
  fi

  run apt-get update
  run apt-get install -y "${APT_PACKAGES[@]}"
}

install_with_dnf() {
  local -a DNF_PACKAGES=(
    gcc
    gcc-c++
    make
    ca-certificates
    curl
    wget
    git
    pkgconf-pkg-config
    cmake
    clang
    lld
    openssl-devel
    sqlite-devel
    dbus-devel
    tpm2-tss-devel
    bubblewrap
    rustc
    cargo
    nodejs
    npm
    python3
    python3-pip
  )

  if [[ "$install_rustup" -eq 1 ]]; then
    DNF_PACKAGES+=(rustup)
  fi

  if [[ "$install_ghidra" -eq 1 ]]; then
    DNF_PACKAGES+=(java-21-openjdk-devel)
  fi

  if [[ "$install_desktop" -eq 1 ]]; then
    DNF_PACKAGES+=(
      glib2-devel
      gtk3-devel
      webkit2gtk4.1-devel
      libappindicator-gtk3-devel
      librsvg2-devel
      patchelf
      desktop-file-utils
      xdg-utils
    )
  fi

  if [[ "$install_browser" -eq 1 ]]; then
    DNF_PACKAGES+=(
      nss
      nspr
      atk
      at-spi2-atk
      cups-libs
      libdrm
      libxkbcommon
      libXcomposite
      libXdamage
      libXfixes
      libXrandr
      mesa-libgbm
      pango
      cairo
      alsa-lib
    )
  fi

  if [[ "$install_optional_tools" -eq 1 ]]; then
    DNF_PACKAGES+=(file binutils gdb lldb strace jq ripgrep fd-find ShellCheck)
  fi

  run dnf install -y "${DNF_PACKAGES[@]}"
}

install_with_pacman() {
  local -a PACMAN_PACKAGES=(
    base-devel
    ca-certificates
    curl
    wget
    git
    pkgconf
    cmake
    clang
    lld
    openssl
    sqlite
    dbus
    tpm2-tss
    bubblewrap
    rustc
    cargo
    nodejs
    npm
    python
    python-pip
  )

  if [[ "$install_rustup" -eq 1 ]]; then
    PACMAN_PACKAGES+=(rustup)
  fi

  if [[ "$install_ghidra" -eq 1 ]]; then
    PACMAN_PACKAGES+=(jdk21-openjdk)
  fi

  if [[ "$install_desktop" -eq 1 ]]; then
    PACMAN_PACKAGES+=(
      glib2
      gtk3
      webkit2gtk-4.1
      libappindicator-gtk3
      librsvg
      patchelf
      desktop-file-utils
      xdg-utils
    )
  fi

  if [[ "$install_browser" -eq 1 ]]; then
    PACMAN_PACKAGES+=(
      nss
      nspr
      at-spi2-core
      libcups
      libdrm
      libxkbcommon
      libxcomposite
      libxdamage
      libxfixes
      libxrandr
      mesa
      pango
      cairo
      alsa-lib
    )
  fi

  if [[ "$install_optional_tools" -eq 1 ]]; then
    PACMAN_PACKAGES+=(file binutils gdb lldb strace jq ripgrep fd shellcheck)
  fi

  run pacman -Syu --needed --noconfirm "${PACMAN_PACKAGES[@]}"
}

need_root_or_sudo "$@"

if command -v apt-get >/dev/null 2>&1; then
  install_with_apt
elif command -v dnf >/dev/null 2>&1; then
  install_with_dnf
elif command -v pacman >/dev/null 2>&1; then
  install_with_pacman
else
  echo "unsupported system: expected apt-get, dnf, or pacman" >&2
  exit 1
fi

cat <<'DONE'

Vegvisir system dependencies are installed.

Next steps from the repository root:
  ./install.sh

Useful focused checks:
  cargo --version
  node --version && npm --version
  python3 --version
  cargo check --workspace
  cd components/desktop && npm ci && npm run check && cargo check --manifest-path src-tauri/Cargo.toml
DONE
