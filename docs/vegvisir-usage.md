# Vegvisir Usage

Vegvisir is the main agent harness. It can run as an interactive terminal UI or as a headless CLI for scripted work and agent servers.

This page is based on the current `vegvisir --help` output and the built-in slash command registry.

## Installed Command

```bash
vegvisir
```

The installer also installs `vegvisir-rust` as an explicit binary alias.

## Top-Level CLI Help Tree

```text
Usage: vegvisir [OPTIONS] [COMMAND]

Commands:
  tui
  run
  remember
  recall
  context
  model-request
  eval
  verify
  help

Options:
  -p, --prompt <PROMPT>
      --workspace <WORKSPACE>
      --max-steps <MAX_STEPS>
      --provider <PROVIDER>
      --model <MODEL>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
  -h, --help
```

## Start The TUI

The default command starts the TUI:

```bash
vegvisir
```

The explicit form is also accepted:

```bash
vegvisir tui
```

Common startup options:

```bash
vegvisir --workspace /path/to/project
vegvisir --provider openai-hbse --model gpt-5.5
vegvisir --agent coder
```

The dangerous bypass mode is startup-only:

```bash
vegvisir --dangerously-bypass-approvals-and-sandbox
```

That mode authorizes all tools, commands, approvals, and sandbox checks for the running session. It is not exposed as a TUI command.

## Headless Commands

### `run`

```text
Usage: vegvisir run [OPTIONS] <GOAL>

Options:
      --workspace <WORKSPACE>
      --max-steps <MAX_STEPS>
      --provider <PROVIDER>
      --model <MODEL>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
```

Use `run` for non-interactive work:

```bash
vegvisir --workspace /path/to/project run "Create a test plan for this crate"
vegvisir --workspace /path/to/project --json run "List risky files"
vegvisir --workspace /path/to/project --scripted run "Run verification"
```

### `remember`

Stores a durable CMS-v2 memory from the CLI.

```bash
vegvisir --workspace /path/to/project remember "Build decision | Use HBSE for provider secrets"
```

### `recall`

```text
Usage: vegvisir recall [OPTIONS] <QUERY>

Options:
      --limit <LIMIT>
      --workspace <WORKSPACE>
      --provider <PROVIDER>
      --model <MODEL>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
```

Example:

```bash
vegvisir --workspace /path/to/project recall --limit 8 "provider setup"
```

### `context`

Prepares CMS-v2 ECM context for a message.

```bash
vegvisir --workspace /path/to/project context "What should I remember about this repo?"
```

### `model-request`

Prepares a provider-cacheable CMS-v2 model request envelope.

```text
Usage: vegvisir model-request [OPTIONS] <MESSAGE>

Options:
      --provider <PROVIDER>
      --model <MODEL>
      --workspace <WORKSPACE>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
```

Example:

```bash
vegvisir --workspace /path/to/project model-request --provider openai-hbse --model gpt-5.5 "Summarize the current work"
```

### `eval`

```text
Usage: vegvisir eval [OPTIONS] [SCOPE]

Arguments:
  [SCOPE]  default: all

Options:
      --file <FILE>
      --provider <PROVIDER>
      --model <MODEL>
      --agent <AGENT>
      --json
      --scripted
      --dangerously-bypass-approvals-and-sandbox
```

Examples:

```bash
vegvisir eval all
vegvisir eval security
vegvisir eval --file ./evals/workspace.json
```

### `verify`

```bash
vegvisir verify all --workspace /path/to/project
vegvisir verify auth
vegvisir verify mcp
vegvisir verify memory
vegvisir verify runtime
```

## TUI Slash Command Tree

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
