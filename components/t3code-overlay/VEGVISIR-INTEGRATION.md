# Vegvisir T3 Code Overlay Integration

This directory contains the T3 Code source tree vendored into the Vegvisir overlay integration branch.

Purpose:

- adapt the T3 Code UI/server shell into a Vegvisir overlay
- keep Vegvisir as the backend authority for providers, tools, approvals, CMS-v2 memory, HBSE secrets, USRL policy, MCP, sessions, and workspace scope
- connect the overlay to Vegvisir through `vegvisir app-server`

The upstream project is MIT licensed. Preserve the original `LICENSE` file and copyright notice.

The intended runtime boundary is:

```text
T3 Code overlay UI/server
        |
        | JSONL stdin/stdout
        v
vegvisir app-server --workspace <project>
        |
        v
Vegvisir Rust runtime + CMS-v2 + HBSE + USRL + MCP
```

Do not bypass Vegvisir by calling model providers, tools, MCP servers, or secret material directly from this overlay. The overlay should present workflow state and user interaction only; Vegvisir is the control surface and execution authority.

## Provider Driver

This fork adds a `vegvisir` provider driver to the T3 Code server layer.

The driver launches:

```bash
vegvisir app-server --workspace <project>
```

Optional provider settings can override the binary path, Vegvisir provider, model, default agent, and startup-only dangerous bypass mode. Chat turns are sent through the JSONL bridge with `turn.send`; provider deltas are forwarded into the T3 runtime stream as assistant text, and Vegvisir approval requests are surfaced as runtime approval requests.

The overlay is not responsible for model credentials, MCP credentials, tool execution, CMS-v2 persistence, approval policy, session authority, or USRL enforcement. Those remain inside Vegvisir.

Operationally, the driver keeps this boundary by translating overlay actions into Vegvisir bridge requests:

- normal chat turns use `turn.send` and stream `content.delta` events back into the overlay
- slash-command turns use `command.run`, so `/tools`, `/memory`, `/approvals`, `/diff`, and similar controls are still executed by Vegvisir
- approval decisions use Vegvisir's `approvals.*AndExecute` bridge methods, causing the approved tool call to run inside Vegvisir rather than inside the overlay
- provider/model/agent selection is passed to `vegvisir app-server` as startup/session parameters; credentials remain HBSE-managed
