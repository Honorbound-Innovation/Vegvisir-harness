# Vegvisir Runtime Architecture

This document describes how the Rust harness currently fits together. It is intentionally system-level rather than a command dump.

## Primary Crate

The runtime lives in:

```text
vegvisir/
```

Important source areas:

```text
vegvisir/src/main.rs                 CLI entrypoint and top-level subcommands
vegvisir/src/app.rs                  TUI application shell
vegvisir/src/app/runtime.rs          TUI runtime/event integration
vegvisir/src/orchestrator.rs         Agent harness execution loop
vegvisir/src/model.rs                Model trait and scripted/local model support
vegvisir/src/provider.rs             Provider abstraction and provider selection
vegvisir/src/openai_sso.rs           OpenAI SSO integration support
vegvisir/src/tools.rs                Tool registry and tool implementations
vegvisir/src/policy.rs               Tool/risk policy behavior
vegvisir/src/sandbox.rs              Workspace and command safety support
vegvisir/src/memory.rs               CMS-v2 integration
vegvisir/src/context.rs              Context preparation surfaces
vegvisir/src/mcp.rs                  MCP client/runtime support
vegvisir/src/lsl.rs                  Linked Skill Library parser/compiler/runtime commands
vegvisir/src/subagents.rs            Subagent supervisor and board records
vegvisir/src/bridge.rs               JSONL app-server bridge
vegvisir/src/compat_server.rs        OpenAI-compatible local server
vegvisir/src/verification.rs         Verification checks
vegvisir/src/evals.rs                Built-in eval surfaces
vegvisir/src/observability.rs        Event logging and trace surfaces
vegvisir/src/defaults/               Default prompts, agents, skills, and LSL examples
```

## CLI Entrypoint

`vegvisir/src/main.rs` defines the top-level `vegvisir` command with these current subcommands:

```text
tui
run
remember
recall
context
model-request
eval
verify
app-server
open-ai-compat-server
setup
skiller
```

Running `vegvisir` with no subcommand starts the TUI. Supplying `--prompt` or `run <goal>` executes a headless task.

Global options include provider/model/agent selection, JSON/scripted mode flags, workspace selection, max steps, and startup-only dangerous bypass mode.

## Agent Harness

The orchestrator is the part that turns a user goal into a bounded run. At a high level, a run has:

- a workspace path
- a model implementation
- a tool registry
- an optional persistent/custom agent profile
- runtime policy settings
- model/tool-call iteration limits
- memory/context support
- a checkpoint/final-answer result

The model does not directly mutate the world. It produces messages and tool-call requests; the harness decides whether a tool exists, whether it is allowed, whether approval is required, and how outputs are returned.

## TUI Runtime

The TUI is the normal operator interface. It provides:

- streaming provider output
- Markdown rendering
- full-width transcript body for readable selection/copy
- command palette and slash command selection
- workspace switching
- provider/model selection
- approvals modal
- transcript search
- activity/tool log surfaces
- tool inventory and command allow-list management
- context/memory/system prompt inspectors
- subagent board visibility

The TUI runtime is also responsible for surfacing important operational events in the transcript so tool work and child-agent work are not invisible.

## Headless Mode

Headless mode is useful for scripted tasks:

```bash
vegvisir --workspace /path/to/project run "Summarize this repository"
```

It should still respect workspace scope, provider configuration, tool policies, memory behavior, and max-step bounds. Use headless mode for automation where an interactive approvals UI is not needed or where policy is already configured for the run.

## App-server Bridge

The app-server bridge is a JSONL protocol for desktop shells or external clients. It is not the model provider. It is a control surface around a Vegvisir session.

Representative methods include:

- `initialize`
- `session.start`
- `session.messages`
- `session.exportMarkdown`
- `turn.send`
- `command.run`
- `session.status`
- `tools.list`
- `providers.list`
- `models.list`
- `agents.list`
- `approvals.list`
- `approvals.approveOnce`
- `approvals.approveSession`
- `approvals.edit`
- `approvals.deny`
- `diff.current`
- `memory.status`
- `system.prompt`
- `system.prompt.set`
- `shutdown`

See [overlay integration](overlay-integration.md) for the protocol details.

## Provider Flow

Provider/model selection is runtime configuration. The harness can use configured provider adapters and supports provider/model inspection surfaces.

