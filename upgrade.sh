#!/usr/bin/env bash
set -euo pipefail

readonly DEFAULT_REPO_URL="https://github.com/Honorbound-Innovation/Vegvisir-harness.git"
readonly DEFAULT_BRANCH="main"

usage() {
  cat <<'USAGE'
Upgrade Vegvisir Agent Harness from GitHub and run install.sh from the updated source.

Usage:
  ./upgrade.sh [upgrade options] [-- install.sh options]

Upgrade options:
  --repo-url <url>       Git repository to upgrade from.
                         Default: https://github.com/Honorbound-Innovation/Vegvisir-harness.git
  --branch <name>        Branch to upgrade from. Default: main
  --download-root <path> Directory used for the temporary clone when not upgrading
                         an existing checkout. Default: ${TMPDIR:-/tmp}
  --force               Reinstall even when the local checkout is already current.
  --no-sync-checkout    Do not update the current Git checkout; use a temporary
                         clone instead. This is the old upgrade behavior.
  --keep-download       Keep the downloaded source directory after install.
  --dry-run             Check versions and print what would happen, but do not sync/install.
  -h, --help            Show this help.

Everything after -- is passed directly to install.sh.
Examples:
  ./upgrade.sh
  ./upgrade.sh -- --prefix "$HOME/.local" --no-usrl
  ./upgrade.sh --force -- --hbse-service user --enable-hbse-service --start-hbse-service

Notes:
  - When run inside a Git checkout for the requested repository, this script now fetches
    and fast-forwards that checkout before installing. That keeps the repository on disk
    at the same revision as the installed binaries/features.
  - Local uncommitted changes are never overwritten. Commit or stash them before upgrading.
  - If the local branch has diverged from the remote branch, the script stops instead of
    rewriting history. Resolve the branch manually, then rerun upgrade.sh.
  - When run outside a matching Git checkout, it falls back to a temporary clone.
USAGE
}

repo_url="$DEFAULT_REPO_URL"
branch="$DEFAULT_BRANCH"
download_root="${TMPDIR:-/tmp}"
force=0
sync_checkout=1
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
    --no-sync-checkout)
      sync_checkout=0
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

normalize_git_url() {
  local url="${1:-}"
  url="${url#git+}"
  url="${url%.git}"
  printf '%s' "$url"
}

same_git_url() {
  [[ "$(normalize_git_url "$1")" == "$(normalize_git_url "$2")" ]]
}

find_matching_remote() {
  local repo_dir="$1"
  local remote name url
  while IFS= read -r remote; do
    name="${remote%%$'\t'*}"
    url="${remote#*$'\t'}"
    if same_git_url "$url" "$repo_url"; then
      printf '%s' "$name"
      return 0
    fi
  done < <(git -C "$repo_dir" remote -v | awk '$3 == "(fetch)" {print $1 "\t" $2}')
  return 1
}

current_script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

require_cmd git
require_cmd mktemp

mkdir -p "$download_root"

remote_sha="$(git ls-remote --heads "$repo_url" "$branch" | awk '{print $1}' | head -n 1)"
if [[ -z "$remote_sha" ]]; then
  echo "could not find branch '$branch' at $repo_url" >&2
  exit 1
fi

repo_root=""
local_sha=""
matching_remote=""
if git -C "$current_script_dir" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  repo_root="$(git -C "$current_script_dir" rev-parse --show-toplevel)"
  local_sha="$(git -C "$repo_root" rev-parse HEAD 2>/dev/null || true)"
  matching_remote="$(find_matching_remote "$repo_root" || true)"
fi

cat <<EOF
Vegvisir upgrade check:
  Repository: $repo_url
  Branch:     $branch
  Remote:     $(short_sha "$remote_sha")
  Local:      $(short_sha "$local_sha")
EOF

use_checkout=0
if [[ "$sync_checkout" -eq 1 && -n "$repo_root" && -n "$matching_remote" ]]; then
  use_checkout=1
  echo "  Checkout:   $repo_root"
  echo "  Remote:     $matching_remote"
elif [[ "$sync_checkout" -eq 1 && -n "$repo_root" && -z "$matching_remote" ]]; then
  echo "Current checkout does not have a fetch remote matching $repo_url; using a temporary clone."
fi

