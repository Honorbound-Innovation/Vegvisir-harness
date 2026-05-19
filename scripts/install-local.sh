#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo build --workspace --release
install -Dm755 target/release/vegvisir-rust "$HOME/.local/bin/vegvisir"
install -Dm755 target/release/hbse "$HOME/.local/bin/hbse"
install -Dm755 target/release/hbse-broker "$HOME/.local/bin/hbse-broker"

if command -v npm >/dev/null 2>&1; then
  (cd components/usrl && npm install && npm run build)
fi

echo "Installed vegvisir, hbse, and hbse-broker into $HOME/.local/bin"

