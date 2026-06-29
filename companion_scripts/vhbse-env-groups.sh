#!/usr/bin/env bash
set -euo pipefail

# Show HBSE/approval-related environment variable names only, grouped by prefix.
# Usage: ./vhbse-env-groups.sh

printenv | cut -d= -f1 | awk '
  BEGIN { IGNORECASE=1 }
  /^(HBSE|SECRET|TOKEN|KEY|PASSWORD|CREDENTIAL|AUTH|APPROV|POLICY)/ { print }
' | sort
