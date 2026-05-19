# HBSE Usage And Command Reference

HBSE is the Hardware Bound Secrets Enclave. Vegvisir integrates with HBSE so provider and service credentials can be stored and brokered outside the harness. Vegvisir should hold secret references and policy metadata, not plaintext credentials.

This document is intentionally comprehensive. It includes the full current help tree, exact usage blocks, explanations, operational notes, and examples for every HBSE command currently exposed by the Rust CLI.

Implementation path:

```text
components/HBSE
```

Installed commands:

```bash
hbse
hbse-broker
```

## Top-Level Help

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
  help            Print this message or the help of the given subcommand(s)

Options:
      --vault <VAULT>  [default: /home/malice/.local/share/hbse/vault.db]
      --json
  -h, --help           Print help
```

## Common Environment

```bash
export HBSE_VAULT_PATH="$HOME/.local/share/hbse/vault.db"
```

Use this when Vegvisir and HBSE must operate on a specific existing vault rather than the default path.

## OpenAI Setup For Vegvisir

```bash
printf '%s' "$OPENAI_API_KEY" | hbse --vault "$HBSE_VAULT_PATH" model-provider setup openai \
  --stdin \
  --secret-ref secret://vegvisir/providers/openai/default \
  --consumer vegvisir.provider.openai-hbse \
  --purpose model.chat \
  --model-discovery-purpose model.discovery
```

This stores the OpenAI credential under `secret://vegvisir/providers/openai/default` and prepares model chat and discovery policy for `vegvisir.provider.openai-hbse`.

## Manual Policy Test

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

A successful result is `allow`. A denial means either the secret does not exist in the selected vault, the consumer/purpose/path does not match policy, or the broker is connected to a different vault than the CLI command.

## Root And Global Commands

#### version

Purpose:

Prints the HBSE version.

When to use it:

Use this to verify the installed binary before debugging behavior or comparing hosts.

Exact help:

```text
Usage: hbse version

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse version
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### doctor

Purpose:

Runs diagnostic checks for the HBSE host, vault, provider support, and runtime environment.

When to use it:

Use this before deploying Vegvisir on a new machine or when a broker/provider request fails unexpectedly.

Exact help:

```text
Usage: hbse doctor

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" doctor
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### setup

Purpose:

Runs first-time hardware provider setup.

When to use it:

Use this on a new host before storing production secrets, especially when TPM-backed operation is expected.

Exact help:

```text
Usage: hbse setup [OPTIONS]

Options:
      --tpm-device <TPM_DEVICE>  [default: /dev/tpmrm0]
  -h, --help                     Print help
```

Examples:

```bash
hbse setup --tpm-device /dev/tpmrm0
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### lockdown

Purpose:

Puts HBSE into a locked-down state with an audit reason.

When to use it:

Use this when the operator wants secrets to stop being served until the environment is reviewed.

Exact help:

```text
Usage: hbse lockdown [OPTIONS]

Options:
      --reason <REASON>          [default: "local lockdown"]
      --passphrase <PASSPHRASE>
  -h, --help                     Print help
```

Examples:

```bash
hbse lockdown --reason "operator requested lockdown"
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### run

Purpose:

Runs a child command while delivering selected HBSE secrets through controlled channels.

When to use it:

Use this for tools or services that need credentials at runtime without writing those credentials into config files.

Exact help:

```text
Usage: hbse run [OPTIONS] --purpose <PURPOSE> [COMMAND]...

Arguments:
  [COMMAND]...

Options:
      --consumer <CONSUMER>                [default: cli]
      --purpose <PURPOSE>
      --secret-env <SECRET_ENV>
      --secret-file-env <SECRET_FILE_ENV>
      --secret-fd-env <SECRET_FD_ENV>
      --secret-stdin <SECRET_STDIN>
      --env <ENV>
      --mfa-code <MFA_CODE>
  -h, --help                               Print help
```

Examples:

```bash
hbse run --consumer vegvisir.tool.github --purpose service.access \
  --secret-env GITHUB_TOKEN=secret://vegvisir/services/github/default \
  -- gh repo view
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### resolve

Purpose:

Resolves a single secret reference either locally or through the broker.

When to use it:

Use this for diagnostics or explicit operator-approved secret delivery; prefer brokered HTTP for provider calls.

Exact help:

```text
Usage: hbse resolve [OPTIONS] <SECRET_REF>

Arguments:
  <SECRET_REF>

Options:
      --passphrase <PASSPHRASE>
      --broker
      --socket <SOCKET>                [default: /run/user/1000/hbse/broker.sock]
      --consumer <CONSUMER>            [default: cli]
      --purpose <PURPOSE>              [default: terminal]
      --delivery-mode <DELIVERY_MODE>  [default: terminal_print]
      --allow-plaintext
      --mfa-code <MFA_CODE>
  -h, --help                           Print help
```

Examples:

```bash
hbse resolve secret://vegvisir/providers/openai/default \
  --broker \
  --consumer vegvisir.provider.openai-hbse \
  --purpose model.chat
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### readiness

Purpose:

Container for readiness subcommands.

When to use it:

Use it to discover readiness checks available in this build.

Exact help:

