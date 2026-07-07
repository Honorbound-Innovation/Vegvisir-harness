#!/usr/bin/env bash
# Demo 03: MSP catches tampered skill artifacts.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"
require_common

REGISTRY="${ARTIFACT_ROOT}/03-msp-tamper-registry"
BUNDLE="${MSP_ROOT}/examples/skiller-bundles/sample-rust-bundle"
SKILL_ID="skill.software_engineering.rust.refactor-module.v1"
BODY="${REGISTRY}/skills/${SKILL_ID}/skill.md"

rm -rf "${REGISTRY}"
mkdir -p "${REGISTRY}"

section "Publish a clean Skiller bundle into MSP"
run msp --registry "${REGISTRY}" publish import-skiller "${BUNDLE}" --issuer demo-local

section "Trust verification passes before tampering"
run msp --registry "${REGISTRY}" trust verify "${SKILL_ID}"
run msp --registry "${REGISTRY}" skills load "${SKILL_ID}" >/dev/null

section "Tamper with the skill body"
printf '\n\nTampered line inserted by demo 03.\n' >> "${BODY}"
run tail -5 "${BODY}"

section "Trust verification reports failure"
VERIFY_OUTPUT="${ARTIFACT_ROOT}/03-trust-verify-after-tamper.json"
msp --registry "${REGISTRY}" trust verify "${SKILL_ID}" | tee "${VERIFY_OUTPUT}"
if grep -q '"passed": false' "${VERIFY_OUTPUT}" && grep -q '"hash_passed": false' "${VERIFY_OUTPUT}"; then
  note "trust verify correctly reported passed=false/hash_passed=false"
else
  fail "trust verify did not report the expected failure"
fi

section "Skill loading hard-rejects the tampered body"
set +e
msp --registry "${REGISTRY}" skills load "${SKILL_ID}" >"${ARTIFACT_ROOT}/03-load-after-tamper.out" 2>"${ARTIFACT_ROOT}/03-load-after-tamper.err"
LOAD_STATUS=$?
set -e
cat "${ARTIFACT_ROOT}/03-load-after-tamper.err"
if [[ "${LOAD_STATUS}" -eq 0 ]]; then
  fail "skills load unexpectedly succeeded after tampering"
fi
note "skills load failed with exit status ${LOAD_STATUS}, as expected"

section "What this proves"
cat <<'EOF'
MSP treats skills as verifiable artifacts. If the body changes after
publication, trust verification surfaces the mismatch and normal skill loading
refuses to materialize the tampered skill.
EOF
