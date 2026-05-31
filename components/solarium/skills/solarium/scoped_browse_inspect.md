---
skill_id: solarium.scoped_browse_inspect
title: Scoped Browse and Inspect
summary: Safely browse and inspect an authorized page with Solarium before planning actions.
domain: browser-automation
version: 0.1.0
inputs:
  - name: url
    description: Target URL to inspect.
  - name: scope
    description: Allowed hosts and authorization note.
  - name: output_path
    description: Optional path for captured evidence artifacts.
outputs:
  - name: page_observation
    description: Title, URL, text/DOM summary, links, forms, screenshots, or report artifacts.
policies:
  - require_scope_check
  - authorized_targets_only
  - no_secret_capture_in_reports
---

# Scoped Browse and Inspect

Use this skill before interacting with unfamiliar pages, creating action plans, crawling, or auditing.

## Preconditions

- The user owns, controls, or has authorization for the target, or the task is limited to ordinary public-page browsing.
- Scope is explicit for non-local targets.
- Authenticated sessions use storage-state profiles or HBSE secret references, not pasted credentials.

## Procedure

1. Normalize the target URL.
2. Define a scope object:

```json
{
  "allowedHosts": ["example.com"],
  "authorizationNote": "Authorized inspection requested by owner"
}
```

3. Run a scope check.
4. Browse or inspect the page headlessly unless visual debugging is needed.
5. Capture evidence when useful:
   - screenshot,
   - page title and final URL,
   - visible text summary,
   - links/forms/buttons,
   - console/network issues if available.
6. Summarize actionable findings.

## Example RPC call

```json
{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"solarium.inspect","arguments":{"url":"https://example.com","scope":{"allowedHosts":["example.com"],"authorizationNote":"Authorized inspection"},"headless":true}}}
```

## Output expectations

Return:

- whether scope passed,
- final URL,
- page title,
- relevant elements/selectors,
- detected problems,
- artifact paths,
- recommended next action.

## Selector strategy

Prefer stable selectors in this order:

1. accessible roles/names if represented in the observation,
2. semantic IDs and `data-testid`,
3. form labels and input names,
4. stable classes,
5. text-based selectors,
6. brittle nth-child selectors only as a last resort.
