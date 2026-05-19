# HBSE Usage

HBSE is the Hardware Bound Secrets Enclave. Vegvisir integrates with HBSE so provider and service credentials can be stored and brokered outside the harness. Vegvisir should hold secret references and policy metadata, not plaintext credentials.

The Rust implementation is stored in:

```text
components/HBSE
```

This page is based on the current `hbse --help` output.

## Installed Commands

```bash
hbse
hbse-broker
```

## Top-Level Help Tree

```text
Usage: hbse [OPTIONS] <COMMAND>

Commands:
  version
  vault
  secret
  audit
  policy
  config
  ticket
  rotation
  provider
  model-provider
  mfa
  broker
  dotenv
  release
  run
  resolve
  doctor
  setup
  lockdown
  readiness

Options:
      --vault <VAULT>
      --json
  -h, --help
```

If you use a non-default vault, export the path before running setup commands:

```bash
export HBSE_VAULT_PATH="$HOME/.local/share/hbse/vault.db"
```

Then pass it explicitly:

```bash
hbse --vault "$HBSE_VAULT_PATH" doctor
```

## Build From Source

```bash
cargo build -p hbse
cargo build -p hbse --release
```

## First-Time Setup

Run diagnostics:

```bash
hbse doctor
```

Initialize or prepare hardware-backed setup:

```text
Usage: hbse setup [OPTIONS]

Options:
      --tpm-device <TPM_DEVICE>  default: /dev/tpmrm0
```

Example:

```bash
hbse setup --tpm-device /dev/tpmrm0
```

## Broker Service

Broker commands:

```text
Usage: hbse broker <COMMAND>

Commands:
  status
  unlock
  mfa-verify
  lock
  checkout
  materialize
  provider-http
  cleanup-socket
  install-service
```

Install the broker service through the repository root installer:

```bash
./install.sh --hbse-service user --enable-hbse-service --start-hbse-service
```

Install it directly with HBSE:

```text
Usage: hbse broker install-service [OPTIONS]

Options:
      --scope <SCOPE>                                default: user
      --unit-dir <UNIT_DIR>
      --socket <SOCKET>
      --idle-timeout-seconds <IDLE_TIMEOUT_SECONDS>  default: 900
      --broker-executable <BROKER_EXECUTABLE>
      --service-user <SERVICE_USER>
      --enable
      --start
      --dry-run
```

Example:

```bash
hbse broker install-service \
  --scope user \
  --broker-executable "$(command -v hbse-broker)" \
  --enable \
  --start
```

Check status:

```bash
hbse broker status
```

## Secret Commands

```text
Usage: hbse secret <COMMAND>

Commands:
  put
  get
  inspect
  disable
  destroy
  list
```

Store a secret:

```text
Usage: hbse secret put [OPTIONS] <SECRET_REF>

Options:
      --value <VALUE>
      --stdin
      --secret-type <SECRET_TYPE>  default: generic
      --passphrase <PASSPHRASE>
```

Use stdin so the shell history does not capture the secret:

```bash
printf '%s' "$OPENAI_API_KEY" | hbse --vault "$HBSE_VAULT_PATH" secret put \
  secret://vegvisir/providers/openai/default \
  --stdin \
  --secret-type api_key
```

Inspect metadata without printing the secret:

```bash
hbse --vault "$HBSE_VAULT_PATH" secret inspect secret://vegvisir/providers/openai/default
```

List known secret records:

```bash
hbse --vault "$HBSE_VAULT_PATH" secret list
```

## Policy Commands

```text
Usage: hbse policy <COMMAND>

Commands:
  put
  list
  export
  hash
  test
```

Add a policy:

```text
Usage: hbse policy put [OPTIONS]

Options:
      --file <FILE>
      --stdin
      --passphrase <PASSPHRASE>
```

Test a policy decision:

```text
Usage: hbse policy test [OPTIONS] --secret-ref <SECRET_REF> --consumer <CONSUMER> --purpose <PURPOSE>

Options:
      --secret-ref <SECRET_REF>
      --consumer <CONSUMER>
      --purpose <PURPOSE>
      --delivery-mode <DELIVERY_MODE>  default: brokered_http
      --raw-export-requested
      --provider-assurance <PROVIDER_ASSURANCE>
      --http-host <HTTP_HOST>
      --http-scheme <HTTP_SCHEME>
      --http-method <HTTP_METHOD>
      --http-path <HTTP_PATH>
      --http-request-body-bytes <HTTP_REQUEST_BODY_BYTES>
```

## Model Provider Setup

Model provider commands:

```text
Usage: hbse model-provider <COMMAND>

Commands:
  list
  setup
```

Setup help:

```text
Usage: hbse model-provider setup [OPTIONS] <PRESET>

Options:
      --api-key-env <API_KEY_ENV>
      --stdin
      --secret-ref <SECRET_REF>
      --policy-id <POLICY_ID>
      --consumer <CONSUMER>
      --purpose <PURPOSE>                                  default: model.chat
      --model-discovery-purpose <MODEL_DISCOVERY_PURPOSE>  default: model.discovery
      --upstream-base-url <UPSTREAM_BASE_URL>
      --listen <LISTEN>
      --credential-header <CREDENTIAL_HEADER>
      --credential-prefix <CREDENTIAL_PREFIX>
      --max-body-bytes <MAX_BODY_BYTES>                    default: 10485760
      --require-mfa
      --passphrase <PASSPHRASE>
```

