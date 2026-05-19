# Vegvisir Agent Harness

Vegvisir Agent Harness is a secure local-first agentic development harness. It combines a Rust terminal and headless agent runtime with CMS-v2 memory, HBSE secret isolation, USRL governed execution, MCP client support, custom persistent agents, skills, approvals, and workspace-aware sessions.

The project is designed for users who want the power of modern coding agents without giving the harness direct access to long-lived secrets or dumping all memory into every model request.

## Repository Layout

```text
Vegvisir-harness/
├── vegvisir/              # Rust harness, TUI, headless CLI, providers, tools, MCP, approvals
├── components/
│   ├── cms-v2/            # Continuum Memory System v2 runtime and CLI
│   ├── HBSE/              # Rust Hardware Bound Secrets Enclave implementation
│   └── usrl/              # USRL parser and contract runtime
├── docs/                  # Usage documentation for the included systems
├── scripts/               # Local setup helpers
└── LICENSE                # MIT license for included project code
```

## Core Features

- Rust TUI and headless CLI for agentic development workflows.
- Streaming provider integration across OpenAI, OpenAI-compatible, SSO, HBSE-brokered, Anthropic, Google, Azure OpenAI, and local/demo providers.
- Tool execution for file IO, commands, tests, memory recall, MCP tools, and custom runtime plugins.
- Human approval queue for risky operations, plus startup-only full bypass mode for explicitly trusted high-risk sessions.
- CMS-v2 memory substrate with global, user, project, workspace, session, and agent scoping.
- HBSE zero-knowledge secret delivery where Vegvisir requests provider/service access without reading plaintext credentials.
- USRL contract skills for tightly bounded regulated workflows.
- Persistent custom agents with dedicated prompts, modes, memory scopes, tool permissions, skills, USRL bindings, MCP exposure, provider, and model defaults.
- Workspace/project switching with relevant session and memory scope restoration.
- Markdown rendering in the TUI, including code fences, common language labels, and tables.
- MCP client support with HBSE-backed auth for remote HTTP MCP services.
- Eval, verification, trace, and audit commands for production hardening.

## Quick Start

Prerequisites:

- Rust toolchain with Cargo.
- Node.js and npm for the USRL TypeScript package.
- Linux is the primary runtime target for full HBSE service integration.

Install the full system:

```bash
./install.sh
```

Install with a user HBSE broker service:

```bash
./install.sh --hbse-service user --enable-hbse-service --start-hbse-service
```

Uninstall:

```bash
./uninstall.sh
```

Build all Rust crates:

```bash
cargo build --workspace
```

Run the test suite:

```bash
cargo test --workspace -- --test-threads=1
```

Build the USRL package:

```bash
cd components/usrl
npm install
npm run build
```

Install Vegvisir locally:

```bash
cargo build -p vegvisir-rust --release
install -Dm755 target/release/vegvisir-rust "$HOME/.local/bin/vegvisir"
```

Start the TUI:

```bash
vegvisir tui
```

Run headlessly:

```bash
vegvisir --workspace /path/to/project --provider openai-hbse --model gpt-5.5 run "Summarize this repository"
```

## Documentation

- [Vegvisir usage](docs/vegvisir-usage.md)
- [CMS-v2 usage](docs/cms-v2-usage.md)
- [HBSE usage](docs/hbse-usage.md)
- [USRL usage](docs/usrl-usage.md)
- [MCP, tools, approvals, and security](docs/security-and-operations.md)
- [Development and release workflow](docs/development.md)

## License

This repository is distributed under the MIT License.

Copyright (c) 2026 Honorbound Innovation, LLC.
