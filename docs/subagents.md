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
- `/subagents config`
- `/subagents config provider <provider> model <model>`
- `/subagents config max <n>`

Parallel implementation must use non-overlapping scopes. Review tasks should remain read-only unless the user explicitly asks for edits.


## Configuration

Default subagent provider/model settings are defined by `vegvisir/src/defaults/subagents.json` and can be overridden persistently in the user config with `/subagents config`. A `spawn_subagent` tool call can still override `provider` or `model` explicitly for a single child.

The default configuration file currently contains:

```json
{
  "default_provider": "openai-sso",
  "default_model": "gpt-5.4-mini",
  "active_limit": 3,
  "spawn_requires_yolo": true
}
```

Launch failures are recorded on the subagent board with executable path, argv, cwd, OS error kind/code, PATH, child env keys, and remediation hints. Use `/subagents show <id-or-name>` after a failure to see those details.
