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

`workspace.switch` is an alias for `session.start`. Use it when the UI action is explicitly a project/workspace switch.

### `session.messages`

Returns the current session messages as structured JSON.

```json
{"id":"messages","method":"session.messages","params":{}}
```

### `session.exportMarkdown`

Returns a Markdown transcript suitable for copying, saving, or exporting.

```json
{"id":"export","method":"session.exportMarkdown","params":{}}
```

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

Provider-native streaming is forwarded as one or more `content.delta` events. Non-streaming providers may still emit a single delta containing the full response.

If a risky tool call needs approval, the bridge emits `approval.required` and then `turn.failed`. The overlay should show the approval request and let the user approve once, approve for session, edit, or deny.

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

### `tools.list`

Returns tool schemas and runtime permission state.

```json
{"id":"tools","method":"tools.list","params":{}}
```

Response event:

```json
{
  "type": "tools.list",
  "id": "tools",
  "payload": {
    "tools": [
      {
        "name": "read_file",
        "description": "Read a workspace-scoped file",
        "parameters": {},
        "risky": false
      }
    ],
    "risky_tools_enabled": false,
    "human_approval_required": true,
    "dangerously_bypass_approvals_and_sandbox": false
  }
}
```

### `providers.list`

Returns the provider catalog and current provider selection.

```json
{"id":"providers","method":"providers.list","params":{}}
```

The payload includes:

- `current_provider`
- `providers`
- `availability`

### `models.list`

Returns the model catalog and current model selection.

```json
{"id":"models","method":"models.list","params":{}}
```

The payload includes:

- `current_model`
- `models`

### `agents.list`

Returns persisted custom agents and the active agent selection.

```json
{"id":"agents","method":"agents.list","params":{}}
```

### `approvals.list`

Returns pending risky tool approval requests.

```json
{"id":"approvals","method":"approvals.list","params":{}}
```

Response event:

```json
{
  "type": "approvals.list",
  "id": "approvals",
  "payload": {
    "approvals": [
      {
        "id": "123",
        "reason": "Risky tool requires human approval: run_command",
        "tool_name": "run_command",
        "args": {
          "command": ["rm", "-rf", "build"]
        },
        "risk_label": "risky"
      }
    ]
  }
}
```

### `approvals.approveOnce`

Approves one matching tool call.

```json
{"id":"approve","method":"approvals.approveOnce","params":{"id":"123"}}
```

### `approvals.approveSession`

Approves the same tool and argument pattern for the current running session.

```json
{"id":"trust","method":"approvals.approveSession","params":{"id":"123"}}
```

### `approvals.edit`

Replaces the queued approval arguments and returns the updated queue.

```json
{
  "id": "edit",
  "method": "approvals.edit",
  "params": {
    "id": "123",
    "args": {
      "command": ["rm", "-rf", "target/tmp-only"]
    }
  }
}
```

### `approvals.deny`

Rejects a pending risky tool request.

```json
{"id":"deny","method":"approvals.deny","params":{"id":"123"}}
```

Approval mutations return:

```json
{
  "type": "approvals.updated",
  "id": "approve",
  "payload": {
    "ok": true,
    "approvals": []
  }
}
```

### `diff.current`

Returns the current workspace diff as rendered Markdown.

```json
{
  "id": "diff",
  "method": "diff.current",
  "params": {
    "staged": false,
    "stat": false,
    "path": "vegvisir/src/bridge.rs"
  }
}
```

### `memory.status`

Returns CMS-v2 scope information and the human-readable `/memory status` output.

```json
{"id":"memory","method":"memory.status","params":{}}
```

### `system.prompt`

Returns the active effective harness system prompt.

```json
{"id":"system","method":"system.prompt","params":{}}
```

### `system.prompt.set`

Sets the active harness system prompt and persists it through the same config/session path used by the TUI.

```json
{
  "id": "system-set",
  "method": "system.prompt.set",
  "params": {
    "prompt": "You are the default Vegvisir agent..."
  }
}
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

1. Add structured events for tool calls, checkpoints, and task steps.
2. Add transcript and diff export methods.
3. Build or adapt an overlay frontend against this protocol on this branch.
4. Add a T3 Code-compatible driver package once the frontend shell target is selected.
5. Keep the current built-in TUI available as a fallback.
