# CMS-v2 Usage And Command Reference

CMS-v2 is Vegvisir's memory system. It stores durable memories, imported history, retrieval indexes, prompt-cacheable envelopes, scoped recall data, and writeback records.

This document is intentionally comprehensive. It includes the full current help tree, exact usage blocks, explanations, operational notes, and examples for every CMS-v2 command currently exposed by the Rust CLI.

Installed command:

```bash
cms-v2
```

Source command:

```bash
cargo run -p cms-v2 --bin cms -- --help
```

## Top-Level Help

```text
Usage: cms [OPTIONS] <COMMAND>

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
  help                   Print this message or the help of the given subcommand(s)

Options:
      --db <DB>  [default: cms.sqlite3]
  -h, --help     Print help
```

## Database Model

Vegvisir normally uses one CMS-v2 SQLite database per configured Vegvisir data root. Project separation is handled by scope fields rather than by creating a new database for every workspace.

Common path:

```text
${XDG_DATA_HOME:-$HOME/.local/share}/vegvisir/cms-v2.sqlite3
```

Important scopes:

- global user memory
- user memory
- project/workspace memory
- session context
- active custom-agent memory

## Setup, Validation, And Health

#### init

Purpose:

Initializes a CMS-v2 database.

When to use it:

Use once for a new database or test fixture.

Exact help:

```text
Usage: cms init

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" init
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### validate

Purpose:

Validates an LML memory file.

When to use it:

Use before ingesting hand-written memory files.

Exact help:

```text
Usage: cms validate <PATH>

Arguments:
  <PATH>

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 validate ./memory.lml
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### check

Purpose:

Runs repository/data checks.

When to use it:

Use during diagnostics and maintenance.

Exact help:

```text
Usage: cms check [OPTIONS] [PATH]

Arguments:
  [PATH]  [default: memories]

Options:
      --json
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" check --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### status

Purpose:

Shows CMS-v2 status.

When to use it:

Use to confirm database health and high-level counts.

Exact help:

```text
Usage: cms status [OPTIONS] [PATH]

Arguments:
  [PATH]  [default: memories]

Options:
      --json
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" status
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### diagnostics

Purpose:

Runs detailed health diagnostics.

When to use it:

Use after imports, migrations, or unexpected retrieval behavior.

Exact help:

```text
Usage: cms diagnostics [OPTIONS] [PATH]

Arguments:
  [PATH]  [default: memories]

Options:
      --audit-limit <AUDIT_LIMIT>  [default: 20]
      --json
  -h, --help                       Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" diagnostics
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### round trip

Purpose:

Exercises serialization round-trip behavior.

When to use it:

Use as a smoke check for parser/writer compatibility.

Exact help:

```text
Usage: cms round-trip <INPUT> <OUTPUT>

Arguments:
  <INPUT>
  <OUTPUT>

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" round-trip --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.

## Ingest And Import

#### ingest

Purpose:

Ingests one LML memory file.

When to use it:

Use for curated memory files.

Exact help:

```text
Usage: cms ingest <PATH>

Arguments:
  <PATH>

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" ingest ./memory.lml
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### ingest dir

Purpose:

Ingests a directory of LML memory files.

When to use it:

Use for bulk curated imports.

Exact help:

```text
Usage: cms ingest-dir <PATH>

Arguments:
  <PATH>

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" ingest-dir ./memories
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### import usrl

Purpose:

Imports a USRL contract as structured memory.

When to use it:

Use when contracts should be searchable and available to Vegvisir.

Exact help:

```text
Usage: cms import-usrl [OPTIONS] <PATH>

Arguments:
  <PATH>

Options:
      --ingest
      --output <OUTPUT>
      --visibility <VISIBILITY>          [default: public]
      --user <USER>
      --project <PROJECT>
      --validator-root <VALIDATOR_ROOT>
      --require-validation
      --json
  -h, --help                             Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-usrl --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### import usrl dir

Purpose:

Imports a directory of USRL contracts.

When to use it:

Use for contract packs.

Exact help:

```text
Usage: cms import-usrl-dir [OPTIONS] <PATH>

Arguments:
  <PATH>

Options:
      --ingest
      --output-dir <OUTPUT_DIR>
      --visibility <VISIBILITY>          [default: public]
      --user <USER>
      --project <PROJECT>
      --validator-root <VALIDATOR_ROOT>
      --require-validation
      --json
  -h, --help                             Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-usrl-dir --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### import chatgpt

Purpose:

Imports a ChatGPT export.

When to use it:

Use to migrate useful previous conversation history into CMS-v2.

Exact help:

