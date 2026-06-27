#!/usr/bin/env bash
# Demo 01: Vegvisir fixes itself.
# Safe default: creates a disposable copy of a tiny Rust fixture and asks Vegvisir
# to inspect/fix it only when RUN_LIVE=1. Otherwise prints the exact command.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"
require_common

DEMO_WORKSPACE="${ARTIFACT_ROOT}/01-vegvisir-fixes-itself-workspace"
rm -rf "${DEMO_WORKSPACE}"
mkdir -p "${DEMO_WORKSPACE}/src"

cat > "${DEMO_WORKSPACE}/Cargo.toml" <<'TOML'
[package]
name = "vegvisir-self-fix-demo"
version = "0.1.0"
edition = "2021"

[workspace]

[lib]
path = "src/lib.rs"
TOML

cat > "${DEMO_WORKSPACE}/src/lib.rs" <<'RS'
pub fn normalize_title(input: &str) -> String {
    // Bug: this preserves leading/trailing whitespace.
    input.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_lowercases_title() {
        assert_eq!(normalize_title("  Vegvisir Demo  "), "vegvisir demo");
    }
}
RS

section "Created disposable bug fixture"
run find "${DEMO_WORKSPACE}" -maxdepth 3 -type f -print

section "Show the failing test"
if ! (cd "${DEMO_WORKSPACE}" && cargo test); then
  note "The failure is expected; this is the bug Vegvisir should fix."
fi

GOAL="Fix the failing Rust test in this disposable demo workspace. Inspect the code, make the smallest safe patch, run cargo test, and summarize the diff."

section "Vegvisir live run"
if maybe_live; then
  run vegvisir run --workspace "${DEMO_WORKSPACE}" --max-steps 8 --artifacts --artifact-dir "${ARTIFACT_ROOT}/01-run-artifacts" "${GOAL}"
else
  cat <<EOF
Run this to record the live demo:

  RUN_LIVE=1 VEGVISIR_ROOT='${VEGVISIR_ROOT}' MSP_ROOT='${MSP_ROOT}' \\
    demos/scripts/01-vegvisir-fixes-itself.sh

Equivalent direct command:

  cargo run -q -p vegvisir-rust --bin vegvisir-rust -- \\
    run --workspace '${DEMO_WORKSPACE}' --max-steps 8 --artifacts \\
    --artifact-dir '${ARTIFACT_ROOT}/01-run-artifacts' \\
    '${GOAL}'
EOF
fi
