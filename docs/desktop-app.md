# Vegvisir Desktop App

Vegvisir Desktop is the graphical shell track for Vegvisir. Its purpose is to make the full harness usable outside the terminal without losing the features, safety boundaries, and operational evidence that make Vegvisir useful.

The desktop app must not become a separate assistant runtime. It is a client for the existing Vegvisir harness.

## Architecture Decision

The desktop shell uses the existing `vegvisir app-server` JSONL bridge as its control boundary.

```text
Vegvisir Desktop UI
      │
      ▼
Tauri command layer
      │ spawn/stdin/stdout JSONL
      ▼
vegvisir app-server
      │
      ├── provider adapters
      ├── workspace-scoped tools
      ├── CMS-v2 / ECM context
      ├── HBSE secret boundary
      ├── approvals and policy
      ├── skills / LSL / USRL
      ├── Skiller helpers
      ├── Solarium integration surface
      ├── MCP clients
      ├── subagents
      └── verification / eval / trace surfaces
```

The GUI owns presentation and user interaction. The harness remains authoritative for providers, memory, secrets, tools, approvals, policy, MCP, skill execution, and workspace mutations.

## Current Scaffold

The initial scaffold lives in:

```text
components/desktop/
```

It contains:

```text
components/desktop/
├── package.json                 # Tauri/Vite/TypeScript scripts
├── tsconfig.json                # TypeScript config
├── vite.config.ts               # Vite dev server config
├── index.html                   # frontend entry
├── postcss.config.js            # Tailwind/PostCSS pipeline
├── tailwind.config.js           # Vegvisir desktop theme tokens
├── src/
│   ├── main.ts                  # desktop UI and bridge client
│   └── styles.css               # Tailwind entrypoint and shared component classes
└── src-tauri/
    ├── Cargo.toml               # Tauri backend crate
    ├── build.rs
    ├── tauri.conf.json
    └── src/main.rs              # spawns and controls `vegvisir app-server`
```


## Layout Direction

The current visual pass uses TailwindCSS and a T3Code-inspired workbench layout provided by the user as a reference. The intent is not to clone that app, but to apply the same useful structure to Vegvisir:

- left project/module rail for sessions and feature panels;
- top title/action bar for current task, bridge state, and high-value actions;
- central transcript/workbench with low-noise message cards;
- translucent tool/event cards for operational evidence;
- large bottom composer plus slash-command row;
- compact footer rail for local/workspace/status hints.

The compass/stave design direction should remain practical: rails, panels, focus glow, and telemetry density are good; decorative radial controls that hurt transcript, diff, or log reading are not.

## Runtime Behavior

The Tauri backend exposes commands to the frontend:

- `bridge_start` — spawn `vegvisir app-server --workspace <path>` with optional provider/model/agent and startup dangerous-bypass flag.
- `bridge_send` — send one JSON request to the app-server stdin.
- `bridge_poll` — collect stdout/stderr JSONL events from the app-server and report fast bridge exits.
- `bridge_status` — report whether the bridge process is still running and clear dead child state.
- `bridge_stop` — request shutdown and terminate the bridge process.

The frontend auto-starts the bridge on launch by default. Users can disable auto-start in Settings.

The frontend then uses the bridge methods documented in [overlay integration](overlay-integration.md), including:

- `initialize`
- `session.status`
- `session.messages`
- `turn.send`
- `command.run`
- `tools.list`
- `providers.list`
- `models.list`
- `agents.list`
- `approvals.list`
- `approvals.approveOnce`
- `approvals.approveSession`
- `approvals.deny`
- `diff.current`
- `memory.status`
- `system.prompt`
- `shutdown`

## Feature Parity Contract

The desktop app must preserve these Vegvisir feature areas.

