#!/usr/bin/env bash
# Demo 04: Bounded subagents review a real change.
# Safe default prints the exact live prompt; RUN_LIVE=1 invokes Vegvisir.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"
require_common

DEMO_WORKSPACE="${ARTIFACT_ROOT}/04-subagents-workspace"
rm -rf "${DEMO_WORKSPACE}"
mkdir -p "${DEMO_WORKSPACE}/src" "${DEMO_WORKSPACE}/docs"

cat > "${DEMO_WORKSPACE}/Cargo.toml" <<'TOML'
[package]
name = "bounded-subagents-demo"
version = "0.1.0"
edition = "2021"

[workspace]
TOML
cat > "${DEMO_WORKSPACE}/src/main.rs" <<'RS'
fn main() {
    println!("bounded subagents demo");
}
RS
cat > "${DEMO_WORKSPACE}/docs/CHANGE.md" <<'MD'
# Proposed change

Add a small health-check command and review test/docs/security impact.
MD

GOAL=$(cat <<'EOF'
Demonstrate bounded subagent delegation in this disposable workspace.
Main thread: inspect the tiny Rust project and propose a minimal health-check change.
Spawn bounded read-only subagents with non-overlapping scopes:
1. docs reviewer for docs/**,
2. test planner for Cargo.toml/src/**,
3. security reviewer for src/**.
After they finish, check the subagent board and summarize findings. Do not use secrets or external services.
EOF
)

section "Created disposable subagent demo workspace"
run find "${DEMO_WORKSPACE}" -maxdepth 3 -type f -print

section "Vegvisir live run"
if maybe_live; then
  run vegvisir run --workspace "${DEMO_WORKSPACE}" --max-steps 10 --artifacts --artifact-dir "${ARTIFACT_ROOT}/04-run-artifacts" "${GOAL}"
else
  cat <<EOF
Run this to record the live subagent demo:

  RUN_LIVE=1 demos/scripts/04-bounded-subagents-review.sh

Expected visible proof:
  - spawn_subagent tool calls with narrow goals and file scopes
  - subagent board records
  - final merged findings/report
EOF
fi
