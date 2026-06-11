# Vegvisir Agent Admin

`vegvisir-agent-admin` is a standalone agent registry administration binary for creating, inspecting, validating, updating, importing, exporting, cloning, and deleting persistent Vegvisir agent profiles without launching the main TUI.

The binary uses the same agent profile storage model as the main Vegvisir runtime:

```text
${VEGVISIR_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/vegvisir}/agents/*.json
```

It stores only profile configuration. Do not put plaintext secrets in agent prompts, metadata, provider fields, MCP configuration, or imported JSON. Credential-bearing integrations should continue to use HBSE secret references.

## Install

The top-level installer installs `vegvisir-agent-admin` by default:

```text
./install.sh --prefix "$HOME/.local"
```

To skip only this binary:

```text
./install.sh --no-agent-admin
```

After install, the binary should be available at:

```text
$HOME/.local/bin/vegvisir-agent-admin
```

## Build

```text
cargo build -p vegvisir-rust --bin vegvisir-agent-admin
```

## Global options

```text
--data-root <path>   Use a specific Vegvisir data root.
--workspace <path>   Use a specific workspace for local skills and Skiller agent-pack discovery.
--json               Print JSON where supported.
```

Use `--data-root` for dry runs and registry smoke tests without touching your normal Vegvisir agents. Use `--workspace` when skill validation, USRL skill-ref expansion, or Skiller agent-pack discovery should use a workspace other than the current directory.

## Core commands

```text
vegvisir-agent-admin paths
vegvisir-agent-admin doctor
vegvisir-agent-admin templates [id]
vegvisir-agent-admin list [--long] [--mode <mode>]
vegvisir-agent-admin show <id>
vegvisir-agent-admin create <id> [options]
vegvisir-agent-admin create-template <mode> <id> [--name <display-name>] [--description <text>]
vegvisir-agent-admin design <id> --mode <mode> --name <name> --prompt <text> [options]
vegvisir-agent-admin set <id> [options]
vegvisir-agent-admin clone <source-id> <new-id> [--name <display-name>] [--force]
vegvisir-agent-admin export <id> [--out <path>]
vegvisir-agent-admin import <path> [--force]
vegvisir-agent-admin delete <id> --yes
vegvisir-agent-admin tui
```

`doctor` validates the registry and reports invalid profile JSON, empty prompts/display names, shared CMS scopes, model-without-provider warnings, and empty list entries.

## Template-aware creation

Built-in templates mirror the main `/agent templates` identities:

```text
planner
researcher
orchestrator
engineer
coder
tester
agent-red
```

List templates:

```text
vegvisir-agent-admin templates
vegvisir-agent-admin templates engineer
```

Create from a template:

```text
vegvisir-agent-admin create-template engineer build-engineer \
  --name "Build Engineer"
```

Create with a template and override fields:

```text
vegvisir-agent-admin create security-reviewer \
  --template agent-red \
  --name "Security Reviewer" \
  --description "Reviews tool, memory, and secret boundaries." \
  --add-tools run_tests
```

## Direct profile design

Use `design` when creating a mostly-complete profile in one command:

```text
vegvisir-agent-admin design repo-tester \
  --mode tester \
  --name "Repo Tester" \
  --prompt "You are a test specialist. Inspect changed behavior and run focused checks." \
  --tools read_file,run_tests,run_command \
  --skills test-repair \
  --provider openai-sso \
  --model gpt-5.4-mini \
  --memory-policy agent-scoped
```

## Editing commands matching `/agent`

Field-specific commands are available so scripts do not have to rewrite full JSON profiles:

```text
vegvisir-agent-admin name <id> <display name>
vegvisir-agent-admin mode <id> <mode>
vegvisir-agent-admin describe <id> <description>
vegvisir-agent-admin provider <id> <provider|clear|->
vegvisir-agent-admin model <id> <model|clear|->
vegvisir-agent-admin prompt <id> <prompt text>
vegvisir-agent-admin prompt <id> --prompt-file ./prompt.md
vegvisir-agent-admin memory-policy <id> <policy>
vegvisir-agent-admin cms-scope <id> --user <cms-user-id> --project <cms-project-id>
vegvisir-agent-admin reset-cms-scope <id>
```

