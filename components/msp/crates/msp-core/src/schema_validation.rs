use crate::{MspError, MspResult};
use jsonschema::{Draft, Registry, Resource, Validator};
use once_cell::sync::Lazy;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MspSchemaKind {
    Manifest,
    SkillPack,
    TrustPolicy,
    VerificationContract,
    ExecutionReport,
    Dependency,
    PublicationDraft,
    PublicationReport,
    ProtocolResults,
    RegistrySearchResultArray,
    PackSearchResultArray,
    SkillLoadResult,
    SkillCompatibilityResult,
    SkillVerificationResult,
    TrustVerifyResult,
    DependencyResolutionResult,
    DependencyTrustEvaluationResult,
    PackMemberValidationResult,
    TrustPolicyEvaluation,
}

impl MspSchemaKind {
    fn name(self) -> &'static str {
        match self {
            Self::Manifest => "manifest",
            Self::SkillPack => "skill-pack",
            Self::TrustPolicy => "trust-policy",
            Self::VerificationContract => "verification-contract",
            Self::ExecutionReport => "execution-report",
            Self::Dependency => "dependency",
            Self::PublicationDraft => "publication-draft",
            Self::PublicationReport => "publication-report",
            Self::ProtocolResults => "protocol-results",
            Self::RegistrySearchResultArray => "registry-search-result-array",
            Self::PackSearchResultArray => "pack-search-result-array",
            Self::SkillLoadResult => "skill-load-result",
            Self::SkillCompatibilityResult => "skill-compatibility-result",
            Self::SkillVerificationResult => "skill-verification-result",
            Self::TrustVerifyResult => "trust-verify-result",
            Self::DependencyResolutionResult => "dependency-resolution-result",
            Self::DependencyTrustEvaluationResult => "dependency-trust-evaluation-result",
            Self::PackMemberValidationResult => "pack-member-validation-result",
            Self::TrustPolicyEvaluation => "trust-policy-evaluation",
        }
    }

    fn schema(self) -> &'static Value {
        match self {
            Self::Manifest => &MANIFEST_SCHEMA,
            Self::SkillPack => &SKILL_PACK_SCHEMA,
            Self::TrustPolicy => &TRUST_POLICY_SCHEMA,
            Self::VerificationContract => &VERIFICATION_CONTRACT_SCHEMA,
            Self::ExecutionReport => &EXECUTION_REPORT_SCHEMA,
            Self::Dependency => &DEPENDENCY_SCHEMA,
            Self::PublicationDraft => &PUBLICATION_DRAFT_SCHEMA,
            Self::PublicationReport => &PUBLICATION_REPORT_SCHEMA,
            Self::ProtocolResults => &PROTOCOL_RESULTS_SCHEMA,
            Self::RegistrySearchResultArray => &REGISTRY_SEARCH_RESULT_ARRAY_SCHEMA,
            Self::PackSearchResultArray => &PACK_SEARCH_RESULT_ARRAY_SCHEMA,
            Self::SkillLoadResult => &SKILL_LOAD_RESULT_SCHEMA,
            Self::SkillCompatibilityResult => &SKILL_COMPATIBILITY_RESULT_SCHEMA,
            Self::SkillVerificationResult => &SKILL_VERIFICATION_RESULT_SCHEMA,
            Self::TrustVerifyResult => &TRUST_VERIFY_RESULT_SCHEMA,
            Self::DependencyResolutionResult => &DEPENDENCY_RESOLUTION_RESULT_SCHEMA,
            Self::DependencyTrustEvaluationResult => &DEPENDENCY_TRUST_EVALUATION_RESULT_SCHEMA,
            Self::PackMemberValidationResult => &PACK_MEMBER_VALIDATION_RESULT_SCHEMA,
            Self::TrustPolicyEvaluation => &TRUST_POLICY_EVALUATION_SCHEMA,
        }
    }
}

