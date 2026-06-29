#!/usr/bin/env bash
# Shared helpers for Vegvisir companion scripts.
# Source this file from scripts; do not execute it directly.

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  echo "common.sh is a library; source it from a companion script." >&2
  exit 2
fi

vs_script_dir() {
  local src="${BASH_SOURCE[1]:-${BASH_SOURCE[0]}}"
  cd "$(dirname "$src")" && pwd
}

vs_companion_dir() {
  local src="${BASH_SOURCE[1]:-${BASH_SOURCE[0]}}"
  local dir
  dir="$(cd "$(dirname "$src")" && pwd)"
  if [[ "$(basename "$dir")" == "lib" ]]; then
    dirname "$dir"
  else
    printf '%s\n' "$dir"
  fi
}

vs_repo_root() {
  git rev-parse --show-toplevel 2>/dev/null || pwd
}

vs_have_command() {
  command -v "$1" >/dev/null 2>&1
}

vs_require_command() {
  local cmd="$1"
  local hint="${2:-Install it or run this script inside a Vegvisir harness environment.}"
  if ! vs_have_command "$cmd"; then
    printf 'Required command not available: %s\n%s\n' "$cmd" "$hint" >&2
    return 127
  fi
}

vs_is_uint() {
  [[ "${1:-}" =~ ^[0-9]+$ ]]
}

vs_redact_memory_stream() {
  sed -E \
    -e 's/(content:|body:|text:|value:|prompt:|message:|query:).*/\1 <redacted>/Ig' \
    -e 's/("(content|body|text|value|prompt|message|query|goal)"[[:space:]]*:[[:space:]]*)"([^"]|\\")*"/\1"<redacted>"/Ig'
}

vs_print_kv() {
  printf '%s=%s\n' "$1" "$2"
}

vs_redact_secret_stream() {
  sed -E \
    -e 's#(https?://[^:/[:space:]]+):([^@/[:space:]]+)@#\1:<redacted>@#g' \
    -e 's/(Authorization:[[:space:]]*Bearer[[:space:]]+)[A-Za-z0-9._~+\/-]+=*/\1<redacted>/Ig' \
    -e 's/((api[_-]?key|token|password|secret|credential)[[:space:]]*[:=][[:space:]]*)[^[:space:]"'"'"']+/\1<redacted>/Ig' \
    -e 's/(AKIA[0-9A-Z]{4})[0-9A-Z]{12}/\1<redacted>/g'
}

vs_human_bytes() {
  awk -v bytes="${1:-0}" 'BEGIN {
    split("B KiB MiB GiB TiB", units, " ");
    value = bytes + 0;
    idx = 1;
    while (value >= 1024 && idx < 5) { value /= 1024; idx++; }
    if (idx == 1) printf "%d %s\n", value, units[idx];
    else printf "%.1f %s\n", value, units[idx];
  }'
}