```text
Usage: cms import-chatgpt [OPTIONS] <PATH>

Arguments:
  <PATH>

Options:
      --ingest
      --output-dir <OUTPUT_DIR>
      --messages-per-memory <MESSAGES_PER_MEMORY>    [default: 40]
      --max-chars-per-memory <MAX_CHARS_PER_MEMORY>  [default: 0]
      --preview
      --json
  -h, --help                                         Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-chatgpt --ingest ~/Downloads/chatgpt-export/conversations.json
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### import doc

Purpose:

Imports one document.

When to use it:

Use for README, design notes, and docs that should become retrievable memory.

Exact help:

```text
Usage: cms import-doc [OPTIONS] <PATH>

Arguments:
  <PATH>

Options:
      --ingest
      --output-dir <OUTPUT_DIR>
      --preview
      --max-chars-per-memory <MAX_CHARS_PER_MEMORY>  [default: 8000]
      --source-kind <SOURCE_KIND>
      --json
  -h, --help                                         Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-doc --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### import doc dir

Purpose:

Imports a directory of documents.

When to use it:

Use for project documentation folders.

Exact help:

```text
Usage: cms import-doc-dir [OPTIONS] <PATH>

Arguments:
  <PATH>

Options:
      --ingest
      --output-dir <OUTPUT_DIR>
      --preview
      --max-chars-per-memory <MAX_CHARS_PER_MEMORY>  [default: 8000]
      --json
  -h, --help                                         Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-doc-dir --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### import jsonl

Purpose:

Imports JSONL memory records.

When to use it:

Use for scripted migrations.

Exact help:

```text
Usage: cms import-jsonl [OPTIONS] <PATH>

Arguments:
  <PATH>

Options:
      --ingest
      --output-dir <OUTPUT_DIR>
      --preview
      --source-kind <SOURCE_KIND>  [default: jsonl-record]
      --json
  -h, --help                       Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" import-jsonl --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.

## Read, Search, And Retrieve

#### get

Purpose:

Shows one memory by ID.

When to use it:

Use when inspecting a specific result.

Exact help:

```text
Usage: cms get [OPTIONS] <ID>

Arguments:
  <ID>

Options:
      --json
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" get --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### list

Purpose:

Lists memories by status and limit.

When to use it:

Use to inspect recent active/archive/quarantine state.

Exact help:

```text
Usage: cms list [OPTIONS]

Options:
      --status <STATUS>  [default: active]
      --limit <LIMIT>    [default: 50]
      --json
  -h, --help             Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" list --status active --limit 10
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### search

Purpose:

Runs exact text search.

When to use it:

Use when keywords should match stored text directly.

Exact help:

```text
Usage: cms search [OPTIONS] <QUERY>

Arguments:
  <QUERY>

Options:
      --limit <LIMIT>  [default: 12]
      --json
  -h, --help           Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" search "approval queue" --limit 10
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### semantic search

Purpose:

Runs vector/semantic search.

When to use it:

Use when exact wording may differ.

Exact help:

```text
Usage: cms semantic-search [OPTIONS] <QUERY>

Arguments:
  <QUERY>

Options:
      --limit <LIMIT>  [default: 12]
      --json
  -h, --help           Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" semantic-search "approval queue" --limit 10
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### retrieve

Purpose:

Runs hybrid retrieval.

When to use it:

Use for the same style of recall Vegvisir uses.

Exact help:

```text
Usage: cms retrieve [OPTIONS] <QUERY>

Arguments:
  <QUERY>

Options:
      --limit <LIMIT>                    [default: 12]
      --mode <MODE>                      [default: hybrid]
      --user <USER>
      --project <PROJECT>
      --visibility <VISIBILITY>
      --correlation-id <CORRELATION_ID>
      --json
  -h, --help                             Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" retrieve --user user:default --project /path/to/project --limit 8 "provider secrets"
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### graph related

Purpose:

Shows graph-neighbor memories.

When to use it:

Use to inspect related context around a memory.

Exact help:

```text
Usage: cms graph-related [OPTIONS] <ID>

Arguments:
  <ID>

Options:
      --depth <DEPTH>  [default: 2]
      --json
  -h, --help           Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" graph-related --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### history

Purpose:

Shows version/history for a memory.

When to use it:

Use to audit changes to a memory.

Exact help:

```text
Usage: cms history [OPTIONS] <ID>

Arguments:
  <ID>

Options:
      --json
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" history --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.

## Scope Commands

#### scope

Purpose:

Container for memory scope commands.

When to use it:

Use it to inspect or debug user/project/visibility scope behavior.

Exact help:

```text
Usage: cms scope <COMMAND>

