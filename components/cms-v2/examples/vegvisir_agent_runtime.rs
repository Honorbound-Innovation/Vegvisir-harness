use cms_v2::cms_api::{CmsMemoryClient, ProjectId, RetrievalMode, RetrievalRequest};
use cms_v2::cms_runtime::LocalCmsMemoryClient;
use cms_v2::ecm::{ContextMode, ContextRequest, ContextSession, EterniumContextManager, UserId};
use cms_v2::graph::{GraphIndex, SqliteGraphIndex};
use cms_v2::lml::LmlParser;
use cms_v2::prompt_cache::{CacheScopeIdentity, PromptCacheEngine, PromptCachePrepareRequest};
use cms_v2::sqlite::SqliteLedger;
use cms_v2::vectors::{SqliteVectorIndex, VectorIndex};
use serde_json::Value;

fn main() -> anyhow::Result<()> {
    let mut ledger = SqliteLedger::open_memory()?;
    seed_vegvisir_memory(&mut ledger)?;

    let planner_prepared = {
        let ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
        ecm.prepare_context(
            ContextRequest::new(
                "codex-agent",
                "Plan the Vegvisir agent runtime handoff",
                ContextMode::Architecture,
            )
            .with_project("Vegvisir"),
        )?
    };
    let planner_prompt = PromptCacheEngine::prepare_model_prompt(
        &planner_prepared,
        PromptCachePrepareRequest::new("openai", "agent-runtime-example").with_scope_identity(
            CacheScopeIdentity::for_user_project("codex-agent", "Vegvisir")
                .with_session(planner_prepared.session_id.0.clone()),
        ),
    );

    let session = ContextSession::new(UserId::new("codex-agent"), Some(ProjectId::new("Vegvisir")));
    let writeback = {
        let mut ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
        ecm.complete_turn(
            &session,
            "Capture the Vegvisir runtime handoff decision.",
            "Decision: Vegvisir agents should use CMS for durable shared memory, ECM for scoped context assembly, and prompt-cache manifests for provider handoff.",
        )?
    };

    let retrieval = {
        let client = LocalCmsMemoryClient::new(&mut ledger);
        let mut request =
            RetrievalRequest::new("durable shared memory scoped context prompt-cache manifests")
                .with_project("Vegvisir");
        request.modes = vec![RetrievalMode::Hybrid, RetrievalMode::Semantic];
        request.limit = 5;
        request.filters.insert(
            "user_id".to_string(),
            Value::String("codex-agent".to_string()),
        );
        client.retrieve(request)?
    };

    println!(
        "planner_cache_key={} writebacks={} retrieved={}",
        planner_prompt.manifest.prompt_cache_key,
        writeback.len(),
        retrieval.results.len()
    );
    Ok(())
}

fn seed_vegvisir_memory(ledger: &mut SqliteLedger) -> anyhow::Result<()> {
    let memory = LmlParser::parse_text(
        r#"
memory {
    id: "mem_example_vegvisir_agent_routing"
    type: "runtime-policy"
    title: "Vegvisir agent routing policy"
    created: "2026-05-16"
    updated: "2026-05-16T12:00:00Z"
    confidence: 0.95
    source: "example"

    summary: """
    Vegvisir agents share durable project memory through CMS before acting.
    """

    retrieval {
        tags: ["HarnessOS", "Vegvisir", "agent-runtime"]
        visibility: "shared"
        user_id: "codex-agent"
        project_id: "Vegvisir"
        prompt_zone: "StableMemoryCapsule"
        prompt_cache_policy: "project_stable"
    }
}
"#,
    )?;
    ledger.upsert_memory(&memory, None)?;
    SqliteGraphIndex::new(ledger).upsert_memory(&memory)?;
    SqliteVectorIndex::new(ledger).upsert_memory(&memory)?;
    Ok(())
}
