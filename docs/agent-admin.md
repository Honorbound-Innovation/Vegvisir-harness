# Vegvisir Agent Admin

`vegvisir-agent-admin` is a standalone agent registry administration binary for creating, inspecting, updating, importing, exporting, cloning, and deleting persistent Vegvisir agent profiles without launching the main TUI.

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

## Commands

```text
vegvisir-agent-admin paths
vegvisir-agent-admin list
vegvisir-agent-admin show <id>
vegvisir-agent-admin create <id> [options]
vegvisir-agent-admin set <id> [options]
vegvisir-agent-admin clone <source-id> <new-id> [--name <display-name>]
vegvisir-agent-admin export <id> [--out <path>]
vegvisir-agent-admin import <path>
vegvisir-agent-admin delete <id> --yes
vegvisir-agent-admin tui
```

Global options:

```text
--data-root <path>   Use a specific Vegvisir data root.
--json               Print JSON where supported.
```

## Examples

Create a simple custom agent:

```text
vegvisir-agent-admin create code-reviewer \
  --name "Code Reviewer" \
  --mode reviewer \
  --description "Reviews code changes with evidence-backed findings." \
  --prompt "You are a code review specialist. Inspect relevant files before making claims." \
  --tools read_file,run_command,run_tests
```

Update provider/model defaults:

```text
vegvisir-agent-admin set code-reviewer --provider openai --model gpt-5.5
```

Clear provider/model defaults:

```text
vegvisir-agent-admin set code-reviewer --provider clear
```

Export and import:

```text
vegvisir-agent-admin export code-reviewer --out code-reviewer.agent.json
vegvisir-agent-admin import code-reviewer.agent.json
```

Use an isolated data root for testing:

```text
vegvisir-agent-admin --data-root /tmp/vegvisir-agent-admin-smoke create test-admin \
  --name "Test Admin" \
  --prompt "Test prompt" \
  --tools read_file,run_tests
```

## Interactive shell

`vegvisir-agent-admin tui` starts a small line-oriented registry editor shell with these commands:

```text
list
show <id>
create <id>
delete <id>
paths
help
quit
```

The interactive shell is intentionally minimal. Use the non-interactive subcommands for full field editing and automation.

## Safety notes

- `delete` requires `--yes` outside the interactive shell.
- `create` refuses to overwrite an existing profile unless `--force` is passed.
- `set --provider clear` also clears the model because model validity is provider-scoped.
- The binary does not validate provider/model names against the live provider registry yet; it edits persisted agent profiles directly.
- The binary does not read or write plaintext credentials.
