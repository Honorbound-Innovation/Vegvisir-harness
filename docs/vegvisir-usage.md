# Vegvisir Usage And Command Reference

Vegvisir is the main agent harness. It can run as an interactive terminal UI or as a headless CLI for scripted work and agent servers.

This document is intentionally comprehensive for the current CLI and TUI slash-command surface.

Installed command:

```bash
vegvisir
```

## Top-Level Help

```text
Usage: vegvisir-rust [OPTIONS] [COMMAND]

Commands:
  tui
  run
  remember
  recall
  context
  model-request
  eval
  verify
  help           Print this message or the help of the given subcommand(s)

Options:
  -p, --prompt <PROMPT>
      --workspace <WORKSPACE>                     [default: /mnt/storage/Projects/Vegvisir-harness]
      --max-steps <MAX_STEPS>                     [default: 4]
      --provider <PROVIDER>
      --model <MODEL>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
  -h, --help                                      Print help
```

## Startup Behavior

Running `vegvisir` with no subcommand starts the TUI. `vegvisir tui` is accepted as the explicit form.

```bash
vegvisir
vegvisir tui
```

## CLI Commands

#### tui

Purpose:

Starts the interactive terminal UI. This is the default behavior when running `vegvisir` with no subcommand, and it is the normal mode for conversational coding, workspace switching, approvals, slash commands, and live provider streaming.

Exact help:

```text
Usage: vegvisir-rust tui [OPTIONS]

Options:
      --provider <PROVIDER>
      --model <MODEL>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
  -h, --help                                      Print help
```

Examples:

```bash
vegvisir
```

```bash
vegvisir --workspace /path/to/project
```

```bash
vegvisir --provider openai-hbse --model gpt-5.5
```

Notes:

- `--workspace` controls project scope for tools and memory.
- `--agent` applies a persistent custom agent profile.
- `--dangerously-bypass-approvals-and-sandbox` is startup-only and should be used only for explicitly trusted sessions.


#### run

Purpose:

Runs one headless agent task from the command line. Use it for scripts, agent servers, CI-like checks, or remote sessions where the full TUI is unnecessary.

Exact help:

```text
Usage: vegvisir-rust run [OPTIONS] <GOAL>

Arguments:
  <GOAL>

Options:
      --workspace <WORKSPACE>                     [default: /mnt/storage/Projects/Vegvisir-harness]
      --max-steps <MAX_STEPS>                     [default: 4]
      --provider <PROVIDER>
      --model <MODEL>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
  -h, --help                                      Print help
```

Examples:

```bash
vegvisir --workspace /path/to/project run "Summarize this repository"
```

Notes:

- `--workspace` controls project scope for tools and memory.
- `--agent` applies a persistent custom agent profile.
- `--dangerously-bypass-approvals-and-sandbox` is startup-only and should be used only for explicitly trusted sessions.


#### remember

Purpose:

Stores a durable CMS-v2 memory in the active workspace/project scope. Use it to record project decisions, preferences, operational facts, and reusable context without starting the TUI.

Exact help:

```text
Usage: vegvisir-rust remember [OPTIONS] <TITLE> <CONTENT>

Arguments:
  <TITLE>
  <CONTENT>

Options:
      --memory-type <MEMORY_TYPE>                 [default: note]
      --workspace <WORKSPACE>                     [default: /mnt/storage/Projects/Vegvisir-harness]
      --provider <PROVIDER>
      --model <MODEL>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
  -h, --help                                      Print help
```

Examples:

```bash
vegvisir --workspace /path/to/project remember "Decision | Use HBSE for provider secrets"
```

Notes:

- `--workspace` controls project scope for tools and memory.
- `--agent` applies a persistent custom agent profile.
- `--dangerously-bypass-approvals-and-sandbox` is startup-only and should be used only for explicitly trusted sessions.


#### recall

Purpose:

Retrieves relevant CMS-v2 memories for a query in the selected workspace/project scope. Use it to verify what Vegvisir can remember before sending a model request.

Exact help:

