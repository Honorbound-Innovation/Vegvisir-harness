# Security And Operations

Vegvisir separates capability from authorization.

## Secrets Boundary

Vegvisir should not store provider or service credentials. HBSE stores secrets and performs brokered requests or ticketed delivery according to policy.

Use HBSE secret refs in Vegvisir configuration:

```text
secret://vegvisir/providers/openai/default
secret://vegvisir/mcp/github/default
secret://vegvisir/services/postgres/default
```

## Risky Tools

Risky tools are controlled by runtime policy and approvals.

```text
/tools status
/tools require-approval
/tools allow-risky
/tools deny-risky
```

Approval workflow:

```text
/approvals
/approvals show <id>
/approvals approve <id>
/approvals approve-pattern <id>
/approvals edit <id> <json-args>
/approvals deny <id>
```

## Dangerous Bypass Mode

The dangerous bypass mode authorizes all tools, commands, and sandbox escapes for the running session. It can only be enabled at startup.

```bash
vegvisir --dangerously-bypass-approvals-and-sandbox tui
```

Do not expose this mode as an in-TUI command.

## MCP

MCP HTTP services should use HBSE-backed service refs.

```text
/hbse service add github-mcp secret://vegvisir/mcp/github/default vegvisir.mcp.github mcp.tool.call
/mcp add-http-service github https://mcp.example.test/rpc github-mcp
/mcp tools
```

STDIO MCP tools can be registered for local development:

```text
/mcp add-stdio local /path/to/mcp-server --stdio
```

## Verification

Use verification before relying on a deployment:

```text
/verify all
/verify auth
/verify mcp
/verify runtime
/verify memory
```

Run tests before publishing:

```bash
cargo test --workspace -- --test-threads=1
```
