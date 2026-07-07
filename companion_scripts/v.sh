#!/usr/bin/env bash
set -euo pipefail

# Vegvisir companion script dispatcher.
# Usage:
#   ./companion_scripts/v.sh <command> [args...]
#   ./companion_scripts/v.sh help
#   ./companion_scripts/v.sh list [category]
#   ./companion_scripts/v.sh categories
#   ./companion_scripts/v.sh search <term>
#   ./companion_scripts/v.sh risks
#
# Command resolution:
#   - `git-status`   -> `companion_scripts/vgit-status.sh`
#   - `vgit-status`  -> `companion_scripts/vgit-status.sh`
#   - `run-latest`   -> `companion_scripts/vrun-latest.sh`
#   - `repo-query`   -> `companion_scripts/vrepo-map.sh --query ...`
#   - `repo-symbol`  -> `companion_scripts/vrepo-map.sh --symbol ...`
#   - `rq`           -> `companion_scripts/vrepo-map.sh --query ...`
#   - `rs`           -> `companion_scripts/vrepo-map.sh --symbol ...`

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPTS_DIR="$DIR"
MANIFEST="$DIR/manifest.tsv"

usage() {
  cat <<'USAGE'
Vegvisir companion dispatcher

Usage:
  ./companion_scripts/v.sh <command> [args...]
  ./companion_scripts/v.sh help
  ./companion_scripts/v.sh list [category]
  ./companion_scripts/v.sh categories
  ./companion_scripts/v.sh search <term>
  ./companion_scripts/v.sh risks

Examples:
  ./companion_scripts/v.sh git-status
  ./companion_scripts/v.sh doctor
  ./companion_scripts/v.sh manifest --category hbse
  ./companion_scripts/v.sh list runs
  ./companion_scripts/v.sh search memory
  ./companion_scripts/v.sh run-latest
  ./companion_scripts/v.sh hbse-search hbse
  ./companion_scripts/v.sh skill-route <bundle> <query>
  ./companion_scripts/v.sh repo-map [--update|--diff <snapshot-dir>|--query <term>|--symbol <name>] [path] [out-dir]
  ./companion_scripts/v.sh repo-query dispatcher
  ./companion_scripts/v.sh repo-symbol run_once
  ./companion_scripts/v.sh rq dispatcher
  ./companion_scripts/v.sh rs run_once

Resolution rules:
  - If the command already starts with `v`, use it directly.
  - Otherwise, prepend `v`.
  - The dispatcher then looks for `companion_scripts/<name>.sh`.

Categories:
  dispatcher, maintenance, workspace, runs, subagents, cms, hbse, approvals, skills

Risk labels:
  read-only, redacted-output, runs-tests, writes-generated-artifacts
USAGE
}

list_commands_plain() {
  find "$SCRIPTS_DIR" -maxdepth 1 -type f -name 'v*.sh' -printf '%f\n' \
    | sed 's/\.sh$//' \
    | sort
}

list_manifest() {
  local category="${1:-}"
  if [[ -f "$MANIFEST" ]]; then
    awk -F '\t' -v category="$category" '
      NR == 1 { next }
      category == "" || $2 == category { printf "  - %-26s %-12s %-26s %s\n", $1, $2, $3, $6 }
    ' "$MANIFEST"
  else
    list_commands_plain | sed 's/^/  - /'
  fi
}

list_categories() {
  if [[ -f "$MANIFEST" ]]; then
    awk -F '\t' 'NR > 1 { print $2 }' "$MANIFEST" | sort -u | sed 's/^/  - /'
  else
    echo "  - manifest unavailable"
  fi
}

list_risks() {
  if [[ -f "$MANIFEST" ]]; then
    awk -F '\t' 'NR > 1 { count[$3]++ } END { for (risk in count) printf "  - %-28s %s command(s)\n", risk, count[risk] }' "$MANIFEST" | sort
  else
    echo "  - manifest unavailable"
  fi
}

search_manifest() {
  local term="$1"
  if [[ -f "$MANIFEST" ]]; then
    awk -F '\t' -v term="$term" '
      BEGIN { q=tolower(term) }
      NR == 1 { next }
      index(tolower($0), q) { printf "  - %-26s %-12s %-26s %s\n", $1, $2, $3, $6 }
    ' "$MANIFEST"
  else
    list_commands_plain | grep -i -- "$term" | sed 's/^/  - /'
  fi
}

cmd="${1:-}"
if [[ -z "$cmd" || "$cmd" == "help" || "$cmd" == "-h" || "$cmd" == "--help" ]]; then
  usage
  echo
  echo "Available commands:"
  list_manifest
  exit 0
fi

shift || true

case "$cmd" in
  list|commands)
    list_manifest "${1:-}"
    exit 0
    ;;
  categories)
    list_categories
    exit 0
    ;;
  risks|risk)
    list_risks
    exit 0
    ;;
  search|find)
    term="${1:-}"
    [[ -n "$term" ]] || { echo "Usage: $0 search <term>" >&2; exit 1; }
    search_manifest "$term"
    exit 0
    ;;
esac

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

if [[ ! -x "$script_path" ]]; then
  echo "Command exists but is not executable: $script_path" >&2
  echo "Run: chmod +x '$script_path'" >&2
  exit 126
fi

exec "$script_path" "${extra_args[@]}" "$@"
