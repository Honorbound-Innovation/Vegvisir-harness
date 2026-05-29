# Vegvisir Agent Harness (RC-1: Usable but still a little rough)

Vegvisir Agent Harness is a secure Rust software development harness for people who want a powerful coding and engineering assistant without handing the harness every secret, every memory, and every permission by default.

It brings the main pieces of the system into one repository: the Vegvisir Rust harness, CMS-v2 memory, HBSE secret brokering, USRL contracts, MCP client support, custom agents, skills, approvals, and workspace-aware sessions.

The goal is straightforward: make high-capability software development work practical, inspectable, and controlled. Vegvisir can run interactively in a terminal UI, run headlessly on an agent server, switch between projects, use persistent memory, call development tools, and route provider or MCP credentials through HBSE instead of storing plaintext secrets in the harness.

Vegvisir is not a general AI automation assistant in the style of broad task automation systems. It is designed first as a secure software development system: project workspaces, coding workflows, controlled tool access, memory scoped to engineering context, auditable actions, approval gates, and zero-knowledge secret handling. It can support automation inside those development workflows, but it should not be classified as a general-purpose automation agent harness.

## What Is Included

```text
Vegvisir-harness/
├── vegvisir/              # Rust harness, TUI, headless CLI, providers, tools, MCP, approvals
├── components/
│   ├── cms-v2/            # Continuum Memory System v2 runtime and CLI
│   ├── HBSE/              # Rust Hardware Bound Secrets Enclave implementation
│   ├── skiller/           # Rust skill compiler and governed skill bundle tooling
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
- Keeps risky tools disabled by default. Risky file, command, external, destructive, or privileged actions must be manually enabled for the current session before they can be used.
- Treats risky-tool enablement and action approval as separate controls. If risky tools are not enabled for the session, the agent cannot run those tools even when the user approves or asks for the action.
- Uses an approval queue for risky operations when risky tools are enabled, with approve-once, approve-for-session, edit, and deny flows.
- Includes a startup-only dangerous bypass mode for explicitly trusted high-risk sessions.
- Uses CMS-v2 as the memory system, with global, user, project, workspace, session, and agent scopes.
- Uses HBSE for secret and auth isolation so provider and service credentials can stay outside the harness.
- Uses USRL contracts for tightly bounded workflows and regulated skills.
- Supports persistent custom agents with their own prompts, modes, memory scopes, tool permissions, skills, USRL bindings, MCP access, provider defaults, and model defaults.
- Supports workspace/project switching with the right session and memory scope restored.
- Renders Markdown in the TUI, including code fences and tables.
- Exposes a JSONL app-server bridge for future external interfaces and desktop shells.
- Integrates Skiller as a first-class Vegvisir feature for compiling technical sources, repos, API specs, CLI specs, and docs into governed, source-grounded skill bundles, Forge workflows, lifecycle reports, and Agent Builder handoff artifacts.
- Includes verification, eval, trace, and audit surfaces for production hardening.

## Terminal UI

The default `vegvisir` command opens the native terminal interface. The TUI is built for long-running agent work rather than a raw text stream:

- Provider responses stream into the chat view as they arrive.
- Scrolling up pauses follow mode so new output does not steal your place; `End` returns to the live bottom.
- Native terminal text selection is enabled by default, so model output can be selected and copied with the terminal's normal mouse and context-menu behavior. The default TUI keeps the conversation in one full-width body so normal drag selection does not cross into status or work-log panes.
- Use `PageUp`, `PageDown`, `Home`, and `End` to move through long chat output without taking over terminal text selection.
- `Ctrl+P` opens the command palette, and `/` opens slash command selection from an empty input.
- Slash command selection supports arrow keys, `PageUp`, `PageDown`, `Home`, `End`, and `Enter` to run the selected command.
- `Ctrl+F` opens transcript search. Type a query, use `Enter` or `Down` for the next match, `Up` for the previous match, and `Esc` to close search.
- Approval prompts are shown as an in-session modal. Use `Enter` or `A` to approve once, `S` to allow the matching action for the current session, and `D` to deny. The older `1`, `2`, and `3` shortcuts still work.
- `Ctrl+C` cancels an in-flight model response first. If no response is running, it exits the TUI.
- Markdown responses render with structured handling for code fences, tables, lists, diffs, and common source languages.
- Inspector overlays keep command output readable for inventory-style commands such as `/models`, `/tools`, `/context`, `/system`, `/providers`, `/approvals`, and `/work`.

Useful TUI commands:

```text
/help                 show commands and controls
/models               list or refresh models for the active provider
/provider             inspect or switch provider
/model                inspect or switch model
/workspace            switch project workspace and restore its active session
/tools                inspect or adjust tool permissions
/tools commands       list, add, remove, or reset allowed shell commands
/tool-limit           show or set max tool-call rounds per model turn
/approvals            inspect pending tool approvals
/diff                 show current workspace diff
/work                 show recent activity, tool calls, and command events
/system               print the active system prompt
/context              inspect prepared context and memory behavior
```

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

Run the app-server bridge for an external app or future overlay:

```bash
vegvisir --provider openai-hbse --model gpt-5.5 app-server --workspace /path/to/project
```

Use the integrated Skiller component:

```bash
vegvisir skiller -- compile ./docs --out ./dist/docs-skills --name docs-skills --domain kubernetes-operations
vegvisir skiller -- validate ./dist/docs-skills
vegvisir skiller -- eval ./dist/docs-skills
vegvisir skiller -- propose-agents ./dist/docs-skills --out ./dist/agents
vegvisir skiller -- verify-agent-proposals ./dist/agents
vegvisir skiller -- build-agent-pack ./dist/docs-skills --agent "Cluster Diagnostic Agent" --out ./dist/cluster-agent --report ./dist/cluster-agent-build-report.yaml
vegvisir skiller -- verify-agent-pack ./dist/cluster-agent
vegvisir skiller -- agent-builder-summary --proposals ./dist/agents --pack ./dist/cluster-agent --out ./dist/agent-builder-summary.yaml
vegvisir skiller -- agent-artifact-index ./dist --out ./dist/agent-artifacts.yaml
```

Skiller is an integrated Vegvisir feature, not a replacement for Vegvisir runtime systems. It adds governed skill compilation, Forge request/response artifacts, corpus lifecycle reports, registry publication, and Agent Builder handoff packages while Vegvisir continues to own runtime execution, memory, secrets, approvals, traces, evals, and app bridge behavior.

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
- [App bridge integration](docs/overlay-integration.md)
- [MCP, tools, approvals, and security](docs/security-and-operations.md)
- [Development and release workflow](docs/development.md)

## License

This repository is distributed under the MIT License.

Copyright (c) 2026 Honorbound Innovation, LLC.
