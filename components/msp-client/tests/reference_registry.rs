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

#[test]
fn imports_recent_skiller_bundle_shape_and_loads_generated_skill() -> anyhow::Result<()> {
    let source_bundle = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../msp/examples/skiller-bundles/sample-rust-bundle");
    let temp = tempfile::tempdir()?;
    let bundle = temp.path().join("bundle");
    copy_dir_recursive(&source_bundle, &bundle)?;

    std::fs::create_dir_all(bundle.join("graph"))?;
    std::fs::write(
        bundle.join("sources/sections.yaml"),
        r#"- section_id: sec-1
  source_id: src-1
  title: Small-step refactor workflow
  content: Refactor in small steps and verify each step.
"#,
    )?;
    std::fs::write(bundle.join("graph/related.yaml"), "[]\n")?;
    std::fs::write(
        bundle.join("graph/concepts.yaml"),
        r#"- concept_id: concept-rust-refactor
  name: Rust refactor
"#,
    )?;
    std::fs::write(
        bundle.join("candidates.yaml"),
        r#"- candidate_id: candidate-refactor
  source_section_ids: [sec-1]
"#,
    )?;
    std::fs::write(
        bundle.join("forge_requests.yaml"),
        r#"- request_id: req-1
  pass_type: ScriptGeneration
"#,
    )?;
    std::fs::write(
        bundle.join("forge_responses.yaml"),
        r#"- response_id: resp-1
  request_id: req-1
"#,
    )?;
    std::fs::write(bundle.join("MANIFEST.sha256"), "sha256:test\n")?;
    std::fs::write(
        bundle.join("PROVENANCE.json"),
        r#"{"generator":"skiller-test"}"#,
    )?;

    let skill_path = bundle.join("skills/refactor-module.yaml");
    let mut skill_yaml = std::fs::read_to_string(&skill_path)?;
    skill_yaml.push_str(
        r#"
scripts:
  - id: check-refactor
    title: Check refactor
    description: Run a focused refactor verification command.
    script_type: Command
    language: shell
    entrypoint: cargo test
    content: cargo test -p example
    inputs: [module path]
    outputs: [test report]
    deterministic: true
    idempotent: true
    requires_approval: false
    permission_level: ReadOnly
    when_to_use:
      - after a small refactor step
    guardrails:
      - do not run destructive commands
    generated_by: forge
    source_section_ids: [sec-1]
confidence:
  overall: 0.87
evidence_breakdown:
  direct: 1
inference_records:
  - inference_id: inf-1
    rationale: Generated from source-backed procedure.
"#,
    );
    std::fs::write(&skill_path, skill_yaml)?;

    let registry = temp.path().join("registry");
    let client = MspClient::open(&registry)?;
    let imported = client.import_skiller_bundle(msp_client::ImportSkillerBundleRequest {
        bundle: bundle.clone(),
        issuer: "vegvisir-test.local".to_string(),
        ..Default::default()
    })?;

    assert_eq!(imported.registry.skill_count, 1);
    assert_eq!(imported.registry.pack_count, 1);
    assert_eq!(
        imported.report.skills_published,
        vec!["skill.software_engineering.rust.refactor-module.v1".to_string()]
    );

    let client = MspClient::open(&registry)?;
    let loaded = client.load_skill(
        "skill.software_engineering.rust.refactor-module.v1",
        LoadMode::Extended,
    )?;
    assert!(loaded.raw.body_hash_valid);
    assert!(loaded.content.contains("Refactor Rust Module"));
    assert!(
        loaded
            .content
            .contains("Run focused checks after each meaningful edit.")
    );

    let manifest = client.get_manifest("skill.software_engineering.rust.refactor-module.v1")?;
    let skiller_ext = manifest
        .extensions
        .get("msp:skiller")
        .and_then(|value| value.as_object())
        .expect("skill skiller extension object");
    assert_eq!(skiller_ext["script_count"], serde_json::json!(1));
    assert_eq!(skiller_ext["inference_record_count"], serde_json::json!(1));
    assert_eq!(
        skiller_ext["scripts"][0]["id"],
        serde_json::json!("check-refactor")
    );

    let pack = client.get_pack_manifest("pack.sample-rust-bundle.v1")?;
    let pack_skiller_ext = pack
        .extensions
        .get("msp:skiller")
        .and_then(|value| value.as_object())
        .expect("pack skiller extension object");
    assert_eq!(pack_skiller_ext["section_count"], serde_json::json!(1));
    assert_eq!(pack_skiller_ext["candidate_count"], serde_json::json!(1));
    assert_eq!(
        pack_skiller_ext["forge_request_count"],
        serde_json::json!(1)
    );
    assert_eq!(
        pack_skiller_ext["forge_response_count"],
        serde_json::json!(1)
    );
    assert_eq!(
        pack_skiller_ext["manifest_sha256_present"],
        serde_json::json!(true)
    );
    assert_eq!(
        pack_skiller_ext["provenance_present"],
        serde_json::json!(true)
    );

    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
