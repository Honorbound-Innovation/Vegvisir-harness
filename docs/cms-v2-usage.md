# CMS-v2 Usage

CMS-v2 is Vegvisir's runtime memory system. It stores durable memory, recent conversation context, imported history, prompt-cacheable context envelopes, and scoped recall indexes.

## Build

```bash
cargo build -p cms-v2
```

## CLI

The CMS binary is named `cms`.

```bash
cargo run -p cms-v2 --bin cms -- --help
```

Typical usage:

```bash
cargo run -p cms-v2 --bin cms -- remember --user user:default --project /path/to/project --title "Decision" --content "Use HBSE for provider secrets."
cargo run -p cms-v2 --bin cms -- recall --user user:default --project /path/to/project "provider secrets"
cargo run -p cms-v2 --bin cms -- recent --user user:default --project /path/to/project --limit 10
```

## ChatGPT Import

Use the CMS import command against an exported ChatGPT archive or conversations file. Scope imports to the user and project that should own the memories.

```bash
cargo run -p cms-v2 --bin cms -- import-chatgpt \
  --user user:default \
  --project /path/to/project \
  --input /path/to/conversations.json
```

Vegvisir can then recall scoped imported memory through `/recall`, `/context`, and the background context assembler.

## Scope Model

CMS-v2 supports:

- global memory for durable cross-project user preferences and facts
- user-scoped memory
- project/workspace-scoped memory
- session-scoped context
- agent-scoped memory for persistent custom agents

Vegvisir uses one CMS database per configured user data root by default. Project and agent separation is handled through scope fields, not by requiring a different database for every workspace.

## Performance Expectations

CMS retrieval should be useful but not intrusive. Short trivial messages should avoid expensive ambient memory expansion. Larger context assembly should happen only when the query benefits from memory.

