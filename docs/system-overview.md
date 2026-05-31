# Vegvisir System Overview

Vegvisir is a local-first agentic software development harness. It is not just a chat frontend. It is the runtime that connects a model provider to an active workspace, scoped tools, durable memory, credential boundaries, skills, subagents, approvals, verification, and a transcript that records what happened.

The practical goal is simple: let an AI assistant do real engineering work while keeping the operator in control of scope, risk, credentials, and evidence.

## Current Repository Layout

```text
Vegvisir-harness/
├── vegvisir/                    # Rust harness: TUI, headless CLI, runtime, tools, providers, MCP, subagents
├── components/
│   ├── cms-v2/                  # Continuum Memory System v2: durable scoped memory and context prep
│   ├── HBSE/                    # Hardware Bound Secrets Enclave: brokered secrets and provider auth
│   ├── skiller/                 # Rust skill compiler, Forge workflow, registry, lifecycle, agent-pack tooling
│   ├── solarium/                # First-party Playwright browser automation / evidence runtime
│   ├── usrl/                    # TypeScript USRL parser/validator/runtime CLI
│   ├── ghidra/                  # Vendored Ghidra source tree for reverse-engineering tooling
│   ├── ghidra-mcp/              # Ghidra UI MCP bridge component
│   └── ghidra-headless-mcp/     # Headless Ghidra MCP bridge component
├── docs/                        # System documentation
├── scripts/                     # Helper scripts, including HBSE provider onboarding
├── install.sh / uninstall.sh    # Full-system install and removal helpers
├── upgrade.sh                   # Local upgrade helper
└── Cargo.toml                   # Rust workspace manifest
```

The Rust workspace currently includes:

- `vegvisir`
- `components/cms-v2`
- `components/HBSE/hbse`
- `components/skiller`

`components/solarium` and `components/usrl` are Node/TypeScript components. The Ghidra-related components are source/runtime integrations rather than Rust workspace crates.

## High-Level Architecture

```text
User / operator
      │
      ▼
Vegvisir TUI / CLI / app-server bridge
      │
      ├── provider adapters ───────────────► OpenAI / compatible / SSO / local/demo / configured providers
      │
      ├── tool registry ───────────────────► filesystem, shell, tests, git, MCP, memory, Skiller helpers
      │
      ├── CMS-v2 memory ───────────────────► durable scoped recall, history import, context packets
      │
      ├── ECM context exposure ─────────────► current-turn context budgeting and prompt assembly
      │
      ├── HBSE secret boundary ─────────────► brokered provider/service credentials, no plaintext memory
      │
      ├── skills / LSL / USRL ──────────────► reusable workflows, contract-bounded behavior, eval hooks
      │
      ├── subagent supervisor ──────────────► bounded child agents, board records, transcript updates
      │
      ├── MCP client layer ─────────────────► local or remote tool servers, preferably HBSE-backed when auth is needed
      │
      └── verification / eval / trace ──────► checks, diagnostics, audit trail, operational evidence
```

## Runtime Surfaces

Vegvisir has several operator-facing surfaces.

### TUI

The default `vegvisir` command starts the terminal UI. It is the main interactive workbench for coding sessions, project switching, tool use, approvals, memory, provider/model selection, command palette use, transcript search, and long-running agent work.

### Headless CLI

`vegvisir run <goal>` runs a bounded headless task. This is useful for scripted workflows, server-driven tasks, CI-like checks, and small automations.

### App-server bridge

`vegvisir app-server` exposes a JSONL bridge for desktop shells or external clients. The bridge supports session start, turn send, messages, command execution, approvals, tool listing, provider/model listing, memory status, diff inspection, and system prompt access.

### OpenAI-compatible server

`vegvisir open-ai-compat-server` exposes an OpenAI-compatible local server surface so compatible clients can talk through Vegvisir’s provider/runtime boundary.

### Integrated Skiller passthrough

`vegvisir skiller -- <args>` runs the Rust Skiller CLI from the monorepo, giving Vegvisir users access to skill compilation, validation, Forge artifacts, lifecycle reports, registry publication, and Agent Builder handoffs.

## Core Operating Loop

Vegvisir’s default agent loop is evidence-driven:

1. **Orient** — inspect the user request, workspace, relevant files, git status, memory, tools, and constraints.
2. **Plan** — choose a practical path, identify risky actions, and decide what verification is needed.
3. **Execute** — use allowed tools to edit files, run commands, call MCP tools, load skills, or delegate subagents.
4. **Verify** — run targeted tests, builds, checks, render passes, evals, or diagnostics before claiming success.
5. **Report** — summarize what changed, what was verified, what failed, and what remains.

That loop matters because the model is not treated as self-validating. The harness gives it tools, but the answer still has to be grounded in command output, source inspection, test results, diffs, and explicit uncertainty when evidence is missing.

## Memory And Context Boundary

Vegvisir separates long-term memory from active prompt context.

- **CMS-v2 owns durable memory.** It stores scoped memory records, imported history, retrieval indexes, writeback records, and model-request/cache artifacts.
- **ECM owns active context exposure.** It decides what relevant memory/context enters the current model request and keeps within budget.
- **The model/provider owns response generation.** Memory retrieval and prompt assembly are harness responsibilities; text generation is provider responsibility.

