#!/usr/bin/env bash
# Shared helpers for Vegvisir demo scripts.
# These scripts are intentionally local-first and safe-by-default.

set -euo pipefail

DEMO_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEMO_ROOT="$(cd "${DEMO_SCRIPT_DIR}/.." && pwd)"
VEGVISIR_ROOT="${VEGVISIR_ROOT:-$(cd "${DEMO_ROOT}/.." && pwd)}"
MSP_ROOT="${MSP_ROOT:-/mnt/storage/Projects/MSP}"
ARTIFACT_ROOT="${ARTIFACT_ROOT:-${DEMO_ROOT}/artifacts}"

mkdir -p "${ARTIFACT_ROOT}"

section() {
  printf '\n\033[1;36m==> %s\033[0m\n' "$*"
}

note() {
  printf '\033[0;33mnote:\033[0m %s\n' "$*"
}

fail() {
  printf '\033[0;31merror:\033[0m %s\n' "$*" >&2
  exit 1
}

run() {
  printf '\n\033[1;32m$\033[0m '
  printf '%q ' "$@"
  printf '\n'
  "$@"
}

require_dir() {
  local path="$1"
  local label="$2"
  [[ -d "${path}" ]] || fail "${label} not found: ${path}"
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

vegvisir() {
  (cd "${VEGVISIR_ROOT}" && cargo run -q -p vegvisir-rust --bin vegvisir-rust -- "$@")
}

msp() {
  (cd "${MSP_ROOT}" && cargo run -q -p msp-cli -- "$@")
}

require_common() {
  require_cmd cargo
  require_dir "${VEGVISIR_ROOT}" "Vegvisir root"
  require_dir "${MSP_ROOT}" "MSP root"
  [[ -f "${VEGVISIR_ROOT}/Cargo.toml" ]] || fail "Vegvisir Cargo.toml missing under ${VEGVISIR_ROOT}"
  [[ -f "${MSP_ROOT}/Cargo.toml" ]] || fail "MSP Cargo.toml missing under ${MSP_ROOT}"
}

maybe_live() {
  if [[ "${RUN_LIVE:-0}" != "1" ]]; then
    note "RUN_LIVE=1 not set; printing the live command instead of invoking a provider-backed agent run."
    return 1
  fi
  return 0
}
