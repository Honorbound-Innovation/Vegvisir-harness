use msp_core::{MspSchemaKind, validate_json_schema};
use serde_json::Value;
use std::process::{Command, Output};

const REGISTRY: &str = "../../examples/registry";
const POLICY: &str = "../../examples/policies/local-reference.trust-policy.json";
const REPORT: &str = "../../examples/reports/rust-refactor.report.json";
const SKILL_ID: &str = "skill.rust.refactor.module.v1";
const PACK_ID: &str = "pack.rust.engineering.v1";

fn msp(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_msp-cli"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("run msp-cli")
}

fn msp_json(args: &[&str]) -> Value {
    let output = msp(args);
    assert!(
        output.status.success(),
        "msp-cli failed\nargs: {args:?}\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout should be valid JSON: {error}\nargs: {args:?}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn assert_schema(kind: MspSchemaKind, value: &Value) {
    validate_json_schema(kind, value).unwrap_or_else(|error| {
        panic!("CLI output should validate as {kind:?}: {error}\n{value:#}")
    });
}

#[test]
fn info_and_index_emit_json_contracts() {
    let info = msp_json(&["info"]);
    assert_eq!(info["name"], "msp-reference");
    assert!(
        info["methods"]
            .as_array()
            .is_some_and(|methods| !methods.is_empty())
    );

    let index = msp_json(&["--registry", REGISTRY, "registry", "index"]);
    assert_eq!(index["skills"], 1);
    assert_eq!(index["packs"], 1);
    assert!(
        index["root"]
            .as_str()
            .is_some_and(|root| root.ends_with("examples/registry"))
    );
}

#[test]
fn skill_discovery_aliases_emit_identical_schema_valid_results() {
    let registry_search = msp_json(&[
        "--registry",
        REGISTRY,
        "registry",
        "search",
        "--task",
        "refactor Rust module",
        "--tool",
        "read_file",
        "--tool",
        "write_file",
        "--max-risk",
        "medium",
    ]);
    let skills_discover = msp_json(&[
        "--registry",
        REGISTRY,
        "skills",
        "discover",
        "--task",
        "refactor Rust module",
        "--tool",
        "read_file",
        "--tool",
        "write_file",
        "--max-risk",
        "medium",
    ]);

    assert_schema(MspSchemaKind::RegistrySearchResultArray, &registry_search);
    assert_schema(MspSchemaKind::RegistrySearchResultArray, &skills_discover);
    assert_eq!(registry_search, skills_discover);
    assert_eq!(registry_search[0]["id"], SKILL_ID);
}

#[test]
fn skill_read_load_dependency_verification_and_compatibility_outputs_match_schemas() {
    let manifest = msp_json(&["--registry", REGISTRY, "skills", "manifest", SKILL_ID]);
    assert_schema(MspSchemaKind::Manifest, &manifest);

    let load = msp_json(&["--registry", REGISTRY, "skills", "load", SKILL_ID]);
    assert_schema(MspSchemaKind::SkillLoadResult, &load);
    assert_eq!(load["manifest"]["id"], SKILL_ID);
    assert_eq!(load["body_hash_valid"], true);

    let dependencies = msp_json(&[
        "--registry",
        REGISTRY,
        "skills",
        "resolve-dependencies",
        SKILL_ID,
    ]);
    assert_schema(MspSchemaKind::DependencyResolutionResult, &dependencies);
    assert_eq!(dependencies["root"], SKILL_ID);

    let verification = msp_json(&["--registry", REGISTRY, "skills", "verify-result", REPORT]);
    assert_schema(MspSchemaKind::SkillVerificationResult, &verification);
    assert_eq!(verification["skill_id"], SKILL_ID);
    assert_eq!(verification["passed"], true);

    let compatibility = msp_json(&[
        "--registry",
        REGISTRY,
        "skills",
        "check-compatibility",
        SKILL_ID,
        "--msp-version",
        "0.1.0",
        "--manifest-version",
        "0.1.0",
        "--format",
        "markdown",
        "--runtime-name",
        "msp-reference",
        "--runtime-capability",
        "workspace_read",
        "--runtime-capability",
        "workspace_write",
        "--model-capability",
        "code_generation",
        "--model-capability",
        "tool_use",
        "--tool",
        "read_file",
        "--tool",
        "write_file",
        "--permission",
        "workspace_read",
        "--permission",
        "workspace_write",
        "--context-window",
        "16000",
        "--platform",
        "linux",
    ]);
    assert_schema(MspSchemaKind::SkillCompatibilityResult, &compatibility);
    assert_eq!(compatibility["skill_id"], SKILL_ID);
    assert_eq!(compatibility["compatible"], true);
}

#[test]
fn pack_commands_emit_schema_valid_results_and_manifest_load_aliases_match() {
    let packs = msp_json(&[
        "--registry",
        REGISTRY,
        "packs",
        "discover",
        "--task",
        "rust engineering",
        "--max-risk",
        "medium",
    ]);
    assert_schema(MspSchemaKind::PackSearchResultArray, &packs);
    assert_eq!(packs[0]["id"], PACK_ID);

    let manifest = msp_json(&["--registry", REGISTRY, "packs", "manifest", PACK_ID]);
    let load = msp_json(&["--registry", REGISTRY, "packs", "load", PACK_ID]);
    assert_schema(MspSchemaKind::SkillPack, &manifest);
    assert_schema(MspSchemaKind::SkillPack, &load);
    assert_eq!(manifest, load);
    assert_eq!(manifest["id"], PACK_ID);

    let trust = msp_json(&["--registry", REGISTRY, "packs", "verify-trust", PACK_ID]);
    assert_schema(MspSchemaKind::TrustVerifyResult, &trust);
    assert_eq!(trust["passed"], true);

    let policy = msp_json(&[
        "--registry",
        REGISTRY,
        "packs",
        "evaluate-trust",
        PACK_ID,
        "--policy",
        POLICY,
    ]);
    assert_schema(MspSchemaKind::TrustPolicyEvaluation, &policy);
    assert_eq!(policy["allowed"], true);

    let members = msp_json(&[
        "--registry",
        REGISTRY,
        "packs",
        "validate-members",
        PACK_ID,
        "--policy",
        POLICY,
    ]);
    assert_schema(MspSchemaKind::PackMemberValidationResult, &members);
    assert_eq!(members["valid"], false);
    assert_eq!(
        members["members"][0]["trust_evaluation"]["action"],
        "require_review"
    );

    let dependencies = msp_json(&[
        "--registry",
        REGISTRY,
        "packs",
        "evaluate-dependencies",
        PACK_ID,
        "--policy",
        POLICY,
    ]);
    assert_schema(
        MspSchemaKind::DependencyTrustEvaluationResult,
        &dependencies,
    );
    assert_eq!(dependencies["allowed"], true);
}

#[test]
fn trust_commands_emit_schema_valid_results() {
    let manifest_trust = msp_json(&["--registry", REGISTRY, "trust", "verify", SKILL_ID]);
    assert_schema(MspSchemaKind::TrustVerifyResult, &manifest_trust);
    assert_eq!(manifest_trust["passed"], true);

    let body_trust = msp_json(&["--registry", REGISTRY, "trust", "verify-body", SKILL_ID]);
    assert_schema(MspSchemaKind::TrustVerifyResult, &body_trust);
    assert_eq!(body_trust["passed"], true);

    let policy = msp_json(&[
        "--registry",
        REGISTRY,
        "trust",
        "evaluate",
        SKILL_ID,
        "--policy",
        POLICY,
    ]);
    assert_schema(MspSchemaKind::TrustPolicyEvaluation, &policy);
    assert_eq!(policy["allowed"], false);
    assert_eq!(policy["action"], "require_review");

    let dependencies = msp_json(&[
        "--registry",
        REGISTRY,
        "trust",
        "evaluate-dependencies",
        SKILL_ID,
        "--policy",
        POLICY,
    ]);
    assert_schema(
        MspSchemaKind::DependencyTrustEvaluationResult,
        &dependencies,
    );
    assert_eq!(dependencies["allowed"], false);
    assert_eq!(dependencies["root_evaluation"]["action"], "require_review");
}

#[test]
fn cli_errors_are_nonzero_and_actionable_for_bad_inputs() {
    let missing_skill = msp(&[
        "--registry",
        REGISTRY,
        "skills",
        "manifest",
        "skill.missing.example.v1",
    ]);
    assert!(!missing_skill.status.success());
    let missing_skill_stderr = String::from_utf8_lossy(&missing_skill.stderr);
    assert!(
        missing_skill_stderr.contains("not found")
            || missing_skill_stderr.contains("No such file")
            || missing_skill_stderr.contains("failed"),
        "unexpected missing skill stderr: {missing_skill_stderr}"
    );

    let bad_registry = msp(&[
        "--registry",
        "../../examples/reports/rust-refactor.report.json",
        "skills",
        "manifest",
        SKILL_ID,
    ]);
    assert!(!bad_registry.status.success());
    let bad_registry_stderr = String::from_utf8_lossy(&bad_registry.stderr);
    assert!(
        bad_registry_stderr.contains("skill not found")
            || bad_registry_stderr.contains("not found"),
        "unexpected bad registry stderr: {bad_registry_stderr}"
    );

    let bad_risk = msp(&[
        "--registry",
        REGISTRY,
        "registry",
        "search",
        "--max-risk",
        "severe",
    ]);
    assert!(!bad_risk.status.success());
    let bad_risk_stderr = String::from_utf8_lossy(&bad_risk.stderr);
    assert!(
        bad_risk_stderr.contains("expected one of: low, medium, high, critical"),
        "unexpected bad risk stderr: {bad_risk_stderr}"
    );
}
