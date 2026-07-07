#!/usr/bin/env bash
# Demo 10: MSP as a cross-harness protocol via a tiny reference host.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"
require_common
require_cmd python3

section "Run tiny non-Vegvisir MSP-consuming host"
run python3 "${DEMO_ROOT}/reference-host/msp_reference_host.py" \
  --msp-root "${MSP_ROOT}" \
  --registry "examples/registry" \
  --skill-id "skill.rust.refactor.module.v1" \
  --task "refactor rust module"

section "What this proves"
cat <<'EOF'
MSP is not only a Vegvisir internal feature. A tiny standalone host can search,
verify, and load the same MSP skill artifact through the protocol/CLI surface.
Production integrations can use MSP JSON-RPC or native bindings instead.
EOF