```text
Usage: vegvisir-rust recall [OPTIONS] <QUERY>

Arguments:
  <QUERY>

Options:
      --limit <LIMIT>                             [default: 8]
      --workspace <WORKSPACE>                     [default: /mnt/storage/Projects/Vegvisir-harness]
      --provider <PROVIDER>
      --model <MODEL>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
  -h, --help                                      Print help
```

Examples:

```bash
vegvisir --workspace /path/to/project recall --limit 8 "provider setup"
```

Notes:

- `--workspace` controls project scope for tools and memory.
- `--agent` applies a persistent custom agent profile.
- `--dangerously-bypass-approvals-and-sandbox` is startup-only and should be used only for explicitly trusted sessions.


#### context

Purpose:

Builds the ECM context packet for a message without necessarily sending it to a provider. Use it to debug memory retrieval, token budgeting, and project-scoped context assembly.

Exact help:

```text
Usage: vegvisir-rust context [OPTIONS] <MESSAGE>

Arguments:
  <MESSAGE>

Options:
      --workspace <WORKSPACE>                     [default: /mnt/storage/Projects/Vegvisir-harness]
      --provider <PROVIDER>
      --model <MODEL>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
  -h, --help                                      Print help
```

Examples:

```bash
vegvisir --workspace /path/to/project context "What context matters?"
```

Notes:

- `--workspace` controls project scope for tools and memory.
- `--agent` applies a persistent custom agent profile.
- `--dangerously-bypass-approvals-and-sandbox` is startup-only and should be used only for explicitly trusted sessions.


#### model-request

Purpose:

Builds the provider-facing model request envelope, including CMS-v2 context and cacheable prompt sections. Use it to inspect what would be sent to a model and diagnose token pressure or cache behavior.

Exact help:

```text
Usage: vegvisir-rust model-request [OPTIONS] <MESSAGE>

Arguments:
  <MESSAGE>

Options:
      --provider <PROVIDER>                       [default: local]
      --model <MODEL>                             [default: unspecified]
      --workspace <WORKSPACE>                     [default: /mnt/storage/Projects/Vegvisir-harness]
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
  -h, --help                                      Print help
```

Examples:

```bash
vegvisir --workspace /path/to/project model-request --provider openai-hbse --model gpt-5.5 "Summarize active work"
```

Notes:

- `--workspace` controls project scope for tools and memory.
- `--agent` applies a persistent custom agent profile.
- `--dangerously-bypass-approvals-and-sandbox` is startup-only and should be used only for explicitly trusted sessions.


#### eval

Purpose:

Runs deterministic harness evaluation checks. Use it after changing memory, providers, tool policy, approvals, USRL contracts, MCP, or custom agent behavior.

Exact help:

```text
Usage: vegvisir-rust eval [OPTIONS] [SCOPE]

Arguments:
  [SCOPE]  [default: all]

Options:
      --file <FILE>
      --provider <PROVIDER>
      --model <MODEL>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
  -h, --help                                      Print help
```

Examples:

```bash
vegvisir eval all
```

```bash
vegvisir eval --file ./evals/security.json
```

Notes:

- `--workspace` controls project scope for tools and memory.
- `--agent` applies a persistent custom agent profile.
- `--dangerously-bypass-approvals-and-sandbox` is startup-only and should be used only for explicitly trusted sessions.


#### verify

Purpose:

Runs production-readiness verification checks. Use it after installation, workspace changes, provider setup, MCP setup, or dangerous-bypass sessions to confirm the harness state.

Exact help:

```text
Usage: vegvisir-rust verify [OPTIONS] [SCOPE]

Arguments:
  [SCOPE]  [default: all]

Options:
      --workspace <WORKSPACE>                     [default: /mnt/storage/Projects/Vegvisir-harness]
      --provider <PROVIDER>
      --model <MODEL>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
  -h, --help                                      Print help
```

Examples:

```bash
vegvisir verify all --workspace /path/to/project
```

Notes:

- `--workspace` controls project scope for tools and memory.
- `--agent` applies a persistent custom agent profile.
- `--dangerously-bypass-approvals-and-sandbox` is startup-only and should be used only for explicitly trusted sessions.

## TUI Slash Commands

Run `/help` inside the TUI to print the live command reference. The current tree is:

