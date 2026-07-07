use crate::{
    DependencyType, RiskLevel, SignatureVerifyResult, SkillManifest, SkillPackManifest,
    TrustAction, TrustPolicyEvaluation,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MspInfo {
    pub name: String,
    pub version: String,
    pub msp_version: String,
    pub methods: Vec<String>,
}

impl Default for MspInfo {
    fn default() -> Self {
        Self {
            name: "msp-reference".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            msp_version: "0.1.0".to_string(),
            methods: core_methods(),
        }
    }
}

pub fn core_methods() -> Vec<String> {
    [
        "msp.info",
        "registry.search",
        "skills.discover",
        "skills.get_manifest",
        "skills.load",
        "skills.resolve_dependencies",
        "skills.verify_result",
        "skills.check_compatibility",
        "packs.discover",
        "packs.get_manifest",
        "packs.load",
        "packs.verify_trust",
        "packs.evaluate_trust",
        "packs.validate_members",
        "packs.evaluate_dependencies",
        "trust.verify",
        "trust.evaluate",
        "trust.evaluate_dependencies",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillSearchQuery {
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub available_tools: Vec<String>,
    #[serde(default)]
    pub max_risk: Option<RiskLevel>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PackSearchQuery {
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub max_risk: Option<RiskLevel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistrySearchResult {
    pub id: String,
    pub name: String,
    pub version: String,
    pub category: String,
    pub summary: String,
    pub risk_level: RiskLevel,
    pub score: u32,
    pub required_tools: Vec<String>,
}

impl RegistrySearchResult {
    pub fn from_manifest(manifest: &SkillManifest, score: u32) -> Self {
        let required_tools = manifest
            .requirements
            .tools
            .iter()
            .filter(|tool| tool.required)
            .map(|tool| tool.name.clone())
            .collect();
        Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            category: manifest.category.clone(),
            summary: manifest.summary.clone(),
            risk_level: manifest.trust.risk_level,
            score,
            required_tools,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackSearchResult {
    pub id: String,
    pub name: String,
    pub version: String,
    pub category: String,
    pub summary: String,
    pub risk_level: RiskLevel,
    pub score: u32,
    pub skill_count: usize,
    pub required_skill_count: usize,
    pub issuer: String,
}

impl PackSearchResult {
    pub fn from_manifest(manifest: &SkillPackManifest, score: u32) -> Self {
        Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            category: manifest.category.clone(),
            summary: manifest.summary.clone(),
            risk_level: manifest.trust.risk_level,
            score,
            skill_count: manifest.skills.len(),
            required_skill_count: manifest
                .skills
                .iter()
                .filter(|skill| skill.required)
                .count(),
            issuer: manifest.trust.issuer.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DependencyNode {
    pub id: String,
    pub requirement: String,
    pub required: bool,
    pub resolved: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DependencyResolutionResult {
    pub root: String,
    pub nodes: Vec<DependencyNode>,
    pub missing: Vec<String>,
    pub cycles: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DependencyTrustEvaluationNode {
    pub parent: String,
    pub depth: u32,
    pub id: String,
    pub dependency_type: DependencyType,
    pub requirement: String,
    pub required: bool,
    pub resolved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<TrustAction>,
    pub allowed: bool,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub matched_rules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DependencyTrustEvaluationResult {
    pub root: String,
    pub allowed: bool,
    pub root_evaluation: TrustPolicyEvaluation,
    pub dependencies: Vec<DependencyTrustEvaluationNode>,
    pub missing: Vec<String>,
    pub cycles: Vec<Vec<String>>,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackMemberValidationNode {
    pub id: String,
    pub expected_version: String,
    pub manifest_uri: String,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
    pub exists: bool,
    pub indexed: bool,
    pub id_matches: bool,
    pub version_matches: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_evaluation: Option<TrustPolicyEvaluation>,
    pub valid: bool,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackMemberValidationResult {
    pub pack_id: String,
    pub valid: bool,
    pub members: Vec<PackMemberValidationNode>,
    pub missing: Vec<String>,
    pub duplicate_ids: Vec<String>,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCompatibilityQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msp_version: Option<String>,
    #[serde(default)]
    pub supported_manifest_versions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_version: Option<String>,
    #[serde(default)]
    pub supported_formats: Vec<String>,
    #[serde(default)]
    pub runtime_capabilities: Vec<String>,
    #[serde(default)]
    pub model_capabilities: Vec<String>,
    #[serde(default)]
    pub available_tools: Vec<String>,
    #[serde(default)]
    pub tool_versions: BTreeMap<String, String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilitySeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityIssue {
    pub code: String,
    pub dimension: String,
    pub severity: CompatibilitySeverity,
    pub required: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillCompatibilityResult {
    pub skill_id: String,
    pub compatible: bool,
    pub score: f64,
    pub msp_version_compatible: bool,
    pub manifest_version_compatible: bool,
    pub format_compatible: bool,
    pub runtime_capabilities_compatible: bool,
    pub model_capabilities_compatible: bool,
    pub tools_compatible: bool,
    pub permissions_compatible: bool,
    pub context_window_compatible: bool,
    pub platform_compatible: bool,
    pub known_runtime: bool,
    pub issues: Vec<CompatibilityIssue>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrustVerifyResult {
    pub artifact: String,
    pub expected_hash: String,
    pub actual_hash: String,
    pub hash_passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<SignatureVerifyResult>,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillVerificationResult {
    pub skill_id: String,
    pub passed: bool,
    pub score: f64,
    pub confidence: f64,
    pub failed_checks: Vec<String>,
    pub warnings: Vec<String>,
    pub check_results: Vec<VerificationCheckResult>,
    pub evidence_results: Vec<VerificationEvidenceResult>,
    pub criteria: VerificationCriteriaResult,
    pub failures: Vec<VerificationFailure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationCheckResult {
    pub id: String,
    #[serde(rename = "type")]
    pub check_type: String,
    pub required: bool,
    pub status: String,
    pub passed: bool,
    pub score_earned: f64,
    pub score_possible: f64,
    pub evidence_keys: Vec<String>,
    pub missing_evidence: Vec<String>,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationEvidenceResult {
    pub key: String,
    #[serde(rename = "type")]
    pub evidence_type: String,
    pub required: bool,
    pub present: bool,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationCriteriaResult {
    pub required_checks_passed: bool,
    pub minimum_score: f64,
    pub score_passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_confidence: Option<f64>,
    pub confidence: f64,
    pub confidence_passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_warnings: Option<u64>,
    pub warning_count: u64,
    pub warnings_passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationFailure {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_id: Option<String>,
}
