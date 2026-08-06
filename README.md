# Vegvisir Agent Harness

Vegvisir is a local-first agentic software development harness for people who want an AI engineering assistant that can actually work inside a repository without being handed every secret, every permission, and every memory by default.

It is not just a chat window. Vegvisir connects a model provider to an active workspace, scoped tools, durable memory, governed skills, subagents, browser evidence, approvals, verification, and a transcript that records what happened. The point is practical software work: inspect the repo, make the change, run the check, show the diff, preserve the evidence, and keep the operator in control.

Vegvisir is designed first for serious engineering workflows: code maintenance, debugging, documentation, migrations, automation, security-aware review, reverse-engineering support, browser-driven evidence capture, and long-running project sessions. It can automate work inside those boundaries, but it is not a generic "do anything on the internet" agent. The harness is intentionally shaped around workspaces, policy, memory scope, tool scope, secret isolation, and verification.

## Screenshots

Vegvisir is built around a terminal workbench that keeps the conversation, tool log, session state, context budget, skills, and command surfaces visible while work is happening.

![Vegvisir verification and Solarium session trace](docs/assets/screenshots/vegvisir-readme-1.jpg)

*Verification output, Solarium notes, tool activity, session state, context usage, and the active input surface in one workspace-bound TUI.*

![Vegvisir command palette and skill routing](docs/assets/screenshots/vegvisir-readme-2.jpg)

*Command palette, persistent agents, approvals, context tools, Skiller routing, and session tool logs during an active project session.*

![Vegvisir long-running agent session](docs/assets/screenshots/vegvisir-readme-3.jpg)

*Long-running agent work with memory, tools, Ghidra/Skiller/CMS references, shell/test evidence, and transcript continuity.*

## What Is Included

```text
Vegvisir-harness/
├── vegvisir/                    # Rust harness: TUI, headless CLI, tools, providers, MCP, approvals, subagents
├── components/
│   ├── cms-v2/                  # Continuum Memory System v2: durable scoped memory and context prep
│   ├── HBSE/                    # Hardware Bound Secrets Enclave: brokered secrets and provider auth
│   ├── skiller/                 # Governed skill compiler, Forge workflow, registry, lifecycle, agent packs
│   ├── solarium/                # Playwright browser automation and evidence runtime
│   ├── usrl/                    # USRL parser, validator, and contract runtime
│   └── ghidra-headless-mcp/     # Headless Ghidra MCP bridge for an installed Ghidra runtime
├── docs/                        # Architecture, usage references, and component documentation
├── scripts/                     # Helper scripts, including HBSE/provider onboarding helpers
├── install.sh                   # Full-system installer
├── upgrade.sh                   # Local upgrade helper
├── uninstall.sh                 # Full-system uninstaller
└── LICENSE                      # MIT license for included project code
```

The Rust workspace currently includes the Vegvisir harness, CMS-v2, HBSE, and Skiller. Solarium and USRL are Node/TypeScript components. Ghidra support uses an installed Ghidra runtime plus the first-party headless MCP bridge.

## What Vegvisir Does

- Runs as a full terminal UI, a bounded headless CLI, a JSONL app-server bridge, or an OpenAI-compatible local server surface.
- Connects model providers to a real engineering runtime instead of leaving them as detached text generators.
- Supports configured providers including OpenAI/OpenAI-compatible flows, OpenAI SSO, HBSE-brokered provider access, Anthropic, Google, Azure OpenAI, and local/demo providers.
- Exposes workspace-scoped tools for file IO, command execution, tests, git/diff inspection, memory recall, MCP calls, Skiller helpers, verification, evals, runtime plugins, and bounded subagent delegation.
- Uses CMS-v2 for durable scoped memory and ECM-style context exposure so relevant project facts can survive sessions without dumping the entire attic into every prompt.
- Uses HBSE as the secret boundary so provider and service credentials can be brokered through secret references instead of pasted into chat or stored in memory.
- Supports persistent custom agents with their own prompts, modes, memory scopes, tool permissions, skills, USRL bindings, MCP access, and provider/model defaults.
- Supports Skiller as a first-class governed skill compiler for turning docs, repos, APIs, CLI help, and technical evidence into source-grounded skill bundles, Forge workflows, lifecycle reports, registry artifacts, and Agent Builder handoffs.
- Supports Linked Skill Libraries and USRL contracts for routeable workflows, policy-bound behavior, eval hooks, approvals, and reusable skill execution.
- Supports bounded subagents for reconnaissance, documentation review, test investigation, compatibility checks, security review, and design critique, with durable board records, explicit file scopes, work budgets, status inspection, cancellation, and active scope-conflict protection.
- Integrates Solarium as the first-party browser automation/evidence runtime for screenshots, observations, scoped crawls, audits, GraphQL audit workflows, profiles, auth-session references, replay, and workflow seed generation.
- Carries a headless Ghidra MCP bridge for binary-intelligence and reverse-engineering workflows against an installed Ghidra runtime.
- Includes verification, eval, trace, audit, approval, sandbox-status, subagent-board, and tool-inventory surfaces for keeping high-capability sessions inspectable.

