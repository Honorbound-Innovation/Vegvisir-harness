# Vegvisir Overlay Integration

This branch is the development lane for adapting a richer terminal or desktop overlay to Vegvisir without moving Vegvisir's security, memory, provider, or tool runtime out of Rust.

The overlay should be treated as presentation and interaction. Vegvisir remains the authority for:

- provider routing and model selection
- CMS-v2 memory and project/user scope
- HBSE-backed secrets and brokered provider access
- USRL policy and skill contracts
- MCP configuration and authenticated service access
- tool inventory, guardrails, approvals, checkpoints, and audit traces
- workspace/session switching

Do not embed the current Vegvisir TUI inside an overlay. The integration target is a process bridge that lets a better UI drive Vegvisir headlessly.

## Development Branch

Overlay work belongs on:

```bash
git switch vegvisir-overlay-integration
```

Keep `main` releasable while the overlay protocol and UI integration mature.

## App Server Command

Vegvisir now exposes a JSONL app-server bridge:

```bash
vegvisir app-server --workspace /path/to/project
```

Global runtime options still apply:

```bash
vegvisir \
  --provider openai-hbse \
  --model gpt-5.5 \
  --agent agent-red \
  app-server \
  --workspace /path/to/project
```

High-risk sessions must still be enabled at startup:

```bash
vegvisir \
  --dangerously-bypass-approvals-and-sandbox \
  app-server \
  --workspace /path/to/project
```

The bridge reads one JSON request per line from stdin and writes one JSON event per line to stdout.

## Protocol Shape

Every request has:

```json
{
  "id": "client-request-id",
  "method": "method.name",
  "params": {}
}
```

Every event has:

```json
{
  "type": "event.name",
  "id": "client-request-id",
  "payload": {}
}
```

Errors are reported as:

```json
{
  "type": "error",
  "id": "client-request-id",
  "payload": {
    "code": "request_failed",
    "message": "human-readable error"
  }
}
```

## Methods

### `initialize`

Returns the active session snapshot.

```json
{"id":"1","method":"initialize","params":{}}
```

Response event:

```json
{
  "type": "session.status",
  "id": "1",
  "payload": {
    "workspace": "/path/to/project",
    "session_id": "abc123",
    "provider": "openai-hbse",
    "model": "gpt-5.5",
    "agent": null,
    "status": "ready",
    "messages": 0,
    "tokens_used": 0,
    "last_latency_ms": 0,
    "dangerously_bypass_approvals_and_sandbox": false,
    "tools_enabled": 13,
    "pending_approvals": 0
  }
}
```

### `session.start`

Starts or switches to a session for a workspace. Provider, model, and agent are optional overrides.

```json
{
  "id": "2",
  "method": "session.start",
  "params": {
    "workspace": "/path/to/project",
    "provider": "openai-hbse",
    "model": "gpt-5.5",
    "agent": "agent-red"
  }
}
```

Response event:

```json
{"type":"session.started","id":"2","payload":{"workspace":"/path/to/project"}}
```

The payload is the full session snapshot.

### `turn.send`

Sends a user turn through the same provider/CMS/tool runtime used by the headless CLI.

```json
{
  "id": "3",
  "method": "turn.send",
  "params": {
    "content": "Inspect this workspace and explain the current test layout."
  }
}
```

Events:

```json
{"type":"turn.started","id":"3","payload":{"session_id":"abc123","workspace":"/path/to/project"}}
{"type":"content.delta","id":"3","payload":{"role":"assistant","text":"..."}}
{"type":"turn.completed","id":"3","payload":{"session_id":"abc123"}}
```

Current implementation emits the assistant content as one `content.delta`. The protocol already uses a delta event so provider-native streaming can be wired through without changing overlay clients.

### `command.run`

Runs a Vegvisir slash command and returns the output.

```json
{
  "id": "4",
  "method": "command.run",
  "params": {
    "command": "/approvals"
  }
}
```

Response event:

```json
{
  "type": "command.completed",
  "id": "4",
  "payload": {
    "command": "/approvals",
    "output": "No pending approvals.",
    "session": {}
  }
}
```

### `session.status`

Returns the current session snapshot.

```json
{"id":"5","method":"session.status","params":{}}
```

### `shutdown`

Cleanly terminates the app-server loop.

```json
{"id":"6","method":"shutdown","params":{}}
```

## Overlay Responsibilities

An overlay should own:

- pane layout
- message rendering
- semantic copy actions
- native terminal selection mode where possible
- diff and review screens
- approval prompts and controls
- command palette
- searchable transcript/log/diff views
- task timeline and progress presentation
- keyboard and mouse interaction

It should not own:

- plaintext provider credentials
- CMS database writes outside Vegvisir APIs
- HBSE policy enforcement
- USRL policy decisions
- direct provider API calls that bypass Vegvisir's provider layer
- direct tool execution that bypasses Vegvisir guardrails

## Near-Term Integration Work

1. Add true streaming to `turn.send` by forwarding provider deltas as they arrive.
2. Add structured events for tool calls, approvals, diffs, checkpoints, and task steps.
3. Add an approval response method that maps overlay buttons to Vegvisir approval ledger operations.
4. Add transcript and diff export methods.
5. Build or adapt an overlay frontend against this protocol on this branch.
6. Keep the current built-in TUI available as a fallback.

