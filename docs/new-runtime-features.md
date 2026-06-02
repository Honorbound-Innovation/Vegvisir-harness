# New Runtime Features

This document summarizes the newer Vegvisir runtime capabilities that are easy to miss if you only read the command reference. It is intended as an operator/developer guide for the features added around autonomy control, sandboxing, provider streaming, subagents, and runtime observability.

## Feature Summary

Recent Vegvisir runtime work added or hardened these areas:

- **Recoverable tool-round control** with an unlimited default and explicit `/tool-limit` overrides.
- **Command allow-list approvals** so non-default shell executables can be approved once or allowed for a session instead of silently failing.
- **Network-aware command approvals** for commands that appear to request external network access.
- **Workspace file hardening** that rejects path escapes and unsafe symlink traversal for file tools.
- **Optional command OS sandboxing** through Bubblewrap with path-only, disabled, bwrap, and strict-bwrap modes.
- **Startup-only dangerous bypass mode** for intentionally high-risk local sessions.
- **Provider reasoning trace fencing** so visible provider reasoning summaries are rendered as a clearly separated thinking/audit block before the answer.
- **Recoverable tool-limit responses** so hitting a model tool-call round cap produces an operator-visible recovery result instead of losing the turn.
- **Subagent delegation board** with durable records, scoped file ownership, bounded work budgets, status inspection, and cancellation.
- **MCP stdio stabilization** with bounded timeouts and restart-on-first-failure behavior.
- **Runtime status surfaces** that report sandbox, dangerous-bypass, subagent, worker, approval, and verification state.
- **Hardware-aware parallelism** for deterministic local fan-out work such as LSL source compilation.

## Tool-Call Round Control

Vegvisir controls the number of model/tool-call loops that can occur in one model turn. This prevents pathological tool loops while still allowing serious engineering tasks to run.

The current default is **unlimited** unless configured otherwise. Operators can set a runtime cap from the TUI:

```text
/tool-limit
/tool-limit 24
/tool-limit unlimited
/tools max-rounds 16
```

Environment-level configuration:

```bash
VEGVISIR_MAX_TOOL_ROUNDS=24
```

Operational notes:

- Use unlimited or a high limit for large documentation, migration, and investigation sessions.
- Use a small explicit cap for deterministic evals or tests that should fail quickly if an agent loops.
- If a cap is reached, Vegvisir should surface a recoverable response that explains the cutoff instead of dropping the turn silently.

## Command Allow-List And Approval Flow

`run_command` is bounded by executable allow-lists, timeouts, output limits, command sandbox policy, and approval state.

Inspect and change the command allow-list:

```text
/tools commands list
/tools commands add gh jq ripgrep
/tools commands remove curl wget
/tools commands reset
```

If the model asks for a shell executable that is not allow-listed, Vegvisir queues an approval request. The operator can then approve once, approve for the session, edit arguments, or deny:

```text
/approvals
/approvals show <id>
/approvals approve <id>
/approvals session <id>
/approvals edit <id> <json-args>
/approvals deny <id>
```

Approval behavior is intentionally separate from tool availability:

- A tool may exist but be denied by policy.
- A risky tool may exist but require approval.
- A command may be generally allowed while network-like invocations still require approval.
- Dangerous bypass mode bypasses these controls only when selected at startup.

## Network-Aware Command Approval

Vegvisir can detect command shapes that likely require network access and route them through the approval queue. This is especially useful for commands such as package installs, remote fetches, or service calls.

The intent is not to classify every possible command perfectly. The intent is to make obvious external side effects visible and controllable while preserving a practical local development workflow.

## Filesystem Hardening

Filesystem tools are scoped to the active workspace. The hardened workspace sandbox rejects common escape patterns:

- `..` traversal outside the workspace.
- absolute path escapes where a relative workspace path is expected.
- symlink traversal that escapes the workspace.
- writes through unsafe symlink paths.

This applies to model-accessible file tools. It is separate from shell command sandboxing: if you allow shell commands, command behavior is controlled by the command allow-list, approval policy, command sandbox mode, and dangerous-bypass setting.

## Command OS Sandboxing

Vegvisir supports command sandbox modes through `VEGVISIR_COMMAND_SANDBOX`:

```bash
VEGVISIR_COMMAND_SANDBOX=path          # default path/workspace policy only
VEGVISIR_COMMAND_SANDBOX=none          # disable command OS sandbox wrapping
VEGVISIR_COMMAND_SANDBOX=bwrap         # Bubblewrap isolation when available
VEGVISIR_COMMAND_SANDBOX=strict-bwrap  # stricter Bubblewrap mode
```

Additional environment controls:

```bash
VEGVISIR_COMMAND_NETWORK=inherit|disable|require-approval
VEGVISIR_COMMAND_WRITABLE_PATHS=/path/a:/path/b
VEGVISIR_COMMAND_READONLY_PATHS=/path/a:/path/b
VEGVISIR_COMMAND_HIDDEN_PATHS=/path/a:/path/b
```

Strict Bubblewrap mode uses a more restrictive baseline, including disabled network behavior by default and a more private execution environment. Bubblewrap must be installed on the host for bwrap modes.

Check the effective sandbox state:

```text
/tools status
/verify runtime
```

## Dangerous Bypass Mode

Dangerous bypass mode is a startup-only mode for trusted local sessions where the operator deliberately wants broad Codex-style autonomy.

```bash
vegvisir --dangerously-bypass-approvals-and-sandbox tui
```

When active, it bypasses:

- risky-tool approvals
- command allow-lists
- active-agent tool allow-lists
- USRL tool gates
- workspace file sandboxing
- command sandbox policy

It cannot be enabled from inside chat or the TUI. `/tools status`, `/verify runtime`, and app-server status surfaces report whether it is active.

## Provider Streaming And Reasoning Trace Fencing

Provider adapters stream output when supported. For providers/models that surface a visible reasoning summary, Vegvisir fences that content as an explicit thinking/audit block before the assistant answer.

This keeps three things separate:

1. provider-visible reasoning summary or audit trace,
2. normal assistant answer,
3. tool observations and verification evidence.

Reasoning trace fencing also prevents a partial reasoning summary from swallowing the final answer if the provider ends unexpectedly. If a tool-round cap or provider stream failure occurs, Vegvisir should report the recoverable state clearly.

## MCP STDIO Stabilization

STDIO MCP servers are useful for local tooling, but they can hang or exit unexpectedly. Vegvisir now treats STDIO MCP calls as bounded operations:

- requests have explicit timeouts,
- a failed request can restart the stdio session once,
- second failure reports both first and second failure evidence,
- session-pool access is guarded so broken MCP state does not silently poison the whole runtime.

Operator commands:

```text
/mcp list
/mcp status
/mcp show <id>
/mcp tools
/mcp reload
/mcp add-stdio <id> <command> [args...]
```

Keep credentials out of STDIO command arguments. Use HBSE-backed service references for authenticated HTTP MCP services.

## Runtime Verification

Use runtime verification after changing provider, tool, sandbox, approval, memory, MCP, or subagent behavior:

```text
/verify runtime
/verify all
/eval tools
/eval security
/eval golden
```

CLI form:

```bash
vegvisir verify runtime --workspace /path/to/project
vegvisir verify all --workspace /path/to/project
```

Runtime checks should include evidence for command sandbox status, dangerous-bypass state, approval state, subagent board availability, active-agent policy, CMS-v2, MCP, cancellation, and bundled eval behavior.
