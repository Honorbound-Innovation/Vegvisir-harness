use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use cms_v2::{cms_api::CommitResult, prompt_cache::CachedPromptEnvelope};

use crate::{
    guardrails::ApprovalRequest,
    provider::ProviderRunEvent,
    subagents::{SubAgentStatus, SubAgentTaskRecord},
};

pub const RUN_ARTIFACT_SCHEMA_VERSION: u32 = 1;
const REDACTION: &str = "[REDACTED]";
const MAX_FILE_CHANGE_ENTRIES: usize = 200;
const MAX_WORKSPACE_DIFF_BYTES: usize = 262_144;

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
        self.write_context_sources(&context_sources_from_envelope(envelope))?;
        self.write_memory_used(envelope)
    }

    pub fn write_memory_used(&self, envelope: &CachedPromptEnvelope) -> anyhow::Result<()> {
        let evidence = RunMemoryUseEvidence::from_envelope(self.run_id.clone(), envelope);
        self.write_json_file("memory-used.json", &evidence)
    }

    pub fn write_memory_written_from_results(
        &self,
        results: &[CommitResult],
    ) -> anyhow::Result<()> {
        let evidence = RunMemoryWriteEvidence::from_results(self.run_id.clone(), results);
        self.write_json_file("memory-written.json", &evidence)
    }

    pub fn write_memory_written_from_outcome(
        &self,
        results: &[CommitResult],
        error: Option<&str>,
    ) -> anyhow::Result<()> {
        let evidence = RunMemoryWriteEvidence::from_outcome(self.run_id.clone(), results, error);
        self.write_json_file("memory-written.json", &evidence)
    }

    pub fn write_memory_written_unavailable(&self, note: impl Into<String>) -> anyhow::Result<()> {
        let evidence = RunMemoryWriteEvidence::unavailable(self.run_id.clone(), note);
        self.write_json_file("memory-written.json", &evidence)
    }

    pub fn write_approvals_from_pending(
        &self,
        pending: &BTreeMap<String, ApprovalRequest>,
    ) -> anyhow::Result<()> {
        let evidence = RunApprovalEvidence::from_pending(self.run_id.clone(), pending);
        self.write_json_file("approvals.json", &evidence)
    }

    pub fn write_approvals_unavailable(&self, note: impl Into<String>) -> anyhow::Result<()> {
        let evidence = RunApprovalEvidence::unavailable(self.run_id.clone(), note);
        self.write_json_file("approvals.json", &evidence)
    }

    pub fn write_subagents_from_records(
        &self,
        records: &[SubAgentTaskRecord],
    ) -> anyhow::Result<()> {
        let evidence = RunSubagentEvidence::from_records(self.run_id.clone(), records);
        self.write_json_file("subagents.json", &evidence)
    }

    pub fn write_subagents_unavailable(&self, note: impl Into<String>) -> anyhow::Result<()> {
        let evidence = RunSubagentEvidence::unavailable(self.run_id.clone(), note);
        self.write_json_file("subagents.json", &evidence)
    }

    pub fn write_subagents_from_board(&self) -> anyhow::Result<()> {
        let path = self.data_root.join("subagents.json");
        if !path.exists() {
            return self.write_json_file(
                "subagents.json",
                &RunSubagentEvidence::no_subagents(
                    self.run_id.clone(),
                    "no subagent board was present for this run",
                ),
            );
        }
        match fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|text| {
                serde_json::from_str::<Vec<SubAgentTaskRecord>>(&text).map_err(Into::into)
            }) {
            Ok(records) => self.write_subagents_from_records(&records),
            Err(error) => self.write_subagents_unavailable(format!(
                "subagent board could not be read from {}: {error}",
                path.display()
            )),
        }
    }

    pub fn write_failure(&self, failure: &RunFailure) -> anyhow::Result<()> {
        self.write_json_file("failure.json", failure)
    }

    pub fn write_workspace_file_changes(&self) -> anyhow::Result<()> {
        let evidence =
            WorkspaceFileChangeEvidence::capture(self.run_id.clone(), self.workspace.clone());
        self.write_json_file("file-changes.json", &evidence)
    }

    pub fn write_workspace_diff(&self) -> anyhow::Result<()> {
        self.write_text_file("diff.patch", &capture_workspace_diff(&self.workspace))
    }

    pub fn write_workspace_change_artifacts(&self) -> anyhow::Result<()> {
        self.write_workspace_file_changes()?;
        self.write_workspace_diff()
    }

    pub fn write_verification(&self, verification: &Value) -> anyhow::Result<()> {
        self.write_json_value_file("verification.json", verification)
    }

    pub fn write_verification_evidence(
        &self,
        verification: &RunVerificationEvidence,
    ) -> anyhow::Result<()> {
        self.write_json_file("verification.json", verification)
    }

    pub fn write_verification_from_provider_events(
        &self,
        events: &[ProviderRunEvent],
    ) -> anyhow::Result<()> {
        let evidence = RunVerificationEvidence::from_provider_events(self.run_id.clone(), events);
        self.write_verification_evidence(&evidence)
    }

    pub fn write_verification_no_checks(&self, note: impl Into<String>) -> anyhow::Result<()> {
        self.write_verification_evidence(&RunVerificationEvidence::no_verification(
            self.run_id.clone(),
            note,
        ))
    }

    pub fn write_verification_unavailable(&self, note: impl Into<String>) -> anyhow::Result<()> {
        self.write_verification_evidence(&RunVerificationEvidence::unavailable(
            self.run_id.clone(),
            note,
        ))
    }

    pub fn write_verification_if_absent(&self, status: &RunStatus) -> anyhow::Result<()> {
        if self.artifact_path("verification.json").exists() {
            return Ok(());
        }
        match status {
            RunStatus::Failed | RunStatus::Cancelled => self.write_verification_unavailable(
                "run ended before verification evidence was captured",
            ),
            RunStatus::Running | RunStatus::Completed => self.write_verification_no_checks(
                "no verification command or test tool evidence was captured for this run",
            ),
        }
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
            )?,
            ProviderRunEvent::ToolEnd {
                name, ok, summary, ..
            } => self.append_tool_event(&ToolRunEvent::end(
                self.run_id.clone(),
                name.clone(),
                *ok,
                summary.clone(),
                None,
            ))?,
            ProviderRunEvent::Activity(_) => {}
        }
        self.record_verification_from_provider_event(event)
    }

    pub fn append_tool_event(&self, event: &ToolRunEvent) -> anyhow::Result<()> {
        self.append_jsonl_value("tool-events.jsonl", &serde_json::to_value(event)?)
    }

    pub fn finish(&self, manifest: &mut RunManifest, status: RunStatus) -> anyhow::Result<()> {
        self.write_verification_if_absent(&status)?;
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
        self.write_memory_written_unavailable(
            "run failed before completion memory writeback was captured",
        )?;
        if !self.artifact_path("approvals.json").exists() {
            self.write_approvals_unavailable("approval ledger was not supplied for failed run")?;
        }
        if !self.artifact_path("subagents.json").exists() {
            self.write_subagents_from_board()?;
        }
        self.write_workspace_change_artifacts()?;
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

    fn record_verification_from_provider_event(
        &self,
        event: &ProviderRunEvent,
    ) -> anyhow::Result<()> {
        let Some(check) = RunVerificationCheck::from_provider_event(event, None) else {
            return Ok(());
        };
        let mut evidence = self
            .read_verification_evidence()
            .unwrap_or_else(|| RunVerificationEvidence::captured(self.run_id.clone(), Vec::new()));
        evidence.status = RunVerificationStatus::Captured;
        evidence.captured_at = Utc::now();
        evidence
            .notes
            .retain(|note| !note.contains("no verification command"));
        evidence.checks.push(check);
        evidence.overall = verification_overall(&evidence.checks);
        self.write_verification_evidence(&evidence)
    }

    fn read_verification_evidence(&self) -> Option<RunVerificationEvidence> {
        let path = self.artifact_path("verification.json");
        let text = fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceFileChangeEvidence {
    pub schema_version: u32,
    pub run_id: String,
    pub workspace: PathBuf,
    pub captured_at: DateTime<Utc>,
    pub status: WorkspaceFileChangeStatus,
    pub changes: Vec<WorkspaceFileChange>,
    pub truncated: bool,
    pub error: Option<String>,
}

impl WorkspaceFileChangeEvidence {
    pub fn capture(run_id: String, workspace: PathBuf) -> Self {
        let captured_at = Utc::now();
        let output = Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .arg("status")
            .arg("--short")
            .output();
        let Ok(output) = output else {
            return Self {
                schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
                run_id,
                workspace,
                captured_at,
                status: WorkspaceFileChangeStatus::Unavailable,
                changes: Vec::new(),
                truncated: false,
                error: Some("git status failed to start".to_string()),
            };
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            return Self {
                schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
                run_id,
                workspace,
                captured_at,
                status: WorkspaceFileChangeStatus::Unavailable,
                changes: Vec::new(),
                truncated: false,
                error: Some(if stderr.is_empty() {
                    format!("git status exited with {}", output.status)
                } else {
                    stderr
                }),
            };
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut truncated = false;
        let mut changes = Vec::new();
        for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
            if changes.len() >= MAX_FILE_CHANGE_ENTRIES {
                truncated = true;
                break;
            }
            changes.push(parse_git_status_line(line));
        }
        let status = if changes.is_empty() {
            WorkspaceFileChangeStatus::Clean
        } else {
            WorkspaceFileChangeStatus::Changed
        };
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id,
            workspace,
            captured_at,
            status,
            changes,
            truncated,
            error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceFileChangeStatus {
    Clean,
    Changed,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceFileChange {
    pub status_code: String,
    pub path: String,
}

fn parse_git_status_line(line: &str) -> WorkspaceFileChange {
    let status_code = line.get(0..2).unwrap_or(line).to_string();
    let path = line.get(3..).unwrap_or("").trim().to_string();
    WorkspaceFileChange { status_code, path }
}

fn capture_workspace_diff(workspace: &Path) -> String {
    let captures = [
        (
            "staged changes",
            vec!["diff", "--cached", "--no-ext-diff", "--no-color", "--"],
        ),
        (
            "unstaged changes",
            vec!["diff", "--no-ext-diff", "--no-color", "--"],
        ),
    ];
    let mut output = String::from("# Vegvisir workspace diff\n");
    let mut wrote_diff = false;
    let mut errors = Vec::new();

    for (label, args) in captures {
        match Command::new("git")
            .arg("-C")
            .arg(workspace)
            .args(args)
            .output()
        {
            Ok(command_output) if command_output.status.success() => {
                let diff = String::from_utf8_lossy(&command_output.stdout);
                if diff.trim().is_empty() {
                    continue;
                }
                if !output.ends_with('\n') {
                    output.push('\n');
                }
                output.push_str("\n# ");
                output.push_str(label);
                output.push('\n');
                push_truncated_diff(&mut output, &diff);
                wrote_diff = true;
            }
            Ok(command_output) => {
                let stderr = String::from_utf8_lossy(&command_output.stderr)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                errors.push(if stderr.is_empty() {
                    format!("{label}: git diff exited with {}", command_output.status)
                } else {
                    format!("{label}: {stderr}")
                });
            }
            Err(_) => errors.push(format!("{label}: git diff failed to start")),
        }
    }

    if !errors.is_empty() {
        output.push_str("\n# Diff capture unavailable or incomplete\n");
        for error in errors {
            output.push_str("# ");
            output.push_str(&error);
            output.push('\n');
        }
    } else if !wrote_diff {
        output.push_str("\n# No tracked workspace diffs captured.\n");
    }

    output
}

fn push_truncated_diff(output: &mut String, diff: &str) {
    if diff.len() <= MAX_WORKSPACE_DIFF_BYTES {
        output.push_str(diff);
        if !diff.ends_with('\n') {
            output.push('\n');
        }
        return;
    }

    let mut end = MAX_WORKSPACE_DIFF_BYTES;
    while end > 0 && !diff.is_char_boundary(end) {
        end -= 1;
    }
    output.push_str(&diff[..end]);
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("# [truncated after ");
    output.push_str(&MAX_WORKSPACE_DIFF_BYTES.to_string());
    output.push_str(" bytes]\n");
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMemoryUseEvidence {
    pub schema_version: u32,
    pub run_id: String,
    pub captured_at: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    pub prompt_cache_key: String,
    pub total_prompt_tokens: usize,
    pub memory_ids: Vec<String>,
    pub blocks: Vec<RunMemoryUseBlock>,
    pub capsules: Vec<RunMemoryUseCapsule>,
}

impl RunMemoryUseEvidence {
    pub fn from_envelope(run_id: String, envelope: &CachedPromptEnvelope) -> Self {
        let mut memory_ids = envelope
            .blocks
            .iter()
            .flat_map(|block| block.source_memory_ids.iter().cloned())
            .chain(
                envelope
                    .capsules
                    .iter()
                    .flat_map(|capsule| capsule.source_memory_ids.iter().cloned()),
            )
            .collect::<Vec<_>>();
        memory_ids.sort();
        memory_ids.dedup();

        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id,
            captured_at: Utc::now(),
            provider: envelope.manifest.provider.clone(),
            model: envelope.manifest.model.clone(),
            prompt_cache_key: envelope.manifest.prompt_cache_key.clone(),
            total_prompt_tokens: envelope.manifest.total_prompt_tokens,
            memory_ids,
            blocks: envelope
                .blocks
                .iter()
                .filter(|block| !block.source_memory_ids.is_empty())
                .map(|block| RunMemoryUseBlock {
                    id: block.id.clone(),
                    kind: format!("{:?}", block.kind),
                    title: block.title.clone(),
                    token_estimate: block.token_estimate,
                    source_memory_ids: block.source_memory_ids.clone(),
                    source_version_hashes: block.source_version_hashes.clone(),
                })
                .collect(),
            capsules: envelope
                .capsules
                .iter()
                .filter(|capsule| !capsule.source_memory_ids.is_empty())
                .map(|capsule| RunMemoryUseCapsule {
                    capsule_id: capsule.capsule_id.clone(),
                    capsule_type: format!("{:?}", capsule.capsule_type),
                    token_estimate: capsule.token_estimate,
                    source_memory_ids: capsule.source_memory_ids.clone(),
                    source_version_hashes: capsule.source_version_hashes.clone(),
                    block_ids: capsule.block_ids.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMemoryUseBlock {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub token_estimate: usize,
    pub source_memory_ids: Vec<String>,
    pub source_version_hashes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMemoryUseCapsule {
    pub capsule_id: String,
    pub capsule_type: String,
    pub token_estimate: usize,
    pub source_memory_ids: Vec<String>,
    pub source_version_hashes: Vec<String>,
    pub block_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunMemoryWriteEvidence {
    pub schema_version: u32,
    pub run_id: String,
    pub captured_at: DateTime<Utc>,
    pub status: RunMemoryWriteStatus,
    pub writes: Vec<RunMemoryWriteResult>,
    pub error: Option<String>,
    pub notes: Vec<String>,
}

impl RunMemoryWriteEvidence {
    pub fn from_results(run_id: String, results: &[CommitResult]) -> Self {
        Self::from_outcome(run_id, results, None)
    }

    pub fn from_outcome(run_id: String, results: &[CommitResult], error: Option<&str>) -> Self {
        let writes = results
            .iter()
            .map(RunMemoryWriteResult::from_commit_result)
            .collect::<Vec<_>>();
        let status = if error.is_some() {
            RunMemoryWriteStatus::Unavailable
        } else if writes.is_empty() {
            RunMemoryWriteStatus::NoWrites
        } else {
            RunMemoryWriteStatus::Captured
        };
        let notes = if error.is_some() {
            vec!["completion memory writeback failed or could not be inspected".to_string()]
        } else {
            Vec::new()
        };
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id,
            captured_at: Utc::now(),
            status,
            writes,
            error: error.map(str::to_string),
            notes,
        }
    }

    pub fn unavailable(run_id: String, note: impl Into<String>) -> Self {
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id,
            captured_at: Utc::now(),
            status: RunMemoryWriteStatus::Unavailable,
            writes: Vec::new(),
            error: None,
            notes: vec![note.into()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMemoryWriteStatus {
    Captured,
    NoWrites,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunMemoryWriteResult {
    pub memory_id: String,
    pub created_new: bool,
    pub updated_existing: bool,
    pub linked_memory_ids: Vec<String>,
    pub trace: Value,
}

impl RunMemoryWriteResult {
    fn from_commit_result(result: &CommitResult) -> Self {
        Self {
            memory_id: result.memory_id.0.clone(),
            created_new: result.created_new,
            updated_existing: result.updated_existing,
            linked_memory_ids: result
                .linked_memory_ids
                .iter()
                .map(|memory_id| memory_id.0.clone())
                .collect(),
            trace: serde_json::to_value(&result.trace).unwrap_or(Value::Null),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunApprovalEvidence {
    pub schema_version: u32,
    pub run_id: String,
    pub captured_at: DateTime<Utc>,
    pub status: RunApprovalStatus,
    pub pending: Vec<RunApprovalRequestEvidence>,
    pub notes: Vec<String>,
}

impl RunApprovalEvidence {
    pub fn from_pending(run_id: String, pending: &BTreeMap<String, ApprovalRequest>) -> Self {
        let pending = pending
            .values()
            .map(RunApprovalRequestEvidence::from_request)
            .collect::<Vec<_>>();
        let status = if pending.is_empty() {
            RunApprovalStatus::NoApprovals
        } else {
            RunApprovalStatus::Captured
        };
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id,
            captured_at: Utc::now(),
            status,
            pending,
            notes: Vec::new(),
        }
    }

    pub fn unavailable(run_id: String, note: impl Into<String>) -> Self {
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id,
            captured_at: Utc::now(),
            status: RunApprovalStatus::Unavailable,
            pending: Vec::new(),
            notes: vec![note.into()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunApprovalStatus {
    Captured,
    NoApprovals,
    Unavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunSubagentEvidence {
    pub schema_version: u32,
    pub run_id: String,
    pub captured_at: DateTime<Utc>,
    pub status: RunSubagentStatus,
    pub records: Vec<SubAgentTaskRecord>,
    pub active_count: usize,
    pub notes: Vec<String>,
}

impl RunSubagentEvidence {
    pub fn from_records(run_id: String, records: &[SubAgentTaskRecord]) -> Self {
        let records = records.to_vec();
        let active_count = records
            .iter()
            .filter(|record| {
                matches!(
                    record.status,
                    SubAgentStatus::Queued | SubAgentStatus::Running
                )
            })
            .count();
        let status = if records.is_empty() {
            RunSubagentStatus::NoSubagents
        } else {
            RunSubagentStatus::Captured
        };
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id,
            captured_at: Utc::now(),
            status,
            records,
            active_count,
            notes: Vec::new(),
        }
    }

    pub fn no_subagents(run_id: String, note: impl Into<String>) -> Self {
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id,
            captured_at: Utc::now(),
            status: RunSubagentStatus::NoSubagents,
            records: Vec::new(),
            active_count: 0,
            notes: vec![note.into()],
        }
    }

    pub fn unavailable(run_id: String, note: impl Into<String>) -> Self {
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id,
            captured_at: Utc::now(),
            status: RunSubagentStatus::Unavailable,
            records: Vec::new(),
            active_count: 0,
            notes: vec![note.into()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSubagentStatus {
    Captured,
    NoSubagents,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunApprovalRequestEvidence {
    pub id: String,
    pub tool_name: String,
    pub risk_label: String,
    pub reason: String,
    pub args_summary: Value,
}

impl RunApprovalRequestEvidence {
    fn from_request(request: &ApprovalRequest) -> Self {
        Self {
            id: request.id.clone(),
            tool_name: request.tool_name.clone(),
            risk_label: request.risk_label.clone(),
            reason: request.reason.clone(),
            args_summary: summarize_approval_args(&request.tool_name, &request.args),
        }
    }
}

fn summarize_approval_args(tool_name: &str, args: &serde_json::Map<String, Value>) -> Value {
    match tool_name {
        "run_command" => json!({
            "command": args.get("command").cloned().unwrap_or(Value::Null),
            "timeout": args.get("timeout").cloned(),
            "output_limit": args.get("output_limit").cloned(),
        }),
        "write_file" => json!({
            "path": args.get("path").cloned().unwrap_or(Value::Null),
            "content_chars": args
                .get("content")
                .and_then(Value::as_str)
                .map(|content| content.chars().count()),
        }),
        "spawn_subagent" => json!({
            "name": args.get("name").cloned(),
            "workspace": args.get("workspace").cloned(),
            "file_scope": args.get("file_scope").cloned(),
            "max_steps": args.get("max_steps").cloned(),
        }),
        _ => {
            let keys = args.keys().cloned().collect::<Vec<_>>();
            json!({ "argument_keys": keys })
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunVerificationEvidence {
    pub schema_version: u32,
    pub run_id: String,
    pub captured_at: DateTime<Utc>,
    pub status: RunVerificationStatus,
    pub overall: RunVerificationOverall,
    pub checks: Vec<RunVerificationCheck>,
    pub notes: Vec<String>,
}

impl RunVerificationEvidence {
    pub fn captured(run_id: String, checks: Vec<RunVerificationCheck>) -> Self {
        let overall = verification_overall(&checks);
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id,
            captured_at: Utc::now(),
            status: RunVerificationStatus::Captured,
            overall,
            checks,
            notes: Vec::new(),
        }
    }

    pub fn from_provider_events(run_id: String, events: &[ProviderRunEvent]) -> Self {
        let checks = events
            .iter()
            .filter_map(|event| RunVerificationCheck::from_provider_event(event, None))
            .collect::<Vec<_>>();
        if checks.is_empty() {
            Self::no_verification(
                run_id,
                "no verification command or test tool evidence was captured for this run",
            )
        } else {
            Self::captured(run_id, checks)
        }
    }

    pub fn no_verification(run_id: String, note: impl Into<String>) -> Self {
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id,
            captured_at: Utc::now(),
            status: RunVerificationStatus::NoVerification,
            overall: RunVerificationOverall::NotRun,
            checks: Vec::new(),
            notes: vec![note.into()],
        }
    }

    pub fn unavailable(run_id: String, note: impl Into<String>) -> Self {
        Self {
            schema_version: RUN_ARTIFACT_SCHEMA_VERSION,
            run_id,
            captured_at: Utc::now(),
            status: RunVerificationStatus::Unavailable,
            overall: RunVerificationOverall::Unknown,
            checks: Vec::new(),
            notes: vec![note.into()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunVerificationStatus {
    Captured,
    NoVerification,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunVerificationOverall {
    Passed,
    Failed,
    Mixed,
    NotRun,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunVerificationCheck {
    pub name: String,
    pub command: Option<String>,
    pub ok: Option<bool>,
    pub summary: String,
    pub detail: Option<String>,
    pub source: RunVerificationSource,
}

impl RunVerificationCheck {
    fn from_provider_event(event: &ProviderRunEvent, command: Option<String>) -> Option<Self> {
        let ProviderRunEvent::ToolEnd {
            name,
            ok,
            summary,
            detail,
        } = event
        else {
            return None;
        };
        if !is_verification_tool_observation(name, summary, detail.as_deref()) {
            return None;
        }
        Some(Self {
            name: name.clone(),
            command,
            ok: Some(*ok),
            summary: summary.clone(),
            detail: detail.clone(),
            source: RunVerificationSource::ProviderToolEvent,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunVerificationSource {
    ProviderToolEvent,
    Harness,
}

fn verification_overall(checks: &[RunVerificationCheck]) -> RunVerificationOverall {
    if checks.is_empty() {
        return RunVerificationOverall::NotRun;
    }
    let passed = checks.iter().any(|check| check.ok == Some(true));
    let failed = checks.iter().any(|check| check.ok == Some(false));
    match (passed, failed) {
        (true, true) => RunVerificationOverall::Mixed,
        (true, false) => RunVerificationOverall::Passed,
        (false, true) => RunVerificationOverall::Failed,
        (false, false) => RunVerificationOverall::Unknown,
    }
}

fn is_verification_tool_observation(name: &str, summary: &str, detail: Option<&str>) -> bool {
    let lower_name = name.to_ascii_lowercase();
    if matches!(
        lower_name.as_str(),
        "run_tests" | "verify" | "skiller_eval" | "skiller_validate" | "skiller_readiness"
    ) {
        return true;
    }
    if lower_name != "run_command" {
        return false;
    }
    let text = format!(
        "{} {}",
        summary.to_ascii_lowercase(),
        detail.unwrap_or_default().to_ascii_lowercase()
    );
    [
        "cargo test",
        "cargo check",
        "cargo clippy",
        "npm test",
        "pytest",
        "run_tests",
        "vegvisir verify",
        "/verify",
    ]
    .iter()
    .any(|needle| text.contains(needle))
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
    if matches!(
        key.as_str(),
        "cacheable_prefix_tokens"
            | "total_prompt_tokens"
            | "token_estimate"
            | "tokens_used"
            | "thinking_budget_tokens"
    ) {
        return false;
    }
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
    fn run_artifacts_manifest_schema_shape_matches_documented_bundle() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path().join("workspace");
        let data_root = tmp.path().join("data");
        fs::create_dir_all(&workspace)?;

        let (manager, manifest) = RunArtifactManager::start_with_run_id(
            &workspace,
            &data_root,
            "run-golden-shape",
            Option::<&Path>::None,
            "session-1",
            "demo",
            "demo-model",
            Some("tester".to_string()),
        )?;

        assert_eq!(manifest.schema_version, RUN_ARTIFACT_SCHEMA_VERSION);
        assert_eq!(manifest.status, RunStatus::Running);
        assert_eq!(manifest.artifact_paths, default_artifact_paths());
        assert_eq!(
            manager.run_dir,
            workspace
                .join(".vegvisir")
                .join("runs")
                .join("run-golden-shape")
        );

        let saved: Value =
            serde_json::from_str(&fs::read_to_string(manager.artifact_path("manifest.json"))?)?;
        assert_eq!(saved["schema_version"], RUN_ARTIFACT_SCHEMA_VERSION);
        assert_eq!(saved["run_id"], "run-golden-shape");
        assert_eq!(saved["session_id"], "session-1");
        assert_eq!(saved["provider"], "demo");
        assert_eq!(saved["model"], "demo-model");
        assert_eq!(saved["agent"], "tester");
        assert_eq!(saved["status"], "running");

        let artifact_paths = saved["artifact_paths"]
            .as_object()
            .expect("manifest artifact_paths object");
        let expected_paths = BTreeMap::from([
            ("manifest", "manifest.json"),
            ("request", "request.json"),
            ("context", "context.md"),
            ("context_sources", "context-sources.json"),
            ("provider_events", "provider-events.jsonl"),
            ("tool_events", "tool-events.jsonl"),
            ("file_changes", "file-changes.json"),
            ("diff", "diff.patch"),
            ("memory_used", "memory-used.json"),
            ("memory_written", "memory-written.json"),
            ("approvals", "approvals.json"),
            ("subagents", "subagents.json"),
            ("result", "result.md"),
            ("verification", "verification.json"),
            ("failure", "failure.json"),
        ]);
        assert_eq!(artifact_paths.len(), expected_paths.len());
        for (name, relative_path) in expected_paths {
            assert_eq!(artifact_paths[name], relative_path);
        }
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
            "Use Artifact context evidence in this memory run.",
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

        let memory_used: RunMemoryUseEvidence = serde_json::from_str(&fs::read_to_string(
            manager.artifact_path("memory-used.json"),
        )?)?;
        assert_eq!(memory_used.schema_version, RUN_ARTIFACT_SCHEMA_VERSION);
        assert_eq!(memory_used.run_id, manager.run_id);
        assert_eq!(memory_used.provider, "demo");
        assert_eq!(memory_used.model, "demo-model");
        assert!(!memory_used.memory_ids.is_empty());
        assert!(
            memory_used
                .blocks
                .iter()
                .any(|block| block.title.contains("Artifact context evidence"))
        );
        Ok(())
    }

    #[test]
    fn run_artifacts_writes_memory_written_from_commit_results() -> anyhow::Result<()> {
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
        let result = cms.remember(
            "ArchitectureChange",
            "Artifact writeback evidence",
            "Decision: run artifacts should persist memory writeback ids.",
        )?;

        manager.write_memory_written_from_results(&[result])?;

        let memory_written: RunMemoryWriteEvidence = serde_json::from_str(&fs::read_to_string(
            manager.artifact_path("memory-written.json"),
        )?)?;
        assert_eq!(memory_written.schema_version, RUN_ARTIFACT_SCHEMA_VERSION);
        assert_eq!(memory_written.run_id, manager.run_id);
        assert_eq!(memory_written.status, RunMemoryWriteStatus::Captured);
        assert_eq!(memory_written.writes.len(), 1);
        assert!(memory_written.writes[0].created_new || memory_written.writes[0].updated_existing);
        Ok(())
    }

    #[test]
    fn run_artifacts_writes_unavailable_memory_written_evidence() -> anyhow::Result<()> {
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

        manager.write_memory_written_unavailable("not captured in this runtime path")?;

        let memory_written: RunMemoryWriteEvidence = serde_json::from_str(&fs::read_to_string(
            manager.artifact_path("memory-written.json"),
        )?)?;
        assert_eq!(memory_written.status, RunMemoryWriteStatus::Unavailable);
        assert!(memory_written.writes.is_empty());
        assert!(memory_written.notes[0].contains("not captured"));
        Ok(())
    }

    #[test]
    fn run_artifacts_writes_approval_evidence_without_raw_content() -> anyhow::Result<()> {
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
        let request = ApprovalRequest {
            id: "approval-1".to_string(),
            reason: "Risky tool requires human approval: write_file".to_string(),
            tool_name: "write_file".to_string(),
            args: json!({"path": "secret.txt", "content": "token=github_pat_123456789012345678901234"})
                .as_object()
                .unwrap()
                .clone(),
            risk_label: "filesystem-write".to_string(),
        };
        let pending = BTreeMap::from([("approval-1".to_string(), request)]);

        manager.write_approvals_from_pending(&pending)?;

        let text = fs::read_to_string(manager.artifact_path("approvals.json"))?;
        assert!(!text.contains("github_pat_123"));
        let approvals: RunApprovalEvidence = serde_json::from_str(&text)?;
        assert_eq!(approvals.schema_version, RUN_ARTIFACT_SCHEMA_VERSION);
        assert_eq!(approvals.status, RunApprovalStatus::Captured);
        assert_eq!(approvals.pending.len(), 1);
        assert_eq!(approvals.pending[0].tool_name, "write_file");
        assert_eq!(approvals.pending[0].args_summary["path"], "secret.txt");
        assert_eq!(approvals.pending[0].args_summary["content_chars"], 41);
        Ok(())
    }

    #[test]
    fn run_artifacts_writes_no_approval_evidence_for_empty_pending_queue() -> anyhow::Result<()> {
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

        manager.write_approvals_from_pending(&BTreeMap::new())?;

        let approvals: RunApprovalEvidence = serde_json::from_str(&fs::read_to_string(
            manager.artifact_path("approvals.json"),
        )?)?;
        assert_eq!(approvals.status, RunApprovalStatus::NoApprovals);
        assert!(approvals.pending.is_empty());
        Ok(())
    }

    #[test]
    fn run_artifacts_writes_subagent_board_evidence() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path().join("workspace");
        let data_root = tmp.path().join("data");
        fs::create_dir_all(&workspace)?;
        fs::create_dir_all(&data_root)?;
        let (manager, _manifest) = RunArtifactManager::start(
            &workspace,
            &data_root,
            "session-1",
            "demo",
            "demo-model",
            None,
        )?;
        let record = SubAgentTaskRecord {
            id: "task-1".to_string(),
            name: "reviewer".to_string(),
            workspace: workspace.clone(),
            goal: "Review run artifact gaps".to_string(),
            parent_run_id: None,
            child_run_id: None,
            artifact_dir: None,
            ownership: None,
            provider: Some("demo".to_string()),
            model: Some("demo-model".to_string()),
            file_scope: vec![workspace.join("vegvisir/src/run_artifacts.rs")],
            work_budget: Default::default(),
            status: SubAgentStatus::Running,
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            finished_at: None,
            checkpoint: None,
            final_answer: None,
            error: None,
            observability: Default::default(),
        };
        fs::write(
            data_root.join("subagents.json"),
            serde_json::to_string_pretty(&vec![record])?,
        )?;

        manager.write_subagents_from_board()?;

        let evidence: RunSubagentEvidence = serde_json::from_str(&fs::read_to_string(
            manager.artifact_path("subagents.json"),
        )?)?;
        assert_eq!(evidence.schema_version, RUN_ARTIFACT_SCHEMA_VERSION);
        assert_eq!(evidence.status, RunSubagentStatus::Captured);
        assert_eq!(evidence.active_count, 1);
        assert_eq!(evidence.records.len(), 1);
        assert_eq!(evidence.records[0].name, "reviewer");
        Ok(())
    }

    #[test]
    fn run_artifacts_writes_no_subagents_when_board_absent() -> anyhow::Result<()> {
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

        manager.write_subagents_from_board()?;

        let evidence: RunSubagentEvidence = serde_json::from_str(&fs::read_to_string(
            manager.artifact_path("subagents.json"),
        )?)?;
        assert_eq!(evidence.status, RunSubagentStatus::NoSubagents);
        assert_eq!(evidence.active_count, 0);
        assert!(evidence.records.is_empty());
        Ok(())
    }

    #[test]
    fn run_artifacts_writes_default_verification_on_finish() -> anyhow::Result<()> {
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

        manager.finish(&mut manifest, RunStatus::Completed)?;

        let verification: RunVerificationEvidence = serde_json::from_str(&fs::read_to_string(
            manager.artifact_path("verification.json"),
        )?)?;
        assert_eq!(verification.status, RunVerificationStatus::NoVerification);
        assert_eq!(verification.overall, RunVerificationOverall::NotRun);
        assert!(verification.checks.is_empty());
        Ok(())
    }

    #[test]
    fn run_artifacts_captures_verification_tool_events() -> anyhow::Result<()> {
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

        manager.append_observed_provider_event(&ProviderRunEvent::ToolEnd {
            name: "run_tests".to_string(),
            ok: true,
            summary: "cargo test -p vegvisir-rust passed".to_string(),
            detail: None,
        })?;

        let verification: RunVerificationEvidence = serde_json::from_str(&fs::read_to_string(
            manager.artifact_path("verification.json"),
        )?)?;
        assert_eq!(verification.status, RunVerificationStatus::Captured);
        assert_eq!(verification.overall, RunVerificationOverall::Passed);
        assert_eq!(verification.checks.len(), 1);
        assert_eq!(verification.checks[0].name, "run_tests");
        assert_eq!(verification.checks[0].ok, Some(true));
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
    fn run_artifacts_captures_workspace_file_changes() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let git_init = std::process::Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .arg("init")
            .output()?;
        if !git_init.status.success() {
            return Ok(());
        }
        fs::write(workspace.join("changed.txt"), "new file")?;

        let (manager, _manifest) = RunArtifactManager::start(
            &workspace,
            tmp.path().join("data"),
            "session-1",
            "demo",
            "demo-model",
            None,
        )?;

        manager.write_workspace_file_changes()?;

        let evidence: WorkspaceFileChangeEvidence = serde_json::from_str(&fs::read_to_string(
            manager.artifact_path("file-changes.json"),
        )?)?;
        assert_eq!(evidence.status, WorkspaceFileChangeStatus::Changed);
        assert!(
            evidence
                .changes
                .iter()
                .any(|change| change.status_code == "??" && change.path == "changed.txt")
        );
        Ok(())
    }

    #[test]
    fn run_artifacts_captures_workspace_diff() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let git_init = std::process::Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .arg("init")
            .output()?;
        if !git_init.status.success() {
            return Ok(());
        }
        fs::write(workspace.join("tracked.txt"), "before\n")?;
        let add = std::process::Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .args(["add", "tracked.txt"])
            .output()?;
        if !add.status.success() {
            return Ok(());
        }
        // No commit is needed: `git diff` compares an unstaged worktree
        // modification against the indexed file. This keeps the test isolated
        // from global git user.name/user.email configuration and commit hooks.
        fs::write(workspace.join("tracked.txt"), "before\nafter\n")?;

        let (manager, _manifest) = RunArtifactManager::start(
            &workspace,
            tmp.path().join("data"),
            "session-1",
            "demo",
            "demo-model",
            None,
        )?;

        manager.write_workspace_diff()?;

        let diff = fs::read_to_string(manager.artifact_path("diff.patch"))?;
        assert!(diff.contains("# Vegvisir workspace diff"));
        assert!(diff.contains("diff --git a/tracked.txt b/tracked.txt"));
        assert!(diff.contains("+after"));
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
