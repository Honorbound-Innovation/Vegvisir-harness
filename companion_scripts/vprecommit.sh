#!/usr/bin/env bash
set -euo pipefail

# Run Vegvisir-oriented pre-commit quality checks.
# Usage: ./vprecommit.sh [--quick|--full] [--no-secret-scan] [--serial-rust-tests]

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

usage() {
  cat <<'USAGE'
Usage: vprecommit.sh [--quick|--full] [--no-secret-scan] [--serial-rust-tests]

Default is --quick: git whitespace checks, Bash syntax, companion doctor, and
staged secret scan. --full also runs project-level test tooling when recognized.
Use --serial-rust-tests to run Rust tests with RUST_TEST_THREADS=1 for
deterministic env-mutating test suites; this can be significantly slower.
USAGE
}

full=0
secret_scan=1
serial_rust_tests=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --quick) full=0 ;;
    --full) full=1 ;;
    --no-secret-scan) secret_scan=0 ;;
    --serial-rust-tests) serial_rust_tests=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 1 ;;
  esac
  shift || true
done

run_check() {
  local name="$1"
  shift
  printf '\n== %s ==\n' "$name"
  "$@"
}

run_check "git status" git status --short --branch
run_check "git diff --check" git diff --check
run_check "git diff --cached --check" git diff --cached --check
run_check "bash syntax" bash -n "$DIR"/*.sh "$DIR"/lib/common.sh

[[ -x "$DIR/vdoctor.sh" ]] || { echo "Required helper is missing or not executable: $DIR/vdoctor.sh" >&2; exit 127; }
run_check "companion doctor" "$DIR/vdoctor.sh" --strict

if (( secret_scan == 1 )); then
  [[ -x "$DIR/vsecret-scan.sh" ]] || { echo "Required helper is missing or not executable: $DIR/vsecret-scan.sh" >&2; exit 127; }
  run_check "staged secret scan" "$DIR/vsecret-scan.sh" --staged
fi

if (( full == 1 )); then
  if [[ -x "$DIR/vtest.sh" ]]; then
    if [[ -f Cargo.toml && "$serial_rust_tests" -eq 1 && -z "${RUST_TEST_THREADS:-}" ]]; then
      run_check "project tests" bash -c 'RUST_TEST_THREADS=1 "$1"' _ "$DIR/vtest.sh"
    else
      run_check "project tests" "$DIR/vtest.sh"
    fi
  else
    echo "vtest.sh not executable; skipping project tests" >&2
  fi
else
  printf '\n== project tests ==\n'
  echo "skipped in --quick mode; run '$0 --full' for project tests"
fi

printf '\nvprecommit: OK\n'