Provider-sensitive behavior:

- streaming when available
- model discovery where supported
- OpenAI SSO where configured
- OpenAI-compatible local/server endpoints
- HBSE-backed provider credential access when configured
- scripted/local models for deterministic tests

The provider is responsible for generation. Vegvisir is responsible for assembling context, enforcing policy, executing tools, recording events, and reporting verification.

## Tool Registry

The tool registry is the model’s controlled interface to the workspace and environment. Tool categories include:

- filesystem reads/writes/listing within the workspace
- bounded shell commands and tests
- git/diff inspection
- CMS-v2 memory recall/write operations
- context/model-request helpers
- Skiller compile/validate/route/load/forge helpers
- MCP tool calls
- verification/eval/trace helpers
- subagent delegation

Tool behavior should be evidence-forward: important reads, writes, commands, errors, and verification results should be visible to the operator.

## Command And Filesystem Safety

Filesystem tools are workspace-scoped. Shell commands are allow-listed and bounded by timeout/output limits. Risky operations may require approval. Dangerous bypass mode exists for trusted high-risk sessions but can only be selected at startup.

Preserve-user-work rule: do not revert, delete, overwrite, reset, or discard unrelated user changes unless explicitly instructed.

## Memory Flow

Memory has three different responsibilities:

1. CMS-v2 durable storage and retrieval.
2. ECM/context preparation for the active turn.
3. Provider prompt use during generation.

The boundary is important: long-term storage is not the same thing as dumping every memory into every prompt. Vegvisir should recall relevant project memory first and use global memory only when cross-project context is clearly relevant.

## Skills And LSL Runtime

The runtime supports built-in skills and filesystem skills. LSL adds structured libraries with subskills, routing metadata, load blocks, dependency links, policy inheritance, eval hooks, traces, detection, curation, and promotion/archive operations.

Important TUI commands include:

```text
/skills status
/skills compile
/skills route <query>
/skills load [--tokens N] <query-or-subskill>
/skills explain <query-or-subskill>
/skills trace
/skills detect
/skills curate
/skills forge <library.subskill> | <title> | <summary> | <body> [| tags=a,b]
/skills eval [target-or-eval]
/skills promote <library.subskill>
/skills archive <library.subskill>
/skills patch <library.subskill> | <operation> | <path> | <value>
/skills invoke <subskill-id> [json-input]
```

Compiled artifacts live under `.vegvisir/compiled` in the workspace.

## Subagent Runtime

The subagent supervisor supports child task records and a board file. Subagent records include identity, workspace, goal, file scope, work budget, status, timestamps, checkpoint, final answer, and error.

Subagent events include:

- `subagent.queued`
- `subagent.started`
- `subagent.completed`
- `subagent.failed`
- `subagent.cancelled`

Subagents are best for bounded independent tasks. Do not delegate credential handling, destructive actions, ambiguous external side effects, or broad unsupervised implementation across overlapping files.

## Verification Surfaces

Use `vegvisir verify` for runtime/environment checks:

```bash
vegvisir verify all --workspace /path/to/project
vegvisir verify auth --workspace /path/to/project
vegvisir verify mcp --workspace /path/to/project
vegvisir verify runtime --workspace /path/to/project
vegvisir verify memory --workspace /path/to/project
```

Use `vegvisir eval` for built-in eval scopes or eval files:

```bash
vegvisir eval all
vegvisir eval memory
vegvisir eval security
vegvisir eval --file ./path/to/eval.json
```

For source changes, run focused tests and usually at least:

```bash
cargo check --workspace
```

For release-quality runtime changes, run:

```bash
cargo test --workspace -- --test-threads=1
```

## Runtime State Locations

Common state roots:

```text
${XDG_DATA_HOME:-$HOME/.local/share}/vegvisir/    user-level Vegvisir state and CMS database
<workspace>/.vegvisir/                           workspace-local compiled skills and run artifacts
~/.vegvisir/tools                                installed component runtimes and wrappers
```

Do not commit local runtime artifacts, databases, generated caches, downloaded browser state, provider credentials, or auth storage.

## Development Notes

- Keep command help docs generated or checked against current CLI behavior when commands change.
- Keep system docs grounded in source paths and current component responsibilities.
- Prefer adding architecture docs over bloating README.
- If `plan.md` is used locally, keep it ignored and untracked.
