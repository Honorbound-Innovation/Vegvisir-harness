#!/usr/bin/env bash
set -euo pipefail

# Summarize HBSE-sensitive environment variables by name only.
# Usage: ./vhbse-env.sh

patterns='^(HBSE|HBSE_|SECRET|TOKEN|KEY|PASSWORD|CREDENTIAL|AUTH)'
printenv | sort | awk -F= -v re="$patterns" '
  $1 ~ re { print $1 "=<redacted>" }
'
