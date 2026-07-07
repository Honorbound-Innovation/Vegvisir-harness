#!/usr/bin/env bash
set -euo pipefail

# Self-check companion script inventory, permissions, docs, syntax, and safety markers.
# Usage: ./vdoctor.sh [--strict]

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"
manifest="$DIR/manifest.tsv"
readme="$DIR/README.md"
strict=0

if [[ "${1:-}" == "--strict" ]]; then
  strict=1
elif [[ -n "${1:-}" && "${1:-}" != "-h" && "${1:-}" != "--help" ]]; then
  echo "Usage: $0 [--strict]" >&2
  exit 1
elif [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  sed -n '2,5p' "$0" | sed 's/^# \{0,1\}//'
  exit 0
fi

errors=0
warnings=0

ok() { printf 'ok: %s\n' "$*"; }
warn() { warnings=$((warnings + 1)); printf 'warn: %s\n' "$*" >&2; }
err() { errors=$((errors + 1)); printf 'error: %s\n' "$*" >&2; }

cd "$ROOT"

[[ -d "$DIR" ]] && ok "companion_scripts directory exists" || err "companion_scripts directory missing"
[[ -f "$readme" ]] && ok "README exists" || err "README missing"
[[ -f "$manifest" ]] && ok "manifest.tsv exists" || err "manifest.tsv missing"

mapfile -t scripts < <(find "$DIR" -maxdepth 1 -type f -name 'v*.sh' -printf '%f\n' | sort)
script_count="${#scripts[@]}"
if (( script_count > 0 )); then
  ok "found $script_count dispatchable scripts"
else
  err "no dispatchable scripts found"
fi

syntax_bad=0
for script in "${scripts[@]}"; do
  if ! bash -n "$DIR/$script"; then
    syntax_bad=$((syntax_bad + 1))
  fi
done
if (( syntax_bad == 0 )); then
  ok "bash syntax passed for dispatchable scripts"
else
  err "bash syntax failed for $syntax_bad scripts"
fi

not_exec=()
for script in "${scripts[@]}"; do
  [[ -x "$DIR/$script" ]] || not_exec+=("$script")
done
if (( ${#not_exec[@]} == 0 )); then
  ok "all dispatchable scripts are executable"
else
  err "non-executable dispatchable scripts: ${not_exec[*]}"
fi

if [[ -f "$readme" ]]; then
  mapfile -t readme_cmds < <(grep -Eo '`v([[:alnum:]-]+)?\.sh`' "$readme" | tr -d '`' | sort -u)
  missing_in_readme=()
  missing_files=()
  for script in "${scripts[@]}"; do
    if ! printf '%s\n' "${readme_cmds[@]}" | grep -Fxq -- "$script"; then
      missing_in_readme+=("$script")
    fi
  done
  for cmd in "${readme_cmds[@]}"; do
    [[ -f "$DIR/$cmd" ]] || missing_files+=("$cmd")
  done
  if (( ${#missing_in_readme[@]} == 0 )); then
    ok "README documents every dispatchable script"
  else
    warn "scripts missing from README: ${missing_in_readme[*]}"
  fi
  if (( ${#missing_files[@]} == 0 )); then
    ok "README references existing scripts only"
  else
    err "README references missing scripts: ${missing_files[*]}"
  fi
fi

if [[ -f "$manifest" ]]; then
  mapfile -t manifest_cmds < <(awk -F '\t' 'NR > 1 {print $1}' "$manifest" | sort -u)
  missing_in_manifest=()
  manifest_missing_files=()
  for script in "${scripts[@]}"; do
    if ! printf '%s\n' "${manifest_cmds[@]}" | grep -Fxq -- "$script"; then
      missing_in_manifest+=("$script")
    fi
  done
  for cmd in "${manifest_cmds[@]}"; do
    [[ -f "$DIR/$cmd" ]] || manifest_missing_files+=("$cmd")
  done
  if (( ${#missing_in_manifest[@]} == 0 )); then
    ok "manifest covers every dispatchable script"
  else
    err "scripts missing from manifest: ${missing_in_manifest[*]}"
  fi
  if (( ${#manifest_missing_files[@]} == 0 )); then
    ok "manifest references existing scripts only"
  else
    err "manifest references missing scripts: ${manifest_missing_files[*]}"
  fi
fi

if grep -RIn --exclude-dir=.git --exclude-dir=.vegvisir --exclude='vdoctor.sh' -E '\./scripts/| scripts/' "$DIR" >/tmp/vdoctor-stale-paths.$$ 2>/dev/null; then
  if [[ -s /tmp/vdoctor-stale-paths.$$ ]]; then
    warn "possible stale scripts/ path references:"
    sed 's/^/  /' /tmp/vdoctor-stale-paths.$$ >&2
  else
    ok "no stale scripts/ references found"
  fi
else
  ok "no stale scripts/ references found"
fi
rm -f /tmp/vdoctor-stale-paths.$$

find "$DIR" -maxdepth 1 -type f -name 'v*.sh' ! -name 'vdoctor.sh' -print0 \
  | xargs -0 awk '
      FNR == 1 && /^#!.*\/env bash$/ { next }
      /printenv|(^|[^[:alnum:]_])env[[:space:]]/ { print FILENAME ":" FNR ":" $0 }
    ' >/tmp/vdoctor-env.$$ 2>/dev/null || true
if [[ -s /tmp/vdoctor-env.$$ ]]; then
  if grep -vE 'redact|redacted|names only|cut -d= -f1|awk -F=' /tmp/vdoctor-env.$$ >/tmp/vdoctor-env-risk.$$ 2>/dev/null; then
    warn "environment-printing lines need review:"
    sed 's/^/  /' /tmp/vdoctor-env-risk.$$ >&2
  else
    ok "environment helpers appear redacted/name-only"
  fi
else
  ok "no environment-printing helpers found"
fi
rm -f /tmp/vdoctor-env.$$ /tmp/vdoctor-env-risk.$$

required=(bash git find sort awk sed grep)
missing_required=()
for cmd in "${required[@]}"; do
  command -v "$cmd" >/dev/null 2>&1 || missing_required+=("$cmd")
done
if (( ${#missing_required[@]} == 0 )); then
  ok "core dependencies available: ${required[*]}"
else
  err "missing core dependencies: ${missing_required[*]}"
fi

if git check-ignore -q .vegvisir 2>/dev/null; then
  ok ".vegvisir is ignored by git"
else
  warn ".vegvisir is not ignored by git or no ignore rule was detected"
fi

if (( errors > 0 )); then
  printf 'vdoctor: FAILED with %d error(s), %d warning(s).\n' "$errors" "$warnings" >&2
  exit 1
fi
if (( strict == 1 && warnings > 0 )); then
  printf 'vdoctor: STRICT FAILED with %d warning(s).\n' "$warnings" >&2
  exit 1
fi
printf 'vdoctor: OK with %d warning(s).\n' "$warnings"