```text
Usage: hbse readiness <COMMAND>

Commands:
  check
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse readiness
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### readiness check

Purpose:

Runs deployment readiness checks.

When to use it:

Use this after installation, vault migration, or broker service setup.

Exact help:

```text
Usage: hbse readiness check [OPTIONS]

Options:
      --target <TARGET>            [default: A2]
      --release-dir <RELEASE_DIR>  [default: release]
      --verify-audit
      --passphrase <PASSPHRASE>
  -h, --help                       Print help
```

Examples:

```bash
hbse readiness check
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## Vault Commands

#### vault

Purpose:

Container for encrypted vault lifecycle commands.

When to use it:

Use it to initialize, inspect, backup, restore, and recover the HBSE vault.

Exact help:

```text
Usage: hbse vault <COMMAND>

Commands:
  init
  status
  backup
  restore
  recovery-create
  recovery-inspect
  recover
  help              Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse vault status
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### vault init

Purpose:

Performs the vault init operation.

When to use it:

Use when managing the vault lifecycle for an HBSE installation.

Exact help:

```text
Usage: hbse vault init [OPTIONS]

Options:
      --namespace <NAMESPACE>    [default: default]
      --provider <PROVIDER>      [default: passphrase]
      --tpm-device <TPM_DEVICE>  [default: /dev/tpmrm0]
      --passphrase <PASSPHRASE>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" vault init
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### vault status

Purpose:

Performs the vault status operation.

When to use it:

Use when managing the vault lifecycle for an HBSE installation.

Exact help:

```text
Usage: hbse vault status

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" vault status
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### vault backup

Purpose:

Performs the vault backup operation.

When to use it:

Use before upgrades, migrations, or destructive maintenance.

Exact help:

```text
Usage: hbse vault backup <DESTINATION>

Arguments:
  <DESTINATION>

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" vault backup /tmp/hbse-vault.backup
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### vault restore

Purpose:

Performs the vault restore operation.

When to use it:

Use during recovery or host migration after confirming the target vault path.

Exact help:

```text
Usage: hbse vault restore <SOURCE>

Arguments:
  <SOURCE>

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" vault restore /tmp/hbse-vault.backup
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### vault recovery create

Purpose:

Performs the vault recovery create operation.

When to use it:

Use during recovery or host migration after confirming the target vault path.

Exact help:

```text
Usage: hbse vault recovery-create [OPTIONS] <DESTINATION>

Arguments:
  <DESTINATION>

Options:
      --passphrase <PASSPHRASE>
      --recovery-secret <RECOVERY_SECRET>
      --mnemonic
  -h, --help                               Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" vault recovery-create /tmp/hbse-recovery.json
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### vault recovery inspect

Purpose:

Performs the vault recovery inspect operation.

When to use it:

Use during recovery or host migration after confirming the target vault path.

Exact help:

```text
Usage: hbse vault recovery-inspect [OPTIONS] <PACKAGE>

Arguments:
  <PACKAGE>

Options:
      --recovery-secret <RECOVERY_SECRET>
      --recovery-mnemonic <RECOVERY_MNEMONIC>
  -h, --help                                   Print help
```

Examples:

```bash
hbse vault recovery-inspect /tmp/hbse-recovery.json
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### vault recover

Purpose:

Performs the vault recover operation.

When to use it:

Use during recovery or host migration after confirming the target vault path.

Exact help:

```text
Usage: hbse vault recover [OPTIONS] --new-provider <NEW_PROVIDER> <PACKAGE>

Arguments:
  <PACKAGE>

Options:
      --recovery-secret <RECOVERY_SECRET>
      --recovery-mnemonic <RECOVERY_MNEMONIC>
      --new-provider <NEW_PROVIDER>
      --new-passphrase <NEW_PASSPHRASE>
      --tpm-device <TPM_DEVICE>                [default: /dev/tpmrm0]
  -h, --help                                   Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" vault recover --new-provider passphrase /tmp/hbse-recovery.json
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## Secret Commands

#### secret

Purpose:

Container for secret-reference commands.

When to use it:

Use it to store, inspect, disable, destroy, and list secrets by secret:// reference.

Exact help:

```text
Usage: hbse secret <COMMAND>

Commands:
  put
  get
  inspect
  disable
  destroy
  list
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse secret list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### secret put

Purpose:

Stores secret material under a secret:// reference.

When to use it:

Use this as part of secret lifecycle management.

Exact help:

```text
Usage: hbse secret put [OPTIONS] <SECRET_REF>

Arguments:
  <SECRET_REF>

Options:
      --value <VALUE>
      --stdin
      --secret-type <SECRET_TYPE>  [default: generic]
      --passphrase <PASSPHRASE>
  -h, --help                       Print help
```

Examples:

```bash
printf '%s' "$OPENAI_API_KEY" | hbse --vault "$HBSE_VAULT_PATH" secret put secret://vegvisir/providers/openai/default --stdin --secret-type api_key
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### secret get

Purpose:

Retrieves secret material according to local policy.

When to use it:

Use direct get only for operator-approved workflows; Vegvisir providers should use brokered delivery.

Exact help:

