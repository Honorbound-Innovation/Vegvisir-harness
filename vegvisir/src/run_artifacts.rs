use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use cms_v2::prompt_cache::CachedPromptEnvelope;

use crate::provider::ProviderRunEvent;

pub const RUN_ARTIFACT_SCHEMA_VERSION: u32 = 1;
const REDACTION: &str = "[REDACTED]";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunManifest {
    pub schema_version: u32,
    pub run_id: String,
    pub session_id: String,
    pub workspace: PathBuf,
    pub provider: String,
    pub model: String,
    pub agent: Option<String>,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub artifact_paths: BTreeMap<String, PathBuf>,
}

impl RunManifest {
    pub fn new(
        run_id: impl Into<String>,
        session_id: impl Into<String>,
        workspace: impl Into<PathBuf>,
        provider: impl Into<String>,
        model: impl Into<String>,
        agent: Option<String>,
    ) -> Self {
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id: run_id.into(),
            session_id: session_id.into(),
            workspace: workspace.into(),
            provider: provider.into(),
            model: model.into(),
            agent,
            status: RunStatus::Running,
            started_at: Utc::now(),
            finished_at: None,
            artifact_paths: default_artifact_paths(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunArtifactManager {
    pub workspace: PathBuf,
    pub data_root: PathBuf,
    pub run_id: String,
    pub run_dir: PathBuf,
}

impl RunArtifactManager {
    pub fn start(
        workspace: impl AsRef<Path>,
        data_root: impl AsRef<Path>,
        session_id: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
        agent: Option<String>,
    ) -> anyhow::Result<(Self, RunManifest)> {
        Self::start_in(
            workspace,
            data_root,
            None::<PathBuf>,
            session_id,
            provider,
            model,
            agent,
        )
    }

    pub fn start_in(
        workspace: impl AsRef<Path>,
        data_root: impl AsRef<Path>,
        artifact_root: Option<impl AsRef<Path>>,
        session_id: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
        agent: Option<String>,
    ) -> anyhow::Result<(Self, RunManifest)> {
        Self::start_with_run_id(
            workspace,
            data_root,
            new_run_id(),
            artifact_root,
            session_id,
            provider,
            model,
            agent,
        )
    }

    pub fn start_with_run_id(
        workspace: impl AsRef<Path>,
        data_root: impl AsRef<Path>,
        run_id: impl Into<String>,
        artifact_root: Option<impl AsRef<Path>>,
        session_id: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
        agent: Option<String>,
    ) -> anyhow::Result<(Self, RunManifest)> {
        let workspace = workspace.as_ref().to_path_buf();
        let data_root = data_root.as_ref().to_path_buf();
        let run_id = run_id.into();
        let run_dir = artifact_root
            .as_ref()
            .map(|root| root.as_ref().join(&run_id))
            .unwrap_or_else(|| workspace.join(".vegvisir").join("runs").join(&run_id));
        fs::create_dir_all(&run_dir)?;

        let manager = Self {
            workspace: workspace.clone(),
            data_root,
            run_id: run_id.clone(),
            run_dir,
        };
        let manifest = RunManifest::new(run_id, session_id, workspace, provider, model, agent);
        manager.write_manifest(&manifest)?;
        Ok((manager, manifest))
    }

    pub fn from_existing(
        workspace: impl Into<PathBuf>,
        data_root: impl Into<PathBuf>,
        run_id: impl Into<String>,
        run_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            workspace: workspace.into(),
            data_root: data_root.into(),
            run_id: run_id.into(),
            run_dir: run_dir.into(),
        }
    }

    pub fn artifact_path(&self, name: &str) -> PathBuf {
        self.run_dir.join(name)
    }

    pub fn write_manifest(&self, manifest: &RunManifest) -> anyhow::Result<()> {
        self.write_json_file("manifest.json", manifest)
    }

    pub fn write_request(&self, request: &Value) -> anyhow::Result<()> {
        self.write_json_value_file("request.json", request)
    }

    pub fn write_result(&self, markdown: &str) -> anyhow::Result<()> {
        self.write_text_file("result.md", markdown)
    }

    pub fn write_context(&self, markdown: &str) -> anyhow::Result<()> {
        self.write_text_file("context.md", markdown)
    }

    pub fn write_context_sources(&self, sources: &Value) -> anyhow::Result<()> {
        self.write_json_value_file("context-sources.json", sources)
    }

    pub fn write_context_artifacts(&self, envelope: &CachedPromptEnvelope) -> anyhow::Result<()> {
        self.write_context(&envelope.model_request.prompt)?;
        self.write_context_sources(&context_sources_from_envelope(envelope))
    }

    pub fn write_failure(&self, failure: &RunFailure) -> anyhow::Result<()> {
        self.write_json_file("failure.json", failure)
    }

    pub fn write_verification(&self, verification: &Value) -> anyhow::Result<()> {
        self.write_json_value_file("verification.json", verification)
    }

    pub fn append_provider_event(&self, event: &ProviderRunEvent) -> anyhow::Result<()> {
        self.append_jsonl_value(
            "provider-events.jsonl",
            &json!({
                "schema_version": RUN_ARTIFACT_SCHEMA_VERSION,
                "timestamp": Utc::now(),
                "run_id": self.run_id,
                "event": event,
            }),
        )
    }

    pub fn append_observed_provider_event(&self, event: &ProviderRunEvent) -> anyhow::Result<()> {
        self.append_provider_event(event)?;
        match event {
            ProviderRunEvent::ToolStart { name, args } => self.append_tool_event(
                &ToolRunEvent::start(self.run_id.clone(), name.clone(), Some(args.clone()), None),
            ),
            ProviderRunEvent::ToolEnd {
                name, ok, summary, ..
            } => self.append_tool_event(&ToolRunEvent::end(
                self.run_id.clone(),
                name.clone(),
                *ok,
                summary.clone(),
                None,
            )),
            ProviderRunEvent::Activity(_) => Ok(()),
        }
    }

    pub fn append_tool_event(&self, event: &ToolRunEvent) -> anyhow::Result<()> {
        self.append_jsonl_value("tool-events.jsonl", &serde_json::to_value(event)?)
    }

    pub fn finish(&self, manifest: &mut RunManifest, status: RunStatus) -> anyhow::Result<()> {
        manifest.status = status;
        manifest.finished_at = Some(Utc::now());
        self.write_manifest(manifest)
    }

    pub fn fail(
        &self,
        manifest: &mut RunManifest,
        message: impl Into<String>,
        recoverable: bool,
    ) -> anyhow::Result<()> {
        let failure = RunFailure {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id: self.run_id.clone(),
            message: message.into(),
            recoverable,
            timestamp: Utc::now(),
        };
        self.write_failure(&failure)?;
        self.finish(manifest, RunStatus::Failed)
    }

    fn write_json_file<T: Serialize>(&self, name: &str, value: &T) -> anyhow::Result<()> {
        let value = serde_json::to_value(value)?;
        self.write_json_value_file(name, &value)
    }

    fn write_json_value_file(&self, name: &str, value: &Value) -> anyhow::Result<()> {
        fs::create_dir_all(&self.run_dir)?;
        let redacted = redact_json_value(value);
        let bytes = serde_json::to_vec_pretty(&redacted)?;
        fs::write(self.artifact_path(name), bytes)?;
        Ok(())
    }

    fn write_text_file(&self, name: &str, text: &str) -> anyhow::Result<()> {
        fs::create_dir_all(&self.run_dir)?;
        fs::write(self.artifact_path(name), redact_text(text))?;
        Ok(())
    }

    fn append_jsonl_value(&self, name: &str, value: &Value) -> anyhow::Result<()> {
        fs::create_dir_all(&self.run_dir)?;
        let redacted = redact_json_value(value);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.artifact_path(name))?;
        writeln!(file, "{}", serde_json::to_string(&redacted)?)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFailure {
    pub schema_version: u32,
    pub run_id: String,
    pub message: String,
    pub recoverable: bool,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRunEvent {
    pub schema_version: u32,
    pub timestamp: DateTime<Utc>,
    pub run_id: String,
    pub tool_name: String,
    pub phase: ToolRunPhase,
    pub args_summary: Option<String>,
    pub ok: Option<bool>,
    pub summary: Option<String>,
    pub approval_id: Option<String>,
}

impl ToolRunEvent {
    pub fn start(
        run_id: impl Into<String>,
        tool_name: impl Into<String>,
        args_summary: Option<String>,
        approval_id: Option<String>,
    ) -> Self {
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            timestamp: Utc::now(),
            run_id: run_id.into(),
            tool_name: tool_name.into(),
            phase: ToolRunPhase::Start,
            args_summary,
            ok: None,
            summary: None,
            approval_id,
        }
    }

    pub fn end(
        run_id: impl Into<String>,
        tool_name: impl Into<String>,
        ok: bool,
        summary: impl Into<String>,
        approval_id: Option<String>,
    ) -> Self {
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            timestamp: Utc::now(),
            run_id: run_id.into(),
            tool_name: tool_name.into(),
            phase: ToolRunPhase::End,
            args_summary: None,
            ok: Some(ok),
            summary: Some(summary.into()),
            approval_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRunPhase {
    Start,
    End,
}

pub fn context_sources_from_envelope(envelope: &CachedPromptEnvelope) -> Value {
    json!({
        "schema_version": RUN_ARTIFACT_SCHEMA_VERSION,
        "manifest": {
            "manifest_id": envelope.manifest.manifest_id,
            "provider": envelope.manifest.provider,
            "model": envelope.manifest.model,
            "prompt_cache_key": envelope.manifest.prompt_cache_key,
            "cacheable_prefix_hash": envelope.manifest.cacheable_prefix_hash,
            "cacheable_prefix_tokens": envelope.manifest.cacheable_prefix_tokens,
            "total_prompt_tokens": envelope.manifest.total_prompt_tokens,
            "renderer_version": envelope.manifest.renderer_version,
            "tokenizer_version": envelope.manifest.tokenizer_version,
            "scope_identity": envelope.manifest.scope_identity,
            "block_hashes": envelope.manifest.block_hashes,
        },
        "model_request": {
            "provider": envelope.model_request.provider,
            "model": envelope.model_request.model,
            "cache_hint": envelope.model_request.cache_hint,
            "metadata": envelope.model_request.metadata,
        },
        "blocks": envelope.blocks.iter().map(|block| json!({
            "id": block.id,
            "kind": block.kind,
            "zone": block.zone,
            "title": block.title,
            "content_hash": block.content_hash,
            "token_estimate": block.token_estimate,
            "source_memory_ids": block.source_memory_ids,
            "source_version_hashes": block.source_version_hashes,
            "stability": block.stability,
            "scope": block.scope,
            "sensitivity": block.sensitivity,
            "cache_policy": block.cache_policy,
            "provider_annotations": block.provider_annotations,
        })).collect::<Vec<_>>(),
        "capsules": envelope.capsules.iter().map(|capsule| json!({
            "capsule_id": capsule.capsule_id,
            "capsule_type": capsule.capsule_type,
            "scope": capsule.scope,
            "scope_identity": capsule.scope_identity,
            "content_hash": capsule.content_hash,
            "token_estimate": capsule.token_estimate,
            "source_memory_ids": capsule.source_memory_ids,
            "source_version_hashes": capsule.source_version_hashes,
            "block_ids": capsule.block_ids,
            "renderer_version": capsule.renderer_version,
        })).collect::<Vec<_>>(),
        "cache_plan": envelope.cache_plan,
    })
}

fn default_artifact_paths() -> BTreeMap<String, PathBuf> {
    BTreeMap::from([
        ("manifest".to_string(), PathBuf::from("manifest.json")),
        ("request".to_string(), PathBuf::from("request.json")),
        ("context".to_string(), PathBuf::from("context.md")),
        (
            "context_sources".to_string(),
            PathBuf::from("context-sources.json"),
        ),
        (
            "provider_events".to_string(),
            PathBuf::from("provider-events.jsonl"),
        ),
        (
            "tool_events".to_string(),
            PathBuf::from("tool-events.jsonl"),
        ),
        (
            "file_changes".to_string(),
            PathBuf::from("file-changes.json"),
        ),
        ("diff".to_string(), PathBuf::from("diff.patch")),
        ("memory_used".to_string(), PathBuf::from("memory-used.json")),
        (
            "memory_written".to_string(),
            PathBuf::from("memory-written.json"),
        ),
        ("approvals".to_string(), PathBuf::from("approvals.json")),
        ("subagents".to_string(), PathBuf::from("subagents.json")),
        ("result".to_string(), PathBuf::from("result.md")),
        (
            "verification".to_string(),
            PathBuf::from("verification.json"),
        ),
        ("failure".to_string(), PathBuf::from("failure.json")),
    ])
}

fn new_run_id() -> String {
    format!("run-{}", Uuid::new_v4())
}

pub fn redact_json_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let redacted_value = if is_sensitive_key(key) {
                        Value::String(REDACTION.to_string())
                    } else {
                        redact_json_value(value)
                    };
                    (key.clone(), redacted_value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_json_value).collect()),
        Value::String(text) => Value::String(redact_text(text)),
        _ => value.clone(),
    }
}

pub fn redact_text(text: &str) -> String {
    let mut redacted = String::with_capacity(text.len());
    let mut token = String::new();

    for ch in text.chars() {
        if ch.is_whitespace() {
            if !token.is_empty() {
                push_redacted_token(&mut redacted, &token);
                token.clear();
            }
            redacted.push(ch);
        } else {
            token.push(ch);
        }
    }
    if !token.is_empty() {
        push_redacted_token(&mut redacted, &token);
    }

    redacted
}

fn push_redacted_token(output: &mut String, token: &str) {
    if looks_secret_like(token) {
        output.push_str(REDACTION);
    } else {
        output.push_str(token);
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "api_key",
        "apikey",
        "authorization",
        "auth",
        "bearer",
        "client_secret",
        "credential",
        "password",
        "private_key",
        "secret",
        "token",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn looks_secret_like(part: &str) -> bool {
    let trimmed = part.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | '.'
                | ';'
                | ':'
                | '!'
                | '?'
                | '\''
                | '"'
                | '`'
                | ')'
                | '('
                | ']'
                | '['
                | '}'
                | '{'
        )
    });
    if trimmed.len() < 20 {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("sk-")
        || lower.starts_with("xoxb-")
        || lower.starts_with("ghp_")
        || lower.starts_with("github_pat_")
        || lower.starts_with("bearer")
        || lower.contains("api_key=")
        || lower.contains("token=")
        || lower.contains("password=")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn run_artifacts_creates_manifest_and_result() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path().join("workspace");
        let data_root = tmp.path().join("data");
        fs::create_dir_all(&workspace)?;

        let (manager, mut manifest) = RunArtifactManager::start(
            &workspace,
            &data_root,
            "session-1",
            "demo",
            "demo-model",
            Some("tester".to_string()),
        )?;
        manager.write_result("completed successfully")?;
        manager.finish(&mut manifest, RunStatus::Completed)?;

        assert!(manager.artifact_path("manifest.json").exists());
        assert!(manager.artifact_path("result.md").exists());

        let saved: RunManifest =
            serde_json::from_str(&fs::read_to_string(manager.artifact_path("manifest.json"))?)?;
        assert_eq!(saved.schema_version, RUN_ARTIFACT_SCHEMA_VERSION);
        assert_eq!(saved.status, RunStatus::Completed);
        assert_eq!(saved.session_id, "session-1");
        assert_eq!(saved.provider, "demo");
        assert_eq!(saved.model, "demo-model");
        assert_eq!(saved.agent.as_deref(), Some("tester"));
        assert!(saved.finished_at.is_some());
        assert_eq!(
            fs::read_to_string(manager.artifact_path("result.md"))?,
            "completed successfully"
        );
        Ok(())
    }

    #[test]
    fn run_artifacts_writes_context_sources_from_prompt_envelope() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let (manager, _manifest) = RunArtifactManager::start(
            &workspace,
            tmp.path().join("data"),
            "session-1",
            "demo",
            "demo-model",
            None,
        )?;
        let mut cms = crate::memory::VegvisirCms::open(crate::memory::VegvisirCmsConfig {
            db_path: tmp.path().join("cms.sqlite3"),
            user_id: "tester".to_string(),
            project_id: Some("project".to_string()),
            context_mode: cms_v2::ecm::ContextMode::Project,
            commit_writebacks: true,
        })?;
        cms.remember(
            "ArchitectureChange",
            "Artifact context evidence",
            "Decision: run artifacts should persist ECM context evidence.",
        )?;
        let envelope = cms.prepare_cached_prompt(
            "Use Artifact context evidence in this run.",
            "demo",
            "demo-model",
        )?;

        manager.write_context_artifacts(&envelope)?;

        let context = fs::read_to_string(manager.artifact_path("context.md"))?;
        assert!(context.contains("Artifact context evidence"));
        let sources: Value = serde_json::from_str(&fs::read_to_string(
            manager.artifact_path("context-sources.json"),
        )?)?;
        assert_eq!(sources["schema_version"], RUN_ARTIFACT_SCHEMA_VERSION);
        assert!(!sources["blocks"].as_array().unwrap().is_empty());
        assert_eq!(sources["model_request"]["provider"], "demo");
        Ok(())
    }

