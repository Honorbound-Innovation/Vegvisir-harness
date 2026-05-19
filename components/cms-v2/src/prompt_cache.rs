use crate::ecm::{ContextFrame, ContextFrameType, PreparedContext, estimate_tokens};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PromptCacheZone {
    ToolDefinitions,
    SystemKernel,
    EterniumRuntime,
    UserScopeCapsule,
    ProjectCapsule,
    StableMemoryCapsule,
    SessionCheckpoint,
    DynamicRetrievedContext,
    CurrentTurn,
    ToolResultTail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CacheStability {
    Static,
    Versioned,
    Session,
    Dynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CacheScope {
    Global,
    User,
    Project,
    Session,
    Turn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CacheSensitivity {
    Public,
    Normal,
    Sensitive,
    Secret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CacheTtlClass {
    Permanent,
    Long,
    Session,
    Turn,
    NoCache,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextCacheHint {
    pub frame_id: String,
    pub preferred_zone: PromptCacheZone,
    pub stability: CacheStability,
    pub scope: CacheScope,
    pub sensitivity: CacheSensitivity,
    pub cache_policy_hint: Option<String>,
    pub source_memory_ids: Vec<String>,
    pub source_version_hashes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheScopeIdentity {
    pub organization_id: Option<String>,
    pub user_id: Option<String>,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub shared_scope_id: Option<String>,
}

impl CacheScopeIdentity {
    pub fn empty() -> Self {
        Self {
            organization_id: None,
            user_id: None,
            project_id: None,
            session_id: None,
            shared_scope_id: None,
        }
    }

    pub fn for_user(user_id: impl Into<String>) -> Self {
        Self {
            user_id: Some(user_id.into()),
            ..Self::empty()
        }
    }

    pub fn for_user_project(user_id: impl Into<String>, project_id: impl Into<String>) -> Self {
        Self {
            user_id: Some(user_id.into()),
            project_id: Some(project_id.into()),
            ..Self::empty()
        }
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptCachePolicy {
    pub allow_local_cache: bool,
    pub allow_provider_cache: bool,
    pub breakpoint_candidate: bool,
    pub ttl_class: CacheTtlClass,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromptBlockKind {
    System,
    Runtime,
    UserPreference,
    Project,
    Memory,
    Session,
    DynamicContext,
    CurrentTurn,
    ToolResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptBlock {
    pub id: String,
    pub kind: PromptBlockKind,
    pub zone: PromptCacheZone,
    pub title: String,
    pub content: String,
    pub content_hash: String,
    pub token_estimate: usize,
    pub source_memory_ids: Vec<String>,
    pub source_version_hashes: Vec<String>,
    pub stability: CacheStability,
    pub scope: CacheScope,
    pub sensitivity: CacheSensitivity,
    pub cache_policy: PromptCachePolicy,
    pub provider_annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PromptCapsuleType {
    ToolDefinitions,
    SystemKernel,
    UserScope,
    Project,
    StableMemory,
    Session,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptCapsule {
    pub capsule_id: String,
    pub capsule_type: PromptCapsuleType,
    pub scope: CacheScope,
    pub scope_identity: CacheScopeIdentity,
    pub content_hash: String,
    pub token_estimate: usize,
    pub source_memory_ids: Vec<String>,
    pub source_version_hashes: Vec<String>,
    pub block_ids: Vec<String>,
    pub renderer_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptCacheBreakpoint {
    pub after_block_id: String,
    pub zone: PromptCacheZone,
    pub token_estimate: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptCachePlan {
    pub cacheable_prefix_blocks: Vec<String>,
    pub dynamic_suffix_blocks: Vec<String>,
    pub breakpoints: Vec<PromptCacheBreakpoint>,
    pub expected_cache_value: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptCacheManifest {
    pub manifest_id: String,
    pub provider: String,
    pub model: String,
    pub prompt_cache_key: String,
    pub cacheable_prefix_hash: String,
    pub cacheable_prefix_tokens: usize,
    pub total_prompt_tokens: usize,
    pub renderer_version: String,
    pub tokenizer_version: String,
    pub scope_identity: CacheScopeIdentity,
    pub block_hashes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCacheHint {
    pub provider: String,
    pub cache_mode: String,
    pub prompt_cache_key: String,
    pub breakpoint_block_ids: Vec<String>,
    pub cacheable_prefix_tokens: usize,
    pub provider_annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequestEnvelope {
    pub provider: String,
    pub model: String,
    pub prompt: String,
    pub cache_hint: ProviderCacheHint,
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedPromptEnvelope {
    pub manifest: PromptCacheManifest,
    pub blocks: Vec<PromptBlock>,
    pub capsules: Vec<PromptCapsule>,
    pub cache_plan: PromptCachePlan,
    pub model_request: ModelRequestEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptCachePrepareRequest {
    pub provider: String,
    pub model: String,
    pub scope_identity: CacheScopeIdentity,
    pub renderer_version: String,
    pub tokenizer_version: String,
}

impl Default for PromptCachePrepareRequest {
    fn default() -> Self {
        Self {
            provider: "local".to_string(),
            model: "unspecified".to_string(),
            scope_identity: CacheScopeIdentity::empty(),
            renderer_version: "prompt-cache-renderer-v1".to_string(),
            tokenizer_version: "token-estimator-v1".to_string(),
        }
    }
}

impl PromptCachePrepareRequest {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            ..Self::default()
        }
    }

    pub fn with_scope_identity(mut self, scope_identity: CacheScopeIdentity) -> Self {
        self.scope_identity = scope_identity;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromptCacheMissReason {
    None,
    SourceVersionChanged,
    ScopeChanged,
    RendererChanged,
    TokenizerChanged,
    ProviderChanged,
    ModelChanged,
    SensitiveBlock,
    DynamicPrefix,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptCacheTrace {
    pub trace_id: String,
    pub manifest_id: String,
    pub provider: String,
    pub model: String,
    pub cacheable_prefix_tokens: usize,
    pub total_prompt_tokens: usize,
    pub provider_cached_tokens: usize,
    pub local_capsule_hits: usize,
    pub local_capsule_misses: usize,
    pub breakpoints: Vec<PromptCacheBreakpoint>,
    pub miss_reasons: Vec<PromptCacheMissReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptCacheUsage {
    pub manifest_id: String,
    pub provider: String,
    pub model: String,
    pub total_input_tokens: usize,
    pub provider_cached_input_tokens: usize,
    pub provider_cache_write_tokens: usize,
    pub provider_cache_read_tokens: usize,
    pub local_capsule_hits: usize,
    pub local_capsule_misses: usize,
    pub latency_ms: usize,
}

pub struct PromptCacheEngine;

impl PromptCacheEngine {
    pub fn prepare_model_prompt(
        prepared_context: &PreparedContext,
        request: PromptCachePrepareRequest,
    ) -> CachedPromptEnvelope {
        let blocks = canonicalize_blocks(build_blocks_from_prepared_context(prepared_context));
        let cache_plan = plan_cache(&blocks);
        let manifest = build_manifest(&blocks, &cache_plan, &request);
        let capsules = build_prompt_capsules(&blocks, &request);
        let model_request = render_model_request(&blocks, &cache_plan, &manifest, &request);

        CachedPromptEnvelope {
            manifest,
            blocks,
            capsules,
            cache_plan,
            model_request,
        }
    }
}

pub fn build_prompt_capsules(
    blocks: &[PromptBlock],
    request: &PromptCachePrepareRequest,
) -> Vec<PromptCapsule> {
    let mut groups: BTreeMap<(PromptCapsuleType, CacheScope), Vec<&PromptBlock>> = BTreeMap::new();
    for block in blocks {
        if !block.cache_policy.allow_local_cache {
            continue;
        }
        let Some(capsule_type) = capsule_type_for_zone(block.zone) else {
            continue;
        };
        groups
            .entry((capsule_type, block.scope))
            .or_default()
            .push(block);
    }

    groups
        .into_iter()
        .map(|((capsule_type, scope), grouped_blocks)| {
            let mut content_hashes = Vec::new();
            let mut source_memory_ids = Vec::new();
            let mut source_version_hashes = Vec::new();
            let mut block_ids = Vec::new();
            let mut token_estimate = 0usize;
            for block in grouped_blocks {
                content_hashes.push(block_cache_identity_hash(block));
                source_memory_ids.extend(block.source_memory_ids.clone());
                source_version_hashes.extend(block.source_version_hashes.clone());
                block_ids.push(block.id.clone());
                token_estimate += block.token_estimate;
            }
            source_memory_ids.sort();
            source_memory_ids.dedup();
            source_version_hashes.sort();
            source_version_hashes.dedup();
            block_ids.sort();
            content_hashes.sort();
            let content_hash = hash_text(&content_hashes.join("\n"));
            let scope_identity = capsule_scope_identity(scope, &request.scope_identity);
            PromptCapsule {
                capsule_id: stable_capsule_id(
                    &capsule_type,
                    scope,
                    &scope_identity,
                    &content_hash,
                    &request.renderer_version,
                ),
                capsule_type,
                scope,
                scope_identity,
                content_hash,
                token_estimate,
                source_memory_ids,
                source_version_hashes,
                block_ids,
                renderer_version: request.renderer_version.clone(),
            }
        })
        .collect()
}

fn capsule_scope_identity(scope: CacheScope, identity: &CacheScopeIdentity) -> CacheScopeIdentity {
    match scope {
        CacheScope::Global => CacheScopeIdentity {
            organization_id: identity.organization_id.clone(),
            user_id: None,
            project_id: None,
            session_id: None,
            shared_scope_id: identity.shared_scope_id.clone(),
        },
        CacheScope::User => CacheScopeIdentity {
            organization_id: identity.organization_id.clone(),
            user_id: identity.user_id.clone(),
            project_id: None,
            session_id: None,
            shared_scope_id: identity.shared_scope_id.clone(),
        },
        CacheScope::Project => CacheScopeIdentity {
            organization_id: identity.organization_id.clone(),
            user_id: identity.user_id.clone(),
            project_id: identity.project_id.clone(),
            session_id: None,
            shared_scope_id: identity.shared_scope_id.clone(),
        },
        CacheScope::Session => CacheScopeIdentity {
            organization_id: identity.organization_id.clone(),
            user_id: identity.user_id.clone(),
            project_id: identity.project_id.clone(),
            session_id: identity.session_id.clone(),
            shared_scope_id: identity.shared_scope_id.clone(),
        },
        CacheScope::Turn => CacheScopeIdentity {
            organization_id: identity.organization_id.clone(),
            user_id: identity.user_id.clone(),
            project_id: identity.project_id.clone(),
            session_id: identity.session_id.clone(),
            shared_scope_id: identity.shared_scope_id.clone(),
        },
    }
}

fn capsule_type_for_zone(zone: PromptCacheZone) -> Option<PromptCapsuleType> {
    match zone {
        PromptCacheZone::ToolDefinitions => Some(PromptCapsuleType::ToolDefinitions),
        PromptCacheZone::SystemKernel | PromptCacheZone::EterniumRuntime => {
            Some(PromptCapsuleType::SystemKernel)
        }
        PromptCacheZone::UserScopeCapsule => Some(PromptCapsuleType::UserScope),
        PromptCacheZone::ProjectCapsule => Some(PromptCapsuleType::Project),
        PromptCacheZone::StableMemoryCapsule => Some(PromptCapsuleType::StableMemory),
        PromptCacheZone::SessionCheckpoint => Some(PromptCapsuleType::Session),
        PromptCacheZone::DynamicRetrievedContext
        | PromptCacheZone::CurrentTurn
        | PromptCacheZone::ToolResultTail => None,
    }
}

fn stable_capsule_id(
    capsule_type: &PromptCapsuleType,
    scope: CacheScope,
    scope_identity: &CacheScopeIdentity,
    content_hash: &str,
    renderer_version: &str,
) -> String {
    let input = serde_json::to_string(&(
        capsule_type,
        scope,
        scope_identity,
        content_hash,
        renderer_version,
    ))
    .unwrap_or_default();
    format!("pcc_{}", &hash_text(&input)[..32])
}

pub trait ProviderPromptCacheAdapter {
    fn provider(&self) -> &'static str;
    fn build_cache_hint(
        &self,
        manifest: &PromptCacheManifest,
        cache_plan: &PromptCachePlan,
    ) -> ProviderCacheHint;
}

pub struct OpenAiPromptCacheAdapter;

impl ProviderPromptCacheAdapter for OpenAiPromptCacheAdapter {
    fn provider(&self) -> &'static str {
        "openai"
    }

    fn build_cache_hint(
        &self,
        manifest: &PromptCacheManifest,
        cache_plan: &PromptCachePlan,
    ) -> ProviderCacheHint {
        let mut provider_annotations = BTreeMap::new();
        provider_annotations.insert(
            "strategy".to_string(),
            "stable-prefix-with-prompt-cache-key".to_string(),
        );
        provider_annotations.insert(
            "prefix_hash".to_string(),
            manifest.cacheable_prefix_hash.clone(),
        );
        ProviderCacheHint {
            provider: self.provider().to_string(),
            cache_mode: "prefix".to_string(),
            prompt_cache_key: manifest.prompt_cache_key.clone(),
            breakpoint_block_ids: cache_plan.cacheable_prefix_blocks.clone(),
            cacheable_prefix_tokens: manifest.cacheable_prefix_tokens,
            provider_annotations,
        }
    }
}

pub struct AnthropicPromptCacheAdapter;

impl ProviderPromptCacheAdapter for AnthropicPromptCacheAdapter {
    fn provider(&self) -> &'static str {
        "anthropic"
    }

    fn build_cache_hint(
        &self,
        manifest: &PromptCacheManifest,
        cache_plan: &PromptCachePlan,
    ) -> ProviderCacheHint {
        let mut provider_annotations = BTreeMap::new();
        provider_annotations.insert(
            "strategy".to_string(),
            "explicit-cache-breakpoints".to_string(),
        );
        provider_annotations.insert(
            "breakpoint_count".to_string(),
            cache_plan.breakpoints.len().to_string(),
        );
        ProviderCacheHint {
            provider: self.provider().to_string(),
            cache_mode: "breakpoints".to_string(),
            prompt_cache_key: manifest.prompt_cache_key.clone(),
            breakpoint_block_ids: cache_plan
                .breakpoints
                .iter()
                .map(|breakpoint| breakpoint.after_block_id.clone())
                .collect(),
            cacheable_prefix_tokens: manifest.cacheable_prefix_tokens,
            provider_annotations,
        }
    }
}

pub struct LocalPromptCacheAdapter;

impl ProviderPromptCacheAdapter for LocalPromptCacheAdapter {
    fn provider(&self) -> &'static str {
        "local"
    }

    fn build_cache_hint(
        &self,
        manifest: &PromptCacheManifest,
        _cache_plan: &PromptCachePlan,
    ) -> ProviderCacheHint {
        let mut provider_annotations = BTreeMap::new();
        provider_annotations.insert("strategy".to_string(), "local-no-op".to_string());
        ProviderCacheHint {
            provider: self.provider().to_string(),
            cache_mode: "none".to_string(),
            prompt_cache_key: manifest.prompt_cache_key.clone(),
            breakpoint_block_ids: Vec::new(),
            cacheable_prefix_tokens: manifest.cacheable_prefix_tokens,
            provider_annotations,
        }
    }
}

pub fn build_blocks_from_prepared_context(prepared_context: &PreparedContext) -> Vec<PromptBlock> {
    let hints_by_frame = prepared_context
        .cache_hints
        .iter()
        .map(|hint| (hint.frame_id.as_str(), hint))
        .collect::<BTreeMap<_, _>>();

    prepared_context
        .frames
        .iter()
        .map(|frame| {
            let fallback_hint = fallback_hint(frame);
            let hint = hints_by_frame
                .get(frame.id.as_str())
                .copied()
                .unwrap_or(&fallback_hint);
            let content = render_block_content(frame);
            let content_hash = hash_text(&content);
            let sensitivity = hint.sensitivity;
            let cache_policy = cache_policy_for(hint);
            PromptBlock {
                id: stable_block_id(hint.preferred_zone, frame, &content_hash),
                kind: block_kind(frame.frame_type, hint.preferred_zone),
                zone: hint.preferred_zone,
                title: frame.title.clone(),
                token_estimate: estimate_tokens(&content),
                content,
                content_hash,
                source_memory_ids: hint.source_memory_ids.clone(),
                source_version_hashes: hint.source_version_hashes.clone(),
                stability: hint.stability,
                scope: hint.scope,
                sensitivity,
                cache_policy,
                provider_annotations: BTreeMap::new(),
            }
        })
        .collect()
}

pub fn canonicalize_blocks(mut blocks: Vec<PromptBlock>) -> Vec<PromptBlock> {
    for block in &mut blocks {
        block.source_memory_ids.sort();
        block.source_memory_ids.dedup();
        block.source_version_hashes.sort();
        block.source_version_hashes.dedup();
        block.content = canonicalize_text(&block.content);
        block.content_hash = hash_text(&block.content);
    }

    blocks.sort_by(|left, right| {
        left.zone
            .cmp(&right.zone)
            .then_with(|| left.source_memory_ids.cmp(&right.source_memory_ids))
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.content_hash.cmp(&right.content_hash))
    });
    blocks
}

pub fn plan_cache(blocks: &[PromptBlock]) -> PromptCachePlan {
    let mut cacheable_prefix_blocks = Vec::new();
    let mut dynamic_suffix_blocks = Vec::new();
    let mut breakpoints = Vec::new();
    let mut prefix_open = true;
    let mut prefix_tokens = 0usize;

    for block in blocks {
        let cacheable = block.cache_policy.allow_provider_cache
            && !matches!(block.stability, CacheStability::Dynamic)
            && !matches!(block.scope, CacheScope::Turn)
            && prefix_open;

        if cacheable {
            prefix_tokens += block.token_estimate;
            cacheable_prefix_blocks.push(block.id.clone());
            if block.cache_policy.breakpoint_candidate
                && matches!(
                    block.zone,
                    PromptCacheZone::SystemKernel
                        | PromptCacheZone::EterniumRuntime
                        | PromptCacheZone::UserScopeCapsule
                        | PromptCacheZone::ProjectCapsule
                        | PromptCacheZone::StableMemoryCapsule
                )
            {
                breakpoints.push(PromptCacheBreakpoint {
                    after_block_id: block.id.clone(),
                    zone: block.zone,
                    token_estimate: prefix_tokens,
                    reason: format!("stable {:?} prefix boundary", block.zone),
                });
            }
        } else {
            prefix_open = false;
            dynamic_suffix_blocks.push(block.id.clone());
        }
    }

    let total_tokens: usize = blocks.iter().map(|block| block.token_estimate).sum();
    let expected_cache_value = if total_tokens == 0 {
        0.0
    } else {
        prefix_tokens as f32 / total_tokens as f32
    };

    PromptCachePlan {
        cacheable_prefix_blocks,
        dynamic_suffix_blocks,
        breakpoints,
        expected_cache_value,
    }
}

fn build_manifest(
    blocks: &[PromptBlock],
    cache_plan: &PromptCachePlan,
    request: &PromptCachePrepareRequest,
) -> PromptCacheManifest {
    let prefix_hashes = blocks
        .iter()
        .filter(|block| cache_plan.cacheable_prefix_blocks.contains(&block.id))
        .map(block_cache_identity_hash)
        .collect::<Vec<_>>()
        .join("\n");
    let cacheable_prefix_hash = hash_text(&prefix_hashes);
    let prompt_cache_key = scoped_cache_key(&cacheable_prefix_hash, request);
    let block_hashes = blocks
        .iter()
        .map(block_cache_identity_hash)
        .collect::<Vec<_>>();
    let cacheable_prefix_tokens = blocks
        .iter()
        .filter(|block| cache_plan.cacheable_prefix_blocks.contains(&block.id))
        .map(|block| block.token_estimate)
        .sum();
    let total_prompt_tokens = blocks.iter().map(|block| block.token_estimate).sum();

    let mut manifest_input = BTreeMap::new();
    manifest_input.insert("provider", request.provider.clone());
    manifest_input.insert("model", request.model.clone());
    manifest_input.insert("prompt_cache_key", prompt_cache_key.clone());
    manifest_input.insert("prefix_hash", cacheable_prefix_hash.clone());
    manifest_input.insert("renderer", request.renderer_version.clone());
    manifest_input.insert("tokenizer", request.tokenizer_version.clone());
    let manifest_id = hash_json_map(&manifest_input);

    PromptCacheManifest {
        manifest_id,
        provider: request.provider.clone(),
        model: request.model.clone(),
        prompt_cache_key,
        cacheable_prefix_hash,
        cacheable_prefix_tokens,
        total_prompt_tokens,
        renderer_version: request.renderer_version.clone(),
        tokenizer_version: request.tokenizer_version.clone(),
        scope_identity: request.scope_identity.clone(),
        block_hashes,
    }
}

fn render_model_request(
    blocks: &[PromptBlock],
    cache_plan: &PromptCachePlan,
    manifest: &PromptCacheManifest,
    request: &PromptCachePrepareRequest,
) -> ModelRequestEnvelope {
    let prompt = blocks
        .iter()
        .map(|block| {
            format!(
                "<{:?} id=\"{}\">\n{}\n</{:?}>",
                block.zone, block.id, block.content, block.zone
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let mut metadata = BTreeMap::new();
    metadata.insert(
        "manifest_id".to_string(),
        Value::String(manifest.manifest_id.clone()),
    );
    metadata.insert(
        "cacheable_prefix_hash".to_string(),
        Value::String(manifest.cacheable_prefix_hash.clone()),
    );

    let cache_hint = provider_adapter(&request.provider).build_cache_hint(manifest, cache_plan);

    ModelRequestEnvelope {
        provider: request.provider.clone(),
        model: request.model.clone(),
        prompt,
        cache_hint,
        metadata,
    }
}

fn provider_adapter(provider: &str) -> Box<dyn ProviderPromptCacheAdapter> {
    match provider {
        "openai" => Box::new(OpenAiPromptCacheAdapter),
        "anthropic" | "claude" => Box::new(AnthropicPromptCacheAdapter),
        _ => Box::new(LocalPromptCacheAdapter),
    }
}

pub fn prompt_cache_trace(
    prepared_context: &PreparedContext,
    envelope: &CachedPromptEnvelope,
) -> PromptCacheTrace {
    PromptCacheTrace {
        trace_id: prepared_context.trace_id.clone(),
        manifest_id: envelope.manifest.manifest_id.clone(),
        provider: envelope.manifest.provider.clone(),
        model: envelope.manifest.model.clone(),
        cacheable_prefix_tokens: envelope.manifest.cacheable_prefix_tokens,
        total_prompt_tokens: envelope.manifest.total_prompt_tokens,
        provider_cached_tokens: 0,
        local_capsule_hits: 0,
        local_capsule_misses: envelope.capsules.len(),
        breakpoints: envelope.cache_plan.breakpoints.clone(),
        miss_reasons: if envelope.manifest.cacheable_prefix_tokens == 0 {
            vec![PromptCacheMissReason::DynamicPrefix]
        } else {
            vec![PromptCacheMissReason::None]
        },
    }
}

fn fallback_hint(frame: &ContextFrame) -> ContextCacheHint {
    let zone = match frame.frame_type {
        ContextFrameType::System => PromptCacheZone::SystemKernel,
        ContextFrameType::UserRequest => PromptCacheZone::CurrentTurn,
        ContextFrameType::ToolResult => PromptCacheZone::ToolResultTail,
        ContextFrameType::RetrievedMemory | ContextFrameType::Decision => {
            PromptCacheZone::StableMemoryCapsule
        }
        _ => PromptCacheZone::DynamicRetrievedContext,
    };

    ContextCacheHint {
        frame_id: frame.id.clone(),
        preferred_zone: zone,
        stability: if matches!(
            zone,
            PromptCacheZone::CurrentTurn
                | PromptCacheZone::ToolResultTail
                | PromptCacheZone::DynamicRetrievedContext
        ) {
            CacheStability::Dynamic
        } else {
            CacheStability::Versioned
        },
        scope: if matches!(zone, PromptCacheZone::CurrentTurn) {
            CacheScope::Turn
        } else {
            CacheScope::Session
        },
        sensitivity: CacheSensitivity::Normal,
        cache_policy_hint: None,
        source_memory_ids: frame.memory_ids.iter().map(|id| id.0.clone()).collect(),
        source_version_hashes: Vec::new(),
    }
}

fn block_kind(frame_type: ContextFrameType, zone: PromptCacheZone) -> PromptBlockKind {
    match (frame_type, zone) {
        (ContextFrameType::System, _) => PromptBlockKind::System,
        (_, PromptCacheZone::EterniumRuntime) => PromptBlockKind::Runtime,
        (ContextFrameType::UserPreference, _) => PromptBlockKind::UserPreference,
        (ContextFrameType::ProjectState | ContextFrameType::Decision, _) => {
            PromptBlockKind::Project
        }
        (ContextFrameType::RetrievedMemory, _) => PromptBlockKind::Memory,
        (ContextFrameType::Summary, _) => PromptBlockKind::Session,
        (ContextFrameType::UserRequest, _) => PromptBlockKind::CurrentTurn,
        (ContextFrameType::ToolResult, _) => PromptBlockKind::ToolResult,
        _ => PromptBlockKind::DynamicContext,
    }
}

fn cache_policy_for(hint: &ContextCacheHint) -> PromptCachePolicy {
    if matches!(
        hint.cache_policy_hint.as_deref(),
        Some("no_cache" | "no-cache" | "none")
    ) {
        return PromptCachePolicy {
            allow_local_cache: false,
            allow_provider_cache: false,
            breakpoint_candidate: false,
            ttl_class: CacheTtlClass::NoCache,
        };
    }

    if matches!(hint.sensitivity, CacheSensitivity::Secret) {
        return PromptCachePolicy {
            allow_local_cache: false,
            allow_provider_cache: false,
            breakpoint_candidate: false,
            ttl_class: CacheTtlClass::NoCache,
        };
    }

    let dynamic = matches!(hint.stability, CacheStability::Dynamic)
        || matches!(hint.scope, CacheScope::Turn)
        || matches!(
            hint.preferred_zone,
            PromptCacheZone::CurrentTurn | PromptCacheZone::ToolResultTail
        );
    PromptCachePolicy {
        allow_local_cache: !dynamic,
        allow_provider_cache: !dynamic,
        breakpoint_candidate: !dynamic,
        ttl_class: match hint.stability {
            CacheStability::Static => CacheTtlClass::Permanent,
            CacheStability::Versioned => CacheTtlClass::Long,
            CacheStability::Session => CacheTtlClass::Session,
            CacheStability::Dynamic => CacheTtlClass::Turn,
        },
    }
}

fn render_block_content(frame: &ContextFrame) -> String {
    let mut metadata = Map::new();
    if !frame.memory_ids.is_empty() {
        metadata.insert(
            "memory_ids".to_string(),
            Value::Array(
                frame
                    .memory_ids
                    .iter()
                    .map(|id| Value::String(id.0.clone()))
                    .collect(),
            ),
        );
    }
    metadata.insert("source".to_string(), Value::String(frame.source.clone()));

    let metadata_json = serde_json::to_string(&Value::Object(metadata)).unwrap_or_default();
    canonicalize_text(&format!(
        "title: {}\nmetadata: {}\n\n{}",
        frame.title, metadata_json, frame.content
    ))
}

fn canonicalize_text(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn stable_block_id(zone: PromptCacheZone, frame: &ContextFrame, content_hash: &str) -> String {
    let memory_key = frame
        .memory_ids
        .iter()
        .map(|id| id.0.as_str())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "pcb_{}",
        &hash_text(&format!(
            "{zone:?}\n{:?}\n{}\n{}\n{}",
            frame.frame_type, frame.title, memory_key, content_hash
        ))[..20]
    )
}

fn block_cache_identity_hash(block: &PromptBlock) -> String {
    let input = serde_json::to_string(&(
        &block.content_hash,
        &block.source_memory_ids,
        &block.source_version_hashes,
        block.stability,
        block.scope,
        block.sensitivity,
    ))
    .unwrap_or_default();
    hash_text(&input)
}

fn scoped_cache_key(prefix_hash: &str, request: &PromptCachePrepareRequest) -> String {
    let value = serde_json::to_value((
        prefix_hash,
        &request.provider,
        &request.model,
        &request.renderer_version,
        &request.tokenizer_version,
        &request.scope_identity,
    ))
    .unwrap_or(Value::Null);
    format!("pck_{}", &hash_text(&value.to_string())[..32])
}

fn hash_json_map(input: &BTreeMap<&str, String>) -> String {
    let value = serde_json::to_string(input).unwrap_or_default();
    format!("pcm_{}", &hash_text(&value)[..32])
}

fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cms_api::MemoryId;
    use crate::ecm::{ContextFrame, ContextPriority, SessionId};

    fn prepared_with_message(message: &str) -> PreparedContext {
        let mut system = ContextFrame::new(
            ContextFrameType::System,
            "System",
            "CMS owns memory. ECM owns context.",
            "eternium",
            ContextPriority::P0Mandatory,
        );
        system.id = "frame_system".to_string();

        let mut memory = ContextFrame::new(
            ContextFrameType::RetrievedMemory,
            "Architecture memory",
            "Prompt cache is after ECM and before provider adapters.",
            "cms-api",
            ContextPriority::P1Critical,
        );
        memory.id = "frame_memory".to_string();
        memory.memory_ids = vec![MemoryId("mem_architecture".to_string())];

        let mut user = ContextFrame::new(
            ContextFrameType::UserRequest,
            "Current user request",
            message,
            "user",
            ContextPriority::P0Mandatory,
        );
        user.id = "frame_user".to_string();

        PreparedContext {
            session_id: SessionId("session_test".to_string()),
            frames: vec![user.clone(), memory.clone(), system.clone()],
            cache_hints: vec![
                ContextCacheHint {
                    frame_id: system.id.clone(),
                    preferred_zone: PromptCacheZone::SystemKernel,
                    stability: CacheStability::Static,
                    scope: CacheScope::Global,
                    sensitivity: CacheSensitivity::Normal,
                    cache_policy_hint: None,
                    source_memory_ids: Vec::new(),
                    source_version_hashes: Vec::new(),
                },
                ContextCacheHint {
                    frame_id: memory.id.clone(),
                    preferred_zone: PromptCacheZone::StableMemoryCapsule,
                    stability: CacheStability::Versioned,
                    scope: CacheScope::Project,
                    sensitivity: CacheSensitivity::Normal,
                    cache_policy_hint: None,
                    source_memory_ids: vec!["mem_architecture".to_string()],
                    source_version_hashes: vec!["v1".to_string()],
                },
                ContextCacheHint {
                    frame_id: user.id.clone(),
                    preferred_zone: PromptCacheZone::CurrentTurn,
                    stability: CacheStability::Dynamic,
                    scope: CacheScope::Turn,
                    sensitivity: CacheSensitivity::Normal,
                    cache_policy_hint: None,
                    source_memory_ids: Vec::new(),
                    source_version_hashes: Vec::new(),
                },
            ],
            packed_text: String::new(),
            token_estimate: 0,
            included_memory_ids: vec![MemoryId("mem_architecture".to_string())],
            excluded_memory_ids: Vec::new(),
            trace_id: "trace_test".to_string(),
            metadata: Default::default(),
        }
    }

    #[test]
    fn canonical_blocks_put_stable_zones_before_current_turn() {
        let prepared = prepared_with_message("continue");
        let blocks = canonicalize_blocks(build_blocks_from_prepared_context(&prepared));
        let zones = blocks.iter().map(|block| block.zone).collect::<Vec<_>>();
        assert_eq!(zones[0], PromptCacheZone::SystemKernel);
        assert_eq!(zones[1], PromptCacheZone::StableMemoryCapsule);
        assert_eq!(zones[2], PromptCacheZone::CurrentTurn);
    }

    #[test]
    fn current_turn_change_does_not_change_prefix_hash() {
        let request = PromptCachePrepareRequest {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
            ..Default::default()
        };
        let first = PromptCacheEngine::prepare_model_prompt(
            &prepared_with_message("first turn"),
            request.clone(),
        );
        let second =
            PromptCacheEngine::prepare_model_prompt(&prepared_with_message("second turn"), request);

        assert_eq!(
            first.manifest.cacheable_prefix_hash,
            second.manifest.cacheable_prefix_hash
        );
        assert_eq!(
            first.cache_plan.cacheable_prefix_blocks,
            second.cache_plan.cacheable_prefix_blocks
        );
        assert_ne!(first.model_request.prompt, second.model_request.prompt);
    }

    #[test]
    fn secret_block_is_not_provider_cacheable() {
        let mut prepared = prepared_with_message("use token");
        prepared.cache_hints[1].sensitivity = CacheSensitivity::Secret;
        let blocks = canonicalize_blocks(build_blocks_from_prepared_context(&prepared));
        let secret = blocks
            .iter()
            .find(|block| block.zone == PromptCacheZone::StableMemoryCapsule)
            .unwrap();

        assert!(!secret.cache_policy.allow_local_cache);
        assert!(!secret.cache_policy.allow_provider_cache);
        assert!(!secret.cache_policy.breakpoint_candidate);
    }

    #[test]
    fn provider_adapters_emit_distinct_cache_modes() {
        let request = PromptCachePrepareRequest {
            provider: "anthropic".to_string(),
            model: "claude-test".to_string(),
            ..Default::default()
        };
        let envelope =
            PromptCacheEngine::prepare_model_prompt(&prepared_with_message("turn"), request);
        assert_eq!(envelope.model_request.cache_hint.cache_mode, "breakpoints");
        assert_eq!(
            envelope.model_request.cache_hint.provider_annotations["strategy"],
            "explicit-cache-breakpoints"
        );

        let local = PromptCacheEngine::prepare_model_prompt(
            &prepared_with_message("turn"),
            PromptCachePrepareRequest::default(),
        );
        assert_eq!(local.model_request.cache_hint.cache_mode, "none");
        assert!(
            local
                .model_request
                .cache_hint
                .breakpoint_block_ids
                .is_empty()
        );
    }

    #[test]
    fn capsules_are_stable_across_current_turn_changes() {
        let request = PromptCachePrepareRequest {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
            scope_identity: CacheScopeIdentity {
                organization_id: None,
                user_id: Some("user-a".to_string()),
                project_id: Some("CMS".to_string()),
                session_id: Some("session-a".to_string()),
                shared_scope_id: None,
            },
            ..Default::default()
        };
        let first = PromptCacheEngine::prepare_model_prompt(
            &prepared_with_message("first"),
            request.clone(),
        );
        let second =
            PromptCacheEngine::prepare_model_prompt(&prepared_with_message("second"), request);

        assert_eq!(first.capsules, second.capsules);
        assert!(
            first
                .capsules
                .iter()
                .all(|capsule| !matches!(capsule.capsule_type, PromptCapsuleType::Session))
        );
        assert!(
            first
                .capsules
                .iter()
                .any(|capsule| capsule.capsule_type == PromptCapsuleType::StableMemory)
        );
    }

    #[test]
    fn source_version_change_invalidates_prefix_and_stable_memory_capsule() {
        let request = PromptCachePrepareRequest {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
            scope_identity: CacheScopeIdentity {
                organization_id: None,
                user_id: Some("user-a".to_string()),
                project_id: Some("CMS".to_string()),
                session_id: Some("session-a".to_string()),
                shared_scope_id: None,
            },
            ..Default::default()
        };
        let mut first_context = prepared_with_message("turn");
        let mut second_context = prepared_with_message("turn");
        first_context.cache_hints[1].source_version_hashes = vec!["memory-version-1".to_string()];
        second_context.cache_hints[1].source_version_hashes = vec!["memory-version-2".to_string()];

        let first = PromptCacheEngine::prepare_model_prompt(&first_context, request.clone());
        let second = PromptCacheEngine::prepare_model_prompt(&second_context, request);

        assert_eq!(first.model_request.prompt, second.model_request.prompt);
        assert_ne!(
            first.manifest.cacheable_prefix_hash,
            second.manifest.cacheable_prefix_hash
        );
        assert_ne!(
            first.manifest.prompt_cache_key,
            second.manifest.prompt_cache_key
        );
        assert_ne!(first.manifest.manifest_id, second.manifest.manifest_id);

        let first_memory_capsule = first
            .capsules
            .iter()
            .find(|capsule| capsule.capsule_type == PromptCapsuleType::StableMemory)
            .unwrap();
        let second_memory_capsule = second
            .capsules
            .iter()
            .find(|capsule| capsule.capsule_type == PromptCapsuleType::StableMemory)
            .unwrap();

        assert_eq!(
            first_memory_capsule.source_memory_ids,
            second_memory_capsule.source_memory_ids
        );
        assert_ne!(
            first_memory_capsule.source_version_hashes,
            second_memory_capsule.source_version_hashes
        );
        assert_ne!(
            first_memory_capsule.capsule_id,
            second_memory_capsule.capsule_id
        );
    }
}
