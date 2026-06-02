# Documentation Index

This directory contains the operator and developer documentation for the Vegvisir harness monorepo.

Start here if you need the real system picture rather than only command help:

- [System overview](system-overview.md) — full monorepo architecture, component responsibilities, runtime model, memory/secrets/tools/skills/subagents overview.
- [Runtime architecture](runtime-architecture.md) — Rust harness internals, CLI surfaces, TUI/headless/app-server flow, tools, memory, skills, subagents, verification.
- [New runtime features](new-runtime-features.md) — recent autonomy, sandboxing, subagent, provider-streaming, MCP, and runtime-status features.
- [Command sandboxing and approvals](command-sandboxing-and-approvals.md) — workspace file hardening, command allow-lists, approval queue, Bubblewrap modes, and dangerous bypass.
- [Subagent delegation](subagent-delegation.md) — bounded child-agent tasks, board records, file scopes, work budgets, and cancellation.
- [Desktop app](desktop-app.md) — graphical shell architecture, bridge boundary, feature parity contract, and implementation plan.
- [Skiller system](skiller-system.md) — governed skill compiler, Forge envelopes, lifecycle artifacts, registry, Agent Builder handoffs.
- [Solarium system](solarium-system.md) — browser automation/evidence runtime, profiles, auth sessions, audits, scope policy, acceptable-use boundary.

Command and component references:

- [Vegvisir usage and command reference](vegvisir-usage.md)
- [CMS-v2 usage and command reference](cms-v2-usage.md)
- [HBSE usage and command reference](hbse-usage.md)
- [USRL usage](usrl-usage.md)
- [USRL language reference](usrl-language-reference.md)
- [Linked Skill Libraries](lsl-skill-system.md)
- [Overlay / app bridge integration](overlay-integration.md)
- [Security and operations](security-and-operations.md)
- [Development workflow](development.md)

## Documentation Maintenance Rules

- Keep architecture docs grounded in current source paths and command surfaces.
- Keep generated/CLI reference docs synchronized when clap/commander command definitions change.
- Do not document local secrets, tokens, private keys, provider credentials, or secret-bearing URLs.
- Keep local planning files such as `plan.md` out of source control.
- Prefer adding focused docs over expanding the README into an unreadable creature.
