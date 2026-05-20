# Security And Operations

Vegvisir separates capability from authorization. The model may be able to ask for powerful actions, but the harness decides whether those actions are available, scoped, approved, and allowed by policy.

## Security Model

Vegvisir uses several layers together:

- workspace sandboxing for filesystem tools
- command allow-lists and output limits
- risky-tool approval queues
- USRL contracts for regulated workflows
- HBSE for secrets and brokered provider/service access
- CMS-v2 safety checks to keep secrets out of memory
- traces and audits for what happened

No single layer is treated as enough by itself.

## Secrets Boundary

Vegvisir should not store provider or service credentials. HBSE stores secrets and performs brokered requests or ticketed delivery according to policy.

Use HBSE secret refs in Vegvisir configuration:

```text
secret://vegvisir/providers/openai/default
secret://vegvisir/mcp/github/default
secret://vegvisir/services/postgres/default
```

Avoid putting secrets in:

- chat messages
- memory
- command arguments
- MCP config URLs
- logs
- workspace files
- screenshots or copied transcripts

## Risky Tools

Risky tools are controlled by runtime policy and approvals.

```text
/tools status
/tools inventory
/tools require-approval
/tools no-approval
/tools allow-risky
/tools deny-risky
```

Recommended normal mode:

```text
/tools require-approval
```

That keeps tools available while making destructive or high-risk calls visible to the user first.

## Approval Queue

Approval workflow:

```text
/approvals
/approvals show <id>
/approvals approve <id>
/approvals session <id>
/approvals edit <id> <json-args>
/approvals deny <id>
```

Use approve-once for one execution. Use session approval when the same tool and arguments should be allowed again for this running Vegvisir session only. Use edit when the proposed tool call is right in intent but wrong in arguments.

## Dangerous Bypass Mode

The dangerous bypass mode authorizes all tools, commands, and sandbox escapes for the running session. It can only be enabled at startup.

```bash
vegvisir --dangerously-bypass-approvals-and-sandbox tui
```

This mode is intentionally not available as a TUI command. It is for trusted sessions where the operator wants Codex-style full bypass behavior and accepts the risk.

## MCP

MCP HTTP services should use HBSE-backed service refs.

Register a service ref:

```text
/hbse service add github-mcp secret://vegvisir/mcp/github/default vegvisir.mcp.github mcp.tool.call
```

Register an HTTP MCP server using that service:

```text
/mcp add-http-service github https://mcp.example.test/rpc github-mcp
```

List available MCP tools:

```text
/mcp tools
```

STDIO MCP tools can be registered for local development:

```text
/mcp add-stdio local /path/to/mcp-server --stdio
```

For custom agents, MCP exposure is controlled per agent:

```text
/agent allow-mcp researcher github
/agent revoke-mcp researcher github
```

## Workspace Safety

Filesystem tools are scoped to the active workspace. Switch workspaces deliberately:

```text
/workspace /path/to/project
/projects use main
```

A workspace switch retargets:

- filesystem tools
- attachments
- workspace skills
- session bindings
- CMS-v2 project scope

## Memory Safety

CMS-v2 memory is for durable non-secret context. Store decisions, preferences, project facts, summaries, and task continuity. Do not store credentials.

Use:

```text
/remember <title> | <content>
/recall <query>
/recall --global <query>
/memory status
```

Use project recall first. Use global recall only when cross-project memory is actually needed.

## Verification

Use verification before relying on a deployment:

```text
/verify all
/verify auth
/verify mcp
/verify runtime
/verify memory
```

Run evals after changes to memory, providers, tools, approvals, USRL, or MCP:

```text
/eval security
/eval memory
/eval golden
```

Run source tests before publishing:

```bash
cargo test --workspace -- --test-threads=1
```

## Tracing

Use traces when debugging harness behavior:

```text
/trace --limit 10
/trace --json --limit 5
```

Trace data is operational evidence. Treat it as sensitive if it contains filenames, prompts, tool arguments, or provider error bodies.
