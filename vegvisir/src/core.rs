use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::lsl::{parse_lsl, subskill_metadata};

fn now_iso() -> DateTime<Utc> {
    Utc::now()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub category: String,
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub risky: bool,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub name: String,
    pub category: String,
    pub description: String,
    #[serde(default = "default_skill_kind")]
    pub kind: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandDefinition {
    pub name: String,
    pub description: String,
    pub usage: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub delegates_to_agent: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attachment {
    pub path: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default = "now_iso")]
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_type: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
    #[serde(default = "now_iso")]
    pub created_at: DateTime<Utc>,
}

impl AuditEvent {
    pub fn new(
        event_type: impl Into<String>,
        message: impl Into<String>,
        session_id: Option<String>,
        metadata: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            event_type: event_type.into(),
            message: message.into(),
            session_id,
            metadata,
            created_at: now_iso(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub provider: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default = "default_true")]
    pub supports_streaming: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: String,
    #[serde(default = "default_agent_mode")]
    pub mode: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    pub system_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_model: Option<String>,
    #[serde(default)]
    pub enabled_tools: Vec<String>,
    #[serde(default)]
    pub enabled_skills: Vec<String>,
    #[serde(default)]
    pub enabled_mcp_servers: Vec<String>,
    #[serde(default)]
    pub usrl_contracts: Vec<String>,
    pub cms_user_id: String,
    pub cms_project_id: String,
    #[serde(default = "default_agent_memory_scope")]
    pub memory_scope: String,
    #[serde(default)]
    pub memory_policy: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
    #[serde(default = "now_iso")]
    pub created_at: DateTime<Utc>,
    #[serde(default = "now_iso")]
    pub updated_at: DateTime<Utc>,
}

impl AgentProfile {
    pub fn new(
        id: impl AsRef<str>,
        display_name: impl Into<String>,
        system_prompt: impl Into<String>,
    ) -> anyhow::Result<Self> {
        let id = normalize_agent_id(id.as_ref());
        if id.is_empty() {
            anyhow::bail!("agent id must contain at least one letter or number");
        }
        let now = now_iso();
        let cms_scope = format!("agent:{id}");
        Ok(Self {
            id,
            mode: default_agent_mode(),
            display_name: display_name.into(),
            description: String::new(),
            system_prompt: system_prompt.into(),
            current_provider: None,
            current_model: None,
            enabled_tools: Vec::new(),
            enabled_skills: Vec::new(),
            enabled_mcp_servers: Vec::new(),
            usrl_contracts: Vec::new(),
            cms_user_id: cms_scope.clone(),
            cms_project_id: cms_scope,
            memory_scope: default_agent_memory_scope(),
            memory_policy: "private".to_string(),
            metadata: BTreeMap::new(),
            created_at: now,
            updated_at: now,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    Stdio,
    Http,
}

impl Default for McpTransport {
    fn default() -> Self {
        Self::Stdio
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub transport: McpTransport,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub hbse_secret_refs: Vec<String>,
    #[serde(default)]
    pub consumer: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub tools: Vec<McpToolConfig>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
    #[serde(default, skip)]
    pub discovery_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpToolConfig {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub schema: Value,
}

#[derive(Clone, Debug)]
pub struct McpConfigStore {
    pub path: PathBuf,
}

impl McpConfigStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn load(&self) -> anyhow::Result<Vec<McpServerConfig>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        #[derive(Deserialize)]
        struct McpConfigFile {
            #[serde(default)]
            servers: Vec<McpServerConfig>,
        }
        let config: McpConfigFile = serde_json::from_str(&fs::read_to_string(&self.path)?)?;
        Ok(config.servers)
    }

    pub fn save(&self, servers: &[McpServerConfig]) -> anyhow::Result<PathBuf> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        #[derive(Serialize)]
        struct McpConfigFile<'a> {
            servers: &'a [McpServerConfig],
        }
        fs::write(
            &self.path,
            serde_json::to_string_pretty(&McpConfigFile { servers })?,
        )?;
        Ok(self.path.clone())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HbseServiceRef {
    pub name: String,
    pub secret_ref: String,
    pub consumer: String,
    pub purpose: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Clone, Debug)]
pub struct HbseServiceRefStore {
    pub path: PathBuf,
}

impl HbseServiceRefStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn load(&self) -> anyhow::Result<Vec<HbseServiceRef>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        #[derive(Deserialize)]
        struct HbseServiceRefFile {
            #[serde(default)]
            services: Vec<HbseServiceRef>,
        }
        let config: HbseServiceRefFile = serde_json::from_str(&fs::read_to_string(&self.path)?)?;
        Ok(config.services)
    }

