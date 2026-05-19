# Vegvisir System Package

This directory builds a local system-install bundle for Vegvisir.

The generated bundle contains:

- Vegvisir Rust source.
- CMS-v2 source as `third_party/CMS-v2`.
- HBSE Rust source as `third_party/HBSE/rust`.
- USRL source as `third_party/USRL`.
- A Cargo vendor directory for crates.io dependencies when `--vendor` is used.
- `install.sh` and `uninstall.sh`.

Build the bundle:

```bash
./packaging/package-system.sh
```

Install from the generated archive:

```bash
tar -xzf target/dist/vegvisir-system-*.tar.gz -C /tmp
cd /tmp/vegvisir-system-*
./install.sh --prefix "$HOME/.local" --hbse-service user --enable-hbse-service --start-hbse-service
```

For Debian-like systems, the installer can also install native build dependencies:

```bash
./install.sh --install-system-deps --prefix "$HOME/.local"
```

The installer never asks for model/provider secrets. Configure provider and service credentials through HBSE after install.

When USRL is installed, the installer writes `VEGVISIR_USRL_VALIDATOR_ROOT` into the environment example so Vegvisir and CMS-v2 use the bundled authoritative validator instead of the development path.
