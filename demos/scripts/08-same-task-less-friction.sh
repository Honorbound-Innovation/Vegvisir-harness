#!/usr/bin/env bash
# Demo 08: Same task, less friction comparison.
# This script prepares the fixture and prints a neutral measurement checklist.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"
require_common

DEMO_WORKSPACE="${ARTIFACT_ROOT}/08-friction-comparison-repo"
rm -rf "${DEMO_WORKSPACE}"
mkdir -p "${DEMO_WORKSPACE}/src"
cat > "${DEMO_WORKSPACE}/Cargo.toml" <<'TOML'
[package]
name = "friction-comparison-demo"
version = "0.1.0"
edition = "2021"

[workspace]
TOML
cat > "${DEMO_WORKSPACE}/src/main.rs" <<'RS'
fn slugify(input: &str) -> String {
    input.trim().to_lowercase().replace(' ', "-")
}

fn main() {
    println!("{}", slugify("Vegvisir Demo"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugifies_multiple_spaces() {
        assert_eq!(slugify("Vegvisir   Demo"), "vegvisir-demo");
    }
}
RS

TASK="Fix the failing slugify test by making the smallest robust change, run tests, and summarize the diff."

section "Prepared comparison fixture"
run find "${DEMO_WORKSPACE}" -maxdepth 3 -type f -print
set +e
(cd "${DEMO_WORKSPACE}" && cargo test)
set -e

section "Vegvisir command"
cat <<EOF
  cargo run -q -p vegvisir-rust --bin vegvisir-rust -- \\
    run --workspace '${DEMO_WORKSPACE}' --max-steps 8 --artifacts \\
    --artifact-dir '${ARTIFACT_ROOT}/08-vegvisir-artifacts' \\
    '${TASK}'
EOF

section "Comparison checklist"
cat <<'EOF'
Record the same task in another harness and measure:

- setup steps before useful work starts
- pasted context required
- number of user interventions
- whether it discovers/runs the right test command
- whether it edits the right file only
- final test status
- quality of final diff summary
- whether any memory/secret/tool boundary issues appear

Keep the comparison factual and non-hostile. The demo claim is lower friction,
not that every other tool is useless.
EOF