```text
Usage: hbse secret get [OPTIONS] <SECRET_REF>

Arguments:
  <SECRET_REF>

Options:
      --passphrase <PASSPHRASE>
      --allow-plaintext
      --mfa-code <MFA_CODE>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" secret get secret://vegvisir/providers/openai/default
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### secret inspect

Purpose:

Shows metadata for a secret without showing the secret value.

When to use it:

Use this as part of secret lifecycle management.

Exact help:

```text
Usage: hbse secret inspect <SECRET_REF>

Arguments:
  <SECRET_REF>

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" secret inspect secret://vegvisir/providers/openai/default
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### secret disable

Purpose:

Marks a secret unavailable without deleting its record.

When to use it:

Use this as part of secret lifecycle management.

Exact help:

```text
Usage: hbse secret disable [OPTIONS] <SECRET_REF>

Arguments:
  <SECRET_REF>

Options:
      --passphrase <PASSPHRASE>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" secret disable secret://vegvisir/providers/openai/default
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### secret destroy

Purpose:

Destroys a secret record with an explicit reason.

When to use it:

Use this as part of secret lifecycle management.

Exact help:

```text
Usage: hbse secret destroy [OPTIONS] --reason <REASON> <SECRET_REF>

Arguments:
  <SECRET_REF>

Options:
      --passphrase <PASSPHRASE>
      --reason <REASON>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" secret destroy --reason "retired after rotation" secret://vegvisir/providers/openai/default
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### secret list

Purpose:

Lists known secret references.

When to use it:

Use this as part of secret lifecycle management.

Exact help:

```text
Usage: hbse secret list

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" secret list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## Audit Commands

#### audit

Purpose:

Container for audit log commands.

When to use it:

Use it to review and verify what happened in the vault.

Exact help:

```text
Usage: hbse audit <COMMAND>

Commands:
  list
  export
  verify
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse audit list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### audit list

Purpose:

Lists HBSE audit data.

When to use it:

Use audit commands during incident review, migration validation, and operational troubleshooting.

Exact help:

```text
Usage: hbse audit list [OPTIONS]

Options:
      --event-type <EVENT_TYPE>
      --limit <LIMIT>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" audit list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### audit export

Purpose:

Exports HBSE audit data.

When to use it:

Use audit commands during incident review, migration validation, and operational troubleshooting.

Exact help:

```text
Usage: hbse audit export [OPTIONS] <DESTINATION>

Arguments:
  <DESTINATION>

Options:
      --event-type <EVENT_TYPE>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" audit export /tmp/hbse-audit.jsonl
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### audit verify

Purpose:

Verifys HBSE audit data.

When to use it:

Use audit commands during incident review, migration validation, and operational troubleshooting.

Exact help:

```text
Usage: hbse audit verify [OPTIONS]

Options:
      --passphrase <PASSPHRASE>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" audit verify
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## Policy Commands

#### policy

Purpose:

Container for access policy commands.

When to use it:

Use it to define and test which consumers may use which secrets for which purposes.

Exact help:

```text
Usage: hbse policy <COMMAND>

Commands:
  put
  list
  export
  hash
  test
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse policy list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### policy put

Purpose:

Creates or updates an allow policy.

When to use it:

Use policy commands whenever broker denial, consumer mismatch, host/path mismatch, or model discovery authorization needs to be configured or verified.

Exact help:

```text
Usage: hbse policy put [OPTIONS]

Options:
      --file <FILE>
      --stdin
      --passphrase <PASSPHRASE>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" policy put --stdin < ./openai-chat-policy.json
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### policy list

Purpose:

Lists configured policies.

When to use it:

Use policy commands whenever broker denial, consumer mismatch, host/path mismatch, or model discovery authorization needs to be configured or verified.

Exact help:

```text
Usage: hbse policy list

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" policy list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### policy export

Purpose:

Exports policies for review or backup.

When to use it:

Use policy commands whenever broker denial, consumer mismatch, host/path mismatch, or model discovery authorization needs to be configured or verified.

Exact help:

```text
Usage: hbse policy export [DESTINATION]

Arguments:
  [DESTINATION]

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" policy export /tmp/hbse-policies.json
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### policy hash

Purpose:

Computes policy hashes for integrity review.

When to use it:

Use policy commands whenever broker denial, consumer mismatch, host/path mismatch, or model discovery authorization needs to be configured or verified.

Exact help:

```text
Usage: hbse policy hash [OPTIONS]

Options:
      --file <FILE>
      --stdin
  -h, --help         Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" policy hash
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### policy test

Purpose:

Evaluates whether a proposed request would be allowed.

When to use it:

Use policy commands whenever broker denial, consumer mismatch, host/path mismatch, or model discovery authorization needs to be configured or verified.

Exact help:

```text
Usage: hbse policy test [OPTIONS] --secret-ref <SECRET_REF> --consumer <CONSUMER> --purpose <PURPOSE>

Options:
      --secret-ref <SECRET_REF>
      --consumer <CONSUMER>
      --purpose <PURPOSE>
      --delivery-mode <DELIVERY_MODE>                      [default: brokered_http]
      --raw-export-requested
      --provider-assurance <PROVIDER_ASSURANCE>
      --http-host <HTTP_HOST>
      --http-scheme <HTTP_SCHEME>
      --http-method <HTTP_METHOD>
      --http-path <HTTP_PATH>
      --http-request-body-bytes <HTTP_REQUEST_BODY_BYTES>
  -h, --help                                               Print help
```

