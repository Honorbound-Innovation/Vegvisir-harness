#!/usr/bin/env bash
set -euo pipefail

# Build a bounded, redacted Vegvisir reproduction evidence bundle.
# Usage: ./vrepro.sh [--last-run|--run DIR] [--out DIR] [--include-diff] [--command CMD]

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$DIR/lib/common.sh"

usage() {
  cat <<'USAGE'
Usage: vrepro.sh [--last-run|--run DIR] [--out DIR] [--include-diff] [--command CMD]

Creates a redacted evidence bundle under .vegvisir/repro by default. It records
repo metadata, git state, companion health, selected run summaries, and optional
command output. It does not copy raw secret-bearing files and redacts common
secret/content fields in captured excerpts.
USAGE
}

run_selector=""
out_root=".vegvisir/repro"
include_diff=0
command_to_run=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --last-run) run_selector="latest" ;;
    --run)
      shift || { echo "--run requires a directory" >&2; exit 1; }
      run_selector="$1"
      ;;
    --out)
      shift || { echo "--out requires a directory" >&2; exit 1; }
      out_root="$1"
      ;;
    --include-diff) include_diff=1 ;;
    --command)
      shift || { echo "--command requires a shell command string" >&2; exit 1; }
      command_to_run="$1"
      ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 1 ;;
  esac
  shift || true
done

resolve_latest_run() {
  local root=".vegvisir/runs"
  [[ -d "$root" ]] || return 1
  find "$root" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' 2>/dev/null | sort -nr | awk 'NR==1 {print $2}'
}

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
out_dir="$out_root/repro-$timestamp"
mkdir -p "$out_dir"

{
  echo "created_utc=$timestamp"
  echo "repo_root=$(vs_repo_root)"
  git rev-parse --is-inside-work-tree >/dev/null 2>&1 && {
    echo "branch=$(git branch --show-current 2>/dev/null || true)"
    echo "commit=$(git rev-parse --short HEAD 2>/dev/null || true)"
    echo "upstream=$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || true)"
  }
} > "$out_dir/metadata.txt"

if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  git status --short --branch > "$out_dir/git-status.txt" 2>&1 || true
  git diff --stat --summary --ignore-submodules > "$out_dir/git-diffstat.txt" 2>&1 || true
  git diff --cached --stat --summary --ignore-submodules > "$out_dir/git-staged-diffstat.txt" 2>&1 || true
  if (( include_diff == 1 )); then
    git diff --ignore-submodules | vs_redact_secret_stream > "$out_dir/git-diff-redacted.patch" 2>&1 || true
    git diff --cached --ignore-submodules | vs_redact_secret_stream > "$out_dir/git-staged-diff-redacted.patch" 2>&1 || true
  fi
fi

if [[ -x "$DIR/vdoctor.sh" ]]; then
  "$DIR/vdoctor.sh" --strict > "$out_dir/companion-doctor.txt" 2>&1 || true
fi
if [[ -x "$DIR/vcontext-budget.sh" ]]; then
  "$DIR/vcontext-budget.sh" --changed > "$out_dir/context-budget-changed.txt" 2>&1 || true
fi
if [[ -x "$DIR/vsecret-scan.sh" ]]; then
  "$DIR/vsecret-scan.sh" --changed > "$out_dir/secret-scan-changed.txt" 2>&1 || true
fi

if [[ -n "$run_selector" ]]; then
  run_dir="$run_selector"
  [[ "$run_selector" == "latest" ]] && run_dir="$(resolve_latest_run || true)"
  if [[ -n "${run_dir:-}" && -d "$run_dir" ]]; then
    echo "$run_dir" > "$out_dir/run-dir.txt"
    find "$run_dir" -maxdepth 2 -type f -printf '%TY-%Tm-%Td %TH:%TM %10s %p\n' 2>/dev/null | sort > "$out_dir/run-artifacts.txt" || true
    if [[ -x "$DIR/vtrace.sh" ]]; then
      "$DIR/vtrace.sh" "$run_dir" --lines 80 > "$out_dir/run-trace-redacted.txt" 2>&1 || true
    fi
  else
    echo "Run directory not found for selector: $run_selector" > "$out_dir/run-missing.txt"
  fi
fi

if [[ -n "$command_to_run" ]]; then
  {
    echo "command=$command_to_run"
    echo "started_utc=$(date -u +%Y%m%dT%H%M%SZ)"
    set +e
    bash -lc "$command_to_run" 2>&1 | vs_redact_memory_stream | vs_redact_secret_stream
    status=${PIPESTATUS[0]}
    set -e
    echo "exit_status=$status"
    echo "finished_utc=$(date -u +%Y%m%dT%H%M%SZ)"
    exit 0
  } > "$out_dir/command-output-redacted.txt"
fi

cat > "$out_dir/README.txt" <<EOF
Vegvisir repro bundle
Created: $timestamp

This bundle is intended for debugging and should still be reviewed before
sharing externally. Common secret/content fields are redacted, but automated
redaction is not a guarantee.
EOF

printf 'repro_bundle=%s\n' "$out_dir"
