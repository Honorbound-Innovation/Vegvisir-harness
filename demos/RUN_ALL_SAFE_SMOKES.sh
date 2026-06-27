#!/usr/bin/env bash
# Run deterministic/safe demo smoke checks. Provider-backed live demos are not
# invoked unless the individual script opts into RUN_LIVE=1.

set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

"${ROOT}/scripts/02-skiller-to-msp-to-vegvisir.sh"
"${ROOT}/scripts/03-msp-tamper-rejection.sh"
"${ROOT}/scripts/08-same-task-less-friction.sh"
"${ROOT}/scripts/10-msp-reference-host-adapter.sh"

cat <<'EOF'

Safe smoke demos completed.
Provider/model-backed demos to record manually with RUN_LIVE=1:
  demos/scripts/01-vegvisir-fixes-itself.sh
  demos/scripts/04-bounded-subagents-review.sh
  demos/scripts/07-five-minute-repo-takeover.sh

Optional local infrastructure demos:
  demos/scripts/05-memory-context-resume.sh
  demos/scripts/06-hbse-no-plaintext-secrets.sh
  demos/scripts/09-usrl-policy-bound-workflow.sh
EOF
