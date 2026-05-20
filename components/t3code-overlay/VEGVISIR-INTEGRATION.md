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

Do not bypass Vegvisir by calling model providers, tools, MCP servers, or secret material directly from this overlay. The overlay should present and control the workflow; Vegvisir should execute it.

