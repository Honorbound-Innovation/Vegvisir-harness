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

biw_share_dir="$HOME/.local/share/vegvisir/binary-intelligence-workbench"
rm -rf "$biw_share_dir"
mkdir -p "$biw_share_dir"
tar -C components/binary-intelligence-workbench \
  --exclude='.git' \
  --exclude='__pycache__' \
  --exclude='*.pyc' \
  --exclude='.pytest_cache' \
  --exclude='.vegvisir' \
  -cf - . | tar -C "$biw_share_dir" -xf -
cat >"$HOME/.local/bin/biw" <<EOF
#!/usr/bin/env bash
export PYTHONPATH="$biw_share_dir:\${PYTHONPATH:-}"
if [[ -z "\${BIW_GHIDRA_WRAPPER:-}" && -x "\${HOME}/.vegvisir/tools/bin/ghidra-headless" ]]; then
  export BIW_GHIDRA_WRAPPER="\${HOME}/.vegvisir/tools/bin/ghidra-headless"
fi
exec python3 -m biw.cli "\$@"
EOF
chmod 0755 "$HOME/.local/bin/biw"

echo "Installed vegvisir, hbse, hbse-broker, and biw into $HOME/.local/bin"

