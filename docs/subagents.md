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
- `/subagents config max-steps <n> min-max-steps <n> max-max-steps <n>`
- `/subagents config tool-calls <n> read-bytes <n> output-bytes <n>`
- `/subagents config allowed-tools <tool-a,tool-b>`
- `/subagents config budget-notes <text>`

Parallel implementation must use non-overlapping scopes. Review tasks should remain read-only unless the user explicitly asks for edits.


## Configuration

Default subagent provider/model, active concurrency, spawn `max_steps` range, and default work-budget settings are defined by `vegvisir/src/defaults/subagents.json` and can be overridden persistently in the user config with `/subagents config`. A `spawn_subagent` tool call can still override provider, model, `max_steps`, and `work_budget` explicitly for a single child.

The default configuration file currently contains:

```json
{
  "default_provider": "openai-sso",
  "default_model": "gpt-5.4-mini",
  "active_limit": 3,
  "default_max_steps": 4,
  "min_max_steps": 1,
  "max_max_steps": 32,
  "default_max_tool_calls": 8,
  "default_max_read_bytes": 65536,
  "default_max_output_bytes": 16384,
  "default_allowed_tools": ["list_files", "read_file"],
  "default_budget_notes": "Prefer targeted search/listing and small file excerpts. Do not read huge files in full; ask for a larger budget if needed.",
  "spawn_requires_yolo": true
}
```

Persistent user-config keys use the `subagent_*` prefix, for example `subagent_default_max_steps`, `subagent_max_max_steps`, `subagent_default_max_tool_calls`, `subagent_default_max_read_bytes`, `subagent_default_max_output_bytes`, `subagent_default_allowed_tools`, and `subagent_default_budget_notes`.

Per-use-case limits should be supplied directly in the `spawn_subagent` request when the task needs a different envelope than the configured defaults:

```json
{
  "goal": "Read-only compatibility review",
  "file_scope": ["vegvisir/src/provider.rs", "docs/"],
  "max_steps": 8,
  "work_budget": {
    "max_steps": 8,
    "max_tool_calls": 20,
    "max_read_bytes": 200000,
    "max_output_bytes": 20000,
    "allowed_tools": ["list_files", "read_file", "run_command"],
    "notes": "Read-only review. Prefer targeted searches. Stop and report if more budget is needed."
  }
}
```

Launch failures are recorded on the subagent board with executable path, argv, cwd, OS error kind/code, PATH, child env keys, and remediation hints. Use `/subagents show <id-or-name>` after a failure to see those details.
