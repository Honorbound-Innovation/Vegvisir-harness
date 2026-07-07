#!/usr/bin/env bash
set -euo pipefail

# Fast Vegvisir-oriented pre-commit quality checks.
# Usage: ./vprecommit.sh [--quick|--strict|--full] [--worktree] [--no-secret-scan] [--serial-rust-tests]

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

usage() {
  cat <<'USAGE'
Usage: vprecommit.sh [--quick|--strict|--full] [--worktree] [--no-secret-scan] [--serial-rust-tests]

Modes:
  --quick   Fast default for git hooks. Checks staged whitespace, staged shell
            syntax, and staged-file secret scan only when files are staged.
  --strict  Quick checks plus companion-wide vdoctor --strict.
  --full    Strict checks plus project-level tests through vtest.sh.

Options:
  --worktree           Also check unstaged/untracked changed files where useful.
                       This is intentionally off by default for hook speed and
                       to avoid blocking commits on unrelated unstaged work.
  --no-secret-scan     Skip staged secret scan.
  --serial-rust-tests  With --full, run Rust tests with RUST_TEST_THREADS=1
                       when the caller has not already set it.

The default is intentionally fast. Expensive whole-repo/project tests only run
when explicitly requested with --full.
USAGE
}

mode="quick"
secret_scan=1
serial_rust_tests=0
include_worktree=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --quick) mode="quick" ;;
    --strict) mode="strict" ;;
    --full) mode="full" ;;
    --worktree|--include-worktree) include_worktree=1 ;;
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

read_git_files() {
  "$@" 2>/dev/null | sort -u || true
}

mapfile -t staged_files < <(read_git_files git diff --cached --name-only --diff-filter=ACMR)
check_files=("${staged_files[@]}")
if (( include_worktree == 1 )); then
  mapfile -t worktree_files < <(
    {
      git diff --name-only --diff-filter=ACMR 2>/dev/null || true
      git ls-files --others --exclude-standard 2>/dev/null || true
    } | sort -u
  )
  check_files+=("${worktree_files[@]}")
fi

# De-duplicate file list while preserving simple Bash-only portability.
if (( ${#check_files[@]} > 0 )); then
  mapfile -t check_files < <(printf '%s\n' "${check_files[@]}" | sort -u)
fi

run_check "git status" git status --short --branch
run_check "git diff --cached --check" git diff --cached --check
if (( include_worktree == 1 )); then
  run_check "git diff --check" git diff --check
else
  printf '\n== git diff --check ==\n'
  echo "skipped; use --worktree to check unstaged whitespace"
fi

# Fast path: syntax-check only staged shell files by default. With --worktree,
# include unstaged/untracked shell files too.
shell_targets=()
for file in "${check_files[@]}"; do
  [[ -f "$file" ]] || continue
  case "$file" in
    *.sh|*/lib/*.sh) shell_targets+=("$file") ;;
  esac
done
if (( ${#shell_targets[@]} > 0 )); then
  run_check "bash syntax (selected shell files)" bash -n "${shell_targets[@]}"
else
  printf '\n== bash syntax (selected shell files) ==\n'
  if (( include_worktree == 1 )); then
    echo "skipped; no staged/changed shell files"
  else
    echo "skipped; no staged shell files"
  fi
fi

if [[ "$mode" == "strict" || "$mode" == "full" ]]; then
  [[ -x "$DIR/vdoctor.sh" ]] || { echo "Required helper is missing or not executable: $DIR/vdoctor.sh" >&2; exit 127; }
  run_check "companion doctor" "$DIR/vdoctor.sh" --strict
else
  printf '\n== companion doctor ==\n'
  echo "skipped in --quick mode; run '$0 --strict' or '$0 --full' for whole companion-suite validation"
fi

if (( secret_scan == 1 )); then
  if (( ${#staged_files[@]} > 0 )); then
    [[ -x "$DIR/vsecret-scan.sh" ]] || { echo "Required helper is missing or not executable: $DIR/vsecret-scan.sh" >&2; exit 127; }
    run_check "staged secret scan" "$DIR/vsecret-scan.sh" --staged
  else
    printf '\n== staged secret scan ==\n'
    echo "skipped; no staged files"
  fi
else
  printf '\n== staged secret scan ==\n'
  echo "skipped by --no-secret-scan"
fi

if [[ "$mode" == "full" ]]; then
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
  echo "skipped; run '$0 --full' for project tests"
fi

printf '\nvprecommit: OK (%s)\n' "$mode"
