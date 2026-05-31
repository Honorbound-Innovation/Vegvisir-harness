---
skill_id: solarium.workflow_recorder_seed
title: Workflow Recorder Skill Seed
summary: Turn a successful Solarium action trace into a reusable Skiller workflow seed.
domain: workflow-automation
version: 0.1.0
inputs:
  - name: action_trace
    description: Successful Solarium action sequence and observations.
  - name: goal
    description: Human-readable workflow outcome.
  - name: scope
    description: Authorized hosts and preconditions.
outputs:
  - name: skill_seed
    description: Draft Skiller skill with inputs, actions, validations, artifacts, and failure handling.
policies:
  - remove_secrets_from_traces
  - generalize_selectors_carefully
  - preserve_authorization_requirements
---

# Workflow Recorder Skill Seed

Use this skill to convert one successful browser session into a reusable procedure.

## Purpose

A useful Solarium workflow should not stay as a one-off trace. It should become a reusable skill with:

- clear goal,
- explicit inputs,
- authorization scope,
- preconditions,
- deterministic action sequence,
- validation checks,
- artifact expectations,
- failure recovery steps,
- secret-handling boundaries.

## Conversion procedure

1. Collect the successful action trace, final result, and evidence artifacts.
2. Remove or replace all sensitive values:
   - passwords,
   - tokens,
   - session URLs,
   - personal data,
   - account-specific identifiers unless required and non-secret.
3. Replace brittle values with parameters.
4. Identify required inputs:
   - base URL,
   - profile/storage state,
   - user role,
   - test data,
   - artifact directory.
5. Add scope requirements.
6. Add validation steps:
   - final URL,
   - visible selector,
   - success text,
   - screenshot path,
   - expected absence of error banners.
7. Add failure handling:
   - selector drift,
   - timeout,
   - auth expired,
   - validation mismatch.
8. Write a draft skill markdown file under an appropriate skill bundle.
9. Validate or compile the skill if tooling is available.

## Skill seed template

```markdown
---
skill_id: <domain.workflow_name>
title: <Human Workflow Name>
summary: <One-line purpose>
domain: browser-automation
version: 0.1.0
inputs:
  - name: base_url
  - name: profile
  - name: artifact_dir
outputs:
  - name: result
policies:
  - require_explicit_scope
  - no_plaintext_credentials
---

# <Human Workflow Name>

## Preconditions

## Procedure

## Solarium actions

```json
[]
```

## Validation

## Failure handling
```

## Quality bar

A recorded workflow is not reusable until another run can execute it using parameters rather than hidden context from the original session.
