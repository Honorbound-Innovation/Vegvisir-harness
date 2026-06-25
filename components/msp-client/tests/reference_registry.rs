use msp_client::{LoadMode, MspClient, SearchRequest};

#[test]
fn searches_and_loads_reference_registry() -> anyhow::Result<()> {
    let registry =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../msp/examples/registry");
    let client = MspClient::open(&registry)?;

    let response = client.search(SearchRequest {
        task: Some("refactor a rust module".to_string()),
        available_tools: vec!["read_file".to_string(), "write_file".to_string()],
        limit: Some(5),
        ..SearchRequest::default()
    });

    assert!(response.registry.skill_count >= 1);
    assert!(
        response
            .results
            .iter()
            .any(|hit| hit.id == "skill.rust.refactor.module.v1")
    );

    let loaded = client.load_skill("skill.rust.refactor.module.v1", LoadMode::Card)?;
    assert!(loaded.content.contains("Rust Module Refactor"));
    assert!(loaded.raw.body_hash_valid);

    Ok(())
}
