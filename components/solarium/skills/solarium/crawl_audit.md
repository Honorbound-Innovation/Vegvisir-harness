---
skill_id: solarium.crawl_audit
title: Authorized Crawl and Audit
summary: Crawl and audit authorized sites with Solarium while preserving scope boundaries and evidence.
domain: web-audit
version: 0.1.0
inputs:
  - name: start_url
    description: Crawl/audit seed URL.
  - name: scope
    description: Allowed hosts and authorization note.
  - name: limits
    description: Max pages, depth, timeout, and artifact settings.
outputs:
  - name: audit_report
    description: Crawl inventory, findings, artifacts, and recommended remediation.
policies:
  - authorized_targets_only
  - enforce_allowed_hosts
  - bounded_crawl_limits
---

# Authorized Crawl and Audit

Use this skill for scoped web inventories, smoke audits, security header checks, broken-link discovery, and evidence-producing site reviews.

## Preconditions

- Scope is explicit.
- Crawl limits are bounded.
- The target permits the requested activity or is owned/controlled by the user.

## Procedure

1. Confirm start URL and allowed hosts.
2. Set crawl limits:
   - max pages,
   - max depth,
   - timeout,
   - same-origin/same-host restrictions,
   - artifact directory.
3. Run `solarium.scopeCheck` on the start URL.
4. Run `solarium.crawl` for inventory or `solarium.audit` for security/quality checks.
5. Capture artifacts and reports.
6. Summarize:
   - pages visited,
   - pages skipped due to scope,
   - HTTP status issues,
   - missing security headers,
   - forms and sensitive surfaces,
   - console/network errors if available,
   - remediation recommendations.

## Example RPC call

```json
{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"solarium.audit","arguments":{"url":"https://example.com","scope":{"allowedHosts":["example.com"],"authorizationNote":"Authorized audit"},"maxPages":25,"maxDepth":2,"headless":true}}}
```

## Guardrails

- Do not expand to third-party hosts without explicit authorization.
- Do not perform exploitation, credential attacks, persistence, stealth, or evasion.
- Prefer read-only checks unless the user explicitly authorizes state-changing tests.
- Record what was skipped due to scope.
