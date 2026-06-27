# Demo 09 — USRL Policy-Bound Workflow

## Goal

Show Vegvisir following explicit workflow/policy constraints rather than relying
only on informal prompt vibes.

## One-line pitch

> Agent behavior can be governed by explicit contracts and evidence gates.

## Script

```bash
demos/scripts/09-usrl-policy-bound-workflow.sh
```

The script verifies USRL docs/component presence and runs focused runtime gate
tests.

## What to show in a live recording

1. A small USRL contract or policy requirement.
2. A task with an allowed path and a prohibited/risky path.
3. Vegvisir gathering required evidence before action.
4. Risky action blocked or routed through approval.
5. Allowed work completed and verified.

## What this proves

- USRL is more concrete than freeform instructions.
- Runtime gates can enforce stage/evidence/risk requirements.
- Operator authority and policy remain visible.

## Suggested live prompt

```text
Use the USRL policy-bound workflow for a low-risk docs update. Follow the
required stages, gather evidence before editing, do not run destructive commands,
and summarize which contract requirements were satisfied.
```
