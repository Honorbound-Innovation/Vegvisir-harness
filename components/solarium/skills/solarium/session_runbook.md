---
skill_id: solarium.session_runbook
title: Deterministic Browser Session Runbook
summary: Run or author Solarium JSON action sessions for repeatable browser workflows.
domain: browser-automation
version: 0.1.0
inputs:
  - name: actions
    description: Ordered Solarium AgentAction array.
  - name: scope
    description: Authorized scope for navigation and external requests.
  - name: artifacts
    description: Optional screenshot/report/event output paths.
outputs:
  - name: session_result
    description: Action results, final page state, artifacts, and failures.
policies:
  - validate_actions_before_execution
  - capture_evidence_for_important_flows
  - avoid_plaintext_credentials
---

# Deterministic Browser Session Runbook

Use this skill when a browser workflow should be repeatable, inspectable, and shareable as JSON actions.

## Supported action families

Typical Solarium actions include:

- `goto`
- `click`
- `dblclick`
- `hover`
- `fill`
- `press`
- `select`
- `submit`
- `wait`
- `waitForSelector`
- `waitForUrl`
- `screenshot`
- `observe`

Check `schemas/actions.schema.json` and `src/types.ts` for the exact current schema.

## Authoring procedure

1. Inspect the page first using `solarium.scoped_browse_inspect`.
2. Build the smallest action sequence that proves the outcome.
3. Include waits based on actual page state, not arbitrary sleeps, when possible.
4. Add screenshots at meaningful milestones.
5. Validate the action file using Solarium validation.
6. Run the session.
7. Review event/report artifacts.
8. If the workflow is reusable, promote it to a Skiller skill seed.

## Example action sequence

```json
[
  { "type": "goto", "url": "https://example.com/login" },
  { "type": "fill", "selector": "#email", "value": "${HBSE_USER_EMAIL}" },
  { "type": "fill", "selector": "#password", "value": "${HBSE_USER_PASSWORD}" },
  { "type": "click", "selector": "button[type=submit]" },
  { "type": "waitForUrl", "url": "**/dashboard", "timeoutMs": 10000 },
  { "type": "waitForSelector", "selector": ".dashboard", "state": "visible", "timeoutMs": 10000 },
  { "type": "screenshot", "path": "artifacts/dashboard.png" }
]
```

Note: placeholders above represent brokered secret injection, not plaintext credential storage.

## Failure handling

- For timeout failures, inspect current URL/title/screenshot before retrying.
- For selector failures, re-run inspection and update selectors.
- For navigation failures, verify scope and network access.
- For auth failures, verify secret refs/profiles without exposing secret values.