static MANIFEST_SCHEMA: Lazy<Value> = Lazy::new(|| {
    parse_schema(
        "manifest",
        include_str!("../../../schemas/manifest.schema.json"),
    )
});
static SKILL_PACK_SCHEMA: Lazy<Value> = Lazy::new(|| {
    parse_schema(
        "skill-pack",
        include_str!("../../../schemas/skill-pack.schema.json"),
    )
});
static TRUST_POLICY_SCHEMA: Lazy<Value> = Lazy::new(|| {
    parse_schema(
        "trust-policy",
        include_str!("../../../schemas/trust-policy.schema.json"),
    )
});
static VERIFICATION_CONTRACT_SCHEMA: Lazy<Value> = Lazy::new(|| {
    parse_schema(
        "verification-contract",
        include_str!("../../../schemas/verification-contract.schema.json"),
    )
});
static EXECUTION_REPORT_SCHEMA: Lazy<Value> = Lazy::new(|| {
    parse_schema(
        "execution-report",
        include_str!("../../../schemas/execution-report.schema.json"),
    )
});
static DEPENDENCY_SCHEMA: Lazy<Value> = Lazy::new(|| {
    parse_schema(
        "dependency",
        include_str!("../../../schemas/dependency.schema.json"),
    )
});
static PUBLICATION_DRAFT_SCHEMA: Lazy<Value> = Lazy::new(|| {
    parse_schema(
        "publication-draft",
        include_str!("../../../schemas/publication-draft.schema.json"),
    )
});
static PUBLICATION_REPORT_SCHEMA: Lazy<Value> = Lazy::new(|| {
    parse_schema(
        "publication-report",
        include_str!("../../../schemas/publication-report.schema.json"),
    )
});
static PROTOCOL_RESULTS_SCHEMA: Lazy<Value> = Lazy::new(|| {
    parse_schema(
        "protocol-results",
        include_str!("../../../schemas/protocol-results.schema.json"),
    )
});

static REGISTRY_SEARCH_RESULT_ARRAY_SCHEMA: Lazy<Value> =
    Lazy::new(|| protocol_result_def_schema("registry_search_result_array"));
static PACK_SEARCH_RESULT_ARRAY_SCHEMA: Lazy<Value> =
    Lazy::new(|| protocol_result_def_schema("pack_search_result_array"));
static SKILL_LOAD_RESULT_SCHEMA: Lazy<Value> =
    Lazy::new(|| protocol_result_def_schema("skill_load_result"));
static SKILL_COMPATIBILITY_RESULT_SCHEMA: Lazy<Value> =
    Lazy::new(|| protocol_result_def_schema("skill_compatibility_result"));
static SKILL_VERIFICATION_RESULT_SCHEMA: Lazy<Value> =
    Lazy::new(|| protocol_result_def_schema("skill_verification_result"));
static TRUST_VERIFY_RESULT_SCHEMA: Lazy<Value> =
    Lazy::new(|| protocol_result_def_schema("trust_verify_result"));
static DEPENDENCY_RESOLUTION_RESULT_SCHEMA: Lazy<Value> =
    Lazy::new(|| protocol_result_def_schema("dependency_resolution_result"));
static DEPENDENCY_TRUST_EVALUATION_RESULT_SCHEMA: Lazy<Value> =
    Lazy::new(|| protocol_result_def_schema("dependency_trust_evaluation_result"));
static PACK_MEMBER_VALIDATION_RESULT_SCHEMA: Lazy<Value> =
    Lazy::new(|| protocol_result_def_schema("pack_member_validation_result"));
static TRUST_POLICY_EVALUATION_SCHEMA: Lazy<Value> =
    Lazy::new(|| protocol_result_def_schema("trust_policy_evaluation"));