Important rule: CMS is for non-secret durable context. Store project decisions, repo roots, user preferences, task continuity, and design notes. Do not store plaintext credentials, private keys, tokens, or secret-bearing URLs.

## Secret Boundary

HBSE is the credential boundary. Vegvisir should not ask users to paste secrets into chat and should not persist credentials in memory or workspace docs. Provider and service credentials should be represented as secret references and accessed through HBSE-backed broker flows.

Typical secret-reference shape:

```text
secret://vegvisir/providers/openai/default
secret://vegvisir/mcp/github/default
secret://vegvisir/services/postgres/default
```

The user can operate Vegvisir with direct environment variables in development, but the intended secure posture is brokered access through HBSE.

## Tools And Approvals

Vegvisir’s tool posture is capable but bounded.

- Filesystem tools are scoped to the active workspace.
- Shell commands are allow-listed and output-limited.
- Risky operations can require approval.
- Dangerous bypass mode exists, but only at startup.
- The harness must preserve unrelated user work and avoid destructive actions unless explicitly authorized.

Approval and risky-tool enablement are separate concepts. A user may approve a proposed action, but if the runtime policy does not make the relevant tool available, the action still cannot run.

## Skills, LSL, And USRL

Vegvisir supports multiple skill forms:

- built-in/default skills from `vegvisir/src/defaults/skills.json`
- filesystem Markdown skills
- USRL-bound skills/contracts
- Linked Skill Libraries (`.lsl`) with routeable subskills, policies, dependencies, eval hooks, lifecycle state, and promotion rules
- Skiller-generated governed skill bundles

Skill roots include:

```text
<workspace>/.vegvisir/skills
<workspace>/skills
<data-root>/skills
```

LSL skills can be compiled into `.vegvisir/compiled`, routed by query, loaded within token budgets, evaluated, forged as candidates, patched, promoted, archived, traced, detected as missing, and curated.

USRL is the contract language used when workflows need explicit permissions, denials, stages, triggers, evidence requirements, or approval constraints.

Skiller is related but distinct: it compiles technical sources into governed skill bundles and lifecycle artifacts. Vegvisir consumes and executes skill/context workflows at runtime.

## Subagents

Vegvisir supports bounded child-agent delegation through the subagent supervisor.

A subagent task records:

- id and name
- workspace
- goal
- file scope
- work budget
- status
- timestamps
- checkpoint
- final answer
- error state

The board is serialized as JSON, and subagent events are emitted for queued, started, completed, failed, and cancelled states. Recent runtime work also mirrors subagent board updates into the parent transcript so delegated work remains visible to the operator.

Subagents are useful for reconnaissance, documentation review, test investigation, compatibility checks, security review, and design critique. They should be narrow, bounded, and non-overlapping when parallel implementation is involved.

## Solarium

Solarium is the first-party browser automation and evidence component under `components/solarium`. It is a Playwright-based Node/TypeScript runtime for legitimate web automation, browser research, QA, and authorized security testing.

Solarium can:

- browse pages and capture screenshots
- extract text and structured page observations
- run sessions from action files
- crawl within scope policies
- run audit, OWASP audit, and GraphQL audit workflows
- inspect pages, network/console evidence, and artifacts
- use browser profiles and auth-session references
- validate workflow/config files
- run or replay jobs
- emit skill seeds and manifests
- serve an automation interface

Solarium is not a license to target third parties without permission. Scope policies, authorization notes, and HBSE-backed auth/session handling are part of the intended boundary.

## Ghidra And Binary Intelligence Components

The monorepo also carries binary-intelligence tooling components:

- `components/ghidra` — vendored Ghidra source tree
- `components/ghidra-mcp` — UI-oriented Ghidra MCP bridge
- `components/ghidra-headless-mcp` — headless Ghidra MCP bridge

These components support reverse-engineering workflows, MCP surfaces, and future/active binary-analysis integrations. Runtime caches, venvs, Gradle outputs, installed distributions, and generated artifacts should live under the user runtime tool root, not in source control.

## Verification And Diagnostics

Vegvisir exposes several verification surfaces:

```bash
vegvisir verify all --workspace /path/to/project
vegvisir eval all
cargo test --workspace -- --test-threads=1
```

The TUI also exposes slash commands for verification, evals, traces, tool inventory, providers, models, memory, context, approvals, and workspace state.

For documentation-only changes, at minimum run:

```bash
cargo check --workspace
```

For runtime changes, prefer targeted tests plus the full workspace test suite when practical.

## What To Read Next

- [Vegvisir runtime architecture](runtime-architecture.md)
- [Skiller system](skiller-system.md)
- [Solarium system](solarium-system.md)
- [Vegvisir usage](vegvisir-usage.md)
- [CMS-v2 usage](cms-v2-usage.md)
- [HBSE usage](hbse-usage.md)
- [USRL usage](usrl-usage.md)
- [LSL skill system](lsl-skill-system.md)
- [Security and operations](security-and-operations.md)
- [Development workflow](development.md)