    #[test]
    fn run_artifacts_appends_jsonl_events() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let (manager, _manifest) = RunArtifactManager::start(
            &workspace,
            tmp.path().join("data"),
            "session-1",
            "demo",
            "demo-model",
            None,
        )?;

        manager.append_provider_event(&ProviderRunEvent::Activity("thinking".to_string()))?;
        manager.append_tool_event(&ToolRunEvent::start(
            manager.run_id.clone(),
            "read_file",
            Some(r#"{"path":"src/lib.rs"}"#.to_string()),
            None,
        ))?;

        let provider_events = fs::read_to_string(manager.artifact_path("provider-events.jsonl"))?;
        assert_eq!(provider_events.lines().count(), 1);
        assert!(provider_events.contains("activity"));
        assert!(provider_events.contains("thinking"));

        let tool_events = fs::read_to_string(manager.artifact_path("tool-events.jsonl"))?;
        assert_eq!(tool_events.lines().count(), 1);
        assert!(tool_events.contains("read_file"));
        assert!(tool_events.contains("src/lib.rs"));
        Ok(())
    }

    #[test]
    fn run_artifacts_finalizes_failed_run() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let (manager, mut manifest) = RunArtifactManager::start(
            &workspace,
            tmp.path().join("data"),
            "session-1",
            "demo",
            "demo-model",
            None,
        )?;