static SCHEMA_REGISTRY: Lazy<Registry<'static>> = Lazy::new(|| {
    Registry::new()
        .add(
            "msp://schemas/manifest.schema.json",
            Resource::from_contents(MANIFEST_SCHEMA.clone()),
        )
        .expect("valid manifest schema URI")
        .add(
            "msp://schemas/skill-pack.schema.json",
            Resource::from_contents(SKILL_PACK_SCHEMA.clone()),
        )
        .expect("valid skill-pack schema URI")
        .add(
            "msp://schemas/trust-policy.schema.json",
            Resource::from_contents(TRUST_POLICY_SCHEMA.clone()),
        )
        .expect("valid trust-policy schema URI")
        .add(
            "msp://schemas/verification-contract.schema.json",
            Resource::from_contents(VERIFICATION_CONTRACT_SCHEMA.clone()),
        )
        .expect("valid verification-contract schema URI")
        .add(
            "msp://schemas/execution-report.schema.json",
            Resource::from_contents(EXECUTION_REPORT_SCHEMA.clone()),
        )
        .expect("valid execution-report schema URI")
        .add(
            "msp://schemas/dependency.schema.json",
            Resource::from_contents(DEPENDENCY_SCHEMA.clone()),
        )
        .expect("valid dependency schema URI")
        .add(
            "msp://schemas/publication-draft.schema.json",
            Resource::from_contents(PUBLICATION_DRAFT_SCHEMA.clone()),
        )
        .expect("valid publication-draft schema URI")
        .add(
            "msp://schemas/publication-report.schema.json",
            Resource::from_contents(PUBLICATION_REPORT_SCHEMA.clone()),
        )
        .expect("valid publication-report schema URI")
        .add(
            "msp://schemas/protocol-results.schema.json",
            Resource::from_contents(PROTOCOL_RESULTS_SCHEMA.clone()),
        )
        .expect("valid protocol-results schema URI")
        .add(
            "dependency.schema.json",
            Resource::from_contents(DEPENDENCY_SCHEMA.clone()),
        )
        .expect("valid relative dependency schema URI")
        .add(
            "manifest.schema.json",
            Resource::from_contents(MANIFEST_SCHEMA.clone()),
        )
        .expect("valid relative manifest schema URI")
        .add(
            "skill-pack.schema.json",
            Resource::from_contents(SKILL_PACK_SCHEMA.clone()),
        )
        .expect("valid relative skill-pack schema URI")
        .add(
            "verification-contract.schema.json",
            Resource::from_contents(VERIFICATION_CONTRACT_SCHEMA.clone()),
        )
        .expect("valid relative verification-contract schema URI")
        .add(
            "publication-report.schema.json",
            Resource::from_contents(PUBLICATION_REPORT_SCHEMA.clone()),
        )
        .expect("valid relative publication-report schema URI")
        .add(
            "protocol-results.schema.json",
            Resource::from_contents(PROTOCOL_RESULTS_SCHEMA.clone()),
        )
        .expect("valid relative protocol-results schema URI")
        .prepare()
        .expect("embedded MSP schema registry should prepare")
});

static MANIFEST_VALIDATOR: Lazy<Validator> = Lazy::new(|| build_validator(MspSchemaKind::Manifest));
static SKILL_PACK_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::SkillPack));
static TRUST_POLICY_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::TrustPolicy));
static VERIFICATION_CONTRACT_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::VerificationContract));
static EXECUTION_REPORT_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::ExecutionReport));
static DEPENDENCY_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::Dependency));
static PUBLICATION_DRAFT_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::PublicationDraft));
static PUBLICATION_REPORT_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::PublicationReport));
static PROTOCOL_RESULTS_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::ProtocolResults));
static REGISTRY_SEARCH_RESULT_ARRAY_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::RegistrySearchResultArray));
static PACK_SEARCH_RESULT_ARRAY_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::PackSearchResultArray));
static SKILL_LOAD_RESULT_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::SkillLoadResult));
static SKILL_COMPATIBILITY_RESULT_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::SkillCompatibilityResult));
static SKILL_VERIFICATION_RESULT_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::SkillVerificationResult));
static TRUST_VERIFY_RESULT_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::TrustVerifyResult));
static DEPENDENCY_RESOLUTION_RESULT_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::DependencyResolutionResult));
static DEPENDENCY_TRUST_EVALUATION_RESULT_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::DependencyTrustEvaluationResult));
static PACK_MEMBER_VALIDATION_RESULT_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::PackMemberValidationResult));
static TRUST_POLICY_EVALUATION_VALIDATOR: Lazy<Validator> =
    Lazy::new(|| build_validator(MspSchemaKind::TrustPolicyEvaluation));

pub fn validate_json_schema(kind: MspSchemaKind, instance: &Value) -> MspResult<()> {
    let validator = validator(kind);
    let errors: Vec<_> = validator
        .iter_errors(instance)
        .map(|error| {
            let path = error.instance_path().to_string();
            if path.is_empty() {
                error.to_string()
            } else {
                format!("{path}: {error}")
            }
        })
        .collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(MspError::SchemaValidation {
            schema: kind.name().to_string(),
            errors,
        })
    }
}

pub fn parse_and_validate_json_schema(kind: MspSchemaKind, content: &str) -> MspResult<Value> {
    let value: Value = serde_json::from_str(content)?;
    validate_json_schema(kind, &value)?;
    Ok(value)
}

