# Solarium JSON-RPC / MCP-style server

Solarium can run as a newline-delimited stdio JSON-RPC 2.0 server for external agents and MCP-style clients.

Start the server:

```bash
npm run dev -- server
# or, after build:
node dist/cli/index.js server
# or, after package install:
solarium server
```

Each request is one JSON object per line. Each response is one JSON object per line. The server supports MCP-style lifecycle and tool methods:

- `initialize`
- `notifications/initialized`
- `ping`
- `tools/list`
- `tools/call`

It also supports direct JSON-RPC calls where the method is a Solarium tool name, such as `solarium.browse`.

## MCP client configuration

For MCP clients that support stdio servers, the command shape is:

```json
{
  "mcpServers": {
    "solarium": {
      "command": "solarium",
      "args": ["server"]
    }
  }
}
```

For local repository development without an installed package:

```json
{
  "mcpServers": {
    "solarium": {
      "command": "node",
      "args": ["/absolute/path/to/Solarium/dist/cli/index.js", "server"]
    }
  }
}
```

Run `npm run build` first when using the `dist` path.

## Initialize

Request:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{"name":"example-agent","version":"0.1.0"},"capabilities":{}}}
```

Response:

```json
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"solarium","version":"0.1.0"},"capabilities":{"tools":{"listChanged":false}},"instructions":"Solarium provides scoped browser automation tools for authorized browsing, inspection, sessions, crawling, audits, evidence manifests, and validation. Prefer explicit scope policies for browser-affecting tools; never pass plaintext secrets in tool arguments."}}
```

After initialization, MCP clients may send:

```json
{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}
```

## List tools

```json
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
```

## Call a tool MCP-style

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"solarium.browse","arguments":{"url":"https://example.com","observe":true,"headless":true,"scope":{"allowedHosts":["example.com"],"authorizationNote":"Authorized example"}}}}
```

`tools/call` returns MCP-style `content`, `isError`, and `structuredContent` containing the raw Solarium result.

## Call a tool directly

```json
{"jsonrpc":"2.0","id":4,"method":"solarium.scopeCheck","params":{"url":"https://example.com","scope":{"allowedHosts":["example.com"],"authorizationNote":"Authorized example"}}}
```

## Available tools

- `solarium.browse` — open a URL; optionally observe, extract text, or screenshot.
- `solarium.inspect` — inspect a page and return selector/action candidates.
- `solarium.plan` — generate conservative session actions from an inspect result.
- `solarium.session` — run an `AgentAction[]` browser session.
- `solarium.loop` — run a bounded inspect-plan-act loop.
- `solarium.crawl` — crawl in-scope pages; requires a scope policy.
- `solarium.audit` — passive defensive audit of an authorized page.
- `solarium.owaspAudit` — OWASP-mapped browser audit and Top 10 scanner for an authorized page. Profiles: `passive`, `strict-headers`, `active-authorized`, `top10-passive`, `top10-active-authorized`.
- `solarium.graphqlAudit` — bounded non-DoS GraphQL endpoint/schema audit for an authorized target.
- `solarium.scopeCheck` — check a URL against a scope policy.
- `solarium.validate` — validate job/scope/actions JSON.
- `solarium.profiles` — list built-in browser profiles.
- `solarium.profile` — show a built-in browser profile.
- `solarium.replay` — summarize a JSONL event timeline from disk.
- `solarium.workflowSeed` — generate Skiller-style workflow seed Markdown from action JSON and optional evidence.
- `solarium.manifest` — create a SHA-256 artifact manifest or `solarium.evidence.v1` evidence run manifest.

## Workflow seed generation

`solarium.workflowSeed` converts a JSON action trace and optional evidence manifest into reusable Skiller-style Markdown.

```json
{"jsonrpc":"2.0","id":6,"method":"solarium.workflowSeed","params":{"actionsPath":"actions/login-smoke.json","evidencePath":".solarium/sessions/login/evidence.json","name":"Login smoke workflow","output":"skills/solarium/generated/login-smoke.md"}}
```

Typed text values are redacted in the generated seed and replaced with `${input.form_values}` placeholders.

## Evidence manifest mode

`solarium.manifest` can return the original artifact manifest or a standardized evidence run manifest. Pass `evidence: true` for the richer wrapper:

```json
{"jsonrpc":"2.0","id":5,"method":"solarium.manifest","params":{"roots":[".solarium/sessions/example"],"evidence":true,"runId":"example","kind":"session","url":"https://example.com"}}
```

The evidence manifest includes `schemaVersion: "solarium.evidence.v1"`, run metadata, target metadata, optional scope, action summaries, network policy, artifact hashes, and errors. Action summaries omit typed text values.

## Error behavior

Malformed JSON returns JSON-RPC parse error `-32700`.
Invalid requests return `-32600`.
Unknown methods/tools return `-32601`.
Invalid parameters return `-32602` where Solarium can classify the issue.
Unexpected tool failures return `-32603`.

## Security boundary

External agents should pass explicit `scope` objects for security-sensitive browsing, crawling, auditing, GraphQL auditing, and loop runs. `solarium.crawl` requires scope. Storage state files, downloads, screenshots, and traces can contain sensitive local artifacts; keep generated paths under an agent-controlled evidence directory and out of source control.

The server uses stdio only. It does not open a network listener.

## TypeScript client / Vegvisir adapter

For launching and supervising this server from Vegvisir or another TypeScript agent harness, see `docs/vegvisir-adapter.md`.
