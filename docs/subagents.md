# Subagents

Subagents are bounded workers recorded on the durable subagent board. Each task records goal, workspace, file scope, work budget, lifecycle timestamps, observed events, file changes, diffs, launch argv/env keys, final output, parent/child run links when available, and ownership metadata.

Operator commands:

- `/subagents list`
- `/subagents show <id-or-name>`
- `/subagents timeline`
- `/subagents diff <id-or-name>`
- `/subagents events <id-or-name>`
- `/subagents artifacts <id-or-name>`
- `/subagents ownership`
- `/subagents cancel <id-or-name>`

Parallel implementation must use non-overlapping scopes. Review tasks should remain read-only unless the user explicitly asks for edits.
