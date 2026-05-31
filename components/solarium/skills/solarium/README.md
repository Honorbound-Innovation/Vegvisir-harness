# Solarium Skiller Bundle

A project-local skill bundle for using Solarium as a scoped browser automation, inspection, QA, crawl, audit, and evidence-capture runtime for Vegvisir and external agents.

Solarium's role in the broader stack:

- **Solarium**: controlled browser execution layer.
- **Vegvisir**: agentic orchestration, code editing, tests, memory, and reporting.
- **Skiller**: reusable task procedures, policies, and domain playbooks.

## Available skills

- `solarium.browser_rpc` — call Solarium through JSON-RPC/MCP-style stdio tools.
- `solarium.scoped_browse_inspect` — inspect a page inside an explicit authorization boundary.
- `solarium.session_runbook` — run deterministic browser action sessions from JSON actions.
- `solarium.webapp_verification_loop` — verify local/staging web app changes through browser interaction.
- `solarium.crawl_audit` — crawl and audit authorized web targets with evidence.
- `solarium.workflow_recorder_seed` — convert observed successful sessions into reusable workflow skill seeds.

## Safety model

All browser workflows should establish explicit scope before navigation or crawling:

```json
{
  "allowedHosts": ["example.com"],
  "authorizationNote": "Authorized owner/staging target"
}
```

Do not place plaintext credentials in skill inputs, examples, reports, or memory. Use HBSE-backed secret references and reusable Solarium storage-state profiles for authenticated sessions.
