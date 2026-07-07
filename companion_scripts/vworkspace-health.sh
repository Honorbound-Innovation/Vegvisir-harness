#!/usr/bin/env bash
set -euo pipefail

# Summarize workspace hygiene signals from git and Vegvisir artifacts.
# Usage: ./vworkspace-health.sh

printf 'git_repo='; git rev-parse --is-inside-work-tree >/dev/null 2>&1 && echo yes || echo no
printf 'companion_scripts_count='; find companion_scripts -maxdepth 1 -type f -name 'v*.sh' 2>/dev/null | wc -l | awk '{print $1}'
printf '.vegvisir_present='; [[ -d .vegvisir ]] && echo yes || echo no
printf 'run_dirs='; find .vegvisir/runs -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | awk '{print $1}'