Commands:
  resolve
  inspect
  list
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" scope --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### scope resolve

Purpose:

resolves CMS scope information.

When to use it:

Use scope commands when imported or recalled memories do not appear in the expected user or project context.

Exact help:

```text
Usage: cms scope resolve [OPTIONS]

Options:
      --user <USER>
      --project <PROJECT>
      --visibility <VISIBILITY>
      --no-memory
      --json
  -h, --help                     Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" scope resolve --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### scope inspect

Purpose:

inspects CMS scope information.

When to use it:

Use scope commands when imported or recalled memories do not appear in the expected user or project context.

Exact help:

```text
Usage: cms scope inspect [OPTIONS] <ID>

Arguments:
  <ID>

Options:
      --json
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" scope inspect --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### scope list

Purpose:

lists CMS scope information.

When to use it:

Use scope commands when imported or recalled memories do not appear in the expected user or project context.

Exact help:

```text
Usage: cms scope list [OPTIONS]

Options:
      --status <STATUS>          [default: active]
      --visibility <VISIBILITY>
      --user <USER>
      --project <PROJECT>
      --limit <LIMIT>            [default: 50]
      --json
  -h, --help                     Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" scope list --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.

## Context And Model Request

#### prepare context

Purpose:

Builds ECM context for a message.

When to use it:

Use to inspect what memory context would be assembled.

Exact help:

```text
Usage: cms prepare-context [OPTIONS] <MESSAGE>

Arguments:
  <MESSAGE>

Options:
      --mode <MODE>                                          [default: project]
      --project <PROJECT>
      --user <USER>                                          [default: default]
      --max-tokens <MAX_TOKENS>                              [default: 16000]
      --reserved-response-tokens <RESERVED_RESPONSE_TOKENS>  [default: 4000]
      --correlation-id <CORRELATION_ID>
      --json
  -h, --help                                                 Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prepare-context --user user:default --project /path/to/project "What should the agent know?"
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### prepare model request

Purpose:

Builds a provider-cacheable request envelope.

When to use it:

Use to debug model requests and prompt-cache boundaries.

Exact help:

```text
Usage: cms prepare-model-request [OPTIONS] <MESSAGE>

Arguments:
  <MESSAGE>

Options:
      --mode <MODE>                                          [default: project]
      --project <PROJECT>
      --user <USER>                                          [default: default]
      --provider <PROVIDER>                                  [default: local]
      --model <MODEL>                                        [default: unspecified]
      --max-tokens <MAX_TOKENS>                              [default: 16000]
      --reserved-response-tokens <RESERVED_RESPONSE_TOKENS>  [default: 4000]
      --correlation-id <CORRELATION_ID>
      --json
  -h, --help                                                 Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prepare-model-request --provider openai-hbse --model gpt-5.5 --user user:default --project /path/to/project "Summarize the active work"
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### complete turn

Purpose:

Evaluates and optionally commits memory writeback for a completed turn.

When to use it:

Use in harness integrations and writeback testing.

Exact help:

```text
Usage: cms complete-turn [OPTIONS] <USER_MESSAGE> <ASSISTANT_RESPONSE>

Arguments:
  <USER_MESSAGE>
  <ASSISTANT_RESPONSE>

Options:
      --user <USER>                      [default: default]
      --project <PROJECT>
      --correlation-id <CORRELATION_ID>
      --commit
      --json
  -h, --help                             Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" complete-turn --user user:default --project /path/to/project --commit "question" "answer"
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.

## Prompt Cache

#### prompt cache

Purpose:

Container for prompt-cache planning, inspection, usage, and invalidation commands.

When to use it:

Use it to understand and manage provider-cacheable context envelopes.

Exact help:

```text
Usage: cms prompt-cache <COMMAND>

Commands:
  plan
  inspect
  usage
  record-usage
  capsules
  invalidate
  invalidate-source
  invalidate-scope
  explain-miss
  help               Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prompt-cache --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### prompt cache plan

Purpose:

Manages prompt-cache plan behavior.

When to use it:

Use prompt-cache commands when debugging token pressure, cache reuse, invalidation, or provider cache accounting.

Exact help:

```text
Usage: cms prompt-cache plan [OPTIONS] <MESSAGE>

Arguments:
  <MESSAGE>

Options:
      --mode <MODE>                                          [default: project]
      --project <PROJECT>
      --user <USER>                                          [default: default]
      --provider <PROVIDER>                                  [default: local]
      --model <MODEL>                                        [default: unspecified]
      --max-tokens <MAX_TOKENS>                              [default: 16000]
      --reserved-response-tokens <RESERVED_RESPONSE_TOKENS>  [default: 4000]
      --json
  -h, --help                                                 Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prompt-cache plan --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### prompt cache inspect

