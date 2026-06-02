# Command Sandboxing And Approvals

Vegvisir exposes powerful local tools, so command and filesystem behavior is governed by several layers: workspace scope, command allow-lists, approval queues, optional OS sandboxing, and dangerous bypass mode.

This document focuses on the newer command/file hardening behavior.

## Layers At A Glance

| Layer | Purpose |
| --- | --- |
| Workspace file sandbox | Keeps file tools inside the active workspace and rejects unsafe symlink/path escapes. |
| Command allow-list | Limits which shell executables the model can run without approval. |
| Risky-tool approval | Queues risky tool calls for operator review. |
| Network-aware approval | Queues obvious network-like command requests for operator review. |
| Command OS sandbox | Optionally wraps shell commands in Bubblewrap isolation. |
| Dangerous bypass | Startup-only mode that bypasses approvals and sandbox controls for trusted high-risk sessions. |

## Workspace File Sandbox

Model-accessible file tools are workspace-scoped. They should not read or write arbitrary host paths.

The hardened file sandbox rejects:

- parent traversal such as `../outside`,
- absolute paths where workspace-relative paths are required,
- symlinks that point outside the workspace,
- writes through symlink escape paths,
- symlinked directories that escape the workspace.

This protects file tools. Shell commands are separately controlled by command policy and sandbox settings.

## Command Allow-List

`run_command` takes an argv array and runs a bounded command in the active workspace. The executable must be allowed unless dangerous bypass is active or the operator approves it.

Inspect and manage allowed commands:

```text
/tools commands list
/tools commands add <cmd...>
/tools commands remove <cmd...>
/tools commands reset
```

Examples:

```text
/tools commands add jq rg
/tools commands remove curl wget
/tools commands reset
```

If a command is not allow-listed, Vegvisir queues a command approval with risk label `command-allow`.

## Approval Queue

Use approval mode when you want high capability but still want review before risky operations:

```text
/tools require-approval
```

Manage the queue:

```text
/approvals
/approvals show <id>
/approvals approve <id>
/approvals session <id>
/approvals edit <id> <json-args>
/approvals deny <id>
```

Approval choices:

- **approve**: allow exactly this pending call once.
- **session**: allow the same tool/arguments for this running session.
- **edit**: change the tool arguments before approval.
- **deny**: reject the call.

Pending approvals persist in the approval ledger. Session approvals are intentionally runtime-only.

## Network-Aware Command Approval

Some commands are local by executable name but external by behavior. Vegvisir can classify command argument patterns that likely require network access and queue them for approval with risk label `command-network`.

Typical examples include package installation, remote fetches, or direct service requests.

Use this mode when you want local build/test commands to proceed normally while still requiring explicit review for commands that reach outside the machine.

## Command OS Sandbox

Configure command sandboxing through environment variables before launching Vegvisir.

Modes:

```bash
VEGVISIR_COMMAND_SANDBOX=path
VEGVISIR_COMMAND_SANDBOX=none
VEGVISIR_COMMAND_SANDBOX=bwrap
VEGVISIR_COMMAND_SANDBOX=strict-bwrap
```

Mode behavior:

| Mode | Behavior |
| --- | --- |
| `path` | Default path/workspace policy without Bubblewrap wrapping. |
| `none` | No command OS sandbox wrapping. |
| `bwrap` | Run commands through Bubblewrap when available. |
| `strict-bwrap` | Use stricter Bubblewrap isolation and a more private execution environment. |

Additional controls:

```bash
VEGVISIR_COMMAND_NETWORK=inherit|disable|require-approval
VEGVISIR_COMMAND_WRITABLE_PATHS=/tmp/build-cache:/some/other/path
VEGVISIR_COMMAND_READONLY_PATHS=/usr/share:/opt/tooling
VEGVISIR_COMMAND_HIDDEN_PATHS=$HOME/.ssh:$HOME/.config/private
```

Notes:

- Bubblewrap modes require the `bwrap` executable on the host.
- Strict Bubblewrap disables network behavior by default.
- Mount path variables are host-specific and should not include secrets in the variable values.
- Dangerous bypass disables command sandbox wrapping.

## Dangerous Bypass

Dangerous bypass is not a TUI toggle. It must be selected at startup:

```bash
vegvisir --dangerously-bypass-approvals-and-sandbox tui
```

When active, it bypasses approval and sandbox controls. Use it only for trusted sessions where the operator intentionally accepts the risk.

Check state:

```text
/tools status
/verify runtime
```

## Recommended Operating Modes

### Normal Development

```bash
VEGVISIR_COMMAND_SANDBOX=path
```

```text
/tools require-approval
/tools commands list
```

Good for normal repo work with approval gates for risky tools and unfamiliar commands.

### Hardened Local Testing

```bash
VEGVISIR_COMMAND_SANDBOX=bwrap
VEGVISIR_COMMAND_NETWORK=require-approval
```

Good for testing untrusted repo commands while still allowing operator-reviewed network requests.

### Strict Local Review

```bash
VEGVISIR_COMMAND_SANDBOX=strict-bwrap
```

Good for read-heavy review sessions and deterministic checks where network should be disabled by default.

### High-Risk Trusted Autonomy

```bash
vegvisir --dangerously-bypass-approvals-and-sandbox tui
```

Good only when the operator wants broad autonomy and accepts the blast radius.

## Verification

After changing sandbox or approval settings:

```text
/tools status
/verify runtime
/eval tools
/eval security
```

From the CLI:

```bash
vegvisir verify runtime --workspace /path/to/project
cargo test --workspace command_sandbox guardrails sandbox -- --test-threads=1
```
