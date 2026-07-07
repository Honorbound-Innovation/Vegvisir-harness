#!/usr/bin/env bash
# Demo 02: Skiller bundle -> MSP registry -> Vegvisir MSP client load.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"
require_common

REGISTRY="${ARTIFACT_ROOT}/02-skiller-to-msp-registry"
BUNDLE="${MSP_ROOT}/examples/skiller-bundles/sample-rust-bundle"
SKILL_ID="skill.software_engineering.rust.refactor-module.v1"

rm -rf "${REGISTRY}"
mkdir -p "${REGISTRY}"

section "Import Skiller bundle into a canonical MSP registry"
run msp --registry "${REGISTRY}" publish import-skiller "${BUNDLE}" --issuer demo-local

section "Index generated MSP registry"
run msp --registry "${REGISTRY}" registry index

section "Search for the imported skill"
run msp --registry "${REGISTRY}" registry search --task rust --max-risk medium

section "Load the skill with MSP CLI and verify body hash"
run msp --registry "${REGISTRY}" skills load "${SKILL_ID}"

section "Verify trust with MSP CLI"
run msp --registry "${REGISTRY}" trust verify "${SKILL_ID}"

section "Consume the same registry through Vegvisir's native MSP client"
run vegvisir msp -- --registry "${REGISTRY}" info
run vegvisir msp -- --registry "${REGISTRY}" search --task rust --max-risk medium
run vegvisir msp -- --registry "${REGISTRY}" load --mode card "${SKILL_ID}"
run vegvisir msp -- --registry "${REGISTRY}" verify-trust "${SKILL_ID}"

section "What this proves"
cat <<'EOF'
Skiller-authored skills can be registered into MSP, indexed, searched,
loaded, trust-checked, and consumed by Vegvisir through MSP rather than by
Vegvisir-specific private state.
EOF