Purpose:

Manages prompt-cache inspect behavior.

When to use it:

Use prompt-cache commands when debugging token pressure, cache reuse, invalidation, or provider cache accounting.

Exact help:

```text
Usage: cms prompt-cache inspect [OPTIONS] [MANIFEST_ID]

Arguments:
  [MANIFEST_ID]

Options:
      --limit <LIMIT>  [default: 20]
      --json
  -h, --help           Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prompt-cache inspect --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### prompt cache usage

Purpose:

Manages prompt-cache usage behavior.

When to use it:

Use prompt-cache commands when debugging token pressure, cache reuse, invalidation, or provider cache accounting.

Exact help:

```text
Usage: cms prompt-cache usage [OPTIONS] [MANIFEST_ID]

Arguments:
  [MANIFEST_ID]

Options:
      --limit <LIMIT>  [default: 20]
      --json
  -h, --help           Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prompt-cache usage --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### prompt cache record usage

Purpose:

Manages prompt-cache record usage behavior.

When to use it:

Use prompt-cache commands when debugging token pressure, cache reuse, invalidation, or provider cache accounting.

Exact help:

```text
Usage: cms prompt-cache record-usage [OPTIONS] <MANIFEST_ID>

Arguments:
  <MANIFEST_ID>

Options:
      --provider-cached-input-tokens <PROVIDER_CACHED_INPUT_TOKENS>  [default: 0]
      --provider-cache-write-tokens <PROVIDER_CACHE_WRITE_TOKENS>    [default: 0]
      --provider-cache-read-tokens <PROVIDER_CACHE_READ_TOKENS>      [default: 0]
      --latency-ms <LATENCY_MS>                                      [default: 0]
      --json
  -h, --help                                                         Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prompt-cache record-usage --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### prompt cache capsules

Purpose:

Manages prompt-cache capsules behavior.

When to use it:

Use prompt-cache commands when debugging token pressure, cache reuse, invalidation, or provider cache accounting.

Exact help:

```text
Usage: cms prompt-cache capsules [OPTIONS]

Options:
      --limit <LIMIT>  [default: 20]
      --json
  -h, --help           Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prompt-cache capsules --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### prompt cache invalidate

Purpose:

Manages prompt-cache invalidate behavior.

When to use it:

Use prompt-cache commands when debugging token pressure, cache reuse, invalidation, or provider cache accounting.

Exact help:

```text
Usage: cms prompt-cache invalidate [OPTIONS] --reason <REASON> <MANIFEST_ID>

Arguments:
  <MANIFEST_ID>

Options:
      --reason <REASON>
      --changed-source <CHANGED_SOURCE>
      --json
  -h, --help                             Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prompt-cache invalidate --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### prompt cache invalidate source

Purpose:

Manages prompt-cache invalidate source behavior.

When to use it:

Use prompt-cache commands when debugging token pressure, cache reuse, invalidation, or provider cache accounting.

Exact help:

```text
Usage: cms prompt-cache invalidate-source [OPTIONS] --reason <REASON> <SOURCE_MEMORY_ID>

Arguments:
  <SOURCE_MEMORY_ID>

Options:
      --reason <REASON>
      --json
  -h, --help             Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prompt-cache invalidate-source --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### prompt cache invalidate scope

Purpose:

Manages prompt-cache invalidate scope behavior.

When to use it:

Use prompt-cache commands when debugging token pressure, cache reuse, invalidation, or provider cache accounting.

Exact help:

```text
Usage: cms prompt-cache invalidate-scope [OPTIONS] --reason <REASON>

Options:
      --user <USER>
      --project <PROJECT>
      --session <SESSION>
      --shared-scope <SHARED_SCOPE>
      --reason <REASON>
      --json
  -h, --help                         Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prompt-cache invalidate-scope --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### prompt cache explain miss

Purpose:

Manages prompt-cache explain miss behavior.

When to use it:

Use prompt-cache commands when debugging token pressure, cache reuse, invalidation, or provider cache accounting.

Exact help:

```text
Usage: cms prompt-cache explain-miss [OPTIONS] <MANIFEST_ID>

Arguments:
  <MANIFEST_ID>

Options:
      --json
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" prompt-cache explain-miss --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.

## Lifecycle

#### delete

Purpose:

Soft-deletes a memory.

When to use it:

Use carefully when memory should leave retrieval.

Exact help:

```text
Usage: cms delete <ID>

Arguments:
  <ID>

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" delete --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### archive

Purpose:

Archives a memory.

When to use it:

Use when memory should stop being active but remain retained.

Exact help:

```text
Usage: cms archive <ID>

