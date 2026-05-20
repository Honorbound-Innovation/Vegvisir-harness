# Vegvisir Agent Harness

Vegvisir Agent Harness is a secure Rust agent runtime for people who want a powerful coding and operations assistant without handing the harness every secret, every memory, and every permission by default.

It brings the main pieces of the system into one repository: the Vegvisir Rust harness, CMS-v2 memory, HBSE secret brokering, USRL contracts, MCP client support, custom agents, skills, approvals, and workspace-aware sessions.

The goal is straightforward: make high-capability agent work practical, inspectable, and controlled. Vegvisir can run interactively in a terminal UI, run headlessly on an agent server, switch between projects, use persistent memory, call tools, and route provider or MCP credentials through HBSE instead of storing plaintext secrets in the harness.

## What Is Included

```text
Vegvisir-harness/
├── vegvisir/              # Rust harness, TUI, headless CLI, providers, tools, MCP, approvals
├── components/
│   ├── cms-v2/            # Continuum Memory System v2 runtime and CLI
│   ├── HBSE/              # Rust Hardware Bound Secrets Enclave implementation
│   └── usrl/              # USRL parser and contract runtime
├── docs/                  # Usage documentation for each included system
├── scripts/               # Helper scripts
├── install.sh             # Full-system installer
├── uninstall.sh           # Full-system uninstaller
└── LICENSE                # MIT license for included project code
```

## What Vegvisir Does

- Runs as a full TUI or as a headless CLI.
- Streams provider output into the harness when the provider supports streaming.
- Supports OpenAI, OpenAI-compatible providers, OpenAI SSO, HBSE-brokered OpenAI-compatible requests, Anthropic, Google, Azure OpenAI, and local/demo providers.
- Exposes workspace-scoped tools for file IO, command execution, tests, memory recall, MCP calls, and runtime plugins.
- Uses an approval queue for risky operations, with approve-once, approve-for-session, edit, and deny flows.
- Includes a startup-only dangerous bypass mode for explicitly trusted high-risk sessions.
- Uses CMS-v2 as the memory system, with global, user, project, workspace, session, and agent scopes.
- Uses HBSE for secret and auth isolation so provider and service credentials can stay outside the harness.
- Uses USRL contracts for tightly bounded workflows and regulated skills.
- Supports persistent custom agents with their own prompts, modes, memory scopes, tool permissions, skills, USRL bindings, MCP access, provider defaults, and model defaults.
- Supports workspace/project switching with the right session and memory scope restored.
- Renders Markdown in the TUI, including code fences and tables.
- Exposes a JSONL app-server bridge for future overlay interfaces and desktop shells.
- Includes verification, eval, trace, and audit surfaces for production hardening.

## Install

Prerequisites:

- Rust toolchain with Cargo.
- Node.js and npm for the USRL TypeScript package.
- Linux for the full HBSE broker service workflow.

Install the full system:

```bash
./install.sh
```

Install with a user HBSE broker service:

```bash
./install.sh --hbse-service user --enable-hbse-service --start-hbse-service
```

Install into a specific prefix:

```bash
./install.sh --prefix "$HOME/.local"
```

Uninstall:

```bash
./uninstall.sh
```

The installer puts these commands under `$prefix/bin`:

- `vegvisir`
- `vegvisir-rust`
- `cms-v2`
- `hbse`
- `hbse-broker`
- `usrl`

## Build And Test From Source

Build Rust crates:

```bash
cargo build --workspace
```

Run Rust tests:

```bash
cargo test --workspace -- --test-threads=1
```

Build and test USRL:

```bash
cd components/usrl
npm install
npm run build
npm test
```

## Basic Use

Start the TUI:

```bash
vegvisir
```

Run headlessly:

```bash
vegvisir --workspace /path/to/project --provider openai-hbse --model gpt-5.5 run "Summarize this repository"
```

Run the app-server bridge for an external overlay:

```bash
vegvisir --provider openai-hbse --model gpt-5.5 app-server --workspace /path/to/project
```

Check the installation:

```bash
vegvisir verify all --workspace /path/to/project
```

Use CMS-v2 directly:

```bash
cms-v2 --help
cms-v2 retrieve --user user:default --project /path/to/project "provider secrets"
```

Use HBSE directly:

```bash
hbse --help
hbse broker install-service --scope user --broker-executable "$(command -v hbse-broker)"
```

Use USRL directly:

```bash
usrl validate ./path/to/contract.usrl
```

## Documentation

The usage docs include command trees, explanations, and examples for the included systems.

- [Vegvisir usage](docs/vegvisir-usage.md)
- [CMS-v2 usage](docs/cms-v2-usage.md)
- [HBSE usage](docs/hbse-usage.md)
- [USRL usage](docs/usrl-usage.md)
- [USRL language reference](docs/usrl-language-reference.md)
- [Overlay integration](docs/overlay-integration.md)
- [MCP, tools, approvals, and security](docs/security-and-operations.md)
- [Development and release workflow](docs/development.md)

## License

This repository is distributed under the MIT License.

Copyright (c) 2026 Honorbound Innovation, LLC.
