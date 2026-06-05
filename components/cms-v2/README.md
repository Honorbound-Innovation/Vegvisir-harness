# CMS-v2

Hybrid Continuum Memory System v2: a local-first Rust memory engine and CLI for structured long-term memory, retrieval, context preparation, imports, archive/restore, prompt-cache planning, and maintenance workflows.

## Status

CMS-v2 is currently an early standalone package exported from the Vegvisir harness monorepo. APIs and CLI behavior may change while the package stabilizes.

## Features

- LML memory parsing, validation, round-tripping, and repair.
- SQLite-backed memory ledger with audit/history support.
- Hybrid retrieval over exact, semantic, graph, recent, project, contradiction, and decision-history modes.
- Scoped memory visibility for user/project-aware retrieval.
- ECM-style context preparation for model requests.
- Prompt-cache planning and usage accounting helpers.
- Importers for USRL, ChatGPT exports, documents, directories, and JSONL.
- Archive, JSON export, database backup, and restore commands.
- Embeddable Rust API for applications that need local memory retrieval.

## Requirements

- Rust toolchain with edition 2024 support.
- SQLite is provided through `rusqlite` with the bundled SQLite feature.

## Build

```bash
cargo build
```

## Test

```bash
cargo test
```

From the Vegvisir monorepo component path:

```bash
cargo test --manifest-path components/cms-v2/Cargo.toml
```

## CLI quick start

Initialize a local CMS database:

```bash
cargo run --bin cms -- init
```

Use a custom database path:

```bash
cargo run --bin cms -- --db ./cms.sqlite3 init
```

Validate and ingest LML memories:

```bash
cargo run --bin cms -- validate memories/example.lml
cargo run --bin cms -- ingest memories/example.lml
cargo run --bin cms -- ingest-dir memories
```

Search and retrieve:

```bash
cargo run --bin cms -- search "architecture decision" --limit 10
cargo run --bin cms -- retrieve "memory import behavior" --mode hybrid --limit 12 --json
```

Prepare context/model requests:

```bash
cargo run --bin cms -- prepare-context "What changed in memory retrieval?" --mode project --json
cargo run --bin cms -- prepare-model-request "Summarize current project memory" --provider local --model unspecified --json
```

Inspect health and maintenance:

```bash
cargo run --bin cms -- status memories --json
cargo run --bin cms -- diagnostics memories --json
cargo run --bin cms -- repair memories --json
```

Archive and restore:

```bash
cargo run --bin cms -- export-archive cms-archive.json --json
cargo run --bin cms -- backup-db cms.sqlite3.bak --json
cargo run --bin cms -- restore-archive cms-archive.json --json
```

List all CLI commands:

```bash
cargo run --bin cms -- --help
```

## Library example

```rust
use cms_v2::cms_api::{CmsMemoryClient, RetrievalRequest};
use cms_v2::cms_runtime::LocalCmsMemoryClient;
use cms_v2::graph::{GraphIndex, SqliteGraphIndex};
use cms_v2::lml::LmlParser;
use cms_v2::sqlite::SqliteLedger;
use cms_v2::vectors::{SqliteVectorIndex, VectorIndex};

fn main() -> anyhow::Result<()> {
    let mut ledger = SqliteLedger::open_memory()?;
    let memory = LmlParser::parse_text(r#"
memory {
    id: "mem_example_embed"
    type: "architecture-note"
    title: "Embedded CMS example"
    created: "2026-05-16"
    updated: "2026-05-16T12:00:00Z"
    confidence: 0.95
    source: "example"

    summary: """
    Applications can embed CMS and retrieve scoped memory through cms-api.
    """

    retrieval {
        tags: ["example", "cms-api"]
        visibility: "public"
    }
}
"#)?;

    ledger.upsert_memory(&memory, None)?;
    SqliteGraphIndex::new(&ledger).upsert_memory(&memory)?;
    SqliteVectorIndex::new(&ledger).upsert_memory(&memory)?;

    let client = LocalCmsMemoryClient::new(&mut ledger);
    let mut request = RetrievalRequest::new("embed CMS");
    request.limit = 5;
    let bundle = client.retrieve(request)?;

    println!("retrieved {} memory object(s)", bundle.results.len());
    Ok(())
}
```

More examples are available in `examples/`.

## Package layout

```text
src/       Rust library and `cms` CLI implementation
examples/  Embedding and runtime examples
tests/     Integration tests
memories/  Example/reference LML memories
```

## Security and privacy notes

CMS-v2 is local-first and can store user/project memory in SQLite. Treat database files, archives, imported documents, and generated memories as potentially sensitive. Do not commit private memory databases or secret-bearing exports.

The CLI includes sensitive-content checks for imported memories and can quarantine findings, but users remain responsible for reviewing imported data before publishing or sharing it.

## License

MIT. See [LICENSE](LICENSE).
