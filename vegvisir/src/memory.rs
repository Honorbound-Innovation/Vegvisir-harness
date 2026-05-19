use std::path::{Path, PathBuf};

use cms_v2::{
    cms_api::{
        CmsMemoryClient, CommitRequest, CommitResult, MemoryId, MemoryObject,
        MemoryRetrievalResult, Metadata, ProjectId, RetrievalBundle, RetrievalMode,
        RetrievalRequest,
    },
    cms_runtime::LocalCmsMemoryClient,
    data_import::{ChatGptImportOptions, import_chatgpt_export},
    ecm::{
        ContextBudget, ContextMode, ContextRequest, ContextSession, EterniumContextManager,
        PreparedContext, UserId,
    },
    graph::{GraphIndex, SqliteGraphIndex},
    prompt_cache::{
        CacheScopeIdentity, CachedPromptEnvelope, PromptCacheEngine, PromptCachePrepareRequest,
    },
    sqlite::{MemoryStatusFilter, SqliteLedger},
    vectors::{SqliteVectorIndex, VectorIndex},
};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct VegvisirCmsConfig {
    pub db_path: PathBuf,
    pub user_id: String,
    pub project_id: Option<String>,
    pub context_mode: ContextMode,
    pub commit_writebacks: bool,
}

#[derive(Clone, Debug)]
pub struct ContextPrepareOptions {
    pub mode: ContextMode,
    pub metadata: Metadata,
    pub budget: Option<ContextBudget>,
}

impl Default for ContextPrepareOptions {
    fn default() -> Self {
        Self {
            mode: ContextMode::Project,
            metadata: Metadata::new(),
            budget: None,
        }
    }
}

impl VegvisirCmsConfig {
    pub fn for_workspace(workspace: impl AsRef<Path>) -> Self {
        Self {
            db_path: default_vegvisir_data_root().join("cms-v2.sqlite3"),
            user_id: "local-user".to_string(),
            project_id: Some(workspace_project_id_fallback(workspace.as_ref())),
            context_mode: ContextMode::Project,
            commit_writebacks: true,
        }
    }
}

pub fn default_vegvisir_data_root() -> PathBuf {
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

fn workspace_project_id_fallback(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let title = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    format!(
        "workspace:{}:{}",
        title,
        short_stable_hash(&canonical.display().to_string())
    )
}

fn short_stable_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub struct VegvisirCms {
    ledger: SqliteLedger,
    pub config: VegvisirCmsConfig,
}

#[derive(Clone, Debug)]
pub struct VegvisirMemorySummary {
    pub id: String,
    pub memory_type: String,
    pub title: String,
    pub summary: String,
    pub user_id: Option<String>,
    pub project_id: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone, Debug)]
pub struct ChatGptImportSummary {
    pub imported: usize,
    pub db_path: PathBuf,
    pub user_id: String,
    pub project_id: Option<String>,
}

