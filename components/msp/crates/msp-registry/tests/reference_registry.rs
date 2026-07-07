use msp_core::{
    ExecutionReport, RuntimeCompatibilityQuery, SkillSearchQuery, SkillTrustPolicy, TrustAction,
};
use msp_registry::LocalRegistry;

#[test]
fn reference_registry_indexes_discovers_loads_and_verifies() {
    let registry = LocalRegistry::open("../../examples/registry").expect("open reference registry");
    assert_eq!(registry.skill_count(), 1);
    assert_eq!(registry.pack_count(), 1);

    let results = registry.search(&SkillSearchQuery {
        task: Some("refactor a rust module".to_string()),
        category: None,
        domain: None,
        language: Some("rust".to_string()),
        available_tools: vec!["read_file".to_string(), "write_file".to_string()],
        max_risk: None,
    });
    assert_eq!(results[0].id, "skill.rust.refactor.module.v1");

    let loaded = registry
        .load_skill("skill.rust.refactor.module.v1")
        .expect("load skill");
    assert!(loaded.body_hash_valid);
    assert!(loaded.body.contains("Rust Module Refactor Skill"));

    let trust = registry
        .verify_trust("skill.rust.refactor.module.v1")
        .expect("verify trust");
    assert!(trust.passed);

    let pack_members = registry
        .validate_pack_members("pack.rust.engineering.v1")
        .expect("validate pack members");
    assert!(pack_members.valid, "{pack_members:?}");
    assert_eq!(pack_members.members.len(), 1);
    assert_eq!(pack_members.members[0].id, "skill.rust.refactor.module.v1");

    let policy: SkillTrustPolicy = serde_json::from_str(include_str!(
        "../../../examples/policies/local-reference.trust-policy.json"
    ))
    .expect("parse policy");
    let pack_dependencies = registry
        .evaluate_pack_dependency_trust(&policy, "pack.rust.engineering.v1")
        .expect("evaluate pack dependencies");
    assert!(pack_dependencies.allowed, "{pack_dependencies:?}");
    assert!(pack_dependencies.dependencies.is_empty());

    let deps = registry
        .resolve_dependencies("skill.rust.refactor.module.v1")
        .expect("resolve dependencies");
    assert!(deps.missing.is_empty());
    assert!(deps.cycles.is_empty());

    let compatibility = registry
        .check_skill_compatibility(
            "skill.rust.refactor.module.v1",
            &RuntimeCompatibilityQuery {
                msp_version: Some("0.1.0".to_string()),
                supported_manifest_versions: vec!["0.1.0".to_string()],
                runtime_name: Some("msp-reference".to_string()),
                runtime_version: Some("0.1.0".to_string()),
                supported_formats: vec!["markdown".to_string()],
                runtime_capabilities: vec![
                    "workspace_read".to_string(),
                    "workspace_write".to_string(),
                ],
                model_capabilities: vec!["code_generation".to_string(), "tool_use".to_string()],
                available_tools: vec!["read_file".to_string(), "write_file".to_string()],
                tool_versions: Default::default(),
                permissions: vec!["workspace_read".to_string(), "workspace_write".to_string()],
                context_window: Some(128_000),
                platform: Some("linux".to_string()),
            },
        )
        .expect("check compatibility");
    assert!(compatibility.compatible, "{compatibility:?}");

    let report: ExecutionReport = serde_json::from_str(include_str!(
        "../../../examples/reports/rust-refactor.report.json"
    ))
    .expect("parse report");
    let verification = registry
        .verify_execution_report(&report)
        .expect("verify execution report");
    assert!(verification.passed);
}

#[test]
fn reference_trust_policy_requires_review_for_workspace_write() {
    let registry = LocalRegistry::open("../../examples/registry").expect("open reference registry");
    let policy: SkillTrustPolicy = serde_json::from_str(include_str!(
        "../../../examples/policies/local-reference.trust-policy.json"
    ))
    .expect("parse policy");
    let evaluation = registry
        .evaluate_trust_policy(&policy, "skill.rust.refactor.module.v1")
        .expect("evaluate policy");
    assert_eq!(evaluation.action, TrustAction::RequireReview);
    assert!(!evaluation.allowed);
    assert!(
        evaluation
            .matched_rules
            .contains(&"review-write-capable-skills".to_string())
    );
}
