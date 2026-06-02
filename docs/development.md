# Development

This document covers the common development workflow for the Vegvisir monorepo.

For architecture context, read [system overview](system-overview.md) and [runtime architecture](runtime-architecture.md) first.

## Build

Build all Rust workspace crates:

```bash
cargo build --workspace
```

Check without producing final binaries:

```bash
cargo check --workspace
```

Build Node/TypeScript components when touching them:

```bash
cd components/usrl
npm install
npm run build
npm test
```

```bash
cd components/solarium
npm install
npm run build
npm test
```

## Test

Run the Rust workspace tests:

```bash
cargo test --workspace -- --test-threads=1
```

Run focused tests while iterating, then run broader checks before publishing clean runtime changes.

Examples:

```bash
cargo test -p vegvisir-rust subagent -- --nocapture
cargo test -p skiller
cargo test -p cms-v2
```

For documentation-only changes, at least run:

```bash
cargo check --workspace
```

## Install Locally

Install into the default prefix:

```bash
./install.sh
```

Install to a specific prefix:

```bash
./install.sh --prefix "$HOME/.local"
```

Install with a user HBSE broker service:

```bash
./install.sh --hbse-service user --enable-hbse-service --start-hbse-service
```

Prepare an optional low-privilege runtime account and workspace root for hardened headless deployments:

```bash
sudo ./install.sh --install-vegvisir-user --workspace-root /srv/vegvisir-workspaces
```

Uninstall:

```bash
./uninstall.sh
```

Upgrade a local install:

```bash
./upgrade.sh
```

## Runtime Verification

Verify the installed runtime against a workspace:

```bash
vegvisir verify all --workspace /path/to/project
```

Useful narrower scopes:

```bash
vegvisir verify auth --workspace /path/to/project
vegvisir verify mcp --workspace /path/to/project
vegvisir verify runtime --workspace /path/to/project
vegvisir verify memory --workspace /path/to/project
```

Run built-in evals:

```bash
vegvisir eval all
vegvisir eval memory
vegvisir eval security
```

## Source Policy

The repository should contain source and intentional documentation/fixtures only.

Do not commit:

- `target/`
- `node_modules/`
- `dist/`
- `.vegvisir/` runtime artifacts
- `.solarium/` runtime artifacts
- SQLite databases
- logs, temp files, backups
- environment files
- provider credentials
- browser storage-state files
- downloaded artifacts
- local scratch plans such as `plan.md`

The root `.gitignore` uses a broad Markdown ignore to keep ad-hoc notes out of the repo while allowing README/docs/component documentation. If you add a first-class Markdown documentation location, update `.gitignore` deliberately.

## Documentation Workflow

When changing commands or runtime behavior:

1. Update source and tests.
2. Update the relevant usage reference in `docs/`.
3. Update architecture/system docs if responsibilities or boundaries changed.
4. Run focused tests and `cargo check --workspace`.
5. Check links and grep for stale names.
6. Commit docs with the code change when they describe the same behavior.

Core docs:

- [System overview](system-overview.md)
- [Runtime architecture](runtime-architecture.md)
- [Skiller system](skiller-system.md)
- [Solarium system](solarium-system.md)
- [Security and operations](security-and-operations.md)

## Component Development

### Vegvisir Rust harness

Primary source:

```text
vegvisir/src
```

Run:

```bash
cargo check -p vegvisir-rust
cargo test -p vegvisir-rust
```

### CMS-v2

Primary source:

```text
components/cms-v2
```

Run:

```bash
cargo check -p cms-v2
cargo test -p cms-v2
```

### HBSE

Primary source:

```text
components/HBSE
```

Run crate-specific checks from the workspace or component package as appropriate.

### Skiller

Primary source:

```text
components/skiller
```

Run:

```bash
cargo check -p skiller
cargo test -p skiller
```

### USRL

Primary source:

```text
components/usrl
```

Run:

```bash
cd components/usrl
npm run build
npm test
```

### Desktop app

Primary source:

```text
components/desktop
```

The desktop app is a Tauri/TypeScript shell over `vegvisir app-server`. It must preserve the existing harness boundary rather than reimplementing providers, tools, memory, secrets, approvals, or policy in the GUI.

Linux desktop build prerequisites include Tauri/WebKit/DBus development packages. On Debian/Ubuntu-like systems install at least:

```bash
sudo apt install pkg-config libdbus-1-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev
```

Run:

```bash
cd components/desktop
npm install
npm run check
cargo check --manifest-path src-tauri/Cargo.toml
```

See [Desktop app](desktop-app.md).

### Solarium

Primary source:

```text
components/solarium
```

Run:

```bash
cd components/solarium
npm run build
npm test
```

Install Playwright browsers when needed:

```bash
npm run install:browsers
```

## Git Hygiene

Before committing:

```bash
git status --short --branch
git diff --stat
git diff --check
```

For docs:

```bash
grep -R "old-term" -n docs README.md
```

For code:

```bash
cargo check --workspace
cargo test --workspace -- --test-threads=1
```

Commit only coherent changes. Do not stage unrelated user work.