fn validator(kind: MspSchemaKind) -> &'static Validator {
    match kind {
        MspSchemaKind::Manifest => &MANIFEST_VALIDATOR,
        MspSchemaKind::SkillPack => &SKILL_PACK_VALIDATOR,
        MspSchemaKind::TrustPolicy => &TRUST_POLICY_VALIDATOR,
        MspSchemaKind::VerificationContract => &VERIFICATION_CONTRACT_VALIDATOR,
        MspSchemaKind::ExecutionReport => &EXECUTION_REPORT_VALIDATOR,
        MspSchemaKind::Dependency => &DEPENDENCY_VALIDATOR,
        MspSchemaKind::PublicationDraft => &PUBLICATION_DRAFT_VALIDATOR,
        MspSchemaKind::PublicationReport => &PUBLICATION_REPORT_VALIDATOR,
        MspSchemaKind::ProtocolResults => &PROTOCOL_RESULTS_VALIDATOR,
        MspSchemaKind::RegistrySearchResultArray => &REGISTRY_SEARCH_RESULT_ARRAY_VALIDATOR,
        MspSchemaKind::PackSearchResultArray => &PACK_SEARCH_RESULT_ARRAY_VALIDATOR,
        MspSchemaKind::SkillLoadResult => &SKILL_LOAD_RESULT_VALIDATOR,
        MspSchemaKind::SkillCompatibilityResult => &SKILL_COMPATIBILITY_RESULT_VALIDATOR,
        MspSchemaKind::SkillVerificationResult => &SKILL_VERIFICATION_RESULT_VALIDATOR,
        MspSchemaKind::TrustVerifyResult => &TRUST_VERIFY_RESULT_VALIDATOR,
        MspSchemaKind::DependencyResolutionResult => &DEPENDENCY_RESOLUTION_RESULT_VALIDATOR,
        MspSchemaKind::DependencyTrustEvaluationResult => {
            &DEPENDENCY_TRUST_EVALUATION_RESULT_VALIDATOR
        }
        MspSchemaKind::PackMemberValidationResult => &PACK_MEMBER_VALIDATION_RESULT_VALIDATOR,
        MspSchemaKind::TrustPolicyEvaluation => &TRUST_POLICY_EVALUATION_VALIDATOR,
    }
}

fn build_validator(kind: MspSchemaKind) -> Validator {
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_registry(&SCHEMA_REGISTRY)
        .should_validate_formats(true)
        .build(kind.schema())
        .unwrap_or_else(|error| panic!("embedded {} schema should compile: {error}", kind.name()))
}

fn parse_schema(name: &str, content: &str) -> Value {
    serde_json::from_str(content)
        .unwrap_or_else(|error| panic!("embedded {name} schema should parse: {error}"))
}

