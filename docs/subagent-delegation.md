# Subagent Delegation

Vegvisir subagents are bounded child tasks used to parallelize evidence-seeking work without turning the main session into an uncontrolled swarm. They are designed for reconnaissance, review, testing investigation, documentation analysis, compatibility checks, security review, and design critique.

Subagents are not a way to bypass user authority, approval policy, workspace scope, HBSE, USRL contracts, or tool safety. They are tracked workers with explicit records.

## When To Use Subagents

Good subagent tasks are independent and bounded:

- inspect one subsystem and summarize how it works,
- review docs for gaps against a feature list,
- investigate a failing test family,
- compare compatibility between two implementations,
- perform a read-only security review of a narrow file set,
- critique a design proposal,
- identify migration impact in a limited directory.

Avoid subagents for:

- trivial single-step tasks,
- plaintext credential handling,
- destructive operations,
- ambiguous external side effects,
- broad implementation across overlapping files,
- unbounded repository crawls without a read/output budget.

## Subagent Records

Each subagent task has a durable board record containing:

- task id,
- worker name,
- workspace,
- goal,
- file scope,
- work budget,
- status,
- timestamps,
- checkpoint,
- result,
- error if failed.

The board is stored under Vegvisir's data root as `subagents.json`.

Task lifecycle events include:

```text
subagent.queued
subagent.started
subagent.completed
subagent.failed
subagent.cancelled
```

## File Scope And Concurrency

Every subagent should receive an explicit file scope. File scopes are used to prevent active workers from owning the same files at the same time.

Rules:

- Use non-overlapping scopes for parallel work.
- Prefer read-only scopes unless the user explicitly asks for parallel implementation.
- Never allow two active implementation workers to edit or own the same files.
- Keep a maximum of three active subagents at once.
- If a scope conflict is reported, narrow the new task or continue on the main thread.

Example scopes:

```text
README.md, docs/
vegvisir/src/provider.rs
vegvisir/src/app/commands/
components/skiller/
```

## Work Budgets

Non-trivial subagents should receive a budget. A useful budget specifies:

- maximum steps,
- maximum tool calls,
- maximum read bytes,
- maximum output bytes,
- allowed tools,
- notes about avoiding huge raw reads.

Example budget shape:

```json
{
  "max_steps": 6,
  "max_tool_calls": 12,
  "max_read_bytes": 120000,
  "max_output_bytes": 12000,
  "allowed_tools": ["list_files", "read_file", "run_command"],
  "notes": "Read-only review. Prefer targeted search and small file excerpts. Report if more budget is needed."
}
```

## TUI Commands

Inspect the board:

```text
/subagents
/subagents list
```

Show a task:

```text
/subagents show <id-or-name>
```

Cancel a queued or running task:

```text
/subagents cancel <id-or-name>
```

Show the active policy help:

```text
/subagents policy
```

## Model-Facing Tool

The model can call the `spawn_subagent` tool when enabled by the runtime. A subagent request should include:

- `name`: short human-readable worker name,
- `agent`: worker profile such as documentation, engineer, tester, security, or researcher,
- `workspace`: active workspace path,
- `goal`: narrow task objective,
- `file_scope`: explicit non-overlapping files/directories,
- `max_steps`: small bound by default,
- `provider` and `model`: current or explicit provider/model selection,
- `work_budget`: structured resource limits for non-trivial reviews.

The main agent should continue useful work while child tasks run and should inspect child results before the final summary when subagents were spawned.

## Common Patterns

### Documentation Review

```text
Goal: Read README.md and docs/ to identify docs gaps for new runtime features.
Scope: README.md, docs/
Budget: read-only, low steps, moderate output.
```

### Provider Regression Recon

```text
Goal: Inspect provider streaming tests and summarize likely coverage gaps.
Scope: vegvisir/src/provider.rs
Budget: read-only, targeted grep around tests.
```

### Security Review

```text
Goal: Review command sandbox and guardrails behavior for obvious bypass risks.
Scope: vegvisir/src/command_sandbox.rs, vegvisir/src/guardrails.rs, vegvisir/src/sandbox.rs
Budget: read-only, no exploit execution, report findings with source references.
```

## Operational Guidance

- Treat subagents as accelerators, not replacements for main-thread responsibility.
- Give them narrow goals and evidence expectations.
- Do not delegate secrets or credentials.
- Do not delegate broad destructive actions.
- Make the final answer explain what subagents were spawned and how their findings affected the result.