impl VegvisirCms {
    pub fn open(config: VegvisirCmsConfig) -> anyhow::Result<Self> {
        if let Some(parent) = config.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Self {
            ledger: SqliteLedger::open(&config.db_path)?,
            config,
        })
    }

    pub fn retrieve(
        &mut self,
        query: impl Into<String>,
        limit: usize,
    ) -> anyhow::Result<RetrievalBundle> {
        let query = query.into();
        let mut request = RetrievalRequest::new(query.clone());
        request.limit = limit.max(1);
        request.modes = vec![
            RetrievalMode::Hybrid,
            RetrievalMode::Semantic,
            RetrievalMode::Recent,
        ];
        if let Some(project_id) = &self.config.project_id {
            request.project_id = Some(ProjectId::new(project_id.clone()));
            request
                .filters
                .insert("project_id".to_string(), Value::String(project_id.clone()));
        }
        request.filters.insert(
            "user_id".to_string(),
            Value::String(self.config.user_id.clone()),
        );
        let client = LocalCmsMemoryClient::new(&mut self.ledger);
        let mut bundle = client.retrieve(request)?;
        bundle.results.retain(|result| {
            let user_matches = result
                .memory
                .metadata
                .get("user_id")
                .and_then(Value::as_str)
                .map(|user_id| user_id == self.config.user_id)
                .unwrap_or(false);
            let project_matches = match &self.config.project_id {
                Some(project_id) => result
                    .memory
                    .metadata
                    .get("project_id")
                    .and_then(Value::as_str)
                    .map(|value| value == project_id)
                    .unwrap_or(false),
                None => true,
            };
            user_matches && project_matches
        });
        if self.config.project_id.is_some() && bundle.results.len() < limit.max(1) {
            let mut global_config = self.config.clone();
            global_config.project_id = None;
            let mut global_cms = Self::open(global_config)?;
            let global_bundle = global_cms.retrieve(query, limit)?;
            for result in global_bundle.results {
                if bundle.results.len() >= limit.max(1) {
                    break;
                }
                if result.memory.metadata.get("project_id").is_some() {
                    continue;
                }
                if bundle
                    .results
                    .iter()
                    .any(|existing| existing.memory.id == result.memory.id)
                {
                    continue;
                }
                bundle.results.push(result);
            }
        }
        Ok(bundle)
    }

    pub fn retrieve_global(
        &mut self,
        query: impl Into<String>,
        limit: usize,
    ) -> anyhow::Result<RetrievalBundle> {
        let query = query.into();
        let mut config = self.config.clone();
        config.project_id = None;
        let mut cms = Self::open(config)?;
        let mut bundle = cms.retrieve(query.clone(), limit)?;
        if !bundle.results.is_empty() {
            return Ok(bundle);
        }
        let needle = query.to_ascii_lowercase();
        let scoped = self.ledger.list_memories_by_scope(
            MemoryStatusFilter::Active,
            Some("private"),
            Some(&self.config.user_id),
            None,
            limit.saturating_mul(4).max(limit),
        )?;
        for entry in scoped {
            if bundle.results.len() >= limit.max(1) {
                break;
            }
            let Some(memory) = self.ledger.get_memory(&entry.id)? else {
                continue;
            };
            let haystack =
                format!("{} {} {}", memory.title, memory.summary, memory.body).to_ascii_lowercase();
            if !needle.is_empty() && !haystack.contains(&needle) {
                continue;
            }
            bundle.results.push(MemoryRetrievalResult {
                memory: core_memory_to_api(memory),
                score: 0.5,
                source_mode: RetrievalMode::Recent,
                reason: "global user-scope recall fallback".to_string(),
            });
        }
        Ok(bundle)
    }

    pub fn recent(&self, limit: usize, global: bool) -> anyhow::Result<Vec<VegvisirMemorySummary>> {
        let project_id = if global {
            None
        } else {
            self.config.project_id.as_deref()
        };
        let entries = self.ledger.list_memories_by_scope(
            MemoryStatusFilter::Active,
            Some("private"),
            Some(&self.config.user_id),
            project_id,
            limit.clamp(1, 50),
        )?;
        let mut out = Vec::new();
        for entry in entries {
            let Some(memory) = self.ledger.get_memory(&entry.id)? else {
                continue;
            };
            out.push(VegvisirMemorySummary {
                id: entry.id,
                memory_type: entry.memory_type,
                title: entry.title,
                summary: summarize(&memory.summary, 220),
                user_id: entry.user_id,
                project_id: entry.project_id,
                updated_at: entry.updated_at,
            });
        }
        Ok(out)
    }

    pub fn prepare_context(
        &mut self,
        message: impl Into<String>,
    ) -> anyhow::Result<PreparedContext> {
        self.prepare_context_with_options(
            message,
            ContextPrepareOptions {
                mode: self.config.context_mode,
                ..ContextPrepareOptions::default()
            },
        )
    }

    pub fn prepare_context_with_options(
        &mut self,
        message: impl Into<String>,
        options: ContextPrepareOptions,
    ) -> anyhow::Result<PreparedContext> {
        let mut request = ContextRequest::new(
            UserId::new(self.config.user_id.clone()),
            message.into(),
            options.mode,
        );
        if let Some(project_id) = &self.config.project_id {
            request = request.with_project(ProjectId::new(project_id.clone()));
        }
        request.metadata = options.metadata;
        if let Some(budget) = options.budget {
            request.budget = budget;
        }
        let ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut self.ledger));
        Ok(ecm.prepare_context(request)?)
    }

    pub fn prepare_cached_prompt(
        &mut self,
        message: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> anyhow::Result<CachedPromptEnvelope> {
        let message = message.into();
        let use_memory = should_use_ambient_memory(&message);
        let mut metadata = Metadata::new();
        if !use_memory {
            metadata.insert("memory_mode".to_string(), Value::String("none".to_string()));
        }
        let prepared = self.prepare_context_with_options(
            message,
            ContextPrepareOptions {
                mode: if use_memory {
                    self.config.context_mode
                } else {
                    ContextMode::Minimal
                },
                metadata,
                budget: Some(prompt_context_budget()),
            },
        )?;
        let scope_identity = match &self.config.project_id {
            Some(project_id) => CacheScopeIdentity::for_user_project(
                self.config.user_id.clone(),
                project_id.clone(),
            ),
            None => CacheScopeIdentity::for_user(self.config.user_id.clone()),
        }
        .with_session(prepared.session_id.0.clone());
        Ok(PromptCacheEngine::prepare_model_prompt(
            &prepared,
            PromptCachePrepareRequest::new(provider, model).with_scope_identity(scope_identity),
        ))
    }

    pub fn complete_turn(
        &mut self,
        user_message: &str,
        assistant_response: &str,
    ) -> anyhow::Result<Vec<CommitResult>> {
        if !self.config.commit_writebacks {
            return Ok(Vec::new());
        }
        let session = ContextSession::new(
            UserId::new(self.config.user_id.clone()),
            self.config.project_id.clone().map(ProjectId::new),
        );
        let mut ecm = EterniumContextManager::new(LocalCmsMemoryClient::new(&mut self.ledger));
        Ok(ecm.complete_turn(&session, user_message, assistant_response)?)
    }

    pub fn remember(
        &mut self,
        memory_type: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> anyhow::Result<CommitResult> {
        self.remember_with_project(memory_type, title, body, self.config.project_id.clone())
    }

    pub fn remember_global(
        &mut self,
        memory_type: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> anyhow::Result<CommitResult> {
        self.remember_with_project(memory_type, title, body, None)
    }

    fn remember_with_project(
        &mut self,
        memory_type: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        project_id: Option<String>,
    ) -> anyhow::Result<CommitResult> {
        let title = title.into();
        let body = body.into();
        let id = deterministic_memory_id(&self.config.user_id, &title, &body);
        let mut memory = MemoryObject::new(MemoryId::new(id), memory_type, title, body.clone());
        memory.summary = summarize(&body, 280);
        memory.metadata.insert(
            "user_id".to_string(),
            Value::String(self.config.user_id.clone()),
        );
        memory.metadata.insert(
            "visibility".to_string(),
            Value::String("private".to_string()),
        );
        if let Some(project_id) = &project_id {
            memory
                .metadata
                .insert("project_id".to_string(), Value::String(project_id.clone()));
        }
        let mut client = LocalCmsMemoryClient::new(&mut self.ledger);
        Ok(client.commit_memory(CommitRequest::new(memory, "Vegvisir explicit memory write"))?)
    }

    pub fn seed_memory_object(
        &mut self,
        memory: &cms_v2::core::MemoryObject,
    ) -> anyhow::Result<()> {
        self.ledger.upsert_memory(memory, None)?;
        SqliteGraphIndex::new(&self.ledger).upsert_memory(memory)?;
        SqliteVectorIndex::new(&self.ledger).upsert_memory(memory)?;
        Ok(())
    }

    pub fn import_chatgpt(
        &mut self,
        path: impl AsRef<Path>,
        messages_per_memory: usize,
        max_chars_per_memory: usize,
    ) -> anyhow::Result<ChatGptImportSummary> {
        let options = ChatGptImportOptions {
            messages_per_memory: messages_per_memory.max(1),
            max_chars_per_memory,
        };
        let mut memories = import_chatgpt_export(path.as_ref(), &options)?;
        for memory in &mut memories {
            memory
                .metadata
                .insert("user_id".to_string(), self.config.user_id.clone());
            memory
                .metadata
                .insert("visibility".to_string(), "private".to_string());
            if let Some(project_id) = &self.config.project_id {
                memory
                    .metadata
                    .insert("project_id".to_string(), project_id.clone());
            }
            if !memory.tags.iter().any(|tag| tag == "vegvisir-import") {
                memory.tags.push("vegvisir-import".to_string());
            }
        }
        for memory in &memories {
            self.seed_memory_object(memory)?;
        }
        Ok(ChatGptImportSummary {
            imported: memories.len(),
            db_path: self.config.db_path.clone(),
            user_id: self.config.user_id.clone(),
            project_id: self.config.project_id.clone(),
        })
    }
}

fn prompt_context_budget() -> ContextBudget {
    let max_tokens = std::env::var("VEGVISIR_CONTEXT_MAX_TOKENS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(6_000)
        .clamp(2_000, 64_000);
    ContextBudget {
        max_tokens,
        reserved_for_response: (max_tokens / 3).clamp(1_000, 8_000),
        reserved_for_system: 1_000,
        reserved_for_tools: 1_000,
    }
}

fn should_use_ambient_memory(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    if lower.trim().is_empty() {
        return false;
    }
    if lower.chars().count() >= 120 {
        return true;
    }
    [
        "remember",
        "recall",
        "memory",
        "memories",
        "continue",
        "where we left",
        "last time",
        "previous",
        "project",
        "workspace",
        "session",
        "decision",
        "plan",
        "cms",
        "eternium",
        "vegvisir",
        "agent",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn core_memory_to_api(memory: cms_v2::core::MemoryObject) -> MemoryObject {
    MemoryObject {
        id: MemoryId::new(memory.id),
        memory_type: memory.memory_type,
        title: memory.title,
        summary: memory.summary,
        body: memory.body,
        claims: memory
            .claims
            .into_iter()
            .map(|claim| cms_v2::cms_api::Claim {
                id: claim.id,
                text: claim.text,
                confidence: claim.confidence as f32,
                source: claim.source,
            })
            .collect(),
        links: memory
            .links
            .into_iter()
            .map(|link| cms_v2::cms_api::MemoryLink {
                source_id: MemoryId::new(link.source_id),
                target_id: link.target_id,
                relation: link.relation,
                confidence: link.confidence as f32,
            })
            .collect(),
        tags: memory.tags,
        confidence: memory.confidence as f32,
        source: memory.source.map(|source| source.reference),
        created_at: memory.created_at,
        updated_at: memory.updated_at,
        metadata: memory
            .metadata
            .into_iter()
            .map(|(key, value)| (key, Value::String(value)))
            .collect(),
    }
}

fn deterministic_memory_id(user_id: &str, title: &str, body: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    user_id.hash(&mut hasher);
    title.hash(&mut hasher);
    body.hash(&mut hasher);
    format!("mem_vegvisir_{:016x}", hasher.finish())
}

fn summarize(text: &str, limit: usize) -> String {
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() <= limit {
        clean
    } else {
        clean
            .chars()
            .take(limit.saturating_sub(1))
            .collect::<String>()
    }
}