    pub fn save(&self, services: &[HbseServiceRef]) -> anyhow::Result<PathBuf> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        #[derive(Serialize)]
        struct HbseServiceRefFile<'a> {
            services: &'a [HbseServiceRef],
        }
        fs::write(
            &self.path,
            serde_json::to_string_pretty(&HbseServiceRefFile { services })?,
        )?;
        Ok(self.path.clone())
    }
}

#[derive(Clone, Debug)]
pub struct AgentProfileStore {
    pub root: PathBuf,
}

impl AgentProfileStore {
    pub fn new(root: impl AsRef<Path>) -> anyhow::Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        Ok(Self {
            root: root.as_ref().to_path_buf(),
        })
    }

    pub fn path_for(&self, agent_id: &str) -> PathBuf {
        self.root
            .join(format!("{}.json", normalize_agent_id(agent_id)))
    }

    pub fn save(&self, profile: &AgentProfile) -> anyhow::Result<PathBuf> {
        let path = self.path_for(&profile.id);
        fs::write(&path, serde_json::to_string_pretty(profile)?)?;
        Ok(path)
    }

    pub fn load(&self, agent_id: &str) -> anyhow::Result<AgentProfile> {
        Ok(serde_json::from_str(&fs::read_to_string(
            self.path_for(agent_id),
        )?)?)
    }

    pub fn delete(&self, agent_id: &str) -> anyhow::Result<PathBuf> {
        let path = self.path_for(agent_id);
        fs::remove_file(&path)?;
        Ok(path)
    }

    pub fn list(&self) -> anyhow::Result<Vec<AgentProfile>> {
        let mut profiles = Vec::new();
        if !self.root.exists() {
            return Ok(profiles);
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext == "json")
                .unwrap_or(false)
            {
                profiles.push(serde_json::from_str(&fs::read_to_string(entry.path())?)?);
            }
        }
        profiles.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(profiles)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionState {
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub title: String,
    pub current_provider: String,
    pub current_model: String,
    pub system_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_agent_name: Option<String>,
    pub cwd: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub pending_attachments: Vec<Attachment>,
    #[serde(default)]
    pub enabled_tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub enabled_skills: Vec<SkillDefinition>,
    #[serde(default)]
    pub input_history: Vec<String>,
    pub tokens_used: u64,
    pub context_limit: u64,
    pub last_latency_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_prompt_cache_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_prompt_manifest_id: Option<String>,
    pub status: String,
    pub activity: String,
    pub activity_tick: u64,
    pub debug: bool,
}

impl SessionState {
    pub fn new(
        cwd: impl AsRef<Path>,
        tools: Vec<ToolDefinition>,
        skills: Vec<SkillDefinition>,
    ) -> Self {
        Self {
            session_id: Uuid::new_v4().simple().to_string()[..12].to_string(),
            created_at: now_iso(),
            title: "untitled".to_string(),
            current_provider: "demo".to_string(),
            current_model: "demo-local".to_string(),
            system_prompt: String::new(),
            active_agent_id: None,
            active_agent_name: None,
            cwd: cwd.as_ref().display().to_string(),
            messages: Vec::new(),
            pending_attachments: Vec::new(),
            enabled_tools: tools,
            enabled_skills: skills,
            input_history: Vec::new(),
            tokens_used: 0,
            context_limit: 8192,
            last_latency_ms: 0,
            last_prompt_cache_key: None,
            last_prompt_manifest_id: None,
            status: "ready".to_string(),
            activity: String::new(),
            activity_tick: 0,
            debug: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionStore {
    pub root: PathBuf,
}

impl SessionStore {
    pub fn new(root: impl AsRef<Path>) -> anyhow::Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        Ok(Self {
            root: root.as_ref().to_path_buf(),
        })
    }

    pub fn path_for(&self, session_id: &str) -> PathBuf {
        self.root.join(format!("{session_id}.json"))
    }

    pub fn save(&self, session: &SessionState) -> anyhow::Result<PathBuf> {
        let path = self.path_for(&session.session_id);
        fs::write(&path, serde_json::to_string_pretty(session)?)?;
        Ok(path)
    }

    pub fn load(&self, session_id: &str) -> anyhow::Result<SessionState> {
        Ok(serde_json::from_str(&fs::read_to_string(
            self.path_for(session_id),
        )?)?)
    }

    pub fn list(&self) -> anyhow::Result<Vec<SessionState>> {
        let mut sessions = Vec::new();
        if !self.root.exists() {
            return Ok(sessions);
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext == "json")
                .unwrap_or(false)
            {
                sessions.push(serde_json::from_str(&fs::read_to_string(entry.path())?)?);
            }
        }
        sessions.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(sessions)
    }
}

pub struct SessionManager {
    pub store: SessionStore,
    pub cwd: PathBuf,
}

impl SessionManager {
    pub fn new(store: SessionStore, cwd: impl AsRef<Path>) -> Self {
        Self {
            store,
            cwd: cwd.as_ref().to_path_buf(),
        }
    }

