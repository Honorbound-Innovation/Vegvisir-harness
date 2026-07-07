# Demo 04 — Bounded Subagents Review a Change

## Goal

Show useful multi-agent work without autonomous chaos: bounded subagents receive
narrow goals and non-overlapping scopes, then the main agent merges findings.

## One-line pitch

> Vegvisir can delegate review work to bounded subagents while preserving
> operator control and workspace safety.

## Script

```bash
demos/scripts/04-bounded-subagents-review.sh
```

Safe mode creates a tiny disposable repo and prints the live command. To run the
provider-backed subagent demo:

```bash
RUN_LIVE=1 demos/scripts/04-bounded-subagents-review.sh
```

## What to show

1. Main thread creates/inspects the change proposal.
2. Subagent for docs scope.
3. Subagent for test planning scope.
4. Subagent for security/risk scope.
5. Subagent board/list/show output.
6. Final merged report with risks/blockers/verification.

## What this proves

- `spawn_subagent` works.
- Subagents can be scoped and bounded.
- The board records status, findings, and errors.
- Main thread remains the integrator; subagents do not become uncontrolled owners.

## Recording notes

Use small file scopes. Avoid asking subagents to read huge raw files in a public
recording; the point is bounded delegation, not maximal token consumption.