## Runtime Model

Vegvisir separates responsibilities deliberately:

```text
User / operator
      │
      ▼
Vegvisir TUI / CLI / bridge
      │
      ├── provider adapters ───────► model generation
      ├── tool registry ───────────► scoped filesystem, shell, tests, git, MCP, memory, Skiller
      ├── CMS-v2 ──────────────────► durable memory and retrieval
      ├── ECM context prep ────────► active-turn context exposure and budgeting
      ├── HBSE ────────────────────► secret references and brokered credentials
      ├── skills / LSL / USRL ─────► reusable workflows and policy contracts
      ├── subagents ───────────────► bounded child-agent work with board records
      ├── Solarium ────────────────► browser automation and evidence capture
      └── verification/evals ──────► checks before claims
```

The default work loop is:

1. **Orient** from the user goal, workspace, git state, files, memory, tools, and constraints.
2. **Plan** the smallest coherent path and identify risky actions or approvals.
3. **Execute** with tools, edits, commands, MCP calls, skills, or bounded subagents.
4. **Verify** with focused tests, builds, render passes, evals, diagnostics, and diff review.
5. **Report** what changed, what was verified, what failed, and what remains.

The model thinks. Vegvisir gives it hands, memory, rules, a workspace, and an evidence trail. Capable, but not feral.

## Terminal UI

The default `vegvisir` command opens the native terminal interface. The TUI is built for long-running agent work rather than a raw text stream:

- Provider responses stream into the chat view when the provider supports streaming.
- Scrolling up pauses follow mode so new output does not steal your place; `End` returns to the live bottom.
- Native terminal text selection is enabled by default, so output can be selected and copied using the terminal's normal mouse/context-menu behavior.
- `PageUp`, `PageDown`, `Home`, and `End` navigate long output.
- `Ctrl+P` opens the command palette, and `/` opens slash command selection from an empty input.
- Slash command selection supports arrow keys, paging, `Home`, `End`, and `Enter`.
- `Ctrl+F` opens transcript search. Use `Enter`/`Down` for the next match, `Up` for the previous match, and `Esc` to close search.
- Approval prompts are shown in-session. Use `Enter`/`A` to approve once, `S` to approve for the session, and `D` to deny.
- `Ctrl+C` cancels an in-flight response first. If no response is running, it exits the TUI.
- Markdown responses render code fences, tables, lists, diffs, and common source languages.
- Inspector overlays keep command output readable for `/models`, `/tools`, `/context`, `/system`, `/providers`, `/approvals`, `/work`, and related inventory commands.

Useful TUI commands:

```text
/help                 show commands and controls
/models               list or refresh models for the active provider
/provider             inspect, switch, compare, or diagnose providers
/model                inspect, switch, or compare models
/workspace            switch project workspace and restore its active session
/tools                inspect or adjust tool permissions
/tools commands       list, add, remove, or reset allowed shell commands
/tool-limit           show or set max tool-call rounds per model turn
/approvals            inspect, explain, and manage pending tool approvals
/runs                 inspect local run artifact bundles
/diff                 show current workspace diff
/work                 show recent activity, tool calls, and command events
/system               print the active system prompt
/context              inspect prepared context and latest captured context
/agent                create, select, and inspect persistent custom agents
```

### Autonomy, Sandboxing, And Delegation Controls

Recent Vegvisir builds make high-capability sessions more controllable and easier to audit:

- Tool-call rounds are unlimited by default, with `/tool-limit <rounds>` and `VEGVISIR_MAX_TOOL_ROUNDS` available when a session needs a hard cap. If a cap is reached, Vegvisir reports a recoverable cutoff instead of silently losing the turn.
- `run_command` is executable allow-listed, timeout-bound, output-limited, and can route non-allow-listed commands through the approval queue.
- Obvious network-like command requests can require explicit approval even when the executable itself is allowed.
- Filesystem tools are workspace-scoped and hardened against path traversal and unsafe symlink escapes.
- Optional command OS sandboxing is configured with `VEGVISIR_COMMAND_SANDBOX=path|none|bwrap|strict-bwrap`, with network and mount controls for hardened local sessions.
- `--dangerously-bypass-approvals-and-sandbox` remains startup-only and is reported in `/tools status`, `/verify runtime`, and app-server status payloads.
- Run artifacts can be browsed from the TUI with `/runs list`, `/runs show <id>`, `/runs diff <id>`, `/runs replay-plan <id>`, plus `/context last`, `/memory used-this-turn`, `/memory writes-this-session`, and `/memory why <id>`.
- Provider/model surfaces include `/provider compare`, `/provider diagnose`, and `/model compare` for local capability inspection without exposing plaintext secrets.
- `/auto level <0-6>` records explicit operator autonomy posture while preserving approvals, workspace containment, secret boundaries, and sandbox policy.
- `/goal start path/to/specification.md` runs a separate unbounded specification implementation loop until every planned exit criterion has validated evidence; use `/goal status` or `/goal stop` to inspect/control it.
- Subagents are tracked as bounded workers with durable board records. Use `/subagents list`, `/subagents show <id-or-name>`, `/subagents cancel <id-or-name>`, and `/subagents policy`.
- Provider reasoning summaries, when surfaced by a provider/model, are hidden from the chat transcript so only the assistant answer is displayed and persisted.
- STDIO MCP calls are timeout-bound and can restart once after an initial failure, making local MCP integrations less fragile.

See [New runtime features](docs/new-runtime-features.md), [Command sandboxing and approvals](docs/command-sandboxing-and-approvals.md), and [Subagent delegation](docs/subagent-delegation.md) for the detailed operator guidance.

## Install

For a fresh Linux machine, use the complete bootstrap path:

```bash
./install.sh --complete
```

`--complete` installs native/system dependencies, bootstraps Rust/Cargo if missing, builds Rust CLIs, installs npm dependencies for USRL/Solarium/desktop, runs `npm audit fix`, installs Playwright browsers, initializes HBSE with provider auto-detection, and installs/enables/starts a user HBSE broker service.

If you prefer explicit phases, install native/system dependencies first:

```bash
sudo bash scripts/install-system-deps.sh
```

Then install the full system:

```bash
./install.sh
```

Install with explicit HBSE setup and user broker service:

```bash
./install.sh --hbse-init auto --hbse-service user --enable-hbse-service --start-hbse-service --hbse-run-doctor
```

Force the system-fingerprint HBSE fallback on a TPM-less host:

```bash
./install.sh --hbse-init system-fingerprint --hbse-service user --enable-hbse-service --start-hbse-service
```

Install into a specific prefix:

```bash
./install.sh --prefix "$HOME/.local"
```

Prepare an optional low-privilege runtime account and workspace root for hardened headless deployments:

```bash
sudo ./install.sh --install-vegvisir-user --workspace-root /srv/vegvisir-workspaces
```

Upgrade an existing local install:

```bash
./upgrade.sh
```

Run a complete upgrade that also refreshes dependencies, repairs npm audit issues, and reapplies HBSE setup checks:

```bash
./upgrade.sh --complete
```

Uninstall:

```bash
./uninstall.sh
```

The installer places these commands under `$prefix/bin` where applicable. Optional component flags such as `--no-solarium`, `--no-biw`, `--no-ghidra`, `--no-ghidra-headless-mcp`, `--no-desktop`, `--no-skiller`, `--no-usrl`, `--no-hbse`, and `--no-cms-cli` can omit individual systems. npm handling is controlled with `--npm-audit <off|check|fix|force>`, and Playwright browser installation with `--install-playwright-browsers` / `--no-playwright-browsers`. When Ghidra is enabled, the installer discovers an existing Ghidra installation from `GHIDRA_HOME`, `GHIDRA_HEADLESS`, or PATH and creates `ghidra`/`analyzeHeadless` wrappers that point to that installation. The upgrade script reruns `install.sh` from the upgraded source, and the uninstall script removes these installed commands and component trees unless data is explicitly kept. See [Installation and upgrade](docs/install-upgrade.md) for the complete operator guide.

