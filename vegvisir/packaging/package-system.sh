#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Create a Vegvisir system-install source bundle.

Usage:
  ./packaging/package-system.sh [options]

Options:
  --output-dir <path>       Output directory. Default: target/dist
  --name <name>             Package directory/archive name.
  --no-vendor               Do not vendor crates.io dependencies.
  --cms-root <path>         CMS-v2 source root. Default: /mnt/storage/Projects/CMS-v2
  --hbse-rust-root <path>   HBSE rust source root. Default: /mnt/storage/Projects/HBSE/rust
  --usrl-root <path>        USRL source root. Default: /mnt/storage/Projects/USRL
  -h, --help                Show this help.
USAGE
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$repo_root/target/dist"
name="vegvisir-system-$(date -u +%Y%m%d%H%M%S)-$(uname -m)-linux"
vendor=1
cms_root="/mnt/storage/Projects/CMS-v2"
hbse_rust_root="/mnt/storage/Projects/HBSE/rust"
usrl_root="/mnt/storage/Projects/USRL"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-dir)
      output_dir="${2:?--output-dir requires a path}"
      shift 2
      ;;
    --name)
      name="${2:?--name requires a value}"
      shift 2
      ;;
    --no-vendor)
      vendor=0
      shift
      ;;
    --cms-root)
      cms_root="${2:?--cms-root requires a path}"
      shift 2
      ;;
    --hbse-rust-root)
      hbse_rust_root="${2:?--hbse-rust-root requires a path}"
      shift 2
      ;;
    --usrl-root)
      usrl_root="${2:?--usrl-root requires a path}"
      shift 2
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

for required in "$repo_root/Cargo.toml" "$cms_root/Cargo.toml" "$hbse_rust_root/Cargo.toml" "$usrl_root/package.json"; do
  if [[ ! -f "$required" ]]; then
    echo "required dependency source not found: $required" >&2
    exit 1
  fi
done

package_dir="$output_dir/$name"
archive="$output_dir/$name.tar.gz"
rm -rf "$package_dir" "$archive"
mkdir -p "$package_dir/app" "$package_dir/third_party/CMS-v2" "$package_dir/third_party/HBSE/rust" "$package_dir/third_party/USRL"

copy_tree() {
  local src="$1"
  local dst="$2"
  shift 2
  tar -C "$src" "$@" -cf - . | tar -C "$dst" -xf -
}

copy_tree "$repo_root" "$package_dir/app" \
  --exclude='./.git' \
  --exclude='./.vegvisir' \
  --exclude='./target'

copy_tree "$cms_root" "$package_dir/third_party/CMS-v2" \
  --exclude='./.git' \
  --exclude='./target' \
  --exclude='./cms.sqlite3'

copy_tree "$hbse_rust_root" "$package_dir/third_party/HBSE/rust" \
  --exclude='./.git' \
  --exclude='./target'

copy_tree "$usrl_root" "$package_dir/third_party/USRL" \
  --exclude='./.git' \
  --exclude='./.claude' \
  --exclude='./.codex'

cp "$repo_root/packaging/install.sh" "$package_dir/install.sh"
cp "$repo_root/packaging/uninstall.sh" "$package_dir/uninstall.sh"
chmod 0755 "$package_dir/install.sh" "$package_dir/uninstall.sh"

perl -0pi -e 's#cms-v2\s*=\s*\{\s*path\s*=\s*"[^"]+"\s*\}#cms-v2 = { path = "../third_party/CMS-v2" }#' "$package_dir/app/Cargo.toml"

cat >"$package_dir/README-INSTALL.md" <<'README'
# Vegvisir System Install Bundle

This bundle contains Vegvisir, CMS-v2, HBSE Rust, USRL, Binary Intelligence Workbench, and optional vendored Cargo dependencies.

Install:

```bash
./install.sh --prefix "$HOME/.local"
```

Install with HBSE user service:

```bash
./install.sh --prefix "$HOME/.local" --hbse-service user --enable-hbse-service --start-hbse-service
```

If native dependencies are missing on Debian-like systems:

```bash
./install.sh --install-system-deps --prefix "$HOME/.local"
```

After install, source or copy the environment example from:

```text
$PREFIX/etc/vegvisir/vegvisir.env.example
```
README

if [[ "$vendor" -eq 1 ]]; then
  mkdir -p "$package_dir/.cargo"
  (
    cd "$package_dir"
    cargo vendor \
      --manifest-path app/Cargo.toml \
      --sync third_party/CMS-v2/Cargo.toml \
      --sync third_party/HBSE/rust/Cargo.toml \
      vendor > .cargo/config.toml
  )
fi

cat >"$package_dir/MANIFEST.txt" <<EOF
Vegvisir source: app
CMS-v2 source: third_party/CMS-v2
HBSE Rust source: third_party/HBSE/rust
USRL source: third_party/USRL
Binary Intelligence Workbench source: app/components/binary-intelligence-workbench
Solarium source: app/components/solarium
Cargo vendor: $([[ "$vendor" -eq 1 ]] && echo vendor || echo not included)
Installer: install.sh
Uninstaller: uninstall.sh
EOF

tar -C "$output_dir" -czf "$archive" "$name"

echo "Created:"
echo "  $package_dir"
echo "  $archive"