Examples:

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

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## Plaintext Export Configuration

#### config

Purpose:

Manages HBSE configuration for plaintext export.

When to use it:

Use this only for controlled operator workflows; plaintext export should stay disabled unless explicitly needed.

Exact help:

```text
Usage: hbse config <COMMAND>

Commands:
  plaintext-export
  help              Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse config plaintext-export status
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### config plaintext export

Purpose:

Manages HBSE configuration for plaintext export.

When to use it:

Use this only for controlled operator workflows; plaintext export should stay disabled unless explicitly needed.

Exact help:

```text
Usage: hbse config plaintext-export <COMMAND>

Commands:
  status
  enable
  disable
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse config plaintext-export status
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### config plaintext export status

Purpose:

Manages HBSE configuration for plaintext export.

When to use it:

Use this only for controlled operator workflows; plaintext export should stay disabled unless explicitly needed.

Exact help:

```text
Usage: hbse config plaintext-export status

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse config plaintext-export status
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### config plaintext export enable

Purpose:

Manages HBSE configuration for plaintext export.

When to use it:

Use this only for controlled operator workflows; plaintext export should stay disabled unless explicitly needed.

Exact help:

```text
Usage: hbse config plaintext-export enable [OPTIONS]

Options:
      --passphrase <PASSPHRASE>
      --mfa-code <MFA_CODE>
      --allow-without-mfa
  -h, --help                     Print help
```

Examples:

```bash
hbse config plaintext-export enable --mfa-code 123456
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### config plaintext export disable

Purpose:

Manages HBSE configuration for plaintext export.

When to use it:

Use this only for controlled operator workflows; plaintext export should stay disabled unless explicitly needed.

Exact help:

```text
Usage: hbse config plaintext-export disable [OPTIONS]

Options:
      --passphrase <PASSPHRASE>
      --mfa-code <MFA_CODE>
  -h, --help                     Print help
```

Examples:

```bash
hbse config plaintext-export disable
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## Ticket Commands

#### ticket

Purpose:

Container for short-lived ticket commands.

When to use it:

Use tickets when an operation needs bounded, revocable authorization.

Exact help:

```text
Usage: hbse ticket <COMMAND>

Commands:
  list
  inspect
  issue
  revoke
  validate
  renew
  consume
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse ticket list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### ticket list

Purpose:

Lists issued tickets.

When to use it:

Use ticket commands when access should be auditable, revocable, time-limited, and purpose-bound.

Exact help:

```text
Usage: hbse ticket list

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" ticket list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### ticket issue

Purpose:

Issues a new ticket for a secret reference and purpose.

When to use it:

Use ticket commands when access should be auditable, revocable, time-limited, and purpose-bound.

Exact help:

```text
Usage: hbse ticket issue [OPTIONS] --purpose <PURPOSE> <SECRET_REF>

Arguments:
  <SECRET_REF>

Options:
      --consumer <CONSUMER>                                [default: cli]
      --purpose <PURPOSE>
      --delivery-mode <DELIVERY_MODE>                      [default: terminal_print]
      --raw-export-requested
      --provider-assurance <PROVIDER_ASSURANCE>
      --http-host <HTTP_HOST>
      --http-scheme <HTTP_SCHEME>
      --http-method <HTTP_METHOD>
      --http-path <HTTP_PATH>
      --http-request-body-bytes <HTTP_REQUEST_BODY_BYTES>
      --passphrase <PASSPHRASE>
  -h, --help                                               Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" ticket issue --consumer vegvisir.provider.openai-hbse --purpose model.chat secret://vegvisir/providers/openai/default
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### ticket inspect

Purpose:

Shows ticket metadata.

When to use it:

Use ticket commands when access should be auditable, revocable, time-limited, and purpose-bound.

Exact help:

```text
Usage: hbse ticket inspect <TICKET_ID>

Arguments:
  <TICKET_ID>

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" ticket inspect <ticket-id>
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### ticket revoke

Purpose:

Revokes a ticket.

When to use it:

Use ticket commands when access should be auditable, revocable, time-limited, and purpose-bound.

Exact help:

```text
Usage: hbse ticket revoke [OPTIONS] <TICKET_ID>

Arguments:
  <TICKET_ID>

Options:
      --passphrase <PASSPHRASE>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" ticket revoke <ticket-id>
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### ticket validate

Purpose:

Checks whether a ticket is valid for a consumer and purpose.

When to use it:

Use ticket commands when access should be auditable, revocable, time-limited, and purpose-bound.

Exact help:

```text
Usage: hbse ticket validate [OPTIONS] --consumer <CONSUMER> --purpose <PURPOSE> <TICKET_ID>

Arguments:
  <TICKET_ID>

Options:
      --consumer <CONSUMER>
      --purpose <PURPOSE>
      --delivery-mode <DELIVERY_MODE>                      [default: terminal_print]
      --raw-export-requested
      --provider-assurance <PROVIDER_ASSURANCE>
      --http-host <HTTP_HOST>
      --http-scheme <HTTP_SCHEME>
      --http-method <HTTP_METHOD>
      --http-path <HTTP_PATH>
      --http-request-body-bytes <HTTP_REQUEST_BODY_BYTES>
      --passphrase <PASSPHRASE>
  -h, --help                                               Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" ticket validate --consumer vegvisir.provider.openai-hbse --purpose model.chat <ticket-id>
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### ticket renew

Purpose:

Renews a ticket for continued authorized use.

When to use it:

Use ticket commands when access should be auditable, revocable, time-limited, and purpose-bound.

Exact help:

```text
Usage: hbse ticket renew [OPTIONS] --consumer <CONSUMER> --purpose <PURPOSE> <TICKET_ID>