- `vegvisir`
- `vegvisir-rust`
- `cms-v2`
- `hbse`
- `hbse-broker`
- `skiller`
- `usrl`
- `solarium`
- `biw`
- `ghidra`
- `analyzeHeadless`
- `ghidra-headless`
- `ghidra-headless-mcp`
- `vegvisir-desktop`

## Build And Test From Source

Build Rust crates:

```bash
cargo build --workspace
```

Check Rust crates:

```bash
cargo check --workspace
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

Build and test Solarium:

```bash
cd components/solarium
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

Write a local run artifact bundle for review/audit evidence:

```bash
vegvisir --workspace /path/to/project --artifacts run "Summarize this repository"
vegvisir --workspace /path/to/project --artifact-dir ./audit-runs eval golden
vegvisir --workspace /path/to/project --artifact-dir ./audit-runs verify runtime
```

Run the app-server bridge for an external app or desktop shell:

```bash
vegvisir --provider openai-hbse --model gpt-5.5 app-server --workspace /path/to/project
```

Run the OpenAI-compatible local server surface:

```bash
vegvisir open-ai-compat-server --host 127.0.0.1 --port 11434
```

Verify the installation/runtime:

```bash
vegvisir verify all --workspace /path/to/project
```

Run with a stricter command sandbox:

```bash
VEGVISIR_COMMAND_SANDBOX=strict-bwrap vegvisir --workspace /path/to/project
```

Run with an explicit tool-round cap for deterministic automation:

```bash
VEGVISIR_MAX_TOOL_ROUNDS=24 vegvisir --workspace /path/to/project run "Inspect and summarize the repo"
```

Run evals:

```bash
vegvisir eval all
```

Use the integrated Skiller component:

```bash
vegvisir skiller -- compile ./docs --out ./dist/docs-skills --name docs-skills --domain vegvisir-operations
vegvisir skiller -- validate ./dist/docs-skills
vegvisir skiller -- route ./dist/docs-skills "how does HBSE provider auth work"
vegvisir skiller -- eval ./dist/docs-skills
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

Use Solarium directly:

```bash
cd components/solarium
npm run dev -- browse https://example.com --observe --extract-text
```

## Security Posture

Vegvisir is permissive enough to get work done, but the harness keeps important boundaries explicit:

- Do not paste plaintext credentials into chat.
- Store durable project facts in CMS-v2, not secrets.
- Use HBSE-backed secret references for provider, MCP, service, and browser-auth credentials where configured.
- Keep risky tools disabled unless the session needs them.
- Treat approval and tool enablement as separate controls.
- Keep filesystem and command work scoped to the active workspace.
- Preserve unrelated user work.
- Use Solarium only for owned, public, or explicitly authorized browser/security work.
- Run verification before claiming success.

## Documentation

Start with the system docs when you need the real architecture, then use the usage references for command-level detail.

- [Documentation index](docs/README.md)
- [System overview](docs/system-overview.md)
- [Runtime architecture](docs/runtime-architecture.md)
- [Desktop app](docs/desktop-app.md)
- [Skiller system](docs/skiller-system.md)
- [Solarium system](docs/solarium-system.md)
- [Vegvisir usage](docs/vegvisir-usage.md)
- [New runtime features](docs/new-runtime-features.md)
- [Command sandboxing and approvals](docs/command-sandboxing-and-approvals.md)
- [Subagent delegation](docs/subagent-delegation.md)
- [CMS-v2 usage](docs/cms-v2-usage.md)
- [HBSE usage](docs/hbse-usage.md)
- [USRL usage](docs/usrl-usage.md)
- [USRL language reference](docs/usrl-language-reference.md)
- [Linked Skill Libraries](docs/lsl-skill-system.md)
- [App bridge integration](docs/overlay-integration.md)
- [MCP, tools, approvals, and security](docs/security-and-operations.md)
- [Development and release workflow](docs/development.md)

## License

This repository is distributed under the MIT License.

Copyright (c) 2026 Honorbound Innovation, LLC.
