# Vegvisir Installation And Upgrade Guide

This guide covers the complete source-based installation path for the full Vegvisir monorepo.

## What the installer covers

`./install.sh` now handles the full stack instead of only copying a few binaries:

- Native OS packages through `scripts/install-system-deps.sh` on `apt-get`, `dnf`, and `pacman` systems.
- Rust/Cargo checks and optional Rust bootstrap through rustup when Cargo is missing.
- Rust release builds for `vegvisir-rust`, `cms-v2`, `hbse`, `hbse-broker`, and `skiller`.
- Node/npm dependency installation for USRL, Solarium, and the desktop web assets.
- npm vulnerability repair through `npm audit fix` by default, with optional `check`, `off`, or `force` modes.
- Playwright browser installation for Solarium by default.
- Python venv creation for Ghidra headless MCP and Python component wrappers.
- Ghidra wrapper discovery using `GHIDRA_HOME`, `GHIDRA_HEADLESS`, or PATH.
- HBSE vault initialization, provider auto-detection, broker service installation, doctor/readiness checks, and model-provider credential onboarding.
- Optional hardened Vegvisir runtime user and workspace root creation.

## Complete install on a fresh Linux machine

From the repository root:

```bash
./install.sh --complete
```

`--complete` expands to the practical first-machine path:

- install system dependencies;
- bootstrap Rust/Cargo if missing;
- install/build all enabled components;
- run npm audit repair;
- install Playwright browsers;
- initialize HBSE using provider auto-detection;
- install, enable, and start a user HBSE broker service;
- run `hbse doctor`.

If the host has no TPM but has a stable system fingerprint, `--complete` initializes HBSE with `system-fingerprint`. If no TPM or system fingerprint is available, set a local passphrase environment variable before running the installer:

```bash
export HBSE_PASSPHRASE='use-a-local-secret-not-chat'
./install.sh --complete
```

Do not paste that value into chat, docs, Git, logs, or CMS memory.

## Explicit HBSE setup choices

Initialize with provider auto-detection:

```bash
./install.sh --install-system-deps \
  --hbse-init auto \
  --hbse-service user \
  --enable-hbse-service \
  --start-hbse-service \
  --hbse-run-doctor
```

Force system fingerprint:

```bash
./install.sh \
  --hbse-init system-fingerprint \
  --hbse-service user \
  --enable-hbse-service \
  --start-hbse-service \
  --hbse-run-doctor
```

Force TPM2 direct:

```bash
./install.sh \
  --hbse-init tpm2-direct \
  --hbse-tpm-device /dev/tpmrm0 \
  --hbse-service user \
  --enable-hbse-service \
  --start-hbse-service
```

Use a custom vault path:

```bash
./install.sh \
  --hbse-vault "$HOME/.local/share/hbse/vault.db" \
  --hbse-init auto \
  --hbse-service user \
  --enable-hbse-service \
  --start-hbse-service
```

## Model provider onboarding through HBSE

The installer can store a model provider API key into HBSE without putting the secret into command arguments. Provide the credential via a local environment variable:

```bash
OPENAI_API_KEY='local-secret-value' ./install.sh \
  --hbse-init auto \
  --hbse-model-provider openai \
  --hbse-model-api-key-env OPENAI_API_KEY \
  --hbse-model-secret-ref secret://vegvisir/providers/openai/default \
  --hbse-model-consumer vegvisir.provider.openai-hbse
```

The installer passes the env var name to `hbse model-provider setup`; it does not require you to paste the secret into chat.

## npm dependency and vulnerability modes

Default behavior is:

```bash
./install.sh --npm-audit fix
```

Available modes:

```bash
./install.sh --npm-audit off    # no npm audit step
./install.sh --npm-audit check  # fail if audit reports issues
./install.sh --npm-audit fix    # run npm audit fix, warn if unresolved
./install.sh --npm-audit force  # run npm audit fix --force, warn if unresolved
```

Use `force` only when you are comfortable with semver-major updates that may require follow-up testing.

## Playwright/Solarium browsers

Solarium installs npm dependencies and builds TypeScript during install. Browser binaries are installed by default:

```bash
./install.sh --install-playwright-browsers
```

Skip them for server images where browser runtime is not needed:

```bash
./install.sh --no-playwright-browsers
```

Make browser installation a hard failure:

```bash
./install.sh --require-playwright-browsers
```

## Upgrades

Normal upgrade:

```bash
./upgrade.sh
```

Complete upgrade with dependency repair and HBSE checks:

```bash
./upgrade.sh --complete
```

Pass installer options after `--`:

```bash
./upgrade.sh --force -- \
  --install-system-deps \
  --npm-audit fix \
  --hbse-init auto \
  --hbse-service user \
  --enable-hbse-service \
  --start-hbse-service
```

The upgrade script refuses to overwrite uncommitted local changes when run inside a matching Git checkout. Commit/stash first, or use `--no-sync-checkout` to install from a temporary clone.

## Focused verification

After install or upgrade:

```bash
vegvisir verify all --workspace /path/to/Vegvisir-harness
hbse --vault "${HBSE_VAULT_PATH:-$HOME/.local/share/hbse/vault.db}" doctor
hbse --vault "${HBSE_VAULT_PATH:-$HOME/.local/share/hbse/vault.db}" vault status
```

For development/source verification:

```bash
cargo check --workspace
cd components/usrl && npm ci && npm audit fix && npm run build && npm test
cd components/solarium && npm ci && npm audit fix && npm run build && npm test
cd components/desktop && npm ci && npm audit fix && npm run check && npm run web:build
```

## Notes and boundaries

- Ghidra itself is not redistributed by Vegvisir. Install Ghidra separately and set `GHIDRA_HOME` or `GHIDRA_HEADLESS` if wrapper installation is enabled.
- HBSE `system-fingerprint` is a fallback, not hardware-backed security. It can break after OS reinstall, VM identity changes, or hardware identity changes. Create HBSE recovery material before migrations.
- Never commit provider keys, passphrases, recovery mnemonics, tokens, or secret-bearing URLs.
