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