Arguments:
  <ID>

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" archive --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### quarantine

Purpose:

Quarantines suspicious memory.

When to use it:

Use for secret-like, unsafe, or untrusted records.

Exact help:

```text
Usage: cms quarantine <ID>

Arguments:
  <ID>

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" quarantine --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### supersede

Purpose:

Marks one memory superseded by another.

When to use it:

Use when a newer memory replaces old information.

Exact help:

```text
Usage: cms supersede <ID> <REPLACEMENT_ID>

Arguments:
  <ID>
  <REPLACEMENT_ID>

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" supersede --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### merge

Purpose:

Merges a duplicate memory into a canonical memory.

When to use it:

Use for deduplication.

Exact help:

```text
Usage: cms merge <DUPLICATE_ID> <CANONICAL_ID>

Arguments:
  <DUPLICATE_ID>
  <CANONICAL_ID>

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" merge --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### restore

Purpose:

Restores a deleted/archived/quarantined memory.

When to use it:

Use when a lifecycle transition should be reversed.

Exact help:

```text
Usage: cms restore <ID>

Arguments:
  <ID>

Options:
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" restore --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.

## Maintenance

#### reindex

Purpose:

Rebuilds derived indexes.

When to use it:

Use after retrieval/index changes or maintenance.

Exact help:

```text
Usage: cms reindex [OPTIONS]

Options:
      --id <ID>
      --vector-provider <VECTOR_PROVIDER>  [default: cms-lexical-v1]
      --vector-chunking <VECTOR_CHUNKING>  [default: default-chunks-v1]
  -h, --help                               Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" reindex
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### repair

Purpose:

Repairs derived indexes and maintenance issues.

When to use it:

Use after diagnostics reports stale or missing projections.

Exact help:

```text
Usage: cms repair [OPTIONS] [PATH]

Arguments:
  [PATH]  [default: memories]

Options:
      --vector-provider <VECTOR_PROVIDER>  [default: cms-lexical-v1]
      --vector-chunking <VECTOR_CHUNKING>  [default: default-chunks-v1]
      --json
  -h, --help                               Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" repair --json
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### audit

Purpose:

Lists audit events.

When to use it:

Use for operational review.

Exact help:

```text
Usage: cms audit [OPTIONS]

Options:
      --limit <LIMIT>  [default: 20]
      --json
  -h, --help           Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" audit --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.

## Export, Backup, And Restore

#### export archive

Purpose:

Exports an archive.

When to use it:

Use for migration or backup with optional redaction.

Exact help:

```text
Usage: cms export-archive [OPTIONS] <OUTPUT>

Arguments:
  <OUTPUT>

Options:
      --visibility <VISIBILITY>
      --user <USER>
      --project <PROJECT>
      --exclude-private
      --redact-sensitive
      --json
  -h, --help                     Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" export-archive --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### export json

Purpose:

Exports JSON.

When to use it:

Use for portable review or migration.

Exact help:

```text
Usage: cms export-json [OPTIONS] <OUTPUT>

Arguments:
  <OUTPUT>

Options:
      --visibility <VISIBILITY>
      --user <USER>
      --project <PROJECT>
      --exclude-private
      --redact-sensitive
      --json
  -h, --help                     Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" export-json --redact-sensitive /tmp/cms-export.json
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### backup db

Purpose:

Backs up the SQLite database.

When to use it:

Use before migrations, imports, or repair.

Exact help:

```text
Usage: cms backup-db [OPTIONS] <OUTPUT>

Arguments:
  <OUTPUT>

Options:
      --json
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" backup-db /tmp/cms-backup.sqlite3
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### restore archive

Purpose:

Restores from an archive export.

When to use it:

Use during migration or recovery.

Exact help:

```text
Usage: cms restore-archive [OPTIONS] <INPUT>

Arguments:
  <INPUT>

Options:
      --json
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" restore-archive --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.


#### restore json

Purpose:

Restores from JSON export.

When to use it:

Use during migration or recovery.

Exact help:

```text
Usage: cms restore-json [OPTIONS] <INPUT>

Arguments:
  <INPUT>

Options:
      --json
  -h, --help  Print help
```

Examples:

```bash
cms-v2 --db "$HOME/.local/share/vegvisir/cms-v2.sqlite3" restore-json --help
```

Notes:

- Pass `--json` when automating commands that support it.
- Use `--user` and `--project` on scoped commands when operating outside Vegvisir.
- Do not store raw secrets in CMS-v2; store HBSE secret refs instead.
