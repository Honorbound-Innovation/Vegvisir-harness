---
skill_id: solarium.webapp_verification_loop
title: Web App Verification Loop
summary: Let Vegvisir implement code changes and use Solarium to verify the resulting web UI in a browser.
domain: software-verification
version: 0.1.0
inputs:
  - name: repo
    description: Local application repository.
  - name: dev_server_command
    description: Command to start the app under test.
  - name: base_url
    description: Local or staging base URL.
  - name: expected_flow
    description: User-visible behavior to verify.
outputs:
  - name: verification_report
    description: Pass/fail status, browser evidence, observed issues, and code/test follow-up.
policies:
  - preserve_user_work
  - run_focused_checks
  - scope_local_or_authorized_targets
---

# Web App Verification Loop

Use this skill when Solarium is paired with Vegvisir's coding abilities to close the loop between implementation and actual browser behavior.

## Goal

Move from:

```text
edit code -> run unit checks -> hope UI works
```

to:

```text
edit code -> run checks -> operate UI in browser -> observe failure -> patch -> verify again -> report
```

## Procedure

1. Inspect the app and determine the expected user flow.
2. Run existing checks:
   - typecheck,
   - unit tests,
   - build,
   - lint where available.
3. Start or reuse the dev server.
4. Define Solarium scope for the local/staging URL.
5. Use Solarium to inspect the initial page.
6. Create a deterministic session for the expected flow.
7. Run the session and capture evidence:
   - screenshots,
   - final URL/title,
   - console/network issues if supported,
   - event logs/reports.
8. If the browser flow fails:
   - identify whether the cause is app code, selector drift, timing, data setup, or environment,
   - patch the app or session as appropriate,
   - rerun focused checks.
9. Summarize changes and verification status.

## Common verification flows

- signup/onboarding,
- login/logout,
- dashboard load,
- settings update,
- checkout/cart,
- admin CRUD,
- documentation search,
- error page handling,
- responsive navigation.

## Output format

```markdown
## Browser Verification

- Target: <base_url>
- Flow: <flow name>
- Result: PASS/FAIL
- Checks run: <commands>
- Solarium artifacts: <paths>
- Issues found: <bullets>
- Fixes applied: <bullets>
- Remaining risks: <bullets>
```

## Escalation

If credentials are needed, request or use HBSE secret refs/storage profiles. Do not ask the user to paste passwords or tokens into chat.
