# Vegvisir / External Agent Adapter

Solarium exposes two complementary integration layers for Vegvisir and other external agents:

1. `solarium server` — a newline-delimited stdio JSON-RPC/MCP-style server.
2. `SolariumJsonRpcClient` — a typed TypeScript client/launcher for embedding or supervising that server from an agent harness.

Together they let an agent treat Solarium as a controlled browser provider while keeping browser execution, scope checks, evidence capture, and reports inside Solarium.

## Launching the server from TypeScript

```ts
import { launchSolariumServer } from "solarium";

const solarium = launchSolariumServer({
  // Defaults to: command: "solarium", args: ["server"]
  requestTimeoutMs: 30_000,
  inheritStderr: true
});

try {
  const init = await solarium.client.initialize({
    protocolVersion: "2024-11-05",
    clientInfo: { name: "vegvisir", version: "local" },
    capabilities: {}
  });
  solarium.client.initialized();
  const tools = await solarium.client.listTools();

  const result = await solarium.client.callTool("solarium.scopeCheck", {
    url: "https://example.com",
    scope: {
      allowedHosts: ["example.com"],
      authorizationNote: "Authorized example host"
    }
  });

  console.log(init.serverInfo, tools.map((tool) => tool.name), result.structuredContent);
} finally {
  await solarium.close();
}
```

For local repository development, launch the source CLI instead of an installed binary:

```ts
const solarium = launchSolariumServer({
  command: "npm",
  args: ["run", "dev", "--", "server"],
  cwd: "/path/to/Solarium",
  inheritStderr: true
});
```

## Binding to an existing transport

If Vegvisir or another harness already owns the process transport, use `createSolariumJsonRpcClient` directly:

```ts
import { createSolariumJsonRpcClient } from "solarium";

const client = createSolariumJsonRpcClient({
  input: solariumStdout,
  output: solariumStdin,
  requestTimeoutMs: 60_000
});

await client.initialize();
client.initialized();
const browse = await client.callTool("solarium.browse", {
  url: "https://example.com",
  observe: true,
  headless: true,
  scope: {
    allowedHosts: ["example.com"],
    authorizationNote: "Authorized browser verification"
  }
});
```

## Calling style

The client supports both MCP-style `tools/call` and direct Solarium JSON-RPC methods.

### MCP-style call

```ts
const response = await client.callTool("solarium.inspect", {
  url: "https://example.com",
  includeObservation: true
});

console.log(response.structuredContent);
```

### Direct Solarium method call

```ts
const result = await client.callSolariumTool("solarium.inspect", {
  url: "https://example.com",
  includeObservation: true
});
```

`callTool` returns the MCP-style wrapper with `content`, `isError`, and `structuredContent`.
`callSolariumTool` returns the direct JSON-RPC result.

## Recommended Vegvisir tool mapping

A Vegvisir adapter can map Solarium tools into harness capabilities like this:

| Vegvisir capability | Solarium tool | Purpose |
| --- | --- | --- |
| Browser open/observe | `solarium.browse` | Open a page, observe text/elements, capture screenshots. |
| Browser inspect | `solarium.inspect` | Extract selector candidates and page observations. |
| Browser action session | `solarium.session` | Run deterministic JSON action sequences. |
| Browser agent loop | `solarium.loop` | Run bounded inspect-plan-act loops. |
| Site crawl | `solarium.crawl` | Inventory authorized pages, links, forms, observations. |
| Defensive audit | `solarium.audit` | Passive page security checks. |
| GraphQL audit | `solarium.graphqlAudit` | Bounded endpoint/schema checks with JSON/Markdown/HTML report renderers available through the package API/CLI. |
| Scope guard | `solarium.scopeCheck` | Check whether a URL is allowed before browsing. |
| Config validation | `solarium.validate` | Validate action/scope/job JSON. |
| Evidence manifest | `solarium.manifest` | Hash evidence/report artifacts for provenance. |

## Safety and authorization conventions

Adapters should preserve these Solarium conventions:

- Provide a `scope` object for real targets, especially crawls, audits, and GraphQL audits.
- Include a human-readable `authorizationNote` for sensitive workflows.
- Prefer `headless: true` for CI/agent operation unless visual debugging is needed.
- Store authenticated browser state in Playwright storage-state files managed by the caller; do not pass plaintext credentials through chat or logs.
- Write screenshots, traces, events, and reports into task-specific evidence directories.
- Run `solarium.scopeCheck` before exploratory or multi-page workflows.

## Minimal smoke test

After building the project:

```bash
npm run build
node --input-type=module <<'JS'
import { launchSolariumServer } from './dist/index.js';

const solarium = launchSolariumServer({
  command: 'node',
  args: ['dist/cli/index.js', 'server'],
  inheritStderr: true
});

try {
  console.log(await solarium.client.initialize());
  solarium.client.initialized();
  console.log((await solarium.client.listTools()).map((tool) => tool.name));
  console.log(await solarium.client.callSolariumTool('solarium.scopeCheck', {
    url: 'https://example.com',
    scope: { allowedHosts: ['example.com'], authorizationNote: 'Smoke test' }
  }));
} finally {
  await solarium.close();
}
JS
```
