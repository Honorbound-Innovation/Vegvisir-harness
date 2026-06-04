# Security Model

Vegvisir treats safety boundaries as inspectable product behavior.

- HBSE stores and brokers credentials; chat and memory should contain refs/metadata only.
- Guardrails enforce risky-tool approvals, command allow-lists, denied tools, and sandbox policy.
- Dangerous bypass is startup-only and should be visible in status, verification, and artifacts.
- `policy_explain` renders why a tool is allowed, denied, or queued.

Commands:

- `/tools explain <tool>`
- `/approvals explain <id>`
- `/verify runtime`
- `/hbse services`
- `/mcp status`
