#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo build --workspace --release
install -Dm755 target/release/vegvisir-rust "$HOME/.local/bin/vegvisir"
install -Dm755 target/release/hbse "$HOME/.local/bin/hbse"
install -Dm755 target/release/hbse-broker "$HOME/.local/bin/hbse-broker"

if command -v npm >/dev/null 2>&1; then
  (cd components/usrl && npm ci && npm run build)
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

solarium_share_dir="$HOME/.local/share/vegvisir/solarium"
rm -rf "$solarium_share_dir"
mkdir -p "$solarium_share_dir"
tar -C components/solarium \
  --exclude='.git' \
  --exclude='node_modules' \
  --exclude='dist' \
  --exclude='.solarium' \
  --exclude='.vegvisir' \
  -cf - . | tar -C "$solarium_share_dir" -xf -
if command -v npm >/dev/null 2>&1; then
  npm --prefix "$solarium_share_dir" ci
  npm --prefix "$solarium_share_dir" run build
  cat >"$HOME/.local/bin/solarium" <<EOF
#!/usr/bin/env bash
exec node "$solarium_share_dir/dist/cli/index.js" "\$@"
EOF
  chmod 0755 "$HOME/.local/bin/solarium"
else
  echo "Skipping Solarium install because npm is not available." >&2
fi

desktop_share_dir="$HOME/.local/share/vegvisir/desktop"
rm -rf "$desktop_share_dir"
mkdir -p "$desktop_share_dir"
tar -C components/desktop \
  --exclude='.git' \
  --exclude='node_modules' \
  --exclude='dist' \
  --exclude='src-tauri/target' \
  --exclude='.vegvisir' \
  -cf - . | tar -C "$desktop_share_dir" -xf -
if command -v npm >/dev/null 2>&1; then
  npm --prefix "$desktop_share_dir" ci
  npm --prefix "$desktop_share_dir" run web:build
  cat >"$HOME/.local/bin/vegvisir-desktop" <<EOF
#!/usr/bin/env bash
export VEGVISIR_DESKTOP_RESOURCE_DIR="$desktop_share_dir/resources"
exec npm --prefix "$desktop_share_dir" run dev -- "\$@"
EOF
  chmod 0755 "$HOME/.local/bin/vegvisir-desktop"
else
  echo "Skipping Vegvisir Desktop install because npm is not available." >&2
fi

installed=()
skipped=()
installed+=(vegvisir hbse hbse-broker biw)
if command -v npm >/dev/null 2>&1; then
  installed+=(usrl solarium vegvisir-desktop)
else
  skipped+=(usrl solarium vegvisir-desktop)
fi
echo "Installed ${installed[*]} into $HOME/.local/bin"
if [[ ${#skipped[@]} -gt 0 ]]; then
  echo "Skipped ${skipped[*]} because npm is not available" >&2
fi

