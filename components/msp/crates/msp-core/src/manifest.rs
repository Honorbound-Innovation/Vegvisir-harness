use crate::{HashDigest, MspError, MspResult, MspSchemaKind, parse_and_validate_json_schema};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub uri: String,
    pub media_type: String,
    pub hash: HashDigest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillArtifacts {
    pub body: ArtifactRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_contract: Option<ArtifactRef>,
    #[serde(default)]
    pub examples: Vec<ArtifactRef>,
    #[serde(default)]
    pub schemas: Vec<ArtifactRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activation {
    pub task_patterns: Vec<String>,
    #[serde(default)]
    pub negative_patterns: Vec<String>,
    #[serde(default)]
    pub required_context: Vec<String>,
    #[serde(default)]
    pub optional_context: Vec<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRequirement {
    pub name: String,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Requirements {
    #[serde(default)]
    pub model_capabilities: Vec<String>,
    #[serde(default)]
    pub runtime_capabilities: Vec<String>,
    #[serde(default)]
    pub tools: Vec<ToolRequirement>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_context_window: Option<u64>,
    #[serde(default)]
    pub supported_platforms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaRefs {
    pub input: String,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
    pub required_checks: Vec<String>,
    #[serde(default)]
    pub optional_checks: Vec<String>,
    #[serde(default)]
    pub minimum_evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    #[default]
    Unreviewed,
    SelfReviewed,
    PeerReviewed,
    SecurityReviewed,
    Deprecated,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustMetadata {
    pub hash: HashDigest,
    pub signed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub issuer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default)]
    pub review_status: ReviewStatus,
    pub risk_level: RiskLevel,
    #[serde(default)]
    pub forbidden_behaviors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyType {
    Skill,
    SkillPack,
    Schema,
    VerificationContract,
    TrustAnchor,
    FormatAdapter,
    RuntimeCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStrategy {
    Exact,
    #[default]
    Compatible,
    LatestPatch,
    LatestMinor,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyTrust {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_hash: Option<HashDigest>,
    #[serde(default)]
    pub must_be_signed: bool,
    #[serde(default)]
    pub allowed_registries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyResolution {
    #[serde(default)]
    pub strategy: ResolutionStrategy,
    #[serde(default)]
    pub allow_prerelease: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDependency {
    #[serde(rename = "type")]
    pub dependency_type: DependencyType,
    pub id: String,
    pub requirement: String,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust: Option<DependencyTrust>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<DependencyResolution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Provenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    #[serde(default)]
    pub source_documents: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Compatibility {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_msp_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_msp_version: Option<String>,
    #[serde(default)]
    pub known_runtimes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deprecation {
    pub deprecated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sunset_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillManifest {
    pub msp_version: String,
    pub manifest_version: String,
    pub kind: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub category: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub formats: Vec<String>,
    pub primary_format: String,
    pub activation: Activation,
    pub requirements: Requirements,
    pub schemas: SchemaRefs,
    pub verification: VerificationSummary,
    pub trust: TrustMetadata,
    #[serde(default)]
    pub dependencies: Vec<SkillDependency>,
    pub artifacts: SkillArtifacts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<Compatibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecation: Option<Deprecation>,
    #[serde(default)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

impl SkillManifest {
    pub fn from_path(path: impl AsRef<Path>) -> MspResult<Self> {
        let content = std::fs::read_to_string(path)?;
        let value = parse_and_validate_json_schema(MspSchemaKind::Manifest, &content)?;
        let manifest: Self = serde_json::from_value(value)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> MspResult<()> {
        if self.kind != "SkillManifest" {
            return Err(MspError::ManifestValidation(format!(
                "expected kind SkillManifest, got {}",
                self.kind
            )));
        }
        if self.id.trim().is_empty() || self.name.trim().is_empty() {
            return Err(MspError::ManifestValidation(
                "skill id and name must be non-empty".to_string(),
            ));
        }
        if !self
            .formats
            .iter()
            .any(|format| format == &self.primary_format)
        {
            return Err(MspError::ManifestValidation(format!(
                "primary_format {} is not listed in formats",
                self.primary_format
            )));
        }
        if self.activation.task_patterns.is_empty() {
            return Err(MspError::ManifestValidation(
                "activation.task_patterns must not be empty".to_string(),
            ));
        }
        if self.verification.required_checks.is_empty() {
            return Err(MspError::ManifestValidation(
                "verification.required_checks must not be empty".to_string(),
            ));
        }
        if self.trust.signed && self.trust.signature.is_none() {
            return Err(MspError::ManifestValidation(
                "trust.signature is required when trust.signed is true".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackSkillRef {
    pub id: String,
    pub version: String,
    pub manifest_uri: String,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillPackManifest {
    pub msp_version: String,
    pub manifest_version: String,
    pub kind: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub category: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub skills: Vec<PackSkillRef>,
    #[serde(default)]
    pub dependencies: Vec<SkillDependency>,
    pub trust: TrustMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecation: Option<Deprecation>,
    #[serde(default)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

impl SkillPackManifest {
    pub fn from_path(path: impl AsRef<Path>) -> MspResult<Self> {
        let content = std::fs::read_to_string(path)?;
        let value = parse_and_validate_json_schema(MspSchemaKind::SkillPack, &content)?;
        let pack: Self = serde_json::from_value(value)?;
        pack.validate()?;
        Ok(pack)
    }

    pub fn validate(&self) -> MspResult<()> {
        if self.kind != "SkillPackManifest" {
            return Err(MspError::ManifestValidation(format!(
                "expected kind SkillPackManifest, got {}",
                self.kind
            )));
        }
        if self.skills.is_empty() {
            return Err(MspError::ManifestValidation(
                "skill pack must contain at least one skill".to_string(),
            ));
        }
        if self.trust.signed && self.trust.signature.is_none() {
            return Err(MspError::ManifestValidation(
                "trust.signature is required when trust.signed is true".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillLoadResult {
    pub manifest: SkillManifest,
    pub body: String,
    pub body_hash_valid: bool,
    #[serde(default)]
    pub verification_contract: Option<serde_json::Value>,
    #[serde(default)]
    pub dependency_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_manifest() -> SkillManifest {
        SkillManifest {
            msp_version: "0.1.0".to_string(),
            manifest_version: "0.1.0".to_string(),
            kind: "SkillManifest".to_string(),
            id: "skill.test.example.v1".to_string(),
            name: "Example".to_string(),
            version: "0.1.0".to_string(),
            category: "test/example".to_string(),
            summary: "Example skill".to_string(),
            description: None,
            keywords: vec![],
            formats: vec!["markdown".to_string()],
            primary_format: "markdown".to_string(),
            activation: Activation {
                task_patterns: vec!["test".to_string()],
                negative_patterns: vec![],
                required_context: vec![],
                optional_context: vec![],
                domains: vec![],
                languages: vec![],
            },
            requirements: Requirements::default(),
            schemas: SchemaRefs {
                input: "msp://schemas/input.v1".to_string(),
                output: "msp://schemas/output.v1".to_string(),
                context: None,
            },
            verification: VerificationSummary {
                contract: None,
                required_checks: vec!["manual_review".to_string()],
                optional_checks: vec![],
                minimum_evidence: vec![],
            },
            trust: TrustMetadata {
                hash: HashDigest::parse("sha256:abcd").unwrap(),
                signed: false,
                signature: None,
                issuer: "test".to_string(),
                license: None,
                review_status: ReviewStatus::Unreviewed,
                risk_level: RiskLevel::Low,
                forbidden_behaviors: vec![],
                sandbox_profile: None,
            },
            dependencies: vec![],
            artifacts: SkillArtifacts {
                body: ArtifactRef {
                    uri: "skill.md".to_string(),
                    media_type: "text/markdown".to_string(),
                    hash: HashDigest::parse("sha256:abcd").unwrap(),
                    size_bytes: None,
                },
                verification_contract: None,
                examples: vec![],
                schemas: vec![],
            },
            provenance: None,
            compatibility: None,
            deprecation: None,
            extensions: BTreeMap::new(),
        }
    }

    #[test]
    fn validates_minimal_manifest() {
        minimal_manifest().validate().unwrap();
    }

    #[test]
    fn rejects_unlisted_primary_format() {
        let mut manifest = minimal_manifest();
        manifest.primary_format = "lsl".to_string();
        assert!(manifest.validate().is_err());
    }
}