    pub fn create(
        &self,
        title: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
        tools: Vec<ToolDefinition>,
        skills: Vec<SkillDefinition>,
    ) -> SessionState {
        let mut session = SessionState::new(&self.cwd, tools, skills);
        session.title = title.into();
        session.current_provider = provider.into();
        session.current_model = model.into();
        session
    }

    pub fn reset(&self, session: &mut SessionState) {
        session.messages.clear();
        session.tokens_used = 0;
        session.status = "ready".to_string();
        session.activity.clear();
        session.activity_tick = 0;
    }

    pub fn save(&self, session: &SessionState) -> anyhow::Result<PathBuf> {
        self.store.save(session)
    }

    pub fn load(&self, session_id: &str) -> anyhow::Result<SessionState> {
        self.store.load(session_id)
    }

    pub fn list(&self) -> anyhow::Result<Vec<SessionState>> {
        self.store.list()
    }

    pub fn undo(&self, session: &mut SessionState) {
        if session.messages.pop().is_some()
            && session
                .messages
                .last()
                .map(|message| message.role.as_str() == "user")
                .unwrap_or(false)
        {
            session.messages.pop();
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConfigStore {
    pub path: PathBuf,
}

impl ConfigStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn load(&self) -> anyhow::Result<BTreeMap<String, Value>> {
        if !self.path.exists() {
            return Ok(BTreeMap::new());
        }
        let text = fs::read_to_string(&self.path)?;
        match serde_json::from_str(&text) {
            Ok(data) => Ok(data),
            Err(error) => {
                let Some(repaired) = repair_multiline_system_prompt_config(&text) else {
                    return Err(error.into());
                };
                self.save(&repaired)?;
                Ok(repaired)
            }
        }
    }

    pub fn save(&self, data: &BTreeMap<String, Value>) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_string_pretty(data)?)?;
        Ok(())
    }
}

fn repair_multiline_system_prompt_config(text: &str) -> Option<BTreeMap<String, Value>> {
    let prompt_start = Regex::new(r#""system_prompt"\s*:\s*""#)
        .ok()?
        .find(text)?
        .end();
    let prompt_end = Regex::new(r#""\s*}\s*$"#).ok()?.find(text)?.start();
    if prompt_end < prompt_start {
        return None;
    }
    let mut config = BTreeMap::new();
    if let Some(provider) = read_json_string_value(text, "current_provider") {
        config.insert("current_provider".to_string(), Value::String(provider));
    }
    if let Some(model) = read_json_string_value(text, "current_model") {
        config.insert("current_model".to_string(), Value::String(model));
    }
    config.insert(
        "system_prompt".to_string(),
        Value::String(text[prompt_start..prompt_end].to_string()),
    );
    Some(config)
}

fn read_json_string_value(text: &str, key: &str) -> Option<String> {
    let pattern = format!(r#""{}"\s*:\s*("(?:[^"\\]|\\.)*")"#, regex::escape(key));
    let raw = Regex::new(&pattern).ok()?.captures(text)?.get(1)?.as_str();
    serde_json::from_str(raw).ok()
}

#[derive(Clone, Debug)]
pub struct AuditLog {
    pub path: PathBuf,
}

impl AuditLog {
    pub fn new(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(Self {
            path: path.as_ref().to_path_buf(),
        })
    }

    pub fn append(&self, event: &AuditEvent) -> anyhow::Result<()> {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", serde_json::to_string(event)?)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProviderRegistry {
    providers: BTreeMap<String, ProviderConfig>,
}

impl ProviderRegistry {
    pub fn default_catalog() -> anyhow::Result<Self> {
        #[derive(Deserialize)]
        struct Catalog {
            providers: Vec<ProviderConfig>,
        }
        let catalog: Catalog = serde_json::from_str(include_str!("defaults/providers.json"))?;
        Ok(Self {
            providers: catalog
                .providers
                .into_iter()
                .map(|p| (p.name.clone(), p))
                .collect(),
        })
    }

    pub fn get(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut ProviderConfig> {
        self.providers.get_mut(name)
    }

    pub fn register(&mut self, provider: ProviderConfig) {
        self.providers.insert(provider.name.clone(), provider);
    }

    pub fn list(&self) -> Vec<&ProviderConfig> {
        self.providers.values().collect()
    }

    pub fn availability(&self) -> BTreeMap<String, bool> {
        self.providers
            .values()
            .map(|provider| {
                let ready = if provider.auth_type == "none" {
                    true
                } else if provider.name == "openai-sso" {
                    let auth_root = provider
                        .metadata
                        .get("auth_root")
                        .and_then(Value::as_str)
                        .map(PathBuf::from);
                    crate::openai_sso::OpenAISsoAuthStore::new(auth_root)
                        .load()
                        .ok()
                        .flatten()
                        .is_some()
                } else if provider.auth_type == "hbse" {
                    crate::provider::hbse_default_or_configured_socket(provider).exists()
                } else if provider.auth_type == "api_key"
                    && !crate::provider::direct_provider_auth_allowed()
                {
                    false
                } else {
                    provider
                        .api_key_env
                        .as_deref()
                        .and_then(crate::environment::get_env)
                        .is_some()
                };
                (provider.name.clone(), ready)
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ModelRegistry {
    ordered: Vec<ModelInfo>,
    models: BTreeMap<String, ModelInfo>,
}

impl ModelRegistry {
    pub fn register(&mut self, model: ModelInfo) {
        if !self.models.contains_key(&model.name) {
            self.ordered.push(model.clone());
        } else if let Some(existing) = self
            .ordered
            .iter_mut()
            .find(|existing| existing.name == model.name)
        {
            *existing = model.clone();
        }
        self.models.insert(model.name.clone(), model);
    }

    pub fn register_many(&mut self, models: Vec<ModelInfo>) {
        for model in models {
            self.register(model);
        }
    }

    pub fn replace_provider_models(&mut self, provider: &str, models: Vec<ModelInfo>) {
        self.models.retain(|_, model| model.provider != provider);
        self.ordered.retain(|model| model.provider != provider);
        self.register_many(models);
    }

    pub fn default_catalog() -> anyhow::Result<Self> {
        #[derive(Deserialize)]
        struct Catalog {
            models: Vec<ModelInfo>,
        }
        let catalog: Catalog = serde_json::from_str(include_str!("defaults/models.json"))?;
        let ordered = catalog.models.clone();
        Ok(Self {
            ordered,
            models: catalog
                .models
                .into_iter()
                .map(|m| (m.name.clone(), m))
                .collect(),
        })
    }

    pub fn get(&self, name: &str) -> Option<&ModelInfo> {
        self.models.get(name)
    }

    pub fn by_provider(&self, provider: &str) -> Vec<&ModelInfo> {
        self.ordered
            .iter()
            .filter(|model| model.enabled && self.is_model_allowed_for_provider(model, provider))
            .collect()
    }

    pub fn list(&self) -> Vec<&ModelInfo> {
        self.ordered.iter().filter(|model| model.enabled).collect()
    }

    pub fn default_for_provider(&self, provider: &str) -> Option<&ModelInfo> {
        self.by_provider(provider).into_iter().next()
    }

    pub fn is_model_allowed_for_provider(&self, model: &ModelInfo, provider: &str) -> bool {
        model.provider == provider
            || (provider == "openai-sso" && model.provider == "openai")
            || (provider == "azure-openai-hbse" && model.provider == "azure-openai")
            || provider
                .strip_suffix("-hbse")
                .is_some_and(|base_provider| model.provider == base_provider)
    }
}

pub fn default_tool_definitions() -> anyhow::Result<Vec<ToolDefinition>> {
    #[derive(Deserialize)]
    struct Catalog {
        tools: Vec<ToolDefinition>,
    }
    Ok(serde_json::from_str::<Catalog>(include_str!("defaults/tools.json"))?.tools)
}

pub fn default_system_prompt() -> String {
    include_str!("defaults/default_system_prompt.md")
        .trim()
        .to_string()
}

pub fn default_skill_definitions() -> anyhow::Result<Vec<SkillDefinition>> {
    #[derive(Deserialize)]
    struct Catalog {
        skills: Vec<SkillDefinition>,
    }
    Ok(serde_json::from_str::<Catalog>(include_str!("defaults/skills.json"))?.skills)
}

pub fn load_skill_definitions(
    cwd: impl AsRef<Path>,
    data_root: impl AsRef<Path>,
) -> anyhow::Result<Vec<SkillDefinition>> {
    let mut skills = default_skill_definitions()?;
    let mut seen = skills
        .iter()
        .map(|skill| skill.name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for root in [
        cwd.as_ref().join(".vegvisir").join("skills"),
        cwd.as_ref().join("skills"),
        data_root.as_ref().join("skills"),
    ] {
        for skill in load_filesystem_skills(&root)? {
            if seen.insert(skill.name.clone()) {
                skills.push(skill);
            }
        }
    }
    Ok(skills)
}

fn load_filesystem_skills(root: &Path) -> anyhow::Result<Vec<SkillDefinition>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut skills = Vec::new();
    collect_filesystem_skills(root, root, &mut skills)?;
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(skills)
}

fn collect_filesystem_skills(
    root: &Path,
    current: &Path,
    skills: &mut Vec<SkillDefinition>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_filesystem_skills(root, &path, skills)?;
            continue;
        }
        let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        let extension = extension.to_ascii_lowercase();
        if extension != "md" && extension != "usrl" && extension != "lsl" {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        let format = match extension.as_str() {
            "usrl" => "usrl",
            "lsl" => "lsl",
            _ => "markdown",
        };
        if format == "lsl" {
            let library = parse_lsl(&content).with_context(|| {
                format!("failed to parse linked skill library {}", path.display())
            })?;
            for subskill in &library.subskills {
                let mut metadata =
                    subskill_metadata(&library, subskill, &path.display().to_string(), &content);
                metadata.insert("format".to_string(), Value::String("lsl".to_string()));
                skills.push(SkillDefinition {
                    name: subskill.id.clone(),
                    category: format!(
                        "linked-skill/{}",
                        subskill.skill_type.as_deref().unwrap_or("subskill")
                    ),
                    description: subskill
                        .summary
                        .clone()
                        .or_else(|| subskill.title.clone())
                        .unwrap_or_else(|| {
                            format!("linked sub-skill loaded from {}", path.display())
                        }),
                    kind: "lsl_subskill".to_string(),
                    enabled: subskill.status.as_deref() != Some("archived"),
                    metadata,
                });
            }
            continue;
        }
        let name = filesystem_skill_name(root, &path, format);
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "path".to_string(),
            Value::String(path.display().to_string()),
        );
        metadata.insert("format".to_string(), Value::String(format.to_string()));
        metadata.insert("body".to_string(), Value::String(content.clone()));
        if format == "usrl" {
            attach_usrl_skill_metadata(&mut metadata, &content, &path)?;
        }
        skills.push(SkillDefinition {
            name,
            category: if format == "usrl" {
                "regulated-workflow".to_string()
            } else {
                "filesystem".to_string()
            },
            description: skill_description_from_content(&content)
                .unwrap_or_else(|| format!("{format} skill loaded from {}", path.display())),
            kind: if format == "usrl" {
                "usrl_contract".to_string()
            } else {
                "markdown".to_string()
            },
            enabled: true,
            metadata,
        });
    }
    Ok(())
}

fn attach_usrl_skill_metadata(
    metadata: &mut BTreeMap<String, Value>,
    content: &str,
    path: &Path,
) -> anyhow::Result<()> {
    let summary = cms_v2::usrl::summarize_usrl(content);
    metadata.insert(
        "usrl_contracts".to_string(),
        json_string_array(summary.contracts),
    );
    metadata.insert(
        "usrl_sections".to_string(),
        json_string_array(summary.sections),
    );
    metadata.insert("usrl_facts".to_string(), json_string_array(summary.facts));
    metadata.insert("usrl_rules".to_string(), json_string_array(summary.rules));
    metadata.insert(
        "usrl_constraints".to_string(),
        json_string_array(summary.constraints),
    );
    metadata.insert("usrl_stages".to_string(), json_string_array(summary.stages));
    metadata.insert(
        "usrl_triggers".to_string(),
        json_string_array(summary.triggers),
    );
    metadata.insert(
        "usrl_validator".to_string(),
        Value::String("cms-v2-lightweight".to_string()),
    );
    metadata.insert(
        "usrl_validation_status".to_string(),
        Value::String("not_requested".to_string()),
    );
    if let Some(root) = std::env::var_os("VEGVISIR_USRL_VALIDATOR_ROOT").map(PathBuf::from) {
        let options = cms_v2::usrl::UsrlImportOptions {
            validator_root: Some(root),
            ..cms_v2::usrl::UsrlImportOptions::default()
        };
        let report = cms_v2::usrl::validate_usrl_file(path, &options)?;
        metadata.insert(
            "usrl_validator".to_string(),
            Value::String(report.validator.clone()),
        );
        metadata.insert(
            "usrl_validation_status".to_string(),
            Value::String(format!("{:?}", report.status).to_ascii_lowercase()),
        );
        metadata.insert(
            "usrl_validation_issue_count".to_string(),
            Value::Number(report.issues.len().into()),
        );
        if !report.issues.is_empty() {
            metadata.insert(
                "usrl_validation_issues".to_string(),
                json_string_array(report.issues),
            );
        }
    }
    Ok(())
}

fn json_string_array(values: Vec<String>) -> Value {
    Value::Array(values.into_iter().map(Value::String).collect())
}

fn filesystem_skill_name(root: &Path, path: &Path, format: &str) -> String {
    if format == "markdown"
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.eq_ignore_ascii_case("skill.md"))
            .unwrap_or(false)
        && let Some(parent) = path.parent().and_then(|parent| parent.file_name())
    {
        return normalize_agent_id(&parent.to_string_lossy());
    }
    let name = path
        .strip_prefix(root)
        .ok()
        .and_then(|path| path.with_extension("").to_str().map(str::to_string))
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("skill")
                .to_string()
        })
        .replace(std::path::MAIN_SEPARATOR, "-");
    normalize_agent_id(&name)
}

fn skill_description_from_content(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("---"))
        .map(|line| line.trim_start_matches('#').trim().to_string())
        .filter(|line| !line.is_empty())
}

fn default_true() -> bool {
    true
}
fn default_kind() -> String {
    "demo".to_string()
}
fn default_auth_type() -> String {
    "api_key".to_string()
}

fn default_skill_kind() -> String {
    "catalog".to_string()
}

fn default_agent_memory_scope() -> String {
    "agent".to_string()
}

fn default_agent_mode() -> String {
    "custom".to_string()
}

pub fn normalize_agent_id(value: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_dash = false;
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash && !normalized.is_empty() {
            normalized.push('-');
            last_was_dash = true;
        }
    }
    while normalized.ends_with('-') {
        normalized.pop();
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::load_skill_definitions;

    #[test]
    fn loads_lsl_subskills_from_workspace_skills() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let skills_dir = tmp.path().join("skills");
        std::fs::create_dir_all(&skills_dir)?;
        std::fs::write(
            skills_dir.join("cryptography.lsl"),
            r#"
            library cryptography {
                meta {
                    id: "cryptography";
                    name: "Cryptography";
                    version: "1.0.0";
                    status: active;
                    risk: high;
                }

                subskill cryptography.secure_randomness {
                    id: cryptography.secure_randomness;
                    title: "Secure Randomness";
                    summary: "CSPRNG and nonce safety.";
                    type: procedure;
                    risk: medium;
                    tags: [rng, nonce];
                    load {
                        card: """Use a CSPRNG.""";
                        body: """Use operating-system cryptographic randomness.""";
                    }
                    verification: ["Random source is cryptographic."];
                }
            }
            "#,
        )?;

        let skills = load_skill_definitions(tmp.path(), tmp.path().join("data"))?;
        let skill = skills
            .iter()
            .find(|skill| skill.name == "cryptography.secure_randomness")
            .expect("lsl subskill loaded");

        assert_eq!(skill.kind, "lsl_subskill");
        assert_eq!(skill.category, "linked-skill/procedure");
        assert_eq!(
            skill
                .metadata
                .get("library_id")
                .and_then(|value| value.as_str()),
            Some("cryptography")
        );
        assert!(
            skill
                .metadata
                .get("body")
                .and_then(|value| value.as_str())
                .unwrap()
                .contains("Use operating-system cryptographic randomness.")
        );
        Ok(())
    }
}