if [[ "$dry_run" -eq 1 ]]; then
  if [[ "$use_checkout" -eq 1 ]]; then
    if [[ -z "$local_sha" ]]; then
      echo "Dry run: would fetch $matching_remote/$branch, fast-forward checkout, and run install.sh."
    elif [[ "$local_sha" == "$remote_sha" ]]; then
      if [[ "$force" -eq 1 ]]; then
        echo "Dry run: checkout is current; would reinstall from $repo_root because --force was provided."
      else
        echo "Dry run: checkout is already current; no install would run."
      fi
    else
      echo "Dry run: would update checkout from $(short_sha "$local_sha") to $(short_sha "$remote_sha") and run install.sh."
    fi
  else
    echo "Dry run: would download $repo_url branch $branch to a temporary directory and run install.sh."
  fi
  exit 0
fi

install_source=""
cleanup() { :; }
trap cleanup EXIT

if [[ "$use_checkout" -eq 1 ]]; then
  if [[ -n "$(git -C "$repo_root" status --porcelain)" ]]; then
    echo "local checkout has uncommitted changes; refusing to upgrade in place." >&2
    echo "Commit or stash changes, then rerun upgrade.sh. To use a temporary clone instead, pass --no-sync-checkout." >&2
    exit 1
  fi

  echo "Fetching latest Vegvisir source into current checkout..."
  git -C "$repo_root" fetch --prune "$matching_remote" "+refs/heads/$branch:refs/remotes/$matching_remote/$branch"
  fetched_sha="$(git -C "$repo_root" rev-parse "refs/remotes/$matching_remote/$branch")"
  if [[ "$fetched_sha" != "$remote_sha" ]]; then
    echo "warning: fetched revision $(short_sha "$fetched_sha") differs from expected remote $(short_sha "$remote_sha")" >&2
  fi

  current_branch="$(git -C "$repo_root" branch --show-current || true)"
  if [[ "$current_branch" != "$branch" ]]; then
    if git -C "$repo_root" show-ref --verify --quiet "refs/heads/$branch"; then
      git -C "$repo_root" checkout "$branch"
    else
      git -C "$repo_root" checkout -b "$branch" --track "$matching_remote/$branch"
    fi
  fi

  local_sha="$(git -C "$repo_root" rev-parse HEAD)"
  if [[ "$local_sha" == "$remote_sha" ]]; then
    if [[ "$force" -eq 0 ]]; then
      echo "Already up to date. Use --force to reinstall anyway."
      exit 0
    fi
  else
    merge_base="$(git -C "$repo_root" merge-base HEAD "$matching_remote/$branch")"
    if [[ "$merge_base" != "$local_sha" ]]; then
      echo "local branch has diverged from $matching_remote/$branch; refusing to rewrite it." >&2
      echo "Resolve the branch manually, or rerun with --no-sync-checkout to install from a temporary clone only." >&2
      exit 1
    fi
    git -C "$repo_root" merge --ff-only "$matching_remote/$branch"
  fi
  install_source="$repo_root"
else
  clone_dir="$(mktemp -d "$download_root/vegvisir-upgrade.XXXXXX")"
  cleanup() {
    if [[ "$keep_download" -eq 0 ]]; then
      rm -rf "$clone_dir"
    else
      echo "Kept downloaded source at: $clone_dir"
    fi
  }

  echo "Downloading latest Vegvisir source..."
  git clone --depth 1 --branch "$branch" "$repo_url" "$clone_dir"
  install_source="$clone_dir"
fi

if [[ ! -x "$install_source/install.sh" ]]; then
  if [[ -f "$install_source/install.sh" ]]; then
    chmod +x "$install_source/install.sh"
  else
    echo "upgrade source does not contain install.sh" >&2
    exit 1
  fi
fi

installed_sha="$(git -C "$install_source" rev-parse HEAD 2>/dev/null || true)"
if [[ -n "$installed_sha" && "$installed_sha" != "$remote_sha" ]]; then
  echo "warning: install source revision $(short_sha "$installed_sha") differs from expected remote $(short_sha "$remote_sha")" >&2
fi

cat <<EOF
Running install.sh:
  Source:  $install_source
  Version: $(short_sha "${installed_sha:-$remote_sha}")
EOF

"$install_source/install.sh" "${install_args[@]}"

echo "Upgrade complete."
if [[ "$use_checkout" -eq 1 ]]; then
  echo "Checkout and installed binaries are now on $(short_sha "${installed_sha:-$remote_sha}")."
fi
