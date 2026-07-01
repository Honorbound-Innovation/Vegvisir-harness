//! Native Vegvisir client component for the Model Skill Protocol (MSP).
//!
//! This crate adapts the local-first MSP reference implementation into a small,
//! host-friendly API that Vegvisir can expose as tools and CLI commands without
//! going through MCP.  The client is intentionally filesystem/local-registry
//! first: callers pass a registry root and receive strongly typed JSON-friendly
//! results that can be embedded in model observations.

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

pub mod cli;
pub use cli::{run_cli, run_cli_from};

use anyhow::Context;
use msp_core::{
    MspInfo, PackSearchQuery, RegistrySearchResult, RiskLevel, RuntimeCompatibilityQuery,
    SkillLoadResult, SkillManifest, SkillPackManifest, SkillSearchQuery, SkillTrustPolicy,
    TrustPolicyEvaluation, TrustVerifyResult, core_methods,
};
use msp_publisher::{PublicationDeprecation, PublishOptions, PublishReport};
use msp_registry::LocalRegistry;
use serde::{Deserialize, Serialize};

pub use msp_core;
pub use msp_publisher;
pub use msp_registry;

/// Conservative result cap for user/model-controlled searches.
pub const DEFAULT_SEARCH_LIMIT: usize = 10;
pub const MAX_SEARCH_LIMIT: usize = 100;

/// Default user-global MSP registry root used by Vegvisir when no explicit registry is supplied.
pub fn default_registry_root() -> PathBuf {
    if let Some(path) = std::env::var_os("VEGVISIR_MSP_REGISTRY").filter(|value| !value.is_empty())
    {
        return PathBuf::from(path);
    }
    default_vegvisir_data_root().join("msp").join("registry")
}

fn default_vegvisir_data_root() -> PathBuf {
    if let Some(path) = std::env::var_os("VEGVISIR_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(path).join("vegvisir");
    }
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("vegvisir");
    }
    PathBuf::from(".vegvisir")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub component_version: String,
    pub msp: MspInfo,
}