Arguments:
  <TICKET_ID>

Options:
      --consumer <CONSUMER>
      --purpose <PURPOSE>
      --delivery-mode <DELIVERY_MODE>                      [default: terminal_print]
      --raw-export-requested
      --provider-assurance <PROVIDER_ASSURANCE>
      --http-host <HTTP_HOST>
      --http-scheme <HTTP_SCHEME>
      --http-method <HTTP_METHOD>
      --http-path <HTTP_PATH>
      --http-request-body-bytes <HTTP_REQUEST_BODY_BYTES>
      --passphrase <PASSPHRASE>
  -h, --help                                               Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" ticket renew --consumer vegvisir.provider.openai-hbse --purpose model.chat <ticket-id>
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### ticket consume

Purpose:

Consumes a ticket for a permitted use.

When to use it:

Use ticket commands when access should be auditable, revocable, time-limited, and purpose-bound.

Exact help:

```text
Usage: hbse ticket consume [OPTIONS] --consumer <CONSUMER> --purpose <PURPOSE> <TICKET_ID>

Arguments:
  <TICKET_ID>

Options:
      --consumer <CONSUMER>
      --purpose <PURPOSE>
      --delivery-mode <DELIVERY_MODE>                      [default: terminal_print]
      --raw-export-requested
      --provider-assurance <PROVIDER_ASSURANCE>
      --http-host <HTTP_HOST>
      --http-scheme <HTTP_SCHEME>
      --http-method <HTTP_METHOD>
      --http-path <HTTP_PATH>
      --http-request-body-bytes <HTTP_REQUEST_BODY_BYTES>
      --passphrase <PASSPHRASE>
      --allow-plaintext
      --mfa-code <MFA_CODE>
  -h, --help                                               Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" ticket consume --consumer vegvisir.provider.openai-hbse --purpose model.chat <ticket-id>
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## Rotation Commands

#### rotation

Purpose:

Container for secret rotation jobs.

When to use it:

Use it to start, verify, promote, roll back, and list rotations.

Exact help:

```text
Usage: hbse rotation <COMMAND>

Commands:
  start
  verify
  promote
  rollback
  list
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse rotation list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### rotation start

Purpose:

Starts a rotation job for a secret reference.

When to use it:

Use rotation commands to change credentials without breaking consumers or losing auditability.

Exact help:

```text
Usage: hbse rotation start [OPTIONS] <SECRET_REF>

Arguments:
  <SECRET_REF>

Options:
      --value <VALUE>
      --stdin
      --secret-type <SECRET_TYPE>  [default: generic]
      --passphrase <PASSPHRASE>
  -h, --help                       Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" rotation start secret://vegvisir/providers/openai/default
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### rotation verify

Purpose:

Verifies a rotation job before promotion.

When to use it:

Use rotation commands to change credentials without breaking consumers or losing auditability.

Exact help:

```text
Usage: hbse rotation verify [OPTIONS] <JOB_ID>

Arguments:
  <JOB_ID>

Options:
      --passphrase <PASSPHRASE>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" rotation verify <job-id>
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### rotation promote

Purpose:

Promotes the rotated secret after verification.

When to use it:

Use rotation commands to change credentials without breaking consumers or losing auditability.

Exact help:

```text
Usage: hbse rotation promote [OPTIONS] <JOB_ID>

Arguments:
  <JOB_ID>

Options:
      --passphrase <PASSPHRASE>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" rotation promote <job-id>
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### rotation rollback

Purpose:

Rolls back a rotation job.

When to use it:

Use rotation commands to change credentials without breaking consumers or losing auditability.

Exact help:

```text
Usage: hbse rotation rollback [OPTIONS] <JOB_ID>

Arguments:
  <JOB_ID>

Options:
      --passphrase <PASSPHRASE>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" rotation rollback <job-id>
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### rotation list

Purpose:

Lists rotation jobs.

When to use it:

Use rotation commands to change credentials without breaking consumers or losing auditability.

Exact help:

```text
Usage: hbse rotation list

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" rotation list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## Provider Binding Commands

#### provider

Purpose:

Container for hardware/software provider binding commands.

When to use it:

Use it to discover, test, and enroll HBSE key providers.

Exact help:

```text
Usage: hbse provider <COMMAND>

Commands:
  list
  detect
  test-tpm2
  test-tpm2-direct
  test-system-fingerprint
  test-yubikey-piv
  enroll
  help                     Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse provider list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### provider list

Purpose:

Runs provider list for HBSE binding support.

When to use it:

Use provider commands during host setup, hardware diagnostics, and provider enrollment.

Exact help:

```text
Usage: hbse provider list [OPTIONS]

Options:
      --device <DEVICE>  [default: /dev/tpmrm0]
  -h, --help             Print help
```

Examples:

```bash
hbse provider list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### provider detect

Purpose:

Runs provider detect for HBSE binding support.

When to use it:

Use provider commands during host setup, hardware diagnostics, and provider enrollment.

Exact help:

```text
Usage: hbse provider detect [OPTIONS]

Options:
      --device <DEVICE>  [default: /dev/tpmrm0]
  -h, --help             Print help
```

Examples:

```bash
hbse provider detect
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### provider test tpm2

Purpose:

Runs provider test tpm2 for HBSE binding support.

When to use it:

Use provider commands during host setup, hardware diagnostics, and provider enrollment.

Exact help:

```text
Usage: hbse provider test-tpm2 [OPTIONS]

Options:
      --device <DEVICE>  [default: /dev/tpmrm0]
  -h, --help             Print help
```

Examples:

```bash
hbse provider test-tpm2
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### provider test tpm2 direct

Purpose:

Runs provider test tpm2 direct for HBSE binding support.

When to use it:

Use provider commands during host setup, hardware diagnostics, and provider enrollment.

Exact help:

```text
Usage: hbse provider test-tpm2-direct [OPTIONS]

Options:
      --device <DEVICE>  [default: /dev/tpmrm0]
  -h, --help             Print help
```

Examples:

```bash
hbse provider test-tpm2-direct
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### provider test system fingerprint

Purpose:

Runs provider test system fingerprint for HBSE binding support.

When to use it:

Use provider commands during host setup, hardware diagnostics, and provider enrollment.

Exact help:

```text
Usage: hbse provider test-system-fingerprint

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse provider test-system-fingerprint
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### provider test yubikey piv

Purpose:

Runs provider test yubikey piv for HBSE binding support.

When to use it:

Use provider commands during host setup, hardware diagnostics, and provider enrollment.

Exact help:

```text
Usage: hbse provider test-yubikey-piv

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse provider test-yubikey-piv
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### provider enroll

Purpose:

Runs provider enroll for HBSE binding support.

When to use it:

Use provider commands during host setup, hardware diagnostics, and provider enrollment.

Exact help:

```text
Usage: hbse provider enroll [OPTIONS] <PROVIDER>

Arguments:
  <PROVIDER>

Options:
      --current-passphrase <CURRENT_PASSPHRASE>
      --new-passphrase <NEW_PASSPHRASE>
      --tpm-device <TPM_DEVICE>                  [default: /dev/tpmrm0]
  -h, --help                                     Print help
```

Examples:

```bash
hbse provider enroll passphrase
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## Model Provider Commands

#### model provider

Purpose:

Container for model-provider setup helpers.

When to use it:

Use it to create provider secrets and policies for model APIs used by Vegvisir.

Exact help:

```text
Usage: hbse model-provider <COMMAND>

Commands:
  list
  setup
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse model-provider list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### model provider list

Purpose:

Lists supported model provider presets.

When to use it:

Use this instead of manually writing policies for common model providers when possible.

Exact help:

```text
Usage: hbse model-provider list

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse model-provider list
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### model provider setup

Purpose:

Stores a model provider secret and creates matching policies.

When to use it:

Use this instead of manually writing policies for common model providers when possible.

Exact help:

```text
Usage: hbse model-provider setup [OPTIONS] <PRESET>

Arguments:
  <PRESET>

Options:
      --api-key-env <API_KEY_ENV>
      --stdin
      --secret-ref <SECRET_REF>
      --policy-id <POLICY_ID>
      --consumer <CONSUMER>
      --purpose <PURPOSE>                                  [default: model.chat]
      --model-discovery-purpose <MODEL_DISCOVERY_PURPOSE>  [default: model.discovery]
      --upstream-base-url <UPSTREAM_BASE_URL>
      --listen <LISTEN>
      --credential-header <CREDENTIAL_HEADER>
      --credential-prefix <CREDENTIAL_PREFIX>
      --max-body-bytes <MAX_BODY_BYTES>                    [default: 10485760]
      --require-mfa
      --passphrase <PASSPHRASE>
  -h, --help                                               Print help
```

Examples:

```bash
printf '%s' "$OPENAI_API_KEY" | hbse --vault "$HBSE_VAULT_PATH" model-provider setup openai \
  --stdin \
  --secret-ref secret://vegvisir/providers/openai/default \
  --consumer vegvisir.provider.openai-hbse \
  --purpose model.chat \
  --model-discovery-purpose model.discovery
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## MFA Commands

#### mfa

Purpose:

Container for MFA commands.

When to use it:

Use it to enroll, verify, and inspect TOTP state.

Exact help:

```text
Usage: hbse mfa <COMMAND>

Commands:
  enroll-totp
  verify-totp
  status
  help         Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse mfa status
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### mfa enroll totp

Purpose:

Enrolls a TOTP MFA factor.

When to use it:

Use MFA commands for policies or operations that require additional operator presence.

Exact help:

```text
Usage: hbse mfa enroll-totp [OPTIONS]

Options:
      --issuer <ISSUER>          [default: HBSE]
      --account <ACCOUNT>
      --passphrase <PASSPHRASE>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" mfa enroll-totp
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### mfa verify totp

Purpose:

Verifies a TOTP code.

When to use it:

Use MFA commands for policies or operations that require additional operator presence.

Exact help:

