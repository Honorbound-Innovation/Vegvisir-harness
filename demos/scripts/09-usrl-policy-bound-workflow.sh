#!/usr/bin/env bash
# Demo 09: USRL policy-bound workflow.
# Runs deterministic USRL-focused tests and prints a live workflow prompt.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"
require_common

section "USRL docs and component surface"
run test -f "${VEGVISIR_ROOT}/docs/usrl-language-reference.md"
run test -d "${VEGVISIR_ROOT}/components/usrl"
run sed -n '1,80p' "${VEGVISIR_ROOT}/docs/usrl-usage.md"

section "Run focused USRL gate tests"
run bash -lc "cd '${VEGVISIR_ROOT}' && cargo test -p vegvisir-rust --test port_smoke runtime_usrl_gate -- --nocapture"

section "Live demo prompt"
cat <<'EOF'
For a recording, use a small contract and ask Vegvisir to perform a task that
has both an allowed path and a prohibited/risky path. The visible proof should be:

- USRL contract loaded or referenced
- stage/evidence requirement followed
- risky action blocked or routed through approval
- allowed work completed and verified

Example prompt:

  Use the USRL policy-bound workflow for a low-risk docs update. Follow the
  required stages, gather evidence before editing, do not run destructive
  commands, and summarize which contract requirements were satisfied.
EOF
