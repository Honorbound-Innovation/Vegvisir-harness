#!/usr/bin/env bash
set -euo pipefail

version="${1:-0.1.0}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
rust_dir="$root/rust"
bundle_dir="$rust_dir/target/hbse-${version}-native-linux"
archive="$rust_dir/target/hbse-${version}-native-linux.tar.gz"

cargo build --manifest-path "$rust_dir/Cargo.toml" --release --quiet

rm -rf "$bundle_dir"
mkdir -p "$bundle_dir/bin" "$bundle_dir/packaging/systemd"

install -m 0755 "$rust_dir/target/release/hbse" "$bundle_dir/bin/hbse"
install -m 0755 "$rust_dir/target/release/hbse-broker" "$bundle_dir/bin/hbse-broker"
install -m 0644 "$root/packaging/systemd/hbse-broker.service" "$bundle_dir/packaging/systemd/hbse-broker.service"
install -m 0644 "$root/packaging/systemd/hbse-broker.socket" "$bundle_dir/packaging/systemd/hbse-broker.socket"

cat > "$bundle_dir/README.md" <<EOF
# HBSE Native Linux Bundle

This bundle contains:

- \`bin/hbse\`
- \`bin/hbse-broker\`
- systemd service/socket templates

Quick smoke:

\`\`\`bash
export PATH="\$PWD/bin:\$PATH"
hbse --help
hbse-broker --help
\`\`\`

Install the binaries into a directory on PATH, then use:

\`\`\`bash
hbse broker install-service --scope user --enable --start
\`\`\`
EOF

(
  cd "$bundle_dir"
  sha256sum bin/hbse bin/hbse-broker packaging/systemd/hbse-broker.service packaging/systemd/hbse-broker.socket > SHA256SUMS
)

tar -C "$(dirname "$bundle_dir")" -czf "$archive" "$(basename "$bundle_dir")"
echo "$archive"
