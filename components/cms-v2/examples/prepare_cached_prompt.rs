use cms_v2::cms_runtime::LocalCmsMemoryClient;
use cms_v2::ecm::{ContextBudget, ContextMode, ContextRequest, EterniumContextManager};
use cms_v2::prompt_cache::{CacheScopeIdentity, PromptCacheEngine, PromptCachePrepareRequest};
use cms_v2::sqlite::SqliteLedger;

fn main() -> anyhow::Result<()> {
    let mut ledger = SqliteLedger::open_memory()?;
    let ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut ledger));
    let mut request = ContextRequest::new(
        "example-user",
        "Continue the architecture plan",
        ContextMode::Architecture,
    )
    .with_project("example-project");
    request.budget = ContextBudget {
        max_tokens: 8_000,
        reserved_for_response: 2_000,
        reserved_for_system: 1_000,
        reserved_for_tools: 1_000,
    };
    let prepared = ecm.prepare_context(request)?;

    let envelope = PromptCacheEngine::prepare_model_prompt(
        &prepared,
        PromptCachePrepareRequest::new("openai", "gpt-example").with_scope_identity(
            CacheScopeIdentity::for_user_project("example-user", "example-project")
                .with_session(prepared.session_id.0.clone()),
        ),
    );

    println!("{}", envelope.manifest.prompt_cache_key);
    Ok(())
}
