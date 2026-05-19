use cms_v2::cms_api::{CmsMemoryClient, RetrievalRequest};
use cms_v2::cms_runtime::LocalCmsMemoryClient;
use cms_v2::graph::{GraphIndex, SqliteGraphIndex};
use cms_v2::lml::LmlParser;
use cms_v2::sqlite::SqliteLedger;
use cms_v2::vectors::{SqliteVectorIndex, VectorIndex};

fn main() -> anyhow::Result<()> {
    let mut ledger = SqliteLedger::open_memory()?;
    let memory = LmlParser::parse_text(
        r#"
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
"#,
    )?;
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