```text
Usage: hbse mfa verify-totp [OPTIONS] <CODE>

Arguments:
  <CODE>

Options:
      --passphrase <PASSPHRASE>
  -h, --help                     Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" mfa verify-totp 123456
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### mfa status

Purpose:

Shows MFA enrollment/status.

When to use it:

Use MFA commands for policies or operations that require additional operator presence.

Exact help:

```text
Usage: hbse mfa status

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse --vault "$HBSE_VAULT_PATH" mfa status
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## Broker Commands

#### broker

Purpose:

Container for broker daemon commands.

When to use it:

Use broker commands to serve secrets through policy-controlled delivery without exposing plaintext to Vegvisir.

Exact help:

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
  help             Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse broker status
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### broker status

Purpose:

Shows broker status.

When to use it:

Use broker commands for normal Vegvisir provider and MCP credential flow.

Exact help:

```text
Usage: hbse broker status [OPTIONS]

Options:
      --socket <SOCKET>  [default: /run/user/1000/hbse/broker.sock]
  -h, --help             Print help
```

Examples:

```bash
hbse broker status
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### broker unlock

Purpose:

Unlocks broker operation.

When to use it:

Use broker commands for normal Vegvisir provider and MCP credential flow.

Exact help:

```text
Usage: hbse broker unlock [OPTIONS]

Options:
      --socket <SOCKET>          [default: /run/user/1000/hbse/broker.sock]
      --passphrase <PASSPHRASE>
      --mfa-code <MFA_CODE>
  -h, --help                     Print help
```

Examples:

```bash
hbse broker unlock
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### broker mfa verify

Purpose:

Verifies MFA with the broker.

When to use it:

Use broker commands for normal Vegvisir provider and MCP credential flow.

Exact help:

```text
Usage: hbse broker mfa-verify [OPTIONS] <CODE>

Arguments:
  <CODE>

Options:
      --socket <SOCKET>  [default: /run/user/1000/hbse/broker.sock]
  -h, --help             Print help
```

Examples:

```bash
hbse broker mfa-verify 123456
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### broker lock

Purpose:

Locks the broker.

When to use it:

Use broker commands for normal Vegvisir provider and MCP credential flow.

Exact help:

```text
Usage: hbse broker lock [OPTIONS]

Options:
      --socket <SOCKET>  [default: /run/user/1000/hbse/broker.sock]
  -h, --help             Print help
```

Examples:

```bash
hbse broker lock
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### broker checkout

Purpose:

Checks out a secret through broker policy.

When to use it:

Use broker commands for normal Vegvisir provider and MCP credential flow.

Exact help:

```text
Usage: hbse broker checkout [OPTIONS] --secret-ref <SECRET_REF> --purpose <PURPOSE>

Options:
      --socket <SOCKET>                [default: /run/user/1000/hbse/broker.sock]
      --secret-ref <SECRET_REF>
      --consumer <CONSUMER>            [default: cli]
      --purpose <PURPOSE>
      --delivery-mode <DELIVERY_MODE>  [default: terminal_print]
  -h, --help                           Print help
```

Examples:

```bash
hbse broker checkout --secret-ref secret://vegvisir/providers/openai/default --consumer vegvisir.provider.openai-hbse --purpose model.chat
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### broker materialize

Purpose:

Materializes a secret through broker policy.

When to use it:

Use broker commands for normal Vegvisir provider and MCP credential flow.

Exact help:

```text
Usage: hbse broker materialize [OPTIONS] --secret-ref <SECRET_REF> --purpose <PURPOSE>

Options:
      --socket <SOCKET>                [default: /run/user/1000/hbse/broker.sock]
      --secret-ref <SECRET_REF>
      --consumer <CONSUMER>            [default: cli]
      --purpose <PURPOSE>
      --delivery-mode <DELIVERY_MODE>  [default: terminal_print]
      --allow-plaintext
  -h, --help                           Print help
```

Examples:

```bash
hbse broker materialize --secret-ref secret://vegvisir/services/example/default --consumer vegvisir.service.example --purpose service.access
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### broker provider http

Purpose:

Sends an HTTP provider request with credentials attached by the broker.

When to use it:

Use broker commands for normal Vegvisir provider and MCP credential flow.

Exact help:

```text
Usage: hbse broker provider-http [OPTIONS] --secret-ref <SECRET_REF> --purpose <PURPOSE> --url <URL>

Options:
      --socket <SOCKET>                          [default: /run/user/1000/hbse/broker.sock]
      --secret-ref <SECRET_REF>
      --consumer <CONSUMER>                      [default: cli]
      --purpose <PURPOSE>
      --method <METHOD>                          [default: GET]
      --url <URL>
      --header <HEADER>
      --body <BODY>
      --credential-header <CREDENTIAL_HEADER>    [default: Authorization]
      --credential-prefix <CREDENTIAL_PREFIX>    [default: "Bearer "]
      --timeout-seconds <TIMEOUT_SECONDS>        [default: 30]
      --max-response-bytes <MAX_RESPONSE_BYTES>  [default: 10485760]
  -h, --help                                     Print help
```

Examples:

