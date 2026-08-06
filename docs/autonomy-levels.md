# Autonomy Levels

Vegvisir exposes autonomy as explicit user-visible levels. Approval and sandbox policy still apply at every level unless dangerous bypass was selected at startup.

- 0: off/manual only
- 1: ask-before-action
- 2: tool-assisted with approvals
- 3: bounded workspace execution
- 4: multi-step autonomous with evidence
- 5: subagent-assisted autonomous
- 6: maximum local autonomy within active policy

Commands:

- `/auto status`
- `/auto level <0-6>`
- `/auto on`
- `/auto off`

## Goal mode

Goal mode is a separate specification-driven implementation loop. It does not change `/auto` or `/autonomy`.

```text
/goal start path/to/specification.md
/goal status
/goal stop
```

When started, Goal mode requires the model to read the Markdown specification, create a complete plan under `.vegvisir/goal/`, extract all requirements and exit/acceptance criteria into a checklist, implement every phase, run the specified validations, and write completion evidence for each plan node. The TUI controller automatically starts the next model turn after each completed turn. It has no arbitrary step-count or no-progress completion limit: it stops only when the controller verifies all checklist items and node evidence, when the user cancels/stops it, or when an approval, policy, provider, or other unrecoverable blocker prevents progress.

Goal mode still inherits the normal workspace, tool, approval, sandbox, secret, cancellation, and user-authority boundaries. A completed model response is not treated as goal completion; the generated plan and validated evidence are the completion gate.