Provider/model validation is still performed by the main runtime when the profile is activated. The standalone admin edits persisted profiles and intentionally does not require provider auth.

## Permission list editing

Tools:

```text
vegvisir-agent-admin allow-tool <id> read_file
vegvisir-agent-admin revoke-tool <id> run_command
vegvisir-agent-admin set-tools <id> read_file,run_tests
```

Skills:

```text
vegvisir-agent-admin enable-skill <id> code-review
vegvisir-agent-admin disable-skill <id> test-repair
vegvisir-agent-admin set-skills <id> repo-orientation,code-review
```

MCP servers:

```text
vegvisir-agent-admin allow-mcp <id> local-docs
vegvisir-agent-admin revoke-mcp <id> local-docs
vegvisir-agent-admin set-mcp <id> local-docs,issue-tracker
```

USRL contracts:

```text
vegvisir-agent-admin bind-usrl <id> safe-dev
vegvisir-agent-admin unbind-usrl <id> safe-dev
vegvisir-agent-admin set-usrl <id> safe-dev,no-secrets
```

Bulk update with `set`:

```text
vegvisir-agent-admin set repo-tester \
  --provider openai-sso \
  --model gpt-5.4-mini \
  --add-tools spawn_subagent \
  --remove-tools write_file \
  --add-usrl no-secrets
```

## Import/export

Export and import:

```text
vegvisir-agent-admin export code-reviewer --out code-reviewer.agent.json
vegvisir-agent-admin import code-reviewer.agent.json
```

`import` refuses to overwrite an existing profile unless `--force` is passed:

```text
vegvisir-agent-admin import code-reviewer.agent.json --force
```

## Isolated smoke test

```text
tmp=$(mktemp -d)
vegvisir-agent-admin --data-root "$tmp" templates
vegvisir-agent-admin --data-root "$tmp" create-template engineer build-engineer --name "Build Engineer"
vegvisir-agent-admin --data-root "$tmp" allow-tool build-engineer spawn_subagent
vegvisir-agent-admin --data-root "$tmp" bind-usrl build-engineer safe-dev
vegvisir-agent-admin --data-root "$tmp" doctor
```

Expected final output includes:

```text
Status: ok
```

## Interactive TUI

`vegvisir-agent-admin tui` starts a full-screen, conventional registry browser/editor. It is intentionally lightweight and avoids Vim-style command entry: there is no `:` command mode, no `/` search command, and no `q` quit binding. Use non-interactive subcommands for scripts and reproducible automation.

Current TUI keys:

```text
Esc         quit, or close help/popups
Ctrl+C      quit
F1          toggle help
F2 / A      open the action menu
F / Ctrl+F  search agents by id, name, mode, profile text, permissions, and metadata
E           edit primary scope metadata for the selected agent
Y           edit memory policy for the selected agent
B           edit budget max steps for the selected agent
P           edit provider for the selected agent
O           edit model for the selected agent
U           edit comma-separated tool allow-list for the selected agent
S           edit comma-separated enabled skills for the selected agent
D           edit comma-separated allowed MCP servers for the selected agent
L           edit comma-separated bound USRL contracts for the selected agent
T           edit comma-separated tags for the selected agent
↑/↓         move selection
Home/End    jump to start/end
PageUp/Down jump by 5
Enter / V   validate selected agent
M           show metrics summary
H           show history count
R           refresh
```

The action menu supports validation, metrics, history count, lifecycle status changes, scope metadata edits, memory-policy edits, budget edits, provider/model edits, tool/skill/MCP/USRL permission edits, and tag edits. `status active` still uses the same hard-error validation gate as the CLI command.