| Feature area | Desktop requirement | Authority |
| --- | --- | --- |
| Chat/session turns | Send user turns, stream assistant deltas, display transcript, cancel or stop work when bridge support exists. | Vegvisir app-server |
| Workspace selection | Select/switch project workspace and restore workspace-scoped session/memory. | Vegvisir app-server |
| Providers/models | List and select configured providers/models without exposing plaintext secrets. | Vegvisir provider layer / HBSE |
| Agents | List/select persistent custom agents and show active agent context. | Vegvisir agent/profile system |
| Tools | Show tool inventory, risky state, command allow-list surfaces, and tool-call progress. | Vegvisir tool registry/policy |
| Approvals | Show pending risky actions and support approve once, approve session, edit, deny. | Vegvisir approval queue |
| Memory/context | Show CMS-v2 memory status and active context behavior. | CMS-v2 / ECM |
| HBSE | Present onboarding/status only; never read or display plaintext secrets. | HBSE |
| Skiller | Provide GUI entry points for compile/validate/route/load/eval/Forge via bridge commands or future structured methods. | Vegvisir + Skiller |
| Solarium | Provide GUI entry points for browser evidence workflows through Vegvisir/Solarium integration, not ad-hoc browser automation. | Vegvisir + Solarium |
| MCP | List/configure MCP tools via Vegvisir surfaces, with HBSE-backed auth where needed. | Vegvisir MCP layer |
| Subagents | Show subagent board/status/final answers and make delegation visible in the work log. | Vegvisir subagent supervisor |
| Diff/review | Display current diff, staged diff, file-specific diff, and review summaries. | Vegvisir/git tooling |
| Verification/evals | Run and display `verify`, `eval`, test, build, and render-check output. | Vegvisir commands/tools |
| Transcript/export | Export Markdown transcript and preserve evidence trail. | Vegvisir session/export methods |
| System prompt | Inspect and optionally edit prompt through bridge methods. | Vegvisir config/session path |
| Security policy | Respect approvals, workspace scope, dangerous-bypass startup-only behavior, and preserve-user-work rules. | Vegvisir policy layer |

## Initial UI Panels

The first scaffold includes these panels:

- **Chat** — transcript, assistant streaming buffer, user turn composer, slash command runner.
- **Work log** — raw bridge events for tool/progress/debug visibility.
- **Approvals** — pending approval queue and approve/deny actions.
- **Tools** — tool inventory and risky marker display.
- **Providers** — provider/model/agent data.
- **Diff** — current diff view.
- **Memory** — memory status view.
- **System** — effective system prompt view.
- **Settings** — app-server binary, workspace, provider, model, agent, and startup dangerous-bypass flag.

This is intentionally broad because the desktop shell must be feature-preserving from the beginning.

## What The Desktop App Must Not Do

The desktop app must not:

- call model providers directly;
- store provider or service credentials;
- read HBSE secrets;
- write directly to CMS-v2 databases;
- execute shell commands outside Vegvisir tool policy;
- mutate workspace files outside Vegvisir tool policy;
- implement its own approval bypass;
- silently enable dangerous bypass mode after startup;
- hide tool calls, subagent work, or verification failures.

If the GUI needs a capability that the bridge does not expose yet, add a bridge method. Do not fork the runtime in the GUI.

## Near-Term Implementation Plan

1. Stabilize the Tauri scaffold and build pipeline.
2. Add structured bridge events for tool calls, checkpoints, subagent updates, and verification progress.
3. Add bridge methods for provider/model switching after session start.
4. Add bridge methods for subagent board listing and detail inspection.
5. Add bridge methods for Skiller workflows beyond slash-command passthrough.
6. Add bridge methods for Solarium jobs and evidence artifacts.
7. Add real Markdown rendering in the desktop transcript.
8. Add transcript search/export and file/diff review panes.
9. Add workspace picker and recent-workspace persistence.
10. Package desktop releases after parity-critical bridge gaps are closed.

## Development Commands

From the desktop component directory:

```bash
cd components/desktop
npm install
npm run check
npm run dev
npm run build
```

The desktop app auto-starts the bridge on launch and expects a `vegvisir` binary by default. Packaged GUI apps do not always inherit the interactive shell `PATH`, so the Tauri backend searches:

- the configured binary path;
- the inherited `PATH`;
- `$HOME/.local/bin/vegvisir`;
- `$HOME/bin/vegvisir`;
- `/usr/local/bin/vegvisir`;
- `/usr/bin/vegvisir`;
- `/bin/vegvisir`;
- directories near the desktop executable.

If the AppImage opens but cannot start the bridge, open **Settings** and set **Vegvisir binary** to an absolute path, for example:

```text
/home/malice/.local/bin/vegvisir
```

Bridge startup failures are shown in the UI instead of silently disappearing.

## Verification

For scaffold changes, first ensure the native Tauri prerequisites are installed. Without them, `cargo check --manifest-path src-tauri/Cargo.toml` fails before checking Vegvisir code because crates such as `libdbus-sys` cannot find system `.pc` files.

```bash
cd components/desktop
npm run check
cargo check --manifest-path src-tauri/Cargo.toml
```

For full harness confidence:

```bash
cargo check --workspace
```

If bridge protocol behavior changes, update both this document and [overlay integration](overlay-integration.md).
