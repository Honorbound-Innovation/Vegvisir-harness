#!/usr/bin/env bash
# Demo 06: HBSE secret boundary / no plaintext secrets in AI chat.
# This demo does not require real credentials and does not print secrets.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"
require_common

section "HBSE CLI surface"
run cargo run -q -p hbse --bin hbse -- --help

section "Vegvisir setup status/doctor surfaces"
run vegvisir setup --doctor --non-interactive || true

section "Reference-only onboarding command"
cat <<'EOF'
For a real provider, do NOT paste API keys into chat.
Use HBSE setup/onboarding and pass only secret references to Vegvisir/provider config.

Example shape, using placeholder values only:

  hbse secret put provider/openai/api-key --purpose model-provider
  vegvisir setup --provider openai-hbse --model <model>

In public recordings, use fake/disposable local refs and show that the secret value
is never echoed into the transcript or stored in CMS memory.
EOF

section "Secret-like memory boundary smoke"
set +e
vegvisir remember --memory-type note "HBSE demo secret boundary" "password=do-not-store-this-demo-secret" >"${ARTIFACT_ROOT}/06-secret-memory.out" 2>"${ARTIFACT_ROOT}/06-secret-memory.err"
STATUS=$?
set -e
cat "${ARTIFACT_ROOT}/06-secret-memory.out" || true
cat "${ARTIFACT_ROOT}/06-secret-memory.err" || true
note "secret-like memory write exit status: ${STATUS}"