```text
/new [name]
/sessions
/load <session-id>
/workspace [path]
/projects [list|use <path-or-alias>|name <alias> [path]|forget <alias>]
/reset
/clear
/redraw
/cancel
/history
/select
/save
/retry
/undo
/title [name]
/branch [name]
/fork
/compress [topic]
/system [show|print|view|set|append|clear|default] [text]
/system-prompt
/agent [list|templates|create|design|create-template|clone|import|export|use|show|delete|mode|provider|model|prompt|describe|allow-tool|revoke-tool|enable-skill|disable-skill|bind-usrl|unbind-usrl|allow-mcp|revoke-mcp|clear] [id]
/attach [path|clear]
/help
/tools [status|allow-risky|deny-risky|require-approval|no-approval]
/approvals [list|show <id>|approve <id>|approve-pattern <id>|edit <id> <json-args>|deny <id>]
/skills
/recall [--limit N] [--global] <query>
/memory [status|recent|import-chatgpt <path>] [--global] [--limit N]
/remember <title> | <content>
/context <message>
/model-request <message>
/models
/model [name]
/provider [name]
/providers
/auth [provider]
/verify [all|auth|mcp|agent|memory|runtime|evals]
/eval [all|memory|security|tools|injection|golden|file <path>]
/trace [--limit N] [--json]
/subagents [list|show <id-or-name>|cancel <id-or-name>]
/mcp [list|status|show|tools|reload|add-http|add-http-service|add-stdio|add-tool|remove-tool|remove|enable|disable]
/hbse [provider <id>|mcp <server> [url]|service <name>|service add|show|enable|disable|remove|services]
/config [status|user <id>|path]
/exit
```

## Session And Workspace Commands

Use these for project movement and conversation lifecycle:

```text
/new [title]
/sessions
/load <session-id>
/workspace [path]
/projects list
/projects name <alias> [path]
/projects use <alias-or-path>
/projects forget <alias>
/save
/retry
/undo
/branch [name]
/compress [topic]
/select
```

Examples:

```text
/workspace ~/Secret_Project
/projects name vegvisir /mnt/storage/Projects/Vegvisir-harness
/projects use vegvisir
/new Test Secret Project
/select
```

`/workspace` changes the active workspace, retargets workspace-scoped tools, and moves CMS project recall to that workspace scope.

## System Prompt Commands

```text
/system show
/system print
/system view
/system set <text>
/system append <text>
/system clear
/system default
/system-prompt
```

Examples:

```text
/system-prompt
/system append Always prefer short verification commands before broad rewrites.
/system default
```

## Provider And Model Commands

```text
/providers
/provider [name]
/models
/model [name]
/auth [provider]
/config status
/config user <id>
/config path
```

Examples:

```text
/providers
/provider openai-hbse
/models
/model gpt-5.5
/config status
```

Provider and model choices can be global defaults or agent-specific defaults depending on whether an agent profile is active.

## Memory Commands

```text
/memory status
/memory recent [--global] [--limit N]
/memory import-chatgpt <export-dir-or-conversations.json> [--messages-per-memory N] [--max-chars-per-memory N]
/remember [--global] <title> | <content>
/recall [--limit N] [--global] <query>
/context <message>
/model-request <message>
```

Examples:

```text
/memory status
/remember Build decision | Use cms-v2 as the memory system for Vegvisir.
/remember --global Preference | Default to streaming provider responses.
/recall --limit 5 provider secrets
/recall --global agent preferences
/memory import-chatgpt ~/Downloads/chatgpt-export/conversations.json --messages-per-memory 8
```

CMS-v2 is scoped by user, project/workspace, session, and active agent. Vegvisir should recall only the context needed for the current turn instead of sending full history by default.

## Tools And Approvals

```text
/tools
/tools status
/tools inventory
/tools allow-risky
/tools deny-risky
/tools require-approval
/tools no-approval
/approvals
/approvals list
/approvals show <id>
/approvals approve <id>
/approvals approve-pattern <id>
/approvals edit <id> <json-args>
/approvals deny <id>
```

Examples:

```text
/tools require-approval
/tools inventory
/approvals
/approvals show 01HV...
/approvals edit 01HV... {"path":"./safe-target"}
/approvals approve 01HV...
```