```bash
hbse broker provider-http \
  --secret-ref secret://vegvisir/providers/openai/default \
  --consumer vegvisir.provider.openai-hbse \
  --purpose model.discovery \
  --method GET \
  --url https://api.openai.com/v1/models
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### broker cleanup socket

Purpose:

Cleans stale broker socket state.

When to use it:

Use broker commands for normal Vegvisir provider and MCP credential flow.

Exact help:

```text
Usage: hbse broker cleanup-socket [OPTIONS]

Options:
      --socket <SOCKET>  [default: /run/user/1000/hbse/broker.sock]
  -h, --help             Print help
```

Examples:

```bash
hbse broker cleanup-socket
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### broker install service

Purpose:

Installs systemd service/socket units for the broker.

When to use it:

Use broker commands for normal Vegvisir provider and MCP credential flow.

Exact help:

```text
Usage: hbse broker install-service [OPTIONS]

Options:
      --scope <SCOPE>                                [default: user]
      --unit-dir <UNIT_DIR>
      --socket <SOCKET>
      --idle-timeout-seconds <IDLE_TIMEOUT_SECONDS>  [default: 900]
      --broker-executable <BROKER_EXECUTABLE>
      --service-user <SERVICE_USER>
      --enable
      --start
      --dry-run
  -h, --help                                         Print help
```

Examples:

```bash
hbse broker install-service --scope user --broker-executable "$(command -v hbse-broker)" --enable --start
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## Dotenv Commands

#### dotenv

Purpose:

Container for dotenv scanning and execution commands.

When to use it:

Use it to find raw secrets and run commands using secret refs from environment files.

Exact help:

```text
Usage: hbse dotenv <COMMAND>

Commands:
  scan
  run
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse dotenv scan .env
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### dotenv scan

Purpose:

Scans a dotenv file for raw secrets and secret refs.

When to use it:

Use dotenv commands to migrate .env workflows toward secret:// references.

Exact help:

```text
Usage: hbse dotenv scan <PATH>

Arguments:
  <PATH>

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse dotenv scan .env
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### dotenv run

Purpose:

Runs a command using a dotenv file with HBSE-managed secret delivery.

When to use it:

Use dotenv commands to migrate .env workflows toward secret:// references.

Exact help:

```text
Usage: hbse dotenv run [OPTIONS] --purpose <PURPOSE> <PATH> [COMMAND]...

Arguments:
  <PATH>
  [COMMAND]...

Options:
      --consumer <CONSUMER>  [default: cli]
      --purpose <PURPOSE>
  -h, --help                 Print help
```

Examples:

```bash
hbse dotenv run --purpose service.access .env -- ./service
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.

## Release Commands

#### release

Purpose:

Container for release evidence and signing commands.

When to use it:

Use it to create and verify release artifacts for HBSE distribution.

Exact help:

```text
Usage: hbse release <COMMAND>

Commands:
  evidence
  keygen
  sign
  verify
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
hbse release evidence
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### release evidence

Purpose:

Generates release evidence.

When to use it:

Use release commands when packaging or verifying HBSE builds.

Exact help:

```text
Usage: hbse release evidence [OPTIONS]

Options:
      --output-dir <OUTPUT_DIR>      [default: release]
      --project-root <PROJECT_ROOT>  [default: .]
      --version <VERSION>            [default: 0.1.0]
  -h, --help                         Print help
```

Examples:

```bash
hbse release evidence
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### release keygen

Purpose:

Generates release signing keys.

When to use it:

Use release commands when packaging or verifying HBSE builds.

Exact help:

```text
Usage: hbse release keygen [OPTIONS] --private-key <PRIVATE_KEY> --public-key <PUBLIC_KEY>

Options:
      --private-key <PRIVATE_KEY>
      --public-key <PUBLIC_KEY>
      --encrypted
      --key-passphrase-env <KEY_PASSPHRASE_ENV>  [default: HBSE_RELEASE_KEY_PASSPHRASE]
  -h, --help                                     Print help
```

Examples:

```bash
hbse release keygen --private-key ./release.key --public-key ./release.pub
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### release sign

Purpose:

Signs release artifacts.

When to use it:

Use release commands when packaging or verifying HBSE builds.

Exact help:

```text
Usage: hbse release sign [OPTIONS] --private-key <PRIVATE_KEY>

Options:
      --release-dir <RELEASE_DIR>                [default: release]
      --private-key <PRIVATE_KEY>
      --public-key-out <PUBLIC_KEY_OUT>
      --artifact <ARTIFACT>
      --version <VERSION>                        [default: 0.1.0]
      --key-passphrase-env <KEY_PASSPHRASE_ENV>  [default: HBSE_RELEASE_KEY_PASSPHRASE]
  -h, --help                                     Print help
```

Examples:

```bash
hbse release sign --private-key ./release.key
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.


#### release verify

Purpose:

Verifies release signatures/evidence.

When to use it:

Use release commands when packaging or verifying HBSE builds.

Exact help:

```text
Usage: hbse release verify [OPTIONS]

Options:
      --release-dir <RELEASE_DIR>  [default: release]
      --public-key <PUBLIC_KEY>
  -h, --help                       Print help
```

Examples:

```bash
hbse release verify
```

Operational notes:

- Use `--vault` when operating on a non-default vault.
- Prefer brokered delivery for Vegvisir provider and MCP credentials.
- Inspect, test, list, or back up before disabling, destroying, promoting, rolling back, restoring, or recovering.