        manager.fail(&mut manifest, "provider timed out", true)?;

        let saved: RunManifest =
            serde_json::from_str(&fs::read_to_string(manager.artifact_path("manifest.json"))?)?;
        assert_eq!(saved.status, RunStatus::Failed);
        assert!(saved.finished_at.is_some());

        let failure: RunFailure =
            serde_json::from_str(&fs::read_to_string(manager.artifact_path("failure.json"))?)?;
        assert_eq!(failure.message, "provider timed out");
        assert!(failure.recoverable);
        Ok(())
    }

    #[test]
    fn text_redaction_preserves_non_secret_whitespace() {
        let redacted =
            redact_text("line one\nline two token=github_pat_123456789012345678901234\nline three");
        assert!(redacted.contains("line one\nline two "));
        assert!(redacted.contains("\nline three"));
        assert!(redacted.contains(REDACTION));
        assert!(!redacted.contains("github_pat_123"));
    }

    #[test]
    fn run_artifacts_redacts_secret_like_values() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let (manager, _manifest) = RunArtifactManager::start(
            &workspace,
            tmp.path().join("data"),
            "session-1",
            "demo",
            "demo-model",
            None,
        )?;

        manager.write_json_value_file(
            "request.json",
            &json!({
                "Authorization": "Bearer sk-123456789012345678901234567890",
                "nested": {"api_key": "sk-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
                "safe": "hello world"
            }),
        )?;
        manager.write_result("token=github_pat_123456789012345678901234 should be hidden")?;

        let request = fs::read_to_string(manager.artifact_path("request.json"))?;
        assert!(request.contains(REDACTION));
        assert!(request.contains("hello world"));
        assert!(!request.contains("sk-123456"));
        assert!(!request.contains("sk-aaaaaaaa"));

        let result = fs::read_to_string(manager.artifact_path("result.md"))?;
        assert!(result.contains(REDACTION));
        assert!(!result.contains("github_pat_123"));
        Ok(())
    }
}
