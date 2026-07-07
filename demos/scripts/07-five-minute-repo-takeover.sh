#!/usr/bin/env bash
# Demo 07: Five-minute repo takeover.
# Safe default creates a small unknown repo fixture and prints the live command.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"
require_common

DEMO_WORKSPACE="${DEMO_WORKSPACE:-${ARTIFACT_ROOT}/07-five-minute-repo}"
if [[ ! -d "${DEMO_WORKSPACE}" || "${DEMO_WORKSPACE}" == "${ARTIFACT_ROOT}/07-five-minute-repo" ]]; then
  rm -rf "${DEMO_WORKSPACE}"
  mkdir -p "${DEMO_WORKSPACE}/src"
  cat > "${DEMO_WORKSPACE}/Cargo.toml" <<'TOML'
[package]
name = "five-minute-takeover-demo"
version = "0.1.0"
edition = "2021"

[workspace]
TOML
  cat > "${DEMO_WORKSPACE}/src/main.rs" <<'RS'
fn greet(name: &str) -> String {
    format!("Hello, {name}")
}

fn main() {
    println!("{}", greet("Vegvisir"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_mentions_name() {
        assert_eq!(greet("MSP"), "Hello, MSP");
    }
}
RS
fi

GOAL="Orient yourself in this repository. Identify the language, architecture, test/build command, run the tests, make one low-risk improvement if appropriate, rerun verification, and summarize evidence."

section "Demo workspace"
run find "${DEMO_WORKSPACE}" -maxdepth 3 -type f -print

section "Baseline tests"
(cd "${DEMO_WORKSPACE}" && cargo test)

section "Vegvisir live run"
if maybe_live; then
  run vegvisir run --workspace "${DEMO_WORKSPACE}" --max-steps 8 --artifacts --artifact-dir "${ARTIFACT_ROOT}/07-run-artifacts" "${GOAL}"
else
  cat <<EOF
Run this to record a five-minute repo takeover:

  RUN_LIVE=1 demos/scripts/07-five-minute-repo-takeover.sh

Or against another local repo:

  RUN_LIVE=1 DEMO_WORKSPACE=/path/to/repo demos/scripts/07-five-minute-repo-takeover.sh
EOF
fi
