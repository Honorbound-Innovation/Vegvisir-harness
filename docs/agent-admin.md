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
--json               Print JSON where supported.
```

Use `--data-root` for dry runs and registry smoke tests without touching your normal Vegvisir agents.

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

## Interactive shell

`vegvisir-agent-admin tui` starts a small line-oriented registry editor shell with these commands:

```text
list
templates [id]
show <id>
create <id>
create-template <mode> <id>
delete <id>
doctor
paths
help
quit
```

The interactive shell is intentionally minimal. Use the non-interactive subcommands for full field editing and automation.

## Safety notes

- `delete` requires `--yes` outside the interactive shell.
- `create`, `create-template`, `design`, `clone`, and `import` refuse to overwrite existing profiles unless `--force` is passed.
- `provider clear` or `provider -` also clears the model because model validity is provider-scoped.
- `model clear` or `model -` clears only the model.
- The binary does not validate provider/model names against live provider credentials; the main runtime validates effective provider/model use when an agent is activated.
- The binary does not read or write plaintext credentials.
