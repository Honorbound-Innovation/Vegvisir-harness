# Vegvisir Usage

Vegvisir is the main agent harness. It can run as a terminal UI or as a headless CLI.

## Start The TUI

```bash
vegvisir tui
```

Common startup options:

```bash
vegvisir --workspace /path/to/project tui
vegvisir --provider openai-hbse --model gpt-5.5 tui
vegvisir --agent coder tui
```

The dangerous bypass mode is intentionally startup-only:

```bash
vegvisir --dangerously-bypass-approvals-and-sandbox tui
```

Use it only for a trusted local session where all tools and terminal commands should be authorized.

## Headless Runs

```bash
vegvisir --workspace /path/to/project run "Create a test plan for this crate"
vegvisir --workspace /path/to/project --json run "List risky files"
vegvisir --workspace /path/to/project --scripted run "Run verification"
```

Headless mode uses the same provider, memory, workspace, tool, approval, and CMS systems as the TUI. It does not require rendering the TUI.

## Slash Commands

Session commands:

```text
/new [title]
/load <session-id>
/sessions
/history
/save
/retry
/undo
/title <name>
/branch [name]
/compress [topic]
```

System prompt commands:

```text
/system-prompt
/system show
/system print
/system set <text>
/system append <text>
/system clear
/system default
```

Provider and model commands:

```text
/providers
/provider <id>
/models
/model <id>
/config provider <id>
/config model <id>
/config status
```

Workspace and project commands:

```text
/workspace
/workspace /absolute/path
/workspace ~/Project
/project /absolute/path
/projects
/projects name <alias> <path>
/projects use <alias>
/projects forget <alias>
```

Memory commands:

```text
/remember <title> | <content>
/recall <query>
/recall --global <query>
/memory status
/memory recent --limit 10
/context <query>
/model-request <prompt>
```

Tool and approval commands:

```text
/tools status
/tools inventory
/tools allow-risky
/tools deny-risky
/tools require-approval
/tools no-approval
/approvals
/approvals show <id>
/approvals approve <id>
/approvals approve-pattern <id>
/approvals edit <id> <json-args>
/approvals deny <id>
```

Agent commands:

```text
/agent list
/agent templates
/agent create <id> | <mode> | <display name> | <system prompt>
/agent design <id> | <mode> | <display name> | <system prompt> | tools=a,b skills=x,y mcp=srv usrl=contract provider=openai-hbse model=gpt-5.5 use=true
/agent use <id>
/agent show <id>
/agent clear
/agent prompt <id> <text>
/agent describe <id> <text>
/agent provider <id> <provider-id>
/agent model <id> <model-id>
/agent allow-tool <id> <tool-name>
/agent revoke-tool <id> <tool-name>
/agent enable-skill <id> <skill-id>
/agent disable-skill <id> <skill-id>
/agent bind-usrl <id> <contract-id>
/agent unbind-usrl <id> <contract-id>
/agent allow-mcp <id> <server-id>
/agent revoke-mcp <id> <server-id>
```

MCP commands:

```text
/mcp list
/mcp status
/mcp tools
/mcp show <server>
/mcp add-http <id> <url> <secret-ref> [consumer] [purpose]
/mcp add-http-service <id> <url> <hbse-service-id>
/mcp add-stdio <id> <command> [args...]
/mcp add-tool <server> <tool-name> <description>
/mcp remove-tool <server> <tool-name>
/mcp enable <server>
/mcp disable <server>
/mcp remove <server>
```

Verification and eval commands:

```text
/verify all
/verify auth
/verify mcp
/verify memory
/verify runtime
/eval security
/eval memory
/eval golden
/eval file <path>
/trace --limit 10
/trace --json --limit 5
```

## Filesystem Tools

Filesystem tools are workspace scoped. Switching workspaces retargets file tools and CMS project scope. Risky file or command operations can require approval depending on runtime policy.

## Input And Scrolling

The input editor supports multi-line expansion, left/right navigation, up/down navigation across input lines, and history recall from the start position. Mouse wheel scrolling controls the chat viewport.

