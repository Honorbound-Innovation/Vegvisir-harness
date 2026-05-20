#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Onboard Vegvisir model-provider API secrets into HBSE.

Usage:
  scripts/hbse-provider-onboard.sh [options] <provider|all>

Options:
  --hbse <path>          HBSE CLI path. Default: hbse
  --vault <path>         HBSE vault path. Defaults to HBSE_VAULT_PATH or HBSE's default.
  --secret-env <name>    Read one provider secret from this environment variable.
  --sso-file <path>      OpenAI SSO token bundle JSON. Default: $VEGVISIR_HOME/auth/openai_sso.json.
  --dry-run              Print the deterministic setup plan without writing secrets.
  --list                 List supported provider ids.
  -h, --help             Show this help.

Examples:
  scripts/hbse-provider-onboard.sh openai
  scripts/hbse-provider-onboard.sh --secret-env OPENAI_API_KEY openai
  scripts/hbse-provider-onboard.sh all
  scripts/hbse-provider-onboard.sh openai-sso
USAGE
}

hbse_bin="${HBSE_BIN:-hbse}"
vault_path="${HBSE_VAULT_PATH:-}"
secret_env=""
sso_file=""
dry_run=0
target=""

declare -A BASE_URL=(
  [openai]="https://api.openai.com/v1"
  [xai]="https://api.x.ai/v1"
  [anthropic]="https://api.anthropic.com/v1"
  [google]="https://generativelanguage.googleapis.com/v1beta"
  [mistral]="https://api.mistral.ai/v1"
  [groq]="https://api.groq.com/openai/v1"
  [openrouter]="https://openrouter.ai/api/v1"
  [deepseek]="https://api.deepseek.com/v1"
  [together]="https://api.together.xyz/v1"
  [perplexity]="https://api.perplexity.ai"
)

declare -A CREDENTIAL_HEADER=(
  [openai]="Authorization"
  [xai]="Authorization"
  [anthropic]="x-api-key"
  [google]="x-goog-api-key"
  [mistral]="Authorization"
  [groq]="Authorization"
  [openrouter]="Authorization"
  [deepseek]="Authorization"
  [together]="Authorization"
  [perplexity]="Authorization"
)

declare -A CREDENTIAL_PREFIX=(
  [openai]="Bearer "
  [xai]="Bearer "
  [anthropic]=""
  [google]=""
  [mistral]="Bearer "
  [groq]="Bearer "
  [openrouter]="Bearer "
  [deepseek]="Bearer "
  [together]="Bearer "
  [perplexity]="Bearer "
)

providers=(openai xai anthropic google mistral groq openrouter deepseek together perplexity openai-sso)

while [[ $# -gt 0 ]]; do
  case "$1" in
    --hbse)
      hbse_bin="${2:?--hbse requires a path}"
      shift 2
      ;;
    --vault)
      vault_path="${2:?--vault requires a path}"
      shift 2
      ;;
    --secret-env)
      secret_env="${2:?--secret-env requires an environment variable name}"
      shift 2
      ;;
    --sso-file)
      sso_file="${2:?--sso-file requires a path}"
      shift 2
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    --list)
      printf '%s\n' "${providers[@]}"
      exit 0
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      if [[ -n "$target" ]]; then
        echo "unexpected argument: $1" >&2
        usage >&2
        exit 2
      fi
      target="$1"
      shift
      ;;
  esac
done

if [[ -z "$target" ]]; then
  usage >&2
  exit 2
fi

hbse_args=()
if [[ -n "$vault_path" ]]; then
  hbse_args+=(--vault "$vault_path")
fi

provider_host() {
  local url="$1"
  local without_scheme="${url#http://}"
  without_scheme="${without_scheme#https://}"
  printf '%s\n' "${without_scheme%%/*}"
}