fn protocol_result_def_schema(def_name: &str) -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": format!("msp://schemas/protocol-results/{def_name}.schema.json"),
        "$ref": format!("msp://schemas/protocol-results.schema.json#/$defs/{def_name}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_reference_manifest_schema() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../examples/registry/software_engineering/rust/refactor_module/skill.manifest.json"
        ))
        .unwrap();
        validate_json_schema(MspSchemaKind::Manifest, &value).unwrap();
    }

    #[test]
    fn rejects_additional_manifest_property() {
        let value = json!({
            "msp_version": "0.1.0",
            "manifest_version": "0.1.0",
            "kind": "SkillManifest",
            "id": "skill.test.example.v1",
            "name": "Example",
            "version": "0.1.0",
            "category": "test/example",
            "summary": "Example skill",
            "formats": ["markdown"],
            "primary_format": "markdown",
            "activation": {"task_patterns": ["test"]},
            "requirements": {},
            "schemas": {"input": "msp://schemas/input.v1", "output": "msp://schemas/output.v1"},
            "verification": {"required_checks": ["manual_review"]},
            "trust": {
                "hash": "sha256:abcd",
                "signed": false,
                "issuer": "test",
                "risk_level": "low"
            },
            "artifacts": {
                "body": {"uri": "skill.md", "media_type": "text/markdown", "hash": "sha256:abcd"}
            },
            "unexpected": true
        });
        let error = validate_json_schema(MspSchemaKind::Manifest, &value).unwrap_err();
        assert!(error.to_string().contains("unexpected"));
    }

    #[test]
    fn validates_reference_trust_policy_schema() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../examples/policies/local-reference.trust-policy.json"
        ))
        .unwrap();
        validate_json_schema(MspSchemaKind::TrustPolicy, &value).unwrap();
    }

    #[test]
    fn validates_publication_report_schema() {
        let unsigned = json!({
            "registry": "/tmp/msp-registry",
            "pack_id": "pack.sample.v1",
            "skills_published": ["skill.sample.example.v1"],
            "files_written": [
                "/tmp/msp-registry/skills/skill.sample.example.v1/skill.md",
                "/tmp/msp-registry/skills/skill.sample.example.v1/skill.manifest.json",
                "/tmp/msp-registry/packs/pack.sample.v1/pack.manifest.json"
            ],
            "warnings": [],
            "signed": false
        });
        validate_json_schema(MspSchemaKind::PublicationReport, &unsigned).unwrap();

        let signed = json!({
            "registry": "/tmp/msp-registry",
            "pack_id": "pack.sample.v1",
            "skills_published": ["skill.sample.example.v1"],
            "files_written": [
                "/tmp/msp-registry/skills/skill.sample.example.v1/skill.md",
                "/tmp/msp-registry/skills/skill.sample.example.v1/skill.manifest.json",
                "/tmp/msp-registry/packs/pack.sample.v1/pack.manifest.json"
            ],
            "warnings": [],
            "signed": true,
            "public_key_ref": "ed25519:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
            "public_key_sha256": "sha256:000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
        });
        validate_json_schema(MspSchemaKind::PublicationReport, &signed).unwrap();
    }

    #[test]
    fn rejects_unsigned_publication_report_with_key_refs() {
        let value = json!({
            "registry": "/tmp/msp-registry",
            "pack_id": "pack.sample.v1",
            "skills_published": ["skill.sample.example.v1"],
            "files_written": ["/tmp/msp-registry/skills/skill.sample.example.v1/skill.md"],
            "warnings": [],
            "signed": false,
            "public_key_ref": "ed25519:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="
        });
        let error = validate_json_schema(MspSchemaKind::PublicationReport, &value).unwrap_err();
        assert!(error.to_string().contains("public_key_ref"));
    }

    #[test]
    fn validates_protocol_result_schema_defs_compile() {
        let examples = [
            (MspSchemaKind::RegistrySearchResultArray, json!([])),
            (MspSchemaKind::PackSearchResultArray, json!([])),
            (
                MspSchemaKind::TrustPolicyEvaluation,
                json!({
                    "skill_id": "skill.test.example.v1",
                    "action": "allow",
                    "allowed": true,
                    "reasons": [],
                    "warnings": [],
                    "matched_rules": []
                }),
            ),
        ];
        for (kind, value) in examples {
            validate_json_schema(kind, &value).unwrap();
        }
    }

    #[test]
    fn validates_reference_protocol_result_fixtures() {
        let search_results = json!([
            {
                "id": "skill.rust.refactor.module.v1",
                "name": "Rust Module Refactor",
                "version": "0.1.0",
                "category": "software_engineering/rust/refactoring",
                "summary": "Guides safe refactoring of Rust modules using small edits and verification loops.",
                "risk_level": "medium",
                "score": 59,
                "required_tools": ["read_file", "write_file"]
            }
        ]);
        validate_json_schema(MspSchemaKind::RegistrySearchResultArray, &search_results).unwrap();

        let pack_results = json!([
            {
                "id": "pack.rust.engineering.v1",
                "name": "Rust Engineering Pack",
                "version": "0.1.0",
                "category": "software_engineering/rust",
                "summary": "Reference Rust engineering MSP skill pack.",
                "risk_level": "medium",
                "score": 63,
                "skill_count": 1,
                "required_skill_count": 1,
                "issuer": "msp-reference.local"
            }
        ]);
        validate_json_schema(MspSchemaKind::PackSearchResultArray, &pack_results).unwrap();

        let manifest: Value = serde_json::from_str(include_str!(
            "../../../examples/registry/software_engineering/rust/refactor_module/skill.manifest.json"
        ))
        .unwrap();
        let contract: Value = serde_json::from_str(include_str!(
            "../../../examples/registry/software_engineering/rust/refactor_module/verify.json"
        ))
        .unwrap();
        let load_result = json!({
            "manifest": manifest,
            "body": "# Rust Module Refactor\n",
            "body_hash_valid": true,
            "verification_contract": contract,
            "dependency_ids": []
        });
        validate_json_schema(MspSchemaKind::SkillLoadResult, &load_result).unwrap();

        let compatibility_result = json!({
            "skill_id": "skill.rust.refactor.module.v1",
            "compatible": true,
            "score": 1.0,
            "msp_version_compatible": true,
            "manifest_version_compatible": true,
            "format_compatible": true,
            "runtime_capabilities_compatible": true,
            "model_capabilities_compatible": true,
            "tools_compatible": true,
            "permissions_compatible": true,
            "context_window_compatible": true,
            "platform_compatible": true,
            "known_runtime": true,
            "issues": [],
            "warnings": ["optional tool optional_lint is not available"]
        });
        validate_json_schema(
            MspSchemaKind::SkillCompatibilityResult,
            &compatibility_result,
        )
        .unwrap();

        let verification_result = json!({
            "skill_id": "skill.rust.refactor.module.v1",
            "passed": true,
            "score": 1.0,
            "confidence": 0.9,
            "failed_checks": [],
            "warnings": [],
            "check_results": [
                {
                    "id": "tests_pass",
                    "type": "command",
                    "required": true,
                    "status": "passed",
                    "passed": true,
                    "score_earned": 1.0,
                    "score_possible": 1.0,
                    "evidence_keys": ["cargo_test"],
                    "missing_evidence": [],
                    "reasons": [],
                    "warnings": []
                }
            ],
            "evidence_results": [
                {
                    "key": "cargo_test",
                    "type": "command",
                    "required": true,
                    "present": true,
                    "reasons": [],
                    "warnings": []
                }
            ],
            "criteria": {
                "required_checks_passed": true,
                "minimum_score": 1.0,
                "score_passed": true,
                "minimum_confidence": 0.8,
                "confidence": 0.9,
                "confidence_passed": true,
                "allowed_warnings": 0,
                "warning_count": 0,
                "warnings_passed": true
            },
            "failures": []
        });
        validate_json_schema(MspSchemaKind::SkillVerificationResult, &verification_result).unwrap();

        let trust_verify_result = json!({
            "artifact": "skill.md",
            "expected_hash": "sha256:abcd",
            "actual_hash": "sha256:abcd",
            "hash_passed": true,
            "passed": true
        });
        validate_json_schema(MspSchemaKind::TrustVerifyResult, &trust_verify_result).unwrap();

        let dependency_resolution = json!({
            "root": "skill.rust.refactor.module.v1",
            "nodes": [],
            "missing": [],
            "cycles": []
        });
        validate_json_schema(
            MspSchemaKind::DependencyResolutionResult,
            &dependency_resolution,
        )
        .unwrap();

        let trust_policy_evaluation = json!({
            "skill_id": "skill.rust.refactor.module.v1",
            "allowed": true,
            "action": "allow",
            "reasons": [],
            "warnings": [],
            "matched_rules": []
        });
        validate_json_schema(
            MspSchemaKind::TrustPolicyEvaluation,
            &trust_policy_evaluation,
        )
        .unwrap();

        let dependency_trust = json!({
            "root": "skill.rust.refactor.module.v1",
            "allowed": true,
            "root_evaluation": trust_policy_evaluation,
            "dependencies": [],
            "missing": [],
            "cycles": [],
            "reasons": [],
            "warnings": []
        });
        validate_json_schema(
            MspSchemaKind::DependencyTrustEvaluationResult,
            &dependency_trust,
        )
        .unwrap();

        let pack_member_validation = json!({
            "pack_id": "pack.rust.engineering.v1",
            "valid": true,
            "members": [
                {
                    "id": "skill.rust.refactor.module.v1",
                    "expected_version": "0.1.0",
                    "manifest_uri": "software_engineering/rust/refactor_module/skill.manifest.json",
                    "required": true,
                    "exists": true,
                    "indexed": true,
                    "id_matches": true,
                    "version_matches": true,
                    "valid": true,
                    "reasons": [],
                    "warnings": []
                }
            ],
            "missing": [],
            "duplicate_ids": [],
            "reasons": [],
            "warnings": []
        });
        validate_json_schema(
            MspSchemaKind::PackMemberValidationResult,
            &pack_member_validation,
        )
        .unwrap();
    }
}
