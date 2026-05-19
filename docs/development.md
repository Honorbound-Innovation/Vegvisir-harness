# Development

## Build

```bash
cargo build --workspace
```

## Test

```bash
cargo test --workspace -- --test-threads=1
```

USRL:

```bash
cd components/usrl
npm install
npm run build
npm test
```

## Install Locally

Install the full harness from the repository root:

```bash
./install.sh
```

Install the full harness and configure a user HBSE broker service:

```bash
./install.sh --hbse-service user --enable-hbse-service --start-hbse-service
```

Remove installed binaries and generated install-time assets:

```bash
./uninstall.sh
```

Manual Vegvisir-only install:

```bash
cargo build -p vegvisir-rust --release
install -Dm755 target/release/vegvisir-rust "$HOME/.local/bin/vegvisir"
```

HBSE:

```bash
cargo build -p hbse --release
install -Dm755 target/release/hbse "$HOME/.local/bin/hbse"
install -Dm755 target/release/hbse-broker "$HOME/.local/bin/hbse-broker"
```

## Source Policy

This repository intentionally excludes generated artifacts, local SQLite state, local `.vegvisir` runtime data, dependency vendor folders, old design notes, and specification or roadmap documents. Keep new implementation work in source, tests, scripts, and usage documentation.
