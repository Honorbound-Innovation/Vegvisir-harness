#!/usr/bin/env bash
set -euo pipefail

# Vegvisir script dispatcher.
# Usage:
#   ./v.sh <command> [args...]
#   ./v.sh help
#
# Command resolution:
#   - `git-status`   -> `scripts/vgit-status.sh`
#   - `vgit-status`  -> `scripts/vgit-status.sh`
#   - `run-latest`   -> `scripts/vrun-latest.sh`
#   - `vrun-latest`  -> `scripts/vrun-latest.sh`
#   - `repo-map`     -> `scripts/vrepo-map.sh`
#   - `repo-query`   -> `scripts/vrepo-map.sh --query ...`
#   - `repo-symbol`  -> `scripts/vrepo-map.sh --symbol ...`
#   - `rq`           -> `scripts/vrepo-map.sh --query ...`
#   - `rs`           -> `scripts/vrepo-map.sh --symbol ...`

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPTS_DIR="$DIR"

usage() {
  cat <<'EOF'
Vegvisir dispatcher

Usage:
  ./v.sh <command> [args...]
  ./v.sh help

Examples:
  ./v.sh git-status
  ./v.sh run-latest
  ./v.sh hbse-search hbse
  ./v.sh skill-route <bundle> <query>
  ./v.sh repo-map [--update|--diff <snapshot-dir>|--query <term>|--symbol <name>] [path] [out-dir]
  ./v.sh repo-query dispatcher
  ./v.sh repo-symbol vsh
  ./v.sh rq dispatcher
  ./v.sh rs vsh

Resolution rules:
  - If the command already starts with `v`, use it directly.
  - Otherwise, prepend `v`.
  - The dispatcher then looks for `scripts/<name>.sh`.
EOF
}

list_commands() {
  find "$SCRIPTS_DIR" -maxdepth 1 -type f -name 'v*.sh' -printf '%f\n' \
    | sed 's/\.sh$//' \
    | sort
}

cmd="${1:-}"
if [[ -z "$cmd" || "$cmd" == "help" || "$cmd" == "-h" || "$cmd" == "--help" ]]; then
  usage
  echo
  echo "Available commands:"
  list_commands | sed 's/^/  - /'
  exit 0
fi

shift || true

extra_args=()
case "$cmd" in
  repo-map|map)
    script_name="vrepo-map.sh"
    ;;
  repo-query|rq)
    script_name="vrepo-map.sh"
    if [[ $# -lt 1 ]]; then
      echo "Usage: $0 $cmd <query> [path] [out-dir]" >&2
      exit 1
    fi
    extra_args=(--query "$1")
    shift
    ;;
  repo-symbol|rs)
    script_name="vrepo-map.sh"
    if [[ $# -lt 1 ]]; then
      echo "Usage: $0 $cmd <symbol> [path] [out-dir]" >&2
      exit 1
    fi
    extra_args=(--symbol "$1")
    shift
    ;;
  *)
    if [[ "$cmd" == v* ]]; then
      script_name="$cmd.sh"
    else
      script_name="v${cmd}.sh"
    fi
    ;;
esac

script_path="$SCRIPTS_DIR/$script_name"
if [[ ! -x "$script_path" && -f "$SCRIPTS_DIR/$cmd.sh" ]]; then
  script_path="$SCRIPTS_DIR/$cmd.sh"
fi

if [[ ! -f "$script_path" ]]; then
  echo "Unknown command: $cmd" >&2
  echo "Run: $0 help" >&2
  exit 1
fi

exec "$script_path" "${extra_args[@]}" "$@"