`allow-risky` enables risky tools for the running session. `require-approval` keeps risky actions gated behind the approval queue.

## Custom Agent Commands

```text
/agent list
/agent templates
/agent create-template <mode> <id> [display name]
/agent create <id> | <mode> | <display name> | <system prompt>
/agent design <id> | <mode> | <display name> | <system prompt> | tools=a,b skills=x,y mcp=server usrl=contract provider=openai-hbse model=gpt-5.5 use=true
/agent use <id>
/agent clone <source-id> <new-id> [display name]
/agent export <id> [path]
/agent import <path>
/agent show <id>
/agent mode <id> <mode>
/agent provider <id> <provider|->
/agent model <id> <model|->
/agent prompt <id> <system prompt>
/agent describe <id> <description>
/agent bind-usrl <id> <contract-id-or-skill-name>
/agent unbind-usrl <id> <contract-id-or-skill-name>
/agent allow-mcp <id> <server-id>
/agent revoke-mcp <id> <server-id>
/agent enable-skill <id> <skill-name>
/agent disable-skill <id> <skill-name>
/agent allow-tool <id> <tool-name>
/agent revoke-tool <id> <tool-name>
/agent clear
/agent delete <id>
```

Examples:

```text
/agent templates
/agent create-template coder project-coder Project Coder
/agent design agent-red | security | Agent Red | Offensive and defensive security auditor. | tools=read_file,list_files,run_command,run_tests,cms_recall skills=risk-check mcp=github usrl=security-audit provider=openai-hbse model=gpt-5.5 use=true
/agent show agent-red
/agent use agent-red
```

Agents persist globally under Vegvisir's data root and can have dedicated prompts, tools, skills, MCP servers, USRL contracts, provider defaults, model defaults, and CMS scopes.

## MCP Commands

```text
/mcp list
/mcp status
/mcp show <id>
/mcp tools
/mcp reload
/mcp add-http <id> <url> <secret_ref> [consumer] [purpose]
/mcp add-http-service <id> <url> <hbse-service-name>
/mcp add-stdio <id> <command> [args...]
/mcp add-tool <server-id> <tool-name> [description]
/mcp remove-tool <server-id> <tool-name>
/mcp remove <id>
/mcp enable <id>
/mcp disable <id>
```

Examples:

```text
/mcp add-http github https://mcp.example.test/rpc secret://vegvisir/mcp/github/default vegvisir.mcp.github mcp.tool.call
/mcp add-http-service github https://mcp.example.test/rpc github
/mcp add-stdio local-tools /usr/local/bin/local-mcp --stdio
/mcp tools
```

HTTP MCP credentials must be HBSE secret refs. STDIO MCP configuration should not contain plaintext credentials.

## HBSE Helper Commands

```text
/hbse status
/hbse provider <provider-id>
/hbse mcp <server> [url] [consumer] [purpose]
/hbse service <name> [consumer] [purpose]
/hbse service add <name> <secret_ref> [consumer] [purpose]
/hbse service show <name>
/hbse service enable <name>
/hbse service disable <name>
/hbse service remove <name>
/hbse services
```

Examples:

```text
/hbse provider openai
/hbse mcp github https://mcp.example.test/rpc
/hbse service add github secret://vegvisir/mcp/github/default vegvisir.mcp.github mcp.tool.call
/hbse services
```

These commands print or register HBSE setup references. Plaintext secrets still need to be entered into HBSE from a trusted terminal.

## Verification, Evals, And Traces

```text
/verify all
/verify auth
/verify mcp
/verify agent
/verify memory
/verify runtime
/verify evals
/eval all
/eval memory
/eval security
/eval tools
/eval injection
/eval golden
/eval file <path>
/trace --limit 10
/trace --json --limit 5
/subagents list
/subagents show <id-or-name>
/subagents cancel <id-or-name>
```

Use these after changing providers, tools, memory, approval behavior, USRL contracts, MCP servers, or agent definitions.

## Input And Scrolling

The input editor supports multi-line expansion, left/right navigation, up/down navigation across input lines, and history recall from the start position. Mouse wheel scrolling controls the chat viewport.