Scope/memory/budget/provider/model/permission/tag edit modes are simple text inputs: type the new value, press `Enter` to save, or press `Esc` to cancel. `E` edits the primary scope directly; the action menu also exposes secondary scopes, workspace scope, file-scope hints, and a clear-scope action. Empty input, `-`, or `clear` removes string scope fields. Empty comma-separated scope lists clear the stored list. `Y` edits the memory policy, and empty input, `-`, or `clear` resets it to `agent-scoped`. `B` edits `default_work_budget.max_steps` directly; the action menu also exposes max tool calls, read/output byte limits, allowed tools, notes, and a clear-budget action. Empty input, `-`, or `clear` removes numeric budget fields and notes; empty allowed-tools input clears the budget tool list. Budget allowed-tools entries are validated against the default tool catalog. Provider/model clear markers match the CLI: empty input, `-`, or `clear` means inherit/clear. Clearing the provider also clears the model because model validity is provider-scoped. TUI provider/model edits validate catalog names and provider/model compatibility, but they do not check live provider credentials. Tool, skill, MCP, and USRL edits replace the full comma-separated list. Tool and skill names are validated against the current default tool catalog and workspace/data-root skill catalog before saving. MCP server ids are validated against `<data-root>/mcp.json`. `*` is accepted for tools only when it is the sole entry. Known USRL contract skills are expanded to their declared contract ids when available; otherwise the entered contract ref is stored as-is, matching `/agent bind-usrl` behavior.

## Safety notes

- `delete` requires `--yes`; deletion is not available from the TUI.
- `create`, `create-template`, `design`, `clone`, and `import` refuse to overwrite existing profiles unless `--force` is passed.
- `provider clear` or `provider -` also clears the model because model validity is provider-scoped.
- `model clear` or `model -` clears only the model.
- The binary does not validate provider/model names against live provider credentials; the main runtime validates effective provider/model use when an agent is activated.
- The binary does not read or write plaintext credentials.

## Registry control-plane commands

The admin binary also implements the broader scoped-agent registry plan:

```text
vegvisir-agent-admin register [--builtins-only] [--skiller-only] [--dry-run]
vegvisir-agent-admin validate [id]
vegvisir-agent-admin status <id> <draft|active|paused|deprecated|archived|broken>
vegvisir-agent-admin scope <id> [--primary <scope>] [--secondary <a,b>] [--workspace-scope <scope>] [--file-scope <paths>]
vegvisir-agent-admin tags <id> <tags>
vegvisir-agent-admin budget <id> [--max-steps N] [--max-tool-calls N] [--max-read-bytes N] [--max-output-bytes N] [--allowed-tools a,b] [--notes <text>]
vegvisir-agent-admin budget <id> --clear
vegvisir-agent-admin metrics <id>
vegvisir-agent-admin compare <left-id> <right-id> [--prompts]
vegvisir-agent-admin history [id]
```

`register` synchronizes missing built-in template identities and Skiller-generated agent packs/proposals from known workspace/data-root discovery paths into the normal profile registry. Use `--dry-run` first when auditing a registry.

`validate` performs standalone readiness checks against the shared runtime catalogs where possible:

- required identity and prompt fields
- normalized ids
- non-empty CMS scope ids
- provider/model catalog compatibility
- known tools from the default tool catalog
- known skills from workspace/data-root skills
- known MCP server ids from `mcp.json`
- secret-like prompt content
- wildcard tool access warnings
- missing scope/description recommendations

`status active` refuses to activate a profile with hard validation errors. Other lifecycle states are metadata-only and support operator workflows:

```text
draft
active
paused
deprecated
archived
broken
```

`scope`, `tags`, and `budget` store operator-facing metadata in the agent profile without changing the runtime schema. These fields are intended for delegation planning, filtering, and future ability/self-improvement workflows.

`metrics` reads optional per-agent metric files from:

```text
<data-root>/agents/metrics/<agent-id>.json
```

`history` reads the append-only admin action log at:

```text
<data-root>/agents/history/events.jsonl
```

The admin history log records profile-management actions only. It does not record plaintext secrets or provider credentials.

## TUI scope and remaining CLI-only operations

The full-screen TUI is for safe, high-frequency profile inspection and small reversible edits. Keep using explicit CLI subcommands for multi-field, destructive, or script-oriented operations:

```text
create / create-template / design
clone / import / export / delete
prompt replacement
bulk set operations
single-field permission-list and budget editing is available in the TUI; use CLI for scripted/bulk permission changes
register / compare / doctor / validate all
```

This separation keeps the TUI predictable while preserving the complete standalone admin surface for automation.
