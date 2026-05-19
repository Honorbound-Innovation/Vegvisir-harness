# CMS-v2 Usage

CMS-v2 is Vegvisir's memory system. It stores durable memories, recent session context, imported history, retrieval indexes, scoped recall data, and provider-cacheable context envelopes.

This page is based on the current `cms-v2 --help` output. In source, the Cargo package is `cms-v2` and the binary target is named `cms`; the installer exposes that binary as `cms-v2`.

## Installed Command

```bash
cms-v2
```

From the repository root during development:

```bash
cargo run -p cms-v2 --bin cms -- --help
```

## Top-Level Help Tree

```text
Usage: cms-v2 [OPTIONS] <COMMAND>

Commands:
  init
  validate
  ingest
  ingest-dir
  import-usrl
  import-usrl-dir
  import-chatgpt
  import-doc
  import-doc-dir
  import-jsonl
  check
  status
  get
  list
  scope
  delete
  archive
  quarantine
  supersede
  merge
  restore
  search
  retrieve
  prepare-context
  prepare-model-request
  prompt-cache
  complete-turn
  graph-related
  semantic-search
  reindex
  repair
  history
  audit
  diagnostics
  round-trip
  export-archive
  export-json
  backup-db
  restore-archive
  restore-json

Options:
      --db <DB>  default: cms.sqlite3
  -h, --help
```

## Database Model

Vegvisir normally uses one CMS-v2 SQLite database per configured Vegvisir data root. Project separation is handled through scope fields rather than by forcing a separate database for every workspace.

Common Vegvisir data location:

```text
${XDG_DATA_HOME:-$HOME/.local/share}/vegvisir/cms-v2.sqlite3
```

Scopes used by Vegvisir:

- global user memory for cross-project preferences and durable facts
- user-scoped memory
- project/workspace-scoped memory
- session-scoped context
- agent-scoped memory for persistent custom agents

Vegvisir passes the active user, project/workspace, session, and agent metadata automatically. When using the CLI directly, pass `--user` and `--project` on commands that support those flags.

## Setup And Health

Initialize a database:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" init
```

Validate memory files:

```bash
cms-v2 validate ./memory.lml
```

Inspect database health:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" status
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" diagnostics
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" audit --limit 20
```

## Ingest And Import Commands

```text
ingest <PATH>
ingest-dir <PATH>
import-usrl [OPTIONS] <PATH>
import-usrl-dir [OPTIONS] <PATH>
import-chatgpt [OPTIONS] <PATH>
import-doc [OPTIONS] <PATH>
import-doc-dir [OPTIONS] <PATH>
import-jsonl [OPTIONS] <PATH>
```

### LML Ingest

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" ingest ./memory.lml
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" ingest-dir ./memories
```

### ChatGPT Import

```text
Usage: cms-v2 import-chatgpt [OPTIONS] <PATH>

Options:
      --ingest
      --output-dir <OUTPUT_DIR>
      --messages-per-memory <MESSAGES_PER_MEMORY>    default: 40
      --max-chars-per-memory <MAX_CHARS_PER_MEMORY>  default: 0
      --preview
      --json
```

Preview first:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-chatgpt \
  /path/to/conversations.json
```

Ingest into the active CMS database:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-chatgpt \
  --ingest \
  --messages-per-memory 40 \
  /path/to/conversations.json
```

Inside Vegvisir, use the background importer:

```text
/memory import-chatgpt ~/Downloads/chatgpt-export/conversations.json --messages-per-memory 40
```

### Document Import

```text
Usage: cms-v2 import-doc [OPTIONS] <PATH>
Usage: cms-v2 import-doc-dir [OPTIONS] <PATH>

Options:
      --ingest
      --output-dir <OUTPUT_DIR>
      --preview
      --max-chars-per-memory <MAX_CHARS_PER_MEMORY>
      --json
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-doc --ingest ./README.md
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-doc-dir --ingest ./docs
```

### JSONL Import

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-jsonl \
  --ingest \
  /path/to/memories.jsonl
```

### USRL Import

`import-usrl` supports user and project fields:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-usrl \
  --user user:default \
  --project /path/to/project \
  --ingest \
  /path/to/contract.usrl
```

Use `--require-validation` when the USRL validator must accept the contract before import.

## Read And Recall Commands

```text
get [--json] <ID>
list [--status <STATUS>] [--limit <LIMIT>] [--json]
search [--limit <LIMIT>] [--json] <QUERY>
semantic-search [--limit <LIMIT>] [--json] <QUERY>
retrieve [OPTIONS] <QUERY>
graph-related [--depth <DEPTH>] [--json] <ID>
history [--json] <ID>
```

Retrieve help:

```text
Usage: cms-v2 retrieve [OPTIONS] <QUERY>

Options:
      --limit <LIMIT>                    default: 12
      --mode <MODE>                      default: hybrid
      --user <USER>
      --project <PROJECT>
      --visibility <VISIBILITY>
      --correlation-id <CORRELATION_ID>
      --json
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" list --status active --limit 10

cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" retrieve \
  --user user:default \
  --project /path/to/project \
  --limit 8 \
  "provider secrets"

cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" search "approval queue" --limit 10
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" semantic-search "approval queue" --limit 10
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" get <memory-id> --json
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" graph-related <memory-id> --depth 2
```

## Scope Commands

```text
Usage: cms-v2 scope <COMMAND>

Commands:
  resolve
  inspect
  list
```

Use these when debugging which user/project/visibility scope a memory belongs to.

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" scope list
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" scope inspect <scope-id>
```

## Context And Model Request Commands

```text
prepare-context [OPTIONS] <MESSAGE>
prepare-model-request [OPTIONS] <MESSAGE>
complete-turn [OPTIONS] <USER_MESSAGE> <ASSISTANT_RESPONSE>
prompt-cache <COMMAND>
```

Prepare ECM context:

```text
Usage: cms-v2 prepare-context [OPTIONS] <MESSAGE>

Options:
      --mode <MODE>                                          default: project
      --project <PROJECT>
      --user <USER>                                          default: default
      --max-tokens <MAX_TOKENS>                              default: 16000
      --reserved-response-tokens <RESERVED_RESPONSE_TOKENS>  default: 4000
      --correlation-id <CORRELATION_ID>
      --json
```

Prepare a provider-cacheable request:

```text
Usage: cms-v2 prepare-model-request [OPTIONS] <MESSAGE>

Options:
      --mode <MODE>                                          default: project
      --project <PROJECT>
      --user <USER>                                          default: default
      --provider <PROVIDER>                                  default: local
      --model <MODEL>                                        default: unspecified
      --max-tokens <MAX_TOKENS>                              default: 16000
      --reserved-response-tokens <RESERVED_RESPONSE_TOKENS>  default: 4000
      --correlation-id <CORRELATION_ID>
      --json
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prepare-context \
  --user user:default \
  --project /path/to/project \
  "What should the agent know before editing?"

cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prepare-model-request \
  --provider openai-hbse \
  --model gpt-5.5 \
  --user user:default \
  --project /path/to/project \
  "Summarize the active work"

cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" complete-turn \
  --user user:default \
  --project /path/to/project \
  --commit \
  "What changed?" \
  "Updated provider routing."
```

Prompt cache subcommands:

```text
prompt-cache plan
prompt-cache inspect
prompt-cache usage
prompt-cache record-usage
prompt-cache capsules
prompt-cache invalidate
prompt-cache invalidate-source
prompt-cache invalidate-scope
prompt-cache explain-miss
```

## Lifecycle Commands

```text
delete <ID>
archive <ID>
quarantine <ID>
supersede <ID> <REPLACEMENT_ID>
merge <DUPLICATE_ID> <CANONICAL_ID>
restore <ID>
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" archive <memory-id>
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" quarantine <memory-id>
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" restore <memory-id>
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" merge <duplicate-id> <canonical-id>
```

Use quarantine for suspicious or unsafe content. Use archive for memory that should stop participating in normal recall.

## Maintenance And Repair

```text
reindex
repair [--json]
round-trip
diagnostics
audit [--limit <LIMIT>] [--json]
check
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" diagnostics
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" repair --json
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" reindex
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" audit --limit 50 --json
```

Run diagnostics and repair after importing large archives, changing retrieval behavior, or moving data between machines.

## Export, Restore, And Backup

```text
export-json [OPTIONS] <OUTPUT>
export-archive [OPTIONS] <OUTPUT>
backup-db <OUTPUT>
restore-json [--json] <INPUT>
restore-archive [--json] <INPUT>
```

Export JSON:

```text
Usage: cms-v2 export-json [OPTIONS] <OUTPUT>

Options:
      --visibility <VISIBILITY>
      --user <USER>
      --project <PROJECT>
      --exclude-private
      --redact-sensitive
      --json
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" export-json \
  --user user:default \
  --project /path/to/project \
  --redact-sensitive \
  /tmp/cms-export.json

cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" export-archive \
  --redact-sensitive \
  /tmp/cms-archive

cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" backup-db /tmp/cms-backup.sqlite3
cms-v2 --db /tmp/restored-cms.sqlite3 restore-json /tmp/cms-export.json
cms-v2 --db /tmp/restored-cms.sqlite3 restore-archive /tmp/cms-archive
```

Use `--redact-sensitive` for exports that may leave your system or be attached to reports.

## Vegvisir Integration

Vegvisir uses CMS-v2 behind these commands:

```text
/remember <title> | <content>
/remember --global <title> | <content>
/recall [--limit N] [--global] <query>
/memory status
/memory recent [--global] [--limit N]
/memory import-chatgpt <path> [--messages-per-memory N] [--max-chars-per-memory N]
/context <query>
/model-request <prompt>
```

Memory recall should stay useful without slowing normal conversation. Short trivial prompts should not trigger heavy retrieval. Project recall should be preferred by default; global recall should be explicit.

## Safety Rules

CMS-v2 is for useful durable context, not secrets.

Do not store provider API keys, private keys, passwords, secret-bearing URLs, raw tokens, or service credentials in memory. Store a reference instead:

```text
secret://vegvisir/providers/openai/default
secret://vegvisir/mcp/github/default
secret://vegvisir/services/postgres/default
```

HBSE owns the credential. CMS-v2 can remember that a secret ref exists and what it is for.
