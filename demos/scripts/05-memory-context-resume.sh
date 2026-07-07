#!/usr/bin/env bash
# Demo 05: Useful memory/context without dumping secrets or full chat history.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"
require_common

TITLE="Demo memory: low-friction Vegvisir stack summary"
CONTENT="Vegvisir is the operational harness; MSP is the portable skill protocol; Skiller authors skills; this is non-secret demo memory."
QUERY="portable skill protocol operational harness"
MESSAGE="Continue the demo planning around Vegvisir, MSP, and Skiller without exposing secrets."

section "Store non-secret durable project memory"
run vegvisir remember --memory-type note "${TITLE}" "${CONTENT}"

section "Recall relevant memory"
run vegvisir recall --limit 5 "${QUERY}"

section "Prepare active context for a new task"
run vegvisir context "${MESSAGE}"

section "Secret-like memory should be blocked"
set +e
vegvisir remember --memory-type note "Demo should block secret-like content" "api_key=sk-demo-do-not-store-this-secret-like-value" >"${ARTIFACT_ROOT}/05-secret-memory.out" 2>"${ARTIFACT_ROOT}/05-secret-memory.err"
STATUS=$?
set -e
cat "${ARTIFACT_ROOT}/05-secret-memory.out" || true
cat "${ARTIFACT_ROOT}/05-secret-memory.err" || true
note "secret-like memory write exit status: ${STATUS}"
cat <<'EOF'
If the current policy blocks secret-like memory, that is the desired outcome.
If the command exits successfully but redacts/ignores content in a local dev mode,
record the actual behavior and use the HBSE demo for the stricter secret boundary.
EOF
