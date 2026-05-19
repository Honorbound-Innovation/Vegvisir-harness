# HBSE Usage

HBSE is the Hardware Bound Secrets Enclave. Vegvisir integrates with HBSE so provider and service credentials can be delivered through brokered operations without Vegvisir reading plaintext secrets.

The Rust implementation is stored in:

```text
components/HBSE
```

## Build

```bash
cargo build -p hbse
```

## Broker Service

Install the broker service from the HBSE component:

```bash
cd components/HBSE
cargo build --release
hbse broker install-service --scope user --broker-executable "$(pwd)/target/release/hbse-broker"
```

Start or restart the service with systemd as appropriate for the chosen scope.

## Add An OpenAI Secret

Set the vault path first if you are using a non-default vault:

```bash
export HBSE_VAULT_PATH="$HOME/.local/share/hbse/vault.sqlite3"
```

Store the secret through stdin:

```bash
printf '%s' "$OPENAI_API_KEY" | hbse --vault "$HBSE_VAULT_PATH" model-provider setup openai \
  --stdin \
  --secret-ref secret://vegvisir/providers/openai/default \
  --consumer vegvisir.provider.openai-hbse \
  --purpose model.chat \
  --model-discovery-purpose model.discovery
```

## Allow OpenAI Chat Requests

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

## Allow OpenAI Model Discovery

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

## Test Policy

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

The expected result is `allow`.