impl Default for ClientInfo {
    fn default() -> Self {
        Self {
            name: "vegvisir-msp-client".to_string(),
            component_version: env!("CARGO_PKG_VERSION").to_string(),
            msp: MspInfo {
                name: "msp-reference-local-registry".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                msp_version: "0.1.0".to_string(),
                methods: core_methods(),
            },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub available_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_risk: Option<RiskLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySummary {
    pub root: String,
    pub skill_count: usize,
    pub pack_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub registry: RegistrySummary,
    pub query: SearchRequest,
    pub results: Vec<RegistrySearchResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadMode {
    Card,
    Body,
    Extended,
    Raw,
}

impl Default for LoadMode {
    fn default() -> Self {
        Self::Body
    }
}

impl FromStr for LoadMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "card" => Ok(Self::Card),
            "body" => Ok(Self::Body),
            "extended" => Ok(Self::Extended),
            "raw" => Ok(Self::Raw),
            other => anyhow::bail!(
                "unknown MSP load mode `{other}`; expected card, body, extended, or raw"
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedSkill {
    pub id: String,
    pub mode: LoadMode,
    pub content: String,
    pub raw: SkillLoadResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityRequest {
    pub skill_id: String,
    #[serde(default)]
    pub query: RuntimeCompatibilityQuery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustEvaluationRequest {
    pub id: String,
    pub policy: SkillTrustPolicy,
    #[serde(default)]
    pub dependency_graph: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportSkillerBundleRequest {
    pub bundle: PathBuf,
    pub issuer: String,
    #[serde(default)]
    pub force: bool,
    /// Explicitly allow replacing an existing same-id/same-version publication with different bytes.
    #[serde(default)]
    pub allow_mutable_version: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_key: Option<PathBuf>,
    #[serde(default)]
    pub deprecation: PublicationDeprecation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSkillerBundleResponse {
    pub registry: RegistrySummary,
    pub report: PublishReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackSearchRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_risk: Option<RiskLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct MspClient {
    registry: LocalRegistry,
}

impl MspClient {
    pub fn open(registry_root: impl AsRef<Path>) -> anyhow::Result<Self> {
        let registry_root = registry_root.as_ref();
        let registry = LocalRegistry::open(registry_root).with_context(|| {
            format!("failed to open MSP registry at {}", registry_root.display())
        })?;
        Ok(Self { registry })
    }

    pub fn info(&self) -> ClientInfo {
        ClientInfo::default()
    }

    pub fn summary(&self) -> RegistrySummary {
        RegistrySummary {
            root: self.registry.root().display().to_string(),
            skill_count: self.registry.skill_count(),
            pack_count: self.registry.pack_count(),
        }
    }

    pub fn search(&self, request: SearchRequest) -> SearchResponse {
        let limit = clamp_limit(request.limit);
        let query = SkillSearchQuery {
            task: request.task.clone(),
            category: request.category.clone(),
            domain: request.domain.clone(),
            language: request.language.clone(),
            available_tools: request.available_tools.clone(),
            max_risk: request.max_risk,
        };
        let mut results = self.registry.search(&query);
        results.truncate(limit);
        SearchResponse {
            registry: self.summary(),
            query: request,
            results,
        }
    }

    pub fn load_skill(&self, id: &str, mode: LoadMode) -> anyhow::Result<LoadedSkill> {
        let raw = self.registry.load_skill(id)?;
        let content = render_loaded_skill(&raw, mode)?;
        Ok(LoadedSkill {
            id: id.to_string(),
            mode,
            content,
            raw,
        })
    }

    pub fn get_manifest(&self, id: &str) -> anyhow::Result<SkillManifest> {
        Ok(self.registry.get_manifest(id)?)
    }

    pub fn verify_trust(&self, id: &str) -> anyhow::Result<TrustVerifyResult> {
        Ok(self.registry.verify_trust(id)?)
    }

    pub fn evaluate_trust(
        &self,
        request: TrustEvaluationRequest,
    ) -> anyhow::Result<serde_json::Value> {
        let value = if request.dependency_graph {
            serde_json::to_value(
                self.registry
                    .evaluate_dependency_trust(&request.policy, &request.id)?,
            )?
        } else {
            serde_json::to_value(
                self.registry
                    .evaluate_trust_policy(&request.policy, &request.id)?,
            )?
        };
        Ok(value)
    }

    pub fn evaluate_trust_policy(
        &self,
        policy: &SkillTrustPolicy,
        id: &str,
    ) -> anyhow::Result<TrustPolicyEvaluation> {
        Ok(self.registry.evaluate_trust_policy(policy, id)?)
    }

    pub fn check_compatibility(
        &self,
        request: CompatibilityRequest,
    ) -> anyhow::Result<msp_core::SkillCompatibilityResult> {
        Ok(self
            .registry
            .check_skill_compatibility(&request.skill_id, &request.query)?)
    }

    pub fn resolve_dependencies(
        &self,
        id: &str,
    ) -> anyhow::Result<msp_core::DependencyResolutionResult> {
        Ok(self.registry.resolve_dependencies(id)?)
    }

    pub fn discover_packs(&self, request: PackSearchRequest) -> Vec<msp_core::PackSearchResult> {
        let limit = clamp_limit(request.limit);
        let query = PackSearchQuery {
            task: request.task,
            category: request.category,
            issuer: request.issuer,
            max_risk: request.max_risk,
        };
        let mut results = self.registry.discover_packs(&query);
        results.truncate(limit);
        results
    }

    pub fn import_skiller_bundle(
        &self,
        request: ImportSkillerBundleRequest,
    ) -> anyhow::Result<ImportSkillerBundleResponse> {
        let options = PublishOptions {
            registry: self.registry.root().to_path_buf(),
            issuer: request.issuer,
            force: request.force,
            allow_mutable_version: request.allow_mutable_version,
            signing_key: request.signing_key,
            deprecation: request.deprecation,
        };
        let report = msp_publisher::publish_skiller_bundle(request.bundle, options)?;
        let refreshed = MspClient::open(&report.registry)?;
        Ok(ImportSkillerBundleResponse {
            registry: refreshed.summary(),
            report,
        })
    }

    pub fn get_pack_manifest(&self, id: &str) -> anyhow::Result<SkillPackManifest> {
        Ok(self.registry.get_pack_manifest(id)?)
    }
}

pub fn parse_risk_level(value: &str) -> anyhow::Result<RiskLevel> {
    match value.trim().to_ascii_lowercase().as_str() {
        "low" => Ok(RiskLevel::Low),
        "medium" => Ok(RiskLevel::Medium),
        "high" => Ok(RiskLevel::High),
        "critical" => Ok(RiskLevel::Critical),
        other => anyhow::bail!(
            "unknown MSP risk level `{other}`; expected low, medium, high, or critical"
        ),
    }
}

fn clamp_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_SEARCH_LIMIT)
        .clamp(1, MAX_SEARCH_LIMIT)
}

fn render_loaded_skill(load: &SkillLoadResult, mode: LoadMode) -> anyhow::Result<String> {
    match mode {
        LoadMode::Raw => Ok(serde_json::to_string_pretty(load)?),
        LoadMode::Body => Ok(load.body.clone()),
        LoadMode::Card => Ok(format!(
            "MSP Skill: {}\nName: {}\nVersion: {}\nCategory: {}\nRisk: {:?}\nIssuer: {}\nSummary: {}\nRequired tools: {}\nBody hash valid: {}",
            load.manifest.id,
            load.manifest.name,
            load.manifest.version,
            load.manifest.category,
            load.manifest.trust.risk_level,
            load.manifest.trust.issuer,
            load.manifest.summary,
            load.manifest
                .requirements
                .tools
                .iter()
                .filter(|tool| tool.required)
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            load.body_hash_valid,
        )),
        LoadMode::Extended => Ok(format!(
            "MSP Skill: {}\nName: {}\nVersion: {}\nCategory: {}\nRisk: {:?}\nIssuer: {}\nSummary: {}\nDescription: {}\nKeywords: {}\nRequired tools: {}\nPermissions: {}\nDependencies: {}\nBody hash valid: {}\n\n{}",
            load.manifest.id,
            load.manifest.name,
            load.manifest.version,
            load.manifest.category,
            load.manifest.trust.risk_level,
            load.manifest.trust.issuer,
            load.manifest.summary,
            load.manifest.description.as_deref().unwrap_or(""),
            load.manifest.keywords.join(", "),
            load.manifest
                .requirements
                .tools
                .iter()
                .filter(|tool| tool.required)
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            load.manifest.requirements.permissions.join(", "),
            load.dependency_ids.join(", "),
            load.body_hash_valid,
            load.body,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_var_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env var test lock poisoned")
    }

    #[test]
    fn parses_load_modes() {
        assert_eq!("card".parse::<LoadMode>().unwrap(), LoadMode::Card);
        assert_eq!("BODY".parse::<LoadMode>().unwrap(), LoadMode::Body);
        assert!("unknown".parse::<LoadMode>().is_err());
    }

    #[test]
    fn caps_limits() {
        assert_eq!(clamp_limit(None), DEFAULT_SEARCH_LIMIT);
        assert_eq!(clamp_limit(Some(0)), 1);
        assert_eq!(clamp_limit(Some(MAX_SEARCH_LIMIT + 1)), MAX_SEARCH_LIMIT);
    }

    #[test]
    fn default_registry_root_is_user_global() {
        let _guard = env_var_test_lock();
        let previous_home = std::env::var_os("VEGVISIR_HOME");
        let previous_registry = std::env::var_os("VEGVISIR_MSP_REGISTRY");
        let temp = tempfile::tempdir().unwrap();
        let data_root = temp.path().join("veg-home");
        unsafe {
            std::env::set_var("VEGVISIR_HOME", &data_root);
            std::env::remove_var("VEGVISIR_MSP_REGISTRY");
        }
        assert_eq!(
            default_registry_root(),
            data_root.join("msp").join("registry")
        );

        let override_root = temp.path().join("custom-registry");
        unsafe {
            std::env::set_var("VEGVISIR_MSP_REGISTRY", &override_root);
        }
        assert_eq!(default_registry_root(), override_root);

        unsafe {
            if let Some(previous) = previous_registry {
                std::env::set_var("VEGVISIR_MSP_REGISTRY", previous);
            } else {
                std::env::remove_var("VEGVISIR_MSP_REGISTRY");
            }
            if let Some(previous) = previous_home {
                std::env::set_var("VEGVISIR_HOME", previous);
            } else {
                std::env::remove_var("VEGVISIR_HOME");
            }
        }
    }
}
