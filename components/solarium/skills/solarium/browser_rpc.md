---
skill_id: solarium.browser_rpc
title: Solarium Browser RPC Tool Use
summary: Use Solarium's JSON-RPC/MCP-style stdio server as a controlled browser tool provider for agents.
domain: browser-automation
version: 0.1.0
inputs:
  - name: method
    description: JSON-RPC method or MCP-style tool name to call.
  - name: arguments
    description: Method/tool arguments, including URL, scope, actions, and artifact paths.
outputs:
  - name: json_rpc_response
    description: JSON-RPC response object from Solarium.
policies:
  - require_explicit_scope_for_external_targets
  - no_plaintext_credentials
  - capture_evidence_for_state_changing_workflows
---

# Solarium Browser RPC Tool Use

Use this skill when Vegvisir or another external agent needs browser capabilities through Solarium's stdio JSON-RPC server.

## Server startup

Development:

```bash
npm run dev -- server
```

Built package:

```bash
node dist/cli/index.js server
```

## Handshake

Send newline-delimited JSON-RPC messages on stdin. Read newline-delimited responses from stdout.

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
```

## Preferred call style

Use MCP-style `tools/call`:

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"solarium.inspect","arguments":{"url":"https://example.com","scope":{"allowedHosts":["example.com"],"authorizationNote":"Authorized inspection"},"headless":true}}}
```

Direct method calls are also accepted:

```json
{"jsonrpc":"2.0","id":4,"method":"solarium.scopeCheck","params":{"url":"https://example.com","scope":{"allowedHosts":["example.com"],"authorizationNote":"Authorized target"}}}
```

## Exposed tools

- `solarium.browse`
- `solarium.inspect`
- `solarium.plan`
- `solarium.session`
- `solarium.loop`
- `solarium.crawl`
- `solarium.audit`
- `solarium.scopeCheck`
- `solarium.validate`
- `solarium.profiles`
- `solarium.profile`
- `solarium.replay`
- `solarium.manifest`

## Operating procedure

1. Establish user-authorized scope before touching non-local web targets.
2. Call `solarium.scopeCheck` for the intended URL and scope.
3. Prefer `solarium.inspect` before acting, so selectors and page state are evidence-based.
4. Use `solarium.session` for deterministic multi-step actions.
5. Capture screenshots, HTML, markdown, JSONL events, or reports when the result must be reviewed later.
6. Return the Solarium response and a concise interpretation to the user.

## Failure handling

- If the RPC server returns `Method not found`, call `tools/list` and choose a supported tool.
- If scope fails, stop and request corrected authorized scope.
- If selectors fail, inspect the page again and choose more stable selectors.
- If authentication is required, use HBSE secret refs and/or prebuilt Solarium storage-state profiles; do not ask for plaintext credentials in chat.
