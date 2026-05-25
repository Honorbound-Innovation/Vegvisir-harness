#!/usr/bin/env bash
set -euo pipefail

readonly DEFAULT_REPO_URL="https://github.com/Honorbound-Innovation/Vegvisir-harness.git"
readonly DEFAULT_BRANCH="main"

usage() {
  cat <<'USAGE'
Upgrade Vegvisir Agent Harness from GitHub and run the downloaded install.sh.

Usage:
  ./upgrade.sh [upgrade options] [-- install.sh options]

Upgrade options:
  --repo-url <url>       Git repository to upgrade from.
                         Default: https://github.com/Honorbound-Innovation/Vegvisir-harness.git
  --branch <name>        Branch to upgrade from. Default: main
  --download-root <path> Directory used for the temporary clone.
                         Default: ${TMPDIR:-/tmp}
  --force               Download and reinstall even when the local checkout is already current.
  --keep-download       Keep the downloaded source directory after install.
  --dry-run             Check versions and print what would happen, but do not download/install.
  -h, --help            Show this help.

Everything after -- is passed directly to the downloaded install.sh.
Examples:
  ./upgrade.sh
  ./upgrade.sh -- --prefix "$HOME/.local" --no-usrl
  ./upgrade.sh --force -- --hbse-service user --enable-hbse-service --start-hbse-service

Notes:
  - The script checks the remote branch with git ls-remote.
  - If run from a Git checkout, it compares the current HEAD to the remote branch.
  - If run outside a Git checkout, it cannot know the installed source revision, so it downloads
    and reinstalls the requested branch unless --dry-run is used.
USAGE
}

repo_url="$DEFAULT_REPO_URL"
branch="$DEFAULT_BRANCH"
download_root="${TMPDIR:-/tmp}"
force=0
keep_download=0
dry_run=0
install_args=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo-url)
      repo_url="${2:?--repo-url requires a URL}"
      shift 2
      ;;
    --branch)
      branch="${2:?--branch requires a branch name}"
      shift 2
      ;;
    --download-root)
      download_root="${2:?--download-root requires a path}"
      shift 2
      ;;
    --force)
      force=1
      shift
      ;;
    --keep-download)
      keep_download=1
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
    --)
      shift
      install_args+=("$@")
      break
      ;;
    *)
      # Treat unknown options as install.sh options for convenience/backward compatibility.
      install_args+=("$1")
      shift
      ;;
  esac
done

require_cmd() {
  local name="$1"
  if ! command -v "$name" >/dev/null 2>&1; then
    echo "required command not found: $name" >&2
    exit 1
  fi
}

short_sha() {
  local sha="${1:-}"
  if [[ -z "$sha" ]]; then
    printf 'unknown'
  else
    printf '%s' "${sha:0:12}"
  fi
}

current_script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

require_cmd git
require_cmd mktemp

mkdir -p "$download_root"

remote_ref="refs/heads/$branch"
remote_sha="$(git ls-remote --heads "$repo_url" "$branch" | awk '{print $1}' | head -n 1)"
if [[ -z "$remote_sha" ]]; then
  echo "could not find branch '$branch' at $repo_url" >&2
  exit 1
fi

local_sha=""
if git -C "$current_script_dir" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  local_sha="$(git -C "$current_script_dir" rev-parse HEAD 2>/dev/null || true)"
fi

cat <<EOF
Vegvisir upgrade check:
  Repository: $repo_url
  Branch:     $branch
  Remote:     $(short_sha "$remote_sha")
  Local:      $(short_sha "$local_sha")
EOF

if [[ "$force" -eq 0 && -n "$local_sha" && "$local_sha" == "$remote_sha" ]]; then
  echo "Already up to date. Use --force to reinstall anyway."
  exit 0
fi

if [[ "$dry_run" -eq 1 ]]; then
  if [[ -z "$local_sha" ]]; then
    echo "Dry run: would download $repo_url branch $branch and run install.sh."
  else
    echo "Dry run: would upgrade from $(short_sha "$local_sha") to $(short_sha "$remote_sha") and run install.sh."
  fi
  exit 0
fi

clone_dir="$(mktemp -d "$download_root/vegvisir-upgrade.XXXXXX")"
cleanup() {
  if [[ "$keep_download" -eq 0 ]]; then
    rm -rf "$clone_dir"
  else
    echo "Kept downloaded source at: $clone_dir"
  fi
}
trap cleanup EXIT

echo "Downloading latest Vegvisir source..."
git clone --depth 1 --branch "$branch" "$repo_url" "$clone_dir"

if [[ ! -x "$clone_dir/install.sh" ]]; then
  if [[ -f "$clone_dir/install.sh" ]]; then
    chmod +x "$clone_dir/install.sh"
  else
    echo "downloaded repository does not contain install.sh" >&2
    exit 1
  fi
fi

downloaded_sha="$(git -C "$clone_dir" rev-parse HEAD 2>/dev/null || true)"
if [[ -n "$downloaded_sha" && "$downloaded_sha" != "$remote_sha" ]]; then
  echo "warning: downloaded revision $(short_sha "$downloaded_sha") differs from expected remote $(short_sha "$remote_sha")" >&2
fi

cat <<EOF
Running downloaded install.sh:
  Source:  $clone_dir
  Version: $(short_sha "${downloaded_sha:-$remote_sha}")
EOF

"$clone_dir/install.sh" "${install_args[@]}"

echo "Upgrade complete."