provider_path_prefix() {
  local url="$1"
  local without_scheme="${url#http://}"
  without_scheme="${without_scheme#https://}"
  local path=""
  if [[ "$without_scheme" == */* ]]; then
    path="/${without_scheme#*/}"
    path="${path%/}/"
  else
    path="/"
  fi
  printf '%s\n' "$path"
}

provider_secret_from_user() {
  local provider="$1"
  local secret=""
  if [[ -n "$secret_env" ]]; then
    secret="${!secret_env:-}"
    if [[ -z "$secret" ]]; then
      echo "environment variable $secret_env is empty" >&2
      exit 2
    fi
  else
    read -rsp "Enter API secret for $provider: " secret
    printf '\n' >&2
  fi
  printf '%s' "$secret"
}

onboard_provider() {
  local provider="$1"
  if [[ "$provider" == "openai-sso" ]]; then
    onboard_openai_sso
    return
  fi
  local base_url="${BASE_URL[$provider]:-}"
  if [[ -z "$base_url" ]]; then
    echo "unsupported provider: $provider" >&2
    echo "supported providers: ${providers[*]}" >&2
    exit 2
  fi

  local secret_ref="secret://vegvisir/providers/$provider/default"
  local consumer="vegvisir.provider.$provider-hbse"
  local policy_id="vegvisir-provider-$provider"
  local host
  local path_prefix
  host="$(provider_host "$base_url")"
  path_prefix="$(provider_path_prefix "$base_url")"

  echo "Provider: $provider"
  echo "  secret_ref: $secret_ref"
  echo "  consumer:   $consumer"
  echo "  base_url:   $base_url"
  echo "  policy:     $policy_id"

  if [[ "$dry_run" -eq 1 ]]; then
    return
  fi

  provider_secret_from_user "$provider" | "$hbse_bin" "${hbse_args[@]}" secret put "$secret_ref" --stdin --secret-type api_key >/dev/null

  "$hbse_bin" "${hbse_args[@]}" policy put --stdin >/dev/null <<JSON
{
  "policy_id": "$policy_id",
  "secret_refs": ["$secret_ref"],
  "allowed_consumers": ["$consumer"],
  "denied_consumers": [],
  "allowed_purposes": ["model.chat", "model.discovery"],
  "denied_purposes": [],
  "allowed_delivery_modes": ["brokered_http"],
  "allowed_http_hosts": ["$host"],
  "denied_http_hosts": [],
  "allowed_http_methods": ["GET", "POST", "DELETE"],
  "denied_http_methods": [],
  "allowed_http_path_prefixes": ["$path_prefix"],
  "denied_http_path_prefixes": [],
  "require_https_for_brokered_http": true,
  "max_http_request_body_bytes": 10485760,
  "allowed_os_uids": [],
  "denied_os_uids": [],
  "allowed_executable_paths": [],
  "denied_executable_paths": [],
  "allowed_executable_sha256": [],
  "denied_executable_sha256": [],
  "exportable": false,
  "max_ticket_ttl_seconds": 60,
  "max_uses": 1,
  "minimum_provider_assurance": "A1",
  "require_mfa": false,
  "expires_at": null
}
JSON

  "$hbse_bin" "${hbse_args[@]}" policy test \
    --secret-ref "$secret_ref" \
    --consumer "$consumer" \
    --purpose model.discovery \
    --http-scheme https \
    --http-host "$host" \
    --http-method GET \
    --http-path "$path_prefix" \
    --http-request-body-bytes 0 >/dev/null

  echo "  status:     onboarded"
}

onboard_openai_sso() {
  local secret_ref="secret://vegvisir/providers/openai-sso/tokens"
  local consumer="vegvisir.provider.openai-sso-hbse"
  local policy_id="vegvisir-provider-openai-sso"
  local token_file="$sso_file"
  if [[ -z "$token_file" ]]; then
    token_file="${VEGVISIR_HOME:-$HOME/.local/share/vegvisir}/auth/openai_sso.json"
  fi

  echo "Provider: openai-sso"
  echo "  secret_ref: $secret_ref"
  echo "  consumer:   $consumer"
  echo "  token_file: $token_file"
  echo "  policy:     $policy_id"

  if [[ "$dry_run" -eq 1 ]]; then
    return
  fi
  if [[ ! -f "$token_file" ]]; then
    echo "OpenAI SSO token file not found: $token_file" >&2
    echo "Run /auth openai-sso first, then rerun this onboarding command." >&2
    exit 2
  fi

  "$hbse_bin" "${hbse_args[@]}" secret put "$secret_ref" --stdin --secret-type sso_token_bundle <"$token_file" >/dev/null

  "$hbse_bin" "${hbse_args[@]}" policy put --stdin >/dev/null <<JSON
{
  "policy_id": "$policy_id",
  "secret_refs": ["$secret_ref"],
  "allowed_consumers": ["$consumer"],
  "denied_consumers": [],
  "allowed_purposes": ["model.chat", "model.discovery"],
  "denied_purposes": [],
  "allowed_delivery_modes": ["brokered_http"],
  "allowed_http_hosts": ["chatgpt.com"],
  "denied_http_hosts": [],
  "allowed_http_methods": ["GET", "POST", "DELETE"],
  "denied_http_methods": [],
  "allowed_http_path_prefixes": ["/backend-api/codex/"],
  "denied_http_path_prefixes": [],
  "require_https_for_brokered_http": true,
  "max_http_request_body_bytes": 10485760,
  "allowed_os_uids": [],
  "denied_os_uids": [],
  "allowed_executable_paths": [],
  "denied_executable_paths": [],
  "allowed_executable_sha256": [],
  "denied_executable_sha256": [],
  "exportable": false,
  "max_ticket_ttl_seconds": 60,
  "max_uses": 1,
  "minimum_provider_assurance": "A1",
  "require_mfa": false,
  "expires_at": null
}
JSON

  echo "  status:     onboarded"
  echo "  note:       HBSE can inject tokens.access_token and tokens.account_id via brokered HTTP JSON credential fields."
}

if [[ "$target" == "all" ]]; then
  for provider in "${providers[@]}"; do
    onboard_provider "$provider"
  done
else
  onboard_provider "$target"
fi