OpenAI setup:

```bash
printf '%s' "$OPENAI_API_KEY" | hbse --vault "$HBSE_VAULT_PATH" model-provider setup openai \
  --stdin \
  --secret-ref secret://vegvisir/providers/openai/default \
  --consumer vegvisir.provider.openai-hbse \
  --purpose model.chat \
  --model-discovery-purpose model.discovery
```

This stores the secret and creates provider policy entries for chat and model discovery.

## Manual OpenAI Chat Policy

If you need to create the policy manually:

```bash
hbse --vault "$HBSE_VAULT_PATH" policy put --stdin <<'JSON'
{
  "policy_id": "vegvisir-openai-chat",
  "secret_refs": ["secret://vegvisir/providers/openai/default"],
  "allowed_consumers": ["vegvisir.provider.openai-hbse"],
  "allowed_purposes": ["model.chat"],
  "allowed_delivery_modes": ["brokered_http"],
  "allowed_http_hosts": ["api.openai.com"],
  "allowed_http_methods": ["POST"],
  "allowed_http_path_prefixes": ["/v1/"],
  "require_https_for_brokered_http": true,
  "max_http_request_body_bytes": 10485760,
  "max_ticket_ttl_seconds": 60,
  "max_uses": 1,
  "minimum_provider_assurance": "A1",
  "require_mfa": false,
  "exportable": false
}
JSON
```

Test it:

```bash
hbse --vault "$HBSE_VAULT_PATH" policy test \
  --secret-ref secret://vegvisir/providers/openai/default \
  --consumer vegvisir.provider.openai-hbse \
  --purpose model.chat \
  --http-scheme https \
  --http-host api.openai.com \
  --http-method POST \
  --http-path /v1/responses \
  --http-request-body-bytes 1000
```

Expected result:

```text
allow
```

## Manual OpenAI Model Discovery Policy

```bash
hbse --vault "$HBSE_VAULT_PATH" policy put --stdin <<'JSON'
{
  "policy_id": "vegvisir-openai-model-discovery",
  "secret_refs": ["secret://vegvisir/providers/openai/default"],
  "allowed_consumers": ["vegvisir.provider.openai-hbse"],
  "allowed_purposes": ["model.discovery"],
  "allowed_delivery_modes": ["brokered_http"],
  "allowed_http_hosts": ["api.openai.com"],
  "allowed_http_methods": ["GET"],
  "allowed_http_path_prefixes": ["/v1/models"],
  "require_https_for_brokered_http": true,
  "max_http_request_body_bytes": 1048576,
  "max_ticket_ttl_seconds": 60,
  "max_uses": 1,
  "minimum_provider_assurance": "A1",
  "require_mfa": false,
  "exportable": false
}
JSON
```

Test it:

```bash
hbse --vault "$HBSE_VAULT_PATH" policy test \
  --secret-ref secret://vegvisir/providers/openai/default \
  --consumer vegvisir.provider.openai-hbse \
  --purpose model.discovery \
  --http-scheme https \
  --http-host api.openai.com \
  --http-method GET \
  --http-path /v1/models \
  --http-request-body-bytes 0
```

## Brokered HTTP

`provider-http` sends an HTTP request through the broker with the credential attached by HBSE:

```text
Usage: hbse broker provider-http [OPTIONS] --secret-ref <SECRET_REF> --purpose <PURPOSE> --url <URL>

Options:
      --socket <SOCKET>
      --secret-ref <SECRET_REF>
      --consumer <CONSUMER>                      default: cli
      --purpose <PURPOSE>
      --method <METHOD>                          default: GET
      --url <URL>
      --header <HEADER>
      --body <BODY>
      --credential-header <CREDENTIAL_HEADER>    default: Authorization
      --credential-prefix <CREDENTIAL_PREFIX>    default: "Bearer "
      --timeout-seconds <TIMEOUT_SECONDS>        default: 30
      --max-response-bytes <MAX_RESPONSE_BYTES>  default: 10485760
```

Example:

```bash
hbse broker provider-http \
  --secret-ref secret://vegvisir/providers/openai/default \
  --consumer vegvisir.provider.openai-hbse \
  --purpose model.discovery \
  --method GET \
  --url https://api.openai.com/v1/models
```

## Raw Checkout

`checkout` can deliver a secret according to policy. Prefer brokered HTTP for provider and MCP use because it keeps plaintext credentials out of the caller.

```text
Usage: hbse broker checkout [OPTIONS] --secret-ref <SECRET_REF> --purpose <PURPOSE>

Options:
      --socket <SOCKET>
      --secret-ref <SECRET_REF>
      --consumer <CONSUMER>            default: cli
      --purpose <PURPOSE>
      --delivery-mode <DELIVERY_MODE>  default: terminal_print
```

## Vegvisir Integration

Vegvisir HBSE providers use references such as:

```text
secret://vegvisir/providers/openai/default
secret://vegvisir/providers/openrouter/default
secret://vegvisir/mcp/github/default
```

Useful Vegvisir TUI helpers:

```text
/hbse status
/hbse provider openai
/hbse mcp github https://mcp.example.test/rpc
/hbse service add github secret://vegvisir/mcp/github/default vegvisir.mcp.github mcp.tool.call
/hbse services
```

Those commands print or register setup references. Enter actual secrets only into HBSE.
