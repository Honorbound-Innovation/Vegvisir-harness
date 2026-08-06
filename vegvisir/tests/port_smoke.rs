use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, Read, Write},
    net::TcpListener,
    os::unix::net::UnixListener,
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use serde_json::{Map, Value, json};
use tempfile::{NamedTempFile, tempdir};
use vegvisir_rust::{
    app::TuiApplication,
    attachments::extract_attachments,
    bridge::{BridgeOptions, run_app_server_with_io},
    checkpoints::CheckpointStore,
    command_registry::CommandRegistry,
    command_sandbox::{
        COMMAND_SANDBOX_ENV, COMMAND_SANDBOX_HIDDEN_PATHS_ENV, COMMAND_SANDBOX_NETWORK_ENV,
    },
    context::ContextManager,
    core::{
        Attachment, AuditEvent, AuditLog, ChatMessage, ConfigStore, ModelInfo, ModelRegistry,
        ProviderConfig, ProviderRegistry, default_system_prompt,
    },
    environment::{load_environment_d, parse_environment_line},
    guardrails::{ApprovalRequest, GuardrailEngine, PermissionPolicy},
    memory::{ContextPrepareOptions, VegvisirCms, VegvisirCmsConfig, default_vegvisir_data_root},
    model::ScriptedModel,
    model_discovery::discover_provider_models,
    orchestrator::{AgentHarness, AgentTask},
    provider::{
        AnthropicProviderAdapter, ConversationRunner, GoogleProviderAdapter,
        HBSEAnthropicProviderAdapter, HBSEAzureOpenAIProviderAdapter, HBSEGoogleProviderAdapter,
        HBSEOpenAICompatibleProviderAdapter, OpenAICompatibleProviderAdapter,
        OpenAISsoProfileAdapter, ProviderAdapter, ProviderRouter, openai_tool_loop,
        openai_tool_schema,
    },
    retrieval::{InMemoryRetriever, RetrievalDocument},
    sandbox::WorkspaceSandbox,
    tools::{
        Tool, ToolExecutor, ToolRegistry, build_builtin_registry, build_builtin_registry_with_cms,
    },
    types::{AgentDecision, Message, Observation, Role, ToolCall},
    ui::layout::visible_width,
};

fn test_model_with_metadata(
    provider: &str,
    name: &str,
    metadata: BTreeMap<String, Value>,
) -> ModelInfo {
    ModelInfo {
        name: name.to_string(),
        provider: provider.to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata,
    }
}

#[derive(Clone, Debug)]
struct TestToolCallingProvider {
    config: ProviderConfig,
}

impl ProviderAdapter for TestToolCallingProvider {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        _messages: &[ChatMessage],
        _model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        anyhow::bail!("tool-capable provider should use complete_with_tools")
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        true
    }

    fn complete_with_tools(
        &self,
        _messages: &[ChatMessage],
        _model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        assert!(tools.iter().any(|tool| tool["name"] == "list_files"));
        Ok(format!(
            "Tool result included: {}",
            execute_tool(
                "list_files",
                json!({"path": "."}).as_object().unwrap().clone()
            )
        ))
    }
}

#[derive(Clone, Debug)]
struct RecordingProvider {
    config: ProviderConfig,
    selected_provider: Arc<Mutex<Option<String>>>,
}

impl ProviderAdapter for RecordingProvider {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        _messages: &[ChatMessage],
        _model: &ModelInfo,
        selected_provider: &str,
    ) -> anyhow::Result<String> {
        *self.selected_provider.lock().unwrap() = Some(selected_provider.to_string());
        Ok("ok".to_string())
    }
}

#[derive(Clone, Debug)]
struct ModelRecordingProvider {
    config: ProviderConfig,
    models: Arc<Mutex<Vec<ModelInfo>>>,
}

impl ProviderAdapter for ModelRecordingProvider {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        _messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        self.models.lock().unwrap().push(model.clone());
        Ok("ok".to_string())
    }
}

#[derive(Clone, Debug)]
struct MessageRecordingProvider {
    config: ProviderConfig,
    messages: Arc<Mutex<Vec<ChatMessage>>>,
}

impl ProviderAdapter for MessageRecordingProvider {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        _model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        *self.messages.lock().unwrap() = messages.to_vec();
        Ok("ok".to_string())
    }
}

#[derive(Clone, Debug)]
struct SteeringToolProvider {
    config: ProviderConfig,
    observation: Arc<Mutex<Option<String>>>,
}

impl ProviderAdapter for SteeringToolProvider {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        _messages: &[ChatMessage],
        _model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        anyhow::bail!("steering tool provider should use complete_with_tools")
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        true
    }

    fn complete_with_tools(
        &self,
        _messages: &[ChatMessage],
        _model: &ModelInfo,
        _tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        let observed = execute_tool("list_files", Map::new());
        *self.observation.lock().unwrap() = Some(observed);
        Ok("done".to_string())
    }
}

#[derive(Clone, Debug)]
struct RiskyToolCallingProvider {
    config: ProviderConfig,
}

impl ProviderAdapter for RiskyToolCallingProvider {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        _messages: &[ChatMessage],
        _model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        anyhow::bail!("risky tool provider should use complete_with_tools")
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        true
    }

    fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        selected_provider: &str,
    ) -> anyhow::Result<String> {
        self.complete_with_tools_streaming(
            messages,
            model,
            tools,
            execute_tool,
            selected_provider,
            &mut |_| {},
        )
    }

    fn complete_with_tools_streaming(
        &self,
        _messages: &[ChatMessage],
        _model: &ModelInfo,
        _tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
        _on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let _ = execute_tool(
            "write_file",
            json!({"path": "approval.txt", "content": "needs approval"})
                .as_object()
                .unwrap()
                .clone(),
        );
        Ok("tool request submitted".to_string())
    }
}

fn write_sso_auth(
    root: &std::path::Path,
    access_token: &str,
    account_id: &str,
) -> anyhow::Result<()> {
    let auth_dir = root.join("auth");
    std::fs::create_dir_all(&auth_dir)?;
    std::fs::write(
        auth_dir.join("openai_sso.json"),
        serde_json::to_string_pretty(&json!({
            "provider": "openai-sso",
            "issuer": "https://auth.openai.com",
            "tokens": {
                "id_token": "",
                "access_token": access_token,
                "refresh_token": "refresh-token",
                "account_id": account_id,
            },
            "last_refresh": 1.0,
        }))?,
    )?;
    Ok(())
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvVarGuard {
    key: &'static str,
    old: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let old = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, old }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(old) = &self.old {
                std::env::set_var(self.key, old);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

fn bwrap_available() -> bool {
    std::process::Command::new("bwrap")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn execute_after_one_approval(executor: &mut ToolExecutor, call: ToolCall) -> Observation {
    let first = executor.execute(call.clone());
    if first.ok {
        return first;
    }
    if !first.content.contains("approval_id=") {
        return first;
    }
    let Some(approval_id) = executor.guardrails.approvals.pending_ids().first().cloned() else {
        return first;
    };
    if executor.guardrails.approvals.approve_once(&approval_id) {
        executor.execute(call)
    } else {
        first
    }
}

fn unsigned_jwt(claims: Value) -> String {
    format!(
        "{}.{}.",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_string(&claims).unwrap())
    )
}

#[test]
fn context_compacts_old_messages() {
    let mut context = ContextManager::new(4);
    for index in 0..8 {
        context.add(Message::new(Role::User, format!("message {index}")));
    }
    assert!(context.messages.len() <= 4);
    assert!(context.summary.contains("message 0"));
    assert_eq!(
        context.visible_messages()[0].name.as_deref(),
        Some("context_summary")
    );
}

#[test]
fn harness_runs_tool_loop_and_checkpoints() -> anyhow::Result<()> {
    let _guard = env_lock().lock().unwrap();
    let tmp = tempdir()?;
    let _env = EnvVarGuard::set("VEGVISIR_HOME", tmp.path().join("home"));
    std::fs::write(tmp.path().join("example.txt"), "hello")?;
    let mut args = Map::new();
    args.insert("path".to_string(), Value::String(".".to_string()));
    let model = ScriptedModel::new(vec![
        AgentDecision {
            thought: "List files first.".to_string(),
            action: Some("list_files".to_string()),
            args,
            final_answer: None,
        },
        AgentDecision::final_decision("Done.", "Found workspace files."),
    ]);
    let mut harness = AgentHarness::default(model, tmp.path())?;
    let mut task = AgentTask::new("inspect", tmp.path());
    task.max_steps = 4;

    let result = harness.run(task)?;

    assert_eq!(result.status, "completed");
    assert_eq!(
        result.final_answer.as_deref(),
        Some("Found workspace files.")
    );
    assert_eq!(result.steps, 2);
    assert!(result.checkpoint.unwrap().exists());
    let snapshot = result.snapshot.unwrap();
    assert!(snapshot.exists());
    let snapshot_text = std::fs::read_to_string(snapshot)?;
    assert!(snapshot_text.contains("\"cms_root\""));
    assert!(!snapshot_text.contains("\"memory_root\""));
    let loaded = CheckpointStore::new(tmp.path().join(".vegvisir/runs")).load(&result.run_id)?;
    assert_eq!(
        loaded.state.metadata["plan"]["items"][0]["status"],
        Value::String("passed".to_string())
    );
    assert!(loaded.cms_root.is_some());
    assert!(default_vegvisir_data_root().join("cms-v2.sqlite3").exists());
    Ok(())
}

#[test]
fn harness_stops_at_max_steps() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut args = Map::new();
    args.insert("path".to_string(), Value::String(".".to_string()));
    let model = ScriptedModel::new(vec![
        AgentDecision {
            thought: "Keep going.".to_string(),
            action: Some("list_files".to_string()),
            args: args.clone(),
            final_answer: None,
        },
        AgentDecision {
            thought: "Keep going.".to_string(),
            action: Some("list_files".to_string()),
            args,
            final_answer: None,
        },
    ]);
    let mut harness = AgentHarness::default(model, tmp.path())?;
    let mut task = AgentTask::new("loop", tmp.path());
    task.max_steps = 1;

    let result = harness.run(task)?;

    assert_eq!(result.status, "max_steps_exceeded");
    assert_eq!(result.steps, 1);
    assert!(result.snapshot.unwrap().exists());
    Ok(())
}

#[test]
fn cli_prompt_and_legacy_run_headless_modes_work() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let binary = env!("CARGO_BIN_EXE_vegvisir-rust");

    let help = std::process::Command::new(binary).arg("--help").output()?;
    assert!(help.status.success());
    let help_stdout = String::from_utf8_lossy(&help.stdout);
    assert!(help_stdout.contains("--artifacts"));
    assert!(help_stdout.contains("--artifact-dir <ARTIFACT_DIR>"));

    let run_help = std::process::Command::new(binary)
        .args(["help", "run"])
        .output()?;
    assert!(run_help.status.success());
    let run_help_stdout = String::from_utf8_lossy(&run_help.stdout);
    assert!(run_help_stdout.contains("--artifacts"));
    assert!(run_help_stdout.contains("--artifact-dir <ARTIFACT_DIR>"));

    let eval_help = std::process::Command::new(binary)
        .args(["help", "eval"])
        .output()?;
    assert!(eval_help.status.success());
    let eval_help_stdout = String::from_utf8_lossy(&eval_help.stdout);
    assert!(eval_help_stdout.contains("--artifacts"));
    assert!(eval_help_stdout.contains("--artifact-dir <ARTIFACT_DIR>"));

    let verify_help = std::process::Command::new(binary)
        .args(["help", "verify"])
        .output()?;
    assert!(verify_help.status.success());
    let verify_help_stdout = String::from_utf8_lossy(&verify_help.stdout);
    assert!(verify_help_stdout.contains("--artifacts"));
    assert!(verify_help_stdout.contains("--artifact-dir <ARTIFACT_DIR>"));

    let prompt = std::process::Command::new(binary)
        .args([
            "-p",
            "inspect",
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--max-steps",
            "1",
            "--scripted",
        ])
        .output()?;
    assert!(prompt.status.success());
    let prompt_stdout = String::from_utf8_lossy(&prompt.stdout);
    assert!(prompt_stdout.contains("max_steps_exceeded:"));

    let legacy_run = std::process::Command::new(binary)
        .args([
            "run",
            "inspect",
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--max-steps",
            "1",
            "--scripted",
        ])
        .output()?;
    assert!(legacy_run.status.success());
    let legacy_stdout = String::from_utf8_lossy(&legacy_run.stdout);
    assert!(legacy_stdout.contains("max_steps_exceeded:"));

    let scripted_artifact_run = std::process::Command::new(binary)
        .args([
            "run",
            "inspect",
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--json",
            "--scripted",
            "--artifacts",
        ])
        .env("VEGVISIR_HOME", tmp.path().join("home-scripted-artifacts"))
        .output()?;
    assert!(
        scripted_artifact_run.status.success(),
        "scripted artifact run failed: {}",
        String::from_utf8_lossy(&scripted_artifact_run.stderr)
    );
    let scripted_artifact_json: Value = serde_json::from_slice(&scripted_artifact_run.stdout)?;
    assert_eq!(scripted_artifact_json["status"], "completed");
    assert_eq!(scripted_artifact_json["mode"], "scripted_harness");
    let scripted_run_id = scripted_artifact_json["run_id"].as_str().unwrap();
    let scripted_artifact_dir =
        std::path::PathBuf::from(scripted_artifact_json["artifact_dir"].as_str().unwrap());
    assert!(scripted_artifact_dir.join("manifest.json").exists());
    assert!(scripted_artifact_dir.join("request.json").exists());
    assert!(scripted_artifact_dir.join("context.md").exists());
    assert!(scripted_artifact_dir.join("context-sources.json").exists());
    assert!(scripted_artifact_dir.join("memory-used.json").exists());
    assert!(scripted_artifact_dir.join("memory-written.json").exists());
    assert!(scripted_artifact_dir.join("approvals.json").exists());
    assert!(scripted_artifact_dir.join("subagents.json").exists());
    assert!(scripted_artifact_dir.join("verification.json").exists());
    assert!(scripted_artifact_dir.join("result.md").exists());
    assert!(scripted_artifact_dir.join("file-changes.json").exists());
    assert!(scripted_artifact_dir.join("diff.patch").exists());
    let scripted_changes: Value = serde_json::from_str(&fs::read_to_string(
        scripted_artifact_dir.join("file-changes.json"),
    )?)?;
    assert_eq!(scripted_changes["status"], "unavailable");
    assert_eq!(scripted_changes["run_id"], scripted_run_id);
    let scripted_manifest: Value = serde_json::from_str(&fs::read_to_string(
        scripted_artifact_dir.join("manifest.json"),
    )?)?;
    assert_eq!(scripted_manifest["run_id"], scripted_run_id);
    assert_eq!(scripted_manifest["status"], "completed");
    let scripted_approvals: Value = serde_json::from_str(&fs::read_to_string(
        scripted_artifact_dir.join("approvals.json"),
    )?)?;
    assert_eq!(scripted_approvals["status"], "no_approvals");
    let scripted_subagents: Value = serde_json::from_str(&fs::read_to_string(
        scripted_artifact_dir.join("subagents.json"),
    )?)?;
    assert_eq!(scripted_subagents["status"], "no_subagents");
    let scripted_verification: Value = serde_json::from_str(&fs::read_to_string(
        scripted_artifact_dir.join("verification.json"),
    )?)?;
    assert_eq!(scripted_verification["status"], "no_verification");
    assert_eq!(scripted_verification["overall"], "not_run");
    assert_eq!(
        scripted_manifest["artifact_paths"]["manifest"],
        "manifest.json"
    );
    assert_eq!(
        scripted_manifest["artifact_paths"]["request"],
        "request.json"
    );
    assert_eq!(
        scripted_manifest["artifact_paths"]["context_sources"],
        "context-sources.json"
    );
    assert_eq!(
        scripted_manifest["artifact_paths"]["provider_events"],
        "provider-events.jsonl"
    );
    assert_eq!(
        scripted_manifest["artifact_paths"]["tool_events"],
        "tool-events.jsonl"
    );
    assert_eq!(
        scripted_manifest["artifact_paths"]["runtime_events"],
        "runtime-events.jsonl"
    );
    assert_eq!(
        scripted_manifest["artifact_paths"]["file_changes"],
        "file-changes.json"
    );
    assert_eq!(
        scripted_manifest["artifact_paths"]["memory_used"],
        "memory-used.json"
    );
    assert_eq!(
        scripted_manifest["artifact_paths"]["memory_written"],
        "memory-written.json"
    );
    assert_eq!(
        scripted_manifest["artifact_paths"]["verification"],
        "verification.json"
    );

    let failed_scripted_artifact_run = std::process::Command::new(binary)
        .args([
            "run",
            "inspect",
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--max-steps",
            "1",
            "--json",
            "--scripted",
            "--artifacts",
        ])
        .env(
            "VEGVISIR_HOME",
            tmp.path().join("home-failed-scripted-artifacts"),
        )
        .output()?;
    assert!(
        failed_scripted_artifact_run.status.success(),
        "failed scripted artifact run failed unexpectedly: {}",
        String::from_utf8_lossy(&failed_scripted_artifact_run.stderr)
    );
    let failed_scripted_json: Value = serde_json::from_slice(&failed_scripted_artifact_run.stdout)?;
    assert_eq!(failed_scripted_json["status"], "max_steps_exceeded");
    let failed_scripted_artifact_dir =
        std::path::PathBuf::from(failed_scripted_json["artifact_dir"].as_str().unwrap());
    let failed_scripted_manifest: Value = serde_json::from_str(&fs::read_to_string(
        failed_scripted_artifact_dir.join("manifest.json"),
    )?)?;
    assert_eq!(failed_scripted_manifest["status"], "failed");
    let failed_scripted_failure: Value = serde_json::from_str(&fs::read_to_string(
        failed_scripted_artifact_dir.join("failure.json"),
    )?)?;
    assert!(
        failed_scripted_failure["message"]
            .as_str()
            .unwrap()
            .contains("max_steps_exceeded")
    );
    assert!(
        failed_scripted_artifact_dir
            .join("file-changes.json")
            .exists()
    );
    assert!(failed_scripted_artifact_dir.join("diff.patch").exists());
    assert!(
        failed_scripted_artifact_dir
            .join("memory-written.json")
            .exists()
    );

    let provider_run = std::process::Command::new(binary)
        .args([
            "run",
            "inspect",
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--provider",
            "demo",
            "--model",
            "demo-local",
            "--json",
        ])
        .env("VEGVISIR_HOME", tmp.path().join("home-run"))
        .output()?;
    assert!(provider_run.status.success());
    let provider_json: Value = serde_json::from_slice(&provider_run.stdout)?;
    assert_eq!(provider_json["status"], "completed");
    assert_eq!(provider_json["provider"], "demo");
    assert_eq!(provider_json["model"], "demo-local");
    assert_eq!(provider_json["mode"], "provider_runtime");
    assert!(
        provider_json["answer"]
            .as_str()
            .unwrap()
            .contains("Demo response")
    );

    let provider_artifact_run = std::process::Command::new(binary)
        .args([
            "run",
            "inspect",
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--provider",
            "demo",
            "--model",
            "demo-local",
            "--json",
            "--artifacts",
        ])
        .env("VEGVISIR_HOME", tmp.path().join("home-provider-artifacts"))
        .output()?;
    assert!(
        provider_artifact_run.status.success(),
        "provider artifact run failed: {}",
        String::from_utf8_lossy(&provider_artifact_run.stderr)
    );
    let provider_artifact_json: Value = serde_json::from_slice(&provider_artifact_run.stdout)?;
    assert_eq!(provider_artifact_json["status"], "completed");
    let provider_artifact_dir =
        std::path::PathBuf::from(provider_artifact_json["artifact_dir"].as_str().unwrap());
    assert!(provider_artifact_dir.join("manifest.json").exists());
    assert!(provider_artifact_dir.join("request.json").exists());
    assert!(provider_artifact_dir.join("context.md").exists());
    assert!(provider_artifact_dir.join("context-sources.json").exists());
    assert!(provider_artifact_dir.join("memory-used.json").exists());
    assert!(provider_artifact_dir.join("memory-written.json").exists());
    assert!(provider_artifact_dir.join("approvals.json").exists());
    assert!(provider_artifact_dir.join("subagents.json").exists());
    assert!(provider_artifact_dir.join("verification.json").exists());
    assert!(provider_artifact_dir.join("result.md").exists());
    assert!(provider_artifact_dir.join("file-changes.json").exists());
    assert!(provider_artifact_dir.join("diff.patch").exists());
    let provider_changes: Value = serde_json::from_str(&fs::read_to_string(
        provider_artifact_dir.join("file-changes.json"),
    )?)?;
    assert_eq!(provider_changes["status"], "unavailable");
    let provider_manifest: Value = serde_json::from_str(&fs::read_to_string(
        provider_artifact_dir.join("manifest.json"),
    )?)?;
    assert_eq!(provider_manifest["status"], "completed");
    assert_eq!(provider_manifest["provider"], "demo");
    assert_eq!(provider_manifest["model"], "demo-local");
    let provider_approvals: Value = serde_json::from_str(&fs::read_to_string(
        provider_artifact_dir.join("approvals.json"),
    )?)?;
    assert_eq!(provider_approvals["status"], "no_approvals");
    let provider_subagents: Value = serde_json::from_str(&fs::read_to_string(
        provider_artifact_dir.join("subagents.json"),
    )?)?;
    assert_eq!(provider_subagents["status"], "no_subagents");
    let provider_verification: Value = serde_json::from_str(&fs::read_to_string(
        provider_artifact_dir.join("verification.json"),
    )?)?;
    assert_eq!(provider_verification["status"], "no_verification");
    assert_eq!(provider_verification["overall"], "not_run");

    let provider_artifact_failure = std::process::Command::new(binary)
        .args([
            "run",
            "inspect",
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--provider",
            "demo",
            "--model",
            "missing-model",
            "--artifacts",
            "--artifact-dir",
            tmp.path()
                .join("failed-provider-artifacts")
                .to_str()
                .unwrap(),
        ])
        .env(
            "VEGVISIR_HOME",
            tmp.path().join("home-provider-artifact-failure"),
        )
        .output()?;
    assert!(!provider_artifact_failure.status.success());
    let failed_provider_runs = fs::read_dir(tmp.path().join("failed-provider-artifacts"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(failed_provider_runs.len(), 1);
    let failed_provider_manifest: Value = serde_json::from_str(&fs::read_to_string(
        failed_provider_runs[0].join("manifest.json"),
    )?)?;
    assert_eq!(failed_provider_manifest["status"], "failed");
    assert!(failed_provider_runs[0].join("file-changes.json").exists());
    assert!(failed_provider_runs[0].join("diff.patch").exists());
    assert!(failed_provider_runs[0].join("memory-written.json").exists());
    assert!(failed_provider_runs[0].join("approvals.json").exists());
    assert!(failed_provider_runs[0].join("subagents.json").exists());
    assert!(failed_provider_runs[0].join("verification.json").exists());
    let failed_provider_verification: Value = serde_json::from_str(&fs::read_to_string(
        failed_provider_runs[0].join("verification.json"),
    )?)?;
    assert_eq!(failed_provider_verification["status"], "unavailable");
    let failed_provider_failure: Value = serde_json::from_str(&fs::read_to_string(
        failed_provider_runs[0].join("failure.json"),
    )?)?;
    assert!(
        failed_provider_failure["message"]
            .as_str()
            .unwrap()
            .contains("model selection failed")
    );

    let eval_artifact_root = tmp.path().join("eval-artifacts");
    let eval_artifact_run = std::process::Command::new(binary)
        .args([
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--artifact-dir",
            eval_artifact_root.to_str().unwrap(),
            "eval",
            "golden",
        ])
        .env("VEGVISIR_HOME", tmp.path().join("home-eval-artifacts"))
        .output()?;
    assert!(
        eval_artifact_run.status.success(),
        "eval artifact run failed: {}",
        String::from_utf8_lossy(&eval_artifact_run.stderr)
    );
    let eval_stdout = String::from_utf8_lossy(&eval_artifact_run.stdout);
    assert!(eval_stdout.contains("eval summary: passed="));
    assert!(eval_stdout.contains("artifact_dir:"));
    let eval_runs = fs::read_dir(&eval_artifact_root)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(eval_runs.len(), 1);
    let eval_manifest: Value =
        serde_json::from_str(&fs::read_to_string(eval_runs[0].join("manifest.json"))?)?;
    assert_eq!(eval_manifest["status"], "completed");
    assert_eq!(eval_manifest["session_id"], "headless-eval");
    assert_eq!(eval_manifest["provider"], "harness");
    assert_eq!(eval_manifest["model"], "eval");
    let eval_request: Value =
        serde_json::from_str(&fs::read_to_string(eval_runs[0].join("request.json"))?)?;
    assert_eq!(eval_request["mode"], "eval");
    assert_eq!(eval_request["scope"], "golden");
    let eval_evidence: Value =
        serde_json::from_str(&fs::read_to_string(eval_runs[0].join("verification.json"))?)?;
    assert_eq!(eval_evidence["status"], "captured");
    assert_eq!(eval_evidence["overall"], "passed");
    assert_eq!(eval_evidence["checks"][0]["source"], "harness");
    assert!(
        eval_evidence["checks"][0]["name"]
            .as_str()
            .unwrap()
            .starts_with("vegvisir_eval/")
    );
    let eval_memory_written: Value = serde_json::from_str(&fs::read_to_string(
        eval_runs[0].join("memory-written.json"),
    )?)?;
    assert_eq!(eval_memory_written["status"], "unavailable");
    let eval_approvals: Value =
        serde_json::from_str(&fs::read_to_string(eval_runs[0].join("approvals.json"))?)?;
    assert_eq!(eval_approvals["status"], "unavailable");
    assert!(eval_runs[0].join("result.md").exists());
    assert!(eval_runs[0].join("subagents.json").exists());
    assert!(eval_runs[0].join("file-changes.json").exists());
    assert!(eval_runs[0].join("diff.patch").exists());

    let verify_artifact_root = tmp.path().join("verify-artifacts");
    let verify_artifact_run = std::process::Command::new(binary)
        .args([
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--artifact-dir",
            verify_artifact_root.to_str().unwrap(),
            "verify",
            "runtime",
        ])
        .env("VEGVISIR_HOME", tmp.path().join("home-verify-artifacts"))
        .output()?;
    assert!(
        verify_artifact_run.status.success(),
        "verify artifact run failed: {}",
        String::from_utf8_lossy(&verify_artifact_run.stderr)
    );
    let verify_stdout = String::from_utf8_lossy(&verify_artifact_run.stdout);
    assert!(verify_stdout.contains("ok runtime/approvals"));
    assert!(verify_stdout.contains("artifact_dir:"));
    let verify_runs = fs::read_dir(&verify_artifact_root)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(verify_runs.len(), 1);
    let verify_manifest: Value =
        serde_json::from_str(&fs::read_to_string(verify_runs[0].join("manifest.json"))?)?;
    assert_eq!(verify_manifest["status"], "completed");
    let verify_request: Value =
        serde_json::from_str(&fs::read_to_string(verify_runs[0].join("request.json"))?)?;
    assert_eq!(verify_request["mode"], "verify");
    assert_eq!(verify_request["scope"], "runtime");
    let verify_evidence: Value = serde_json::from_str(&fs::read_to_string(
        verify_runs[0].join("verification.json"),
    )?)?;
    assert_eq!(verify_evidence["status"], "captured");
    assert_eq!(verify_evidence["overall"], "passed");
    assert_eq!(verify_evidence["checks"][0]["source"], "harness");
    assert!(verify_runs[0].join("result.md").exists());
    assert!(verify_runs[0].join("subagents.json").exists());

    let agent_home = tmp.path().join("home-agent-cli");
    let mut agent_app = TuiApplication::with_data_root(tmp.path(), &agent_home)?;
    agent_app
        .execute_command("/agent create coder | coder | Coder | You are a focused coding agent.")?
        .unwrap();
    agent_app
        .execute_command("/agent provider coder demo")?
        .unwrap();
    agent_app
        .execute_command("/agent model coder demo-local")?
        .unwrap();
    let agent_run = std::process::Command::new(binary)
        .args([
            "run",
            "inspect",
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--agent",
            "coder",
            "--json",
        ])
        .env("VEGVISIR_HOME", &agent_home)
        .output()?;
    assert!(agent_run.status.success());
    let agent_json: Value = serde_json::from_slice(&agent_run.stdout)?;
    assert_eq!(agent_json["agent"], "coder");
    assert_eq!(agent_json["provider"], "demo");
    assert_eq!(agent_json["model"], "demo-local");

    let bad_provider = std::process::Command::new(binary)
        .args([
            "run",
            "inspect",
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--provider",
            "missing-provider",
        ])
        .env("VEGVISIR_HOME", tmp.path().join("home-bad-provider"))
        .output()?;
    assert!(!bad_provider.status.success());
    assert!(String::from_utf8_lossy(&bad_provider.stderr).contains("provider selection failed"));

    let verify = std::process::Command::new(binary)
        .args([
            "verify",
            "runtime",
            "--workspace",
            tmp.path().to_str().unwrap(),
        ])
        .env("VEGVISIR_HOME", tmp.path().join("home"))
        .output()?;
    assert!(verify.status.success());
    let verify_stdout = String::from_utf8_lossy(&verify.stdout);
    assert!(verify_stdout.contains("ok runtime/approvals"));
    assert!(verify_stdout.contains("ok runtime/dangerous_bypass disabled startup_only=true"));

    let skiller_help = std::process::Command::new(binary)
        .args(["skiller", "--", "--help"])
        .output()?;
    assert!(skiller_help.status.success());
    let skiller_stdout = String::from_utf8_lossy(&skiller_help.stdout);
    assert!(skiller_stdout.contains("Compile technical sources"));
    assert!(skiller_stdout.contains("forge-handoff"));
    Ok(())
}

#[test]
fn resume_from_snapshot_continues_existing_run() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut args = Map::new();
    args.insert("path".to_string(), Value::String(".".to_string()));
    let first = ScriptedModel::new(vec![AgentDecision {
        thought: "Need evidence.".to_string(),
        action: Some("list_files".to_string()),
        args,
        final_answer: None,
    }]);
    let mut harness = AgentHarness::default(first, tmp.path())?;
    let mut task = AgentTask::new("inspect", tmp.path());
    task.max_steps = 1;
    let partial = harness.run(task)?;

    let second = ScriptedModel::new(vec![AgentDecision::final_decision("Now done.", "resumed")]);
    let mut resumed = AgentHarness::default(second, tmp.path())?;
    let mut resume_task = AgentTask::new("inspect", tmp.path());
    resume_task.resume_run_id = Some(partial.run_id.clone());
    let result = resumed.run(resume_task)?;

    assert_eq!(result.run_id, partial.run_id);
    assert_eq!(result.status, "completed");
    assert_eq!(result.final_answer.as_deref(), Some("resumed"));
    Ok(())
}

#[test]
fn sandbox_and_guardrails_block_risky_actions() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let sandbox = WorkspaceSandbox::new(tmp.path())?;
    assert!(sandbox.resolve("../outside.txt").is_err());
    let err = sandbox.resolve("~/notes.txt").unwrap_err().to_string();
    assert!(err.contains("Home-relative paths are not supported"));

    let mut guardrails = GuardrailEngine {
        policy: PermissionPolicy {
            allow_risky_tools: false,
            ..PermissionPolicy::default()
        },
        ..GuardrailEngine::default()
    };
    let tool = Tool::new(
        "write_file",
        "write",
        Arc::new(|_| Observation::ok("ok")),
        json!({}),
        true,
    );
    assert!(guardrails.authorize_tool(&tool, &Map::new()).is_err());
    Ok(())
}

#[test]
fn command_allow_list_blocks_unknown_executables() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    fs::write(tmp.path().join("sample.txt"), "needle\n")?;
    let mut registry = build_builtin_registry(tmp.path())?;
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry: registry.clone(),
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };
    let grep = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["grep", "needle", "sample.txt"]})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(grep.ok, "{}", grep.content);
    assert!(grep.content.contains("needle"));

    let obs = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["rm", "-rf", "."]})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(!obs.ok);
    assert!(obs.content.contains("Shell command is not allow-listed"));
    assert!(obs.content.contains("approval_id="));
    registry.register(Tool::new(
        "noop",
        "noop",
        Arc::new(|_| Observation::ok("ok")),
        json!({}),
        false,
    ))?;
    Ok(())
}

#[test]
fn command_allow_list_can_be_managed_and_approved_from_tui() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
    let commands = app.execute_command("/tools commands")?.unwrap();
    assert!(commands.contains("grep"), "{commands}");
    assert!(commands.contains("rg"), "{commands}");

    let removed = app.execute_command("/tools commands remove grep")?.unwrap();
    assert!(removed.contains("grep"));
    let commands = app.execute_command("/tools commands")?.unwrap();
    assert!(!commands.contains("grep,"));
    let added = app.execute_command("/tools commands add grep")?.unwrap();
    assert!(added.contains("grep"));

    app.execute_command("/tools allow-risky")?;
    let blocked = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["sh", "-c", "printf approved-command"]})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(!blocked.ok);
    assert!(
        blocked
            .content
            .contains("Shell command is not allow-listed")
    );
    let approval_id = app
        .tool_executor
        .guardrails
        .approvals
        .pending_ids()
        .first()
        .cloned()
        .expect("pending command approval");
    let approved = app
        .execute_command(&format!("/approvals session {approval_id}"))?
        .unwrap();
    assert!(
        app.ephemeral_notice
            .as_ref()
            .is_some_and(|notice| notice.text.contains("allowed shell command `sh`")),
        "{approved}"
    );
    assert!(
        !approved.contains("allowed shell command `sh`"),
        "{approved}"
    );
    assert!(approved.contains("approved-command"), "{approved}");
    assert!(
        app.tool_executor
            .guardrails
            .policy
            .allowed_commands
            .contains("sh")
    );
    Ok(())
}

#[test]
fn startup_dangerous_bypass_authorizes_tools_commands_and_sandbox_escape() -> anyhow::Result<()> {
    let _env_guard = env_lock().lock().unwrap();
    let _network_env = EnvVarGuard::set(COMMAND_SANDBOX_NETWORK_ENV, "inherit");
    let tmp = tempdir()?;
    let workspace = tmp.path().join("workspace");
    let outside = tmp.path().join("outside.txt");
    fs::create_dir_all(&workspace)?;
    let mut app = TuiApplication::with_data_root_and_dangerous_bypass(
        &workspace,
        tmp.path().join("home"),
        true,
    )?;

    let status = app.execute_command("/tools status")?.unwrap();
    assert!(status.contains("Dangerous bypass: enabled at startup"));
    assert!(status.contains("Workspace file containment: disabled by dangerous bypass"));
    assert!(status.contains("File access hardening: disabled by dangerous bypass"));
    assert!(status.contains("Command OS sandbox: disabled by dangerous bypass"));
    assert!(status.contains("Bubblewrap available: "));
    assert!(status.contains("Command network policy: inherit"));
    assert_eq!(
        app.execute_command("/tools deny-risky")?.unwrap(),
        "Dangerous bypass was enabled at startup and cannot be changed from the TUI."
    );

    let command = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["sh", "-c", "printf bypass"]})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(command.ok, "{}", command.content);
    assert_eq!(command.content, "bypass");

    let write = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "write_file".to_string(),
        args: json!({"path": outside.display().to_string(), "content": "escaped"})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(write.ok, "{}", write.content);
    assert_eq!(fs::read_to_string(&outside)?, "escaped");

    app.execute_command("/agent create limited | tester | Limited | Limited tools.")?;
    app.execute_command("/agent allow-tool limited read_file")?;
    app.execute_command("/agent use limited")?;
    let allowed_despite_agent_policy = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["sh", "-c", "printf agent-bypass"]})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(
        allowed_despite_agent_policy.ok,
        "{}",
        allowed_despite_agent_policy.content
    );
    assert_eq!(allowed_despite_agent_policy.content, "agent-bypass");
    Ok(())
}

#[test]
fn msp_client_import_skiller_tool_publishes_current_bundle_shape() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let workspace = tmp.path().join("workspace");
    fs::create_dir_all(&workspace)?;
    let source_bundle = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../components/msp/examples/skiller-bundles/sample-rust-bundle");
    let bundle = workspace.join("bundle");
    copy_dir_recursive_for_msp_tool_test(&source_bundle, &bundle)?;
    fs::write(
        bundle.join("sources/sections.yaml"),
        r#"- section_id: sec-1
  source_id: src-1
  title: Small-step refactor workflow
  content: Refactor in small steps and verify each step.
"#,
    )?;
    fs::write(bundle.join("candidates.yaml"), "[]\n")?;
    fs::write(bundle.join("forge_requests.yaml"), "[]\n")?;
    fs::write(bundle.join("forge_responses.yaml"), "[]\n")?;
    fs::write(bundle.join("MANIFEST.sha256"), "sha256:test\n")?;

    let registry_path = workspace.join("registry");
    let mut executor = ToolExecutor {
        registry: build_builtin_registry(&workspace)?,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let imported = executor.execute(ToolCall {
        name: "msp_client_import_skiller".to_string(),
        args: json!({
            "registry": "registry",
            "bundle": "bundle",
            "issuer": "vegvisir-test.local"
        })
        .as_object()
        .unwrap()
        .clone(),
    });
    assert!(imported.ok, "{}", imported.content);
    assert_eq!(
        imported
            .data
            .get("registry")
            .and_then(|value| value.get("skill_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert!(registry_path.join("skills").exists());

    let loaded = executor.execute(ToolCall {
        name: "msp_client_load".to_string(),
        args: json!({
            "registry": "registry",
            "id": "skill.software_engineering.rust.refactor-module.v1",
            "mode": "card"
        })
        .as_object()
        .unwrap()
        .clone(),
    });
    assert!(loaded.ok, "{}", loaded.content);
    assert!(loaded.content.contains("Refactor Rust Module"));
    assert_eq!(
        loaded.data.get("body_hash_valid").and_then(Value::as_bool),
        Some(true)
    );

    Ok(())
}

fn copy_dir_recursive_for_msp_tool_test(
    src: &std::path::Path,
    dst: &std::path::Path,
) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive_for_msp_tool_test(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[test]
fn risky_tools_use_pending_approval_queue() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let registry = build_builtin_registry(tmp.path())?;
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                require_human_approval: true,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };
    let call = vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["pwd"]}).as_object().unwrap().clone(),
    };

    let blocked = executor.execute(call.clone());
    assert!(!blocked.ok);
    assert!(blocked.content.contains("approval_id="));
    let approval_id = executor
        .guardrails
        .approvals
        .pending_ids()
        .first()
        .cloned()
        .expect("pending approval");
    assert!(executor.guardrails.approvals.approve_once(&approval_id));
    let allowed = executor.execute(call);
    assert!(allowed.ok, "{}", allowed.content);
    assert_eq!(executor.guardrails.approvals.pending_len(), 0);
    Ok(())
}

#[test]
fn approval_queue_can_allow_matching_action_for_current_session_only() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let registry = build_builtin_registry(tmp.path())?;
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                require_human_approval: true,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };
    let call = vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["pwd"]}).as_object().unwrap().clone(),
    };
    let different_call = vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["pwd"], "timeout": 1})
            .as_object()
            .unwrap()
            .clone(),
    };

    let blocked = executor.execute(call.clone());
    assert!(!blocked.ok);
    let approval_id = executor
        .guardrails
        .approvals
        .pending_ids()
        .first()
        .cloned()
        .expect("pending approval");
    let session_request = executor
        .guardrails
        .approvals
        .approve_for_session(&approval_id)
        .expect("session approval");
    assert_eq!(session_request.tool_name, "run_command");
    assert_eq!(executor.guardrails.approvals.pending_len(), 0);

    let first = executor.execute(call.clone());
    assert!(first.ok, "{}", first.content);
    let second = executor.execute(call);
    assert!(second.ok, "{}", second.content);
    let different = executor.execute(different_call);
    assert!(!different.ok);
    assert!(different.content.contains("approval_id="));
    Ok(())
}

#[test]
fn session_approvals_are_not_persisted_across_restarts() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let approval_path = tmp.path().join("approvals.json");
    let registry = build_builtin_registry(tmp.path())?;
    let approvals = vegvisir_rust::guardrails::ApprovalLedger::new_persisted(&approval_path)?;
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry: registry.clone(),
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                require_human_approval: true,
                ..PermissionPolicy::default()
            },
            approvals,
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };
    let call = vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["pwd"]}).as_object().unwrap().clone(),
    };

    let blocked = executor.execute(call.clone());
    assert!(!blocked.ok);
    let approval_id = executor
        .guardrails
        .approvals
        .pending_ids()
        .first()
        .cloned()
        .expect("pending approval");
    assert!(
        executor
            .guardrails
            .approvals
            .approve_for_session(&approval_id)
            .is_some()
    );
    assert!(executor.execute(call.clone()).ok);

    let reloaded_approvals =
        vegvisir_rust::guardrails::ApprovalLedger::new_persisted(&approval_path)?;
    let mut reloaded_executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                require_human_approval: true,
                ..PermissionPolicy::default()
            },
            approvals: reloaded_approvals,
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };
    let blocked_after_reload = reloaded_executor.execute(call);
    assert!(!blocked_after_reload.ok);
    assert!(blocked_after_reload.content.contains("approval_id="));
    Ok(())
}

#[test]
fn approval_queue_can_edit_arguments_before_approval() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let registry = build_builtin_registry(tmp.path())?;
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                require_human_approval: true,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };
    let original = vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["pwd"]}).as_object().unwrap().clone(),
    };
    let edited = vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["pwd"], "timeout": 1})
            .as_object()
            .unwrap()
            .clone(),
    };

    let blocked = executor.execute(original.clone());
    assert!(!blocked.ok);
    let original_id = executor
        .guardrails
        .approvals
        .pending_ids()
        .first()
        .cloned()
        .expect("pending approval");
    let edited_request = executor
        .guardrails
        .approvals
        .edit(&original_id, edited.args.clone())
        .expect("edited approval");
    assert_ne!(edited_request.id, original_id);
    assert!(!executor.guardrails.approvals.approve_once(&original_id));
    assert!(
        executor
            .guardrails
            .approvals
            .approve_once(&edited_request.id)
    );
    let allowed = executor.execute(edited);
    assert!(allowed.ok, "{}", allowed.content);
    assert_eq!(executor.guardrails.approvals.pending_len(), 0);
    Ok(())
}

#[test]
fn approval_ledger_is_shared_across_executor_clones_and_persisted() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let approval_path = tmp.path().join("approvals.json");
    let registry = build_builtin_registry(tmp.path())?;
    let approvals = vegvisir_rust::guardrails::ApprovalLedger::new_persisted(&approval_path)?;
    let executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                require_human_approval: true,
                ..PermissionPolicy::default()
            },
            approvals,
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };
    let mut worker_executor = executor.clone();
    let blocked = worker_executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["pwd"]}).as_object().unwrap().clone(),
    });
    assert!(!blocked.ok);
    assert_eq!(executor.guardrails.approvals.pending_len(), 1);
    assert!(approval_path.exists());

    let reloaded = vegvisir_rust::guardrails::ApprovalLedger::new_persisted(&approval_path)?;
    assert_eq!(reloaded.pending_len(), 1);
    assert_eq!(
        reloaded.pending_ids(),
        executor.guardrails.approvals.pending_ids()
    );
    Ok(())
}

#[test]
fn run_command_enforces_timeout_and_output_limit() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let registry = build_builtin_registry(tmp.path())?;
    let mut allowed_commands = PermissionPolicy::default().allowed_commands;
    allowed_commands.insert("sleep".to_string());
    allowed_commands.insert("printf".to_string());
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                allowed_commands,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let timed_out = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["sleep", "2"], "timeout": 1})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(!timed_out.ok);
    assert_eq!(timed_out.error.as_deref(), Some("CommandTimeout"));
    assert_eq!(timed_out.data.get("timed_out"), Some(&json!(true)));

    let truncated = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["printf", "%5000s", "x"], "output_limit": 1024})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(truncated.ok, "{}", truncated.content);
    assert!(truncated.content.contains("[output compacted:"));
    assert_eq!(truncated.data.get("output_truncated"), Some(&json!(true)));
    Ok(())
}

#[test]
fn run_tests_executes_bounded_test_command() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let registry = build_builtin_registry(tmp.path())?;
    let mut allowed_commands = PermissionPolicy::default().allowed_commands;
    allowed_commands.insert("printf".to_string());
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                allowed_commands,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let observed = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_tests".to_string(),
        args: json!({"command": ["printf", "tests-ok"], "timeout": 1})
            .as_object()
            .unwrap()
            .clone(),
    });

    assert!(observed.ok, "{}", observed.content);
    assert!(observed.content.contains("tests-ok"));
    assert_eq!(
        observed.data.get("command"),
        Some(&json!(["printf", "tests-ok"]))
    );
    Ok(())
}

#[test]
fn run_command_reports_bubblewrap_sandbox_metadata_when_enabled() -> anyhow::Result<()> {
    if !bwrap_available() {
        return Ok(());
    }
    let _guard = env_lock().lock().unwrap();
    let _sandbox_env = EnvVarGuard::set(COMMAND_SANDBOX_ENV, "bwrap");
    let tmp = tempdir()?;
    let registry = build_builtin_registry(tmp.path())?;
    let mut allowed_commands = PermissionPolicy::default().allowed_commands;
    allowed_commands.insert("printf".to_string());
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                allowed_commands,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let observed = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["printf", "bwrap-ok"], "timeout": 1})
            .as_object()
            .unwrap()
            .clone(),
    });

    assert!(observed.ok, "{}", observed.content);
    assert!(observed.content.contains("bwrap-ok"));
    assert_eq!(observed.data.get("command_sandboxed"), Some(&json!(true)));
    assert_eq!(
        observed.data.get("command_sandbox"),
        Some(&json!("bubblewrap"))
    );
    Ok(())
}

#[test]
fn bubblewrap_unsets_secret_env_but_keeps_nonsecret_env() -> anyhow::Result<()> {
    if !bwrap_available() {
        return Ok(());
    }
    let _guard = env_lock().lock().unwrap();
    let _sandbox_env = EnvVarGuard::set(COMMAND_SANDBOX_ENV, "bwrap");
    let _secret_env = EnvVarGuard::set("VEGVISIR_TEST_API_KEY", "host-secret");
    let _visible_env = EnvVarGuard::set("VEGVISIR_TEST_VISIBLE_SETTING", "host-visible");
    let tmp = tempdir()?;
    let registry = build_builtin_registry(tmp.path())?;
    let mut allowed_commands = PermissionPolicy::default().allowed_commands;
    allowed_commands.insert("sh".to_string());
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                allowed_commands,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let observed = execute_after_one_approval(&mut executor, vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({
            "command": [
                "sh",
                "-c",
                "test -z \"${VEGVISIR_TEST_API_KEY+x}\" && test \"$VEGVISIR_TEST_VISIBLE_SETTING\" = host-visible && printf env-ok"
            ],
            "timeout": 1
        })
        .as_object()
        .unwrap()
        .clone(),
    });

    assert!(observed.ok, "{}", observed.content);
    assert!(observed.content.contains("env-ok"));
    assert_eq!(observed.data.get("command_sandboxed"), Some(&json!(true)));
    assert_eq!(
        observed.data.get("command_sandbox"),
        Some(&json!("bubblewrap"))
    );
    Ok(())
}

#[test]
fn bubblewrap_network_disable_is_reported_and_enforced() -> anyhow::Result<()> {
    if !bwrap_available() {
        return Ok(());
    }
    let _guard = env_lock().lock().unwrap();
    let _sandbox_env = EnvVarGuard::set(COMMAND_SANDBOX_ENV, "bwrap");
    let _network_env = EnvVarGuard::set(COMMAND_SANDBOX_NETWORK_ENV, "disable");
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    let mut app = TuiApplication::with_data_root_and_dangerous_bypass(tmp.path(), &home, false)?;

    let status = app.execute_command("/tools status")?.unwrap();
    assert!(status.contains("Command OS sandbox: configured: bubblewrap"));
    assert!(status.contains("Command network policy: disabled"));
    let runtime = app.execute_command("/verify runtime")?.unwrap();
    assert!(runtime.contains("ok runtime/command_network_policy disabled"));

    let registry = build_builtin_registry(tmp.path())?;
    let mut allowed_commands = PermissionPolicy::default().allowed_commands;
    allowed_commands.insert("printf".to_string());
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                allowed_commands,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let observed = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["printf", "network-policy-ok"], "timeout": 1})
            .as_object()
            .unwrap()
            .clone(),
    });

    assert!(observed.ok, "{}", observed.content);
    assert!(observed.content.contains("network-policy-ok"));
    assert_eq!(
        observed.data.get("command_network_policy"),
        Some(&json!("disabled"))
    );
    Ok(())
}

#[test]
fn network_require_approval_enqueues_network_command_approval() -> anyhow::Result<()> {
    let _guard = env_lock().lock().unwrap();
    let _network_env = EnvVarGuard::set(COMMAND_SANDBOX_NETWORK_ENV, "require-approval");
    let tmp = tempdir()?;
    let registry = build_builtin_registry(tmp.path())?;
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let blocked = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["git", "fetch"], "timeout": 1})
            .as_object()
            .unwrap()
            .clone(),
    });

    assert!(!blocked.ok);
    assert!(
        blocked
            .content
            .contains("Command network access requires human approval")
    );
    assert!(blocked.content.contains("approval_id="));
    let pending = executor.guardrails.approvals.pending();
    assert_eq!(pending.len(), 1);
    let request = pending.values().next().expect("pending network approval");
    assert_eq!(request.risk_label, "command-network");
    assert!(request.reason.contains("git fetch"));

    let safe_local = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["git", "status"], "timeout": 1})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(
        !safe_local
            .content
            .contains("Command network access requires human approval"),
        "{}",
        safe_local.content
    );
    Ok(())
}

#[test]
fn bubblewrap_hidden_paths_env_hides_host_file() -> anyhow::Result<()> {
    if !bwrap_available() {
        return Ok(());
    }
    let _guard = env_lock().lock().unwrap();
    let _sandbox_env = EnvVarGuard::set(COMMAND_SANDBOX_ENV, "bwrap");
    let mut hidden_file = NamedTempFile::new()?;
    hidden_file.write_all(b"host-secret")?;
    let _hidden_env = EnvVarGuard::set(COMMAND_SANDBOX_HIDDEN_PATHS_ENV, hidden_file.path());
    let tmp = tempdir()?;
    let registry = build_builtin_registry(tmp.path())?;
    let mut allowed_commands = PermissionPolicy::default().allowed_commands;
    allowed_commands.insert("sh".to_string());
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                allowed_commands,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let script = format!(
        "test ! -s '{}' && printf hidden-ok",
        hidden_file.path().display()
    );
    let observed = execute_after_one_approval(
        &mut executor,
        vegvisir_rust::types::ToolCall {
            name: "run_command".to_string(),
            args: json!({"command": ["sh", "-c", script], "timeout": 1})
                .as_object()
                .unwrap()
                .clone(),
        },
    );

    assert!(observed.ok, "{}", observed.content);
    assert!(observed.content.contains("hidden-ok"));
    assert_eq!(observed.data.get("command_sandboxed"), Some(&json!(true)));
    Ok(())
}

#[test]
fn tools_status_reports_strict_bubblewrap_mode_when_enabled() -> anyhow::Result<()> {
    let _guard = env_lock().lock().unwrap();
    let _sandbox_env = EnvVarGuard::set(COMMAND_SANDBOX_ENV, "strict-bwrap");
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    let mut app = TuiApplication::with_data_root_and_dangerous_bypass(tmp.path(), &home, false)?;

    let status = app.execute_command("/tools status")?.unwrap();
    if bwrap_available() {
        assert!(status.contains("Command OS sandbox: configured: strict-bubblewrap"));
    } else {
        assert!(status.contains("Command OS sandbox: configured but bubblewrap unavailable"));
    }
    assert!(status.contains("Command network policy: disabled"));
    assert!(status.contains("Writable mount policy: workspace writable"));
    assert!(status.contains("Read-only mount policy: system mounts read-only"));

    let runtime = app.execute_command("/verify runtime")?.unwrap();
    if bwrap_available() {
        assert!(runtime.contains("ok runtime/command_os_sandbox configured:_strict-bubblewrap"));
    } else {
        assert!(
            runtime.contains("ok runtime/command_os_sandbox configured_but_bubblewrap_unavailable")
        );
    }
    assert!(runtime.contains("ok runtime/command_network_policy disabled"));
    assert!(runtime.contains("ok runtime/command_mount_policy writable=workspace_writable"));
    assert!(runtime.contains("readonly=system_mounts_read-only"));
    Ok(())
}

#[test]
fn strict_bubblewrap_allows_workspace_write_and_hides_host_tmp() -> anyhow::Result<()> {
    if !bwrap_available() {
        return Ok(());
    }
    let _guard = env_lock().lock().unwrap();
    let _sandbox_env = EnvVarGuard::set(COMMAND_SANDBOX_ENV, "strict-bwrap");
    let _secret_env = EnvVarGuard::set("VEGVISIR_TEST_STRICT_SECRET", "host-secret");
    let tmp = tempdir()?;
    let mut host_tmp_marker = NamedTempFile::new()?;
    host_tmp_marker.write_all(b"host-only")?;
    let registry = build_builtin_registry(tmp.path())?;
    let mut allowed_commands = PermissionPolicy::default().allowed_commands;
    allowed_commands.insert("sh".to_string());
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                allowed_commands,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };
    let host_tmp_marker_path = host_tmp_marker.path().display().to_string();

    let script = format!(
        "test \"$HOME\" = /home/vegvisir && test -z \"${{VEGVISIR_TEST_STRICT_SECRET+x}}\" && test ! -e '{}' && printf strict-ok > strict.txt && printf strict-ran",
        host_tmp_marker_path
    );
    let observed = execute_after_one_approval(
        &mut executor,
        vegvisir_rust::types::ToolCall {
            name: "run_command".to_string(),
            args: json!({"command": ["sh", "-c", script], "timeout": 1})
                .as_object()
                .unwrap()
                .clone(),
        },
    );

    assert!(observed.ok, "{}", observed.content);
    assert!(observed.content.contains("strict-ran"));
    assert_eq!(
        fs::read_to_string(tmp.path().join("strict.txt"))?,
        "strict-ok"
    );
    assert_eq!(observed.data.get("command_sandboxed"), Some(&json!(true)));
    assert_eq!(
        observed.data.get("command_sandbox"),
        Some(&json!("strict-bubblewrap"))
    );
    Ok(())
}

#[test]
fn provider_and_model_catalogs_load() -> anyhow::Result<()> {
    let providers = ProviderRegistry::default_catalog()?;
    let models = ModelRegistry::default_catalog()?;

    assert_eq!(
        providers.get("openai").unwrap().api_key_env.as_deref(),
        Some("OPENAI_API_KEY")
    );
    assert_eq!(providers.get("openai-sso").unwrap().auth_type, "oauth");
    assert_eq!(providers.get("openai-hbse").unwrap().auth_type, "hbse");
    assert_eq!(providers.get("xai-hbse").unwrap().auth_type, "hbse");
    assert_eq!(providers.get("anthropic-hbse").unwrap().auth_type, "hbse");
    assert_eq!(
        providers.get("anthropic-hbse").unwrap().kind,
        "hbse_anthropic"
    );
    assert_eq!(providers.get("google-hbse").unwrap().auth_type, "hbse");
    assert_eq!(providers.get("google-hbse").unwrap().kind, "hbse_google");
    assert_eq!(
        providers.get("azure-openai-hbse").unwrap().auth_type,
        "hbse"
    );
    assert_eq!(
        providers.get("azure-openai-hbse").unwrap().kind,
        "hbse_azure_openai"
    );
    for provider in [
        "mistral-hbse",
        "groq-hbse",
        "openrouter-hbse",
        "deepseek-hbse",
        "together-hbse",
        "perplexity-hbse",
    ] {
        assert_eq!(providers.get(provider).unwrap().auth_type, "hbse");
        assert_eq!(
            providers.get(provider).unwrap().kind,
            "hbse_openai_compatible"
        );
    }
    assert_eq!(models.get("demo-local").unwrap().provider, "demo");
    assert_eq!(
        models.default_for_provider("openai").unwrap().name,
        "gpt-5.4-mini"
    );
    assert_eq!(
        models.default_for_provider("openai-sso").unwrap().name,
        "gpt-5.4-mini"
    );
    assert_eq!(
        models.default_for_provider("openai-hbse").unwrap().name,
        "gpt-5.4-mini"
    );
    assert_eq!(
        models.default_for_provider("anthropic-hbse").unwrap().name,
        "claude-opus-4-1"
    );
    assert_eq!(
        models.default_for_provider("google-hbse").unwrap().name,
        "gemini-2.5-pro"
    );
    assert!(
        models.default_for_provider("xai").is_none(),
        "xAI models must be discovered from the provider, not seeded from the static catalog"
    );
    assert!(
        models.default_for_provider("xai-hbse").is_none(),
        "xAI HBSE models must be discovered from the provider, not seeded from the static catalog"
    );
    assert_eq!(
        models.default_for_provider("mistral-hbse").unwrap().name,
        "mistral-large-latest"
    );
    assert_eq!(
        models.default_for_provider("groq-hbse").unwrap().name,
        "llama-3.3-70b-versatile"
    );
    assert_eq!(
        models.default_for_provider("openrouter-hbse").unwrap().name,
        "openai/gpt-5.4"
    );
    assert_eq!(
        models.default_for_provider("deepseek-hbse").unwrap().name,
        "deepseek-chat"
    );
    assert_eq!(
        models.default_for_provider("together-hbse").unwrap().name,
        "meta-llama/Llama-3.3-70B-Instruct-Turbo"
    );
    assert_eq!(
        models.default_for_provider("perplexity-hbse").unwrap().name,
        "sonar-pro"
    );
    assert_eq!(
        models
            .default_for_provider("azure-openai-hbse")
            .unwrap()
            .name,
        "azure:gpt-5.4"
    );
    assert!(models.is_model_allowed_for_provider(models.get("gpt-5.5").unwrap(), "openai-sso"));
    for model in ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
        assert!(models.is_model_allowed_for_provider(models.get(model).unwrap(), "openai-sso"));
    }
    assert!(models.is_model_allowed_for_provider(models.get("gpt-5.5").unwrap(), "openai-hbse"));
    assert!(
        models.is_model_allowed_for_provider(
            models.get("claude-sonnet-4-5").unwrap(),
            "anthropic-hbse"
        )
    );
    assert!(
        models
            .is_model_allowed_for_provider(models.get("gemini-2.5-flash").unwrap(), "google-hbse")
    );
    assert!(models.is_model_allowed_for_provider(
        models.get("mistral-large-latest").unwrap(),
        "mistral-hbse"
    ));
    assert!(
        models.is_model_allowed_for_provider(
            models.get("openai/gpt-5.4").unwrap(),
            "openrouter-hbse"
        )
    );
    assert!(
        models.is_model_allowed_for_provider(
            models.get("azure:gpt-5.4").unwrap(),
            "azure-openai-hbse"
        )
    );
    let router = ProviderRouter::from_registry(&providers);
    assert_eq!(router.get("demo").unwrap().config().name, "demo");
    assert_eq!(
        router
            .for_model(models.get("gpt-5.5").unwrap(), "openai-sso")
            .unwrap()
            .config()
            .name,
        "openai-sso"
    );
    assert_eq!(
        router
            .for_model(models.get("gpt-5.5").unwrap(), "openai-hbse")
            .unwrap()
            .config()
            .name,
        "openai-hbse"
    );
    assert_eq!(
        router
            .for_model(models.get("claude-sonnet-4-5").unwrap(), "anthropic-hbse")
            .unwrap()
            .config()
            .name,
        "anthropic-hbse"
    );
    assert_eq!(
        router
            .for_model(models.get("gemini-2.5-flash").unwrap(), "google-hbse")
            .unwrap()
            .config()
            .name,
        "google-hbse"
    );
    assert_eq!(
        router
            .for_model(models.get("mistral-large-latest").unwrap(), "mistral-hbse")
            .unwrap()
            .config()
            .name,
        "mistral-hbse"
    );
    assert_eq!(
        router
            .for_model(models.get("azure:gpt-5.4").unwrap(), "azure-openai-hbse")
            .unwrap()
            .config()
            .name,
        "azure-openai-hbse"
    );
    assert_eq!(
        router
            .for_model(models.get("gpt-5.5").unwrap(), "demo")
            .unwrap()
            .config()
            .name,
        "openai"
    );
    Ok(())
}

#[test]
fn conversation_runner_preserves_selected_alias_provider() -> anyhow::Result<()> {
    for (provider, model_name) in [
        ("openai-hbse", "gpt-5.5"),
        ("anthropic-hbse", "claude-sonnet-4-5"),
        ("google-hbse", "gemini-2.5-flash"),
        ("mistral-hbse", "mistral-large-latest"),
        ("openrouter-hbse", "openai/gpt-5.4"),
        ("deepseek-hbse", "deepseek-chat"),
        ("together-hbse", "meta-llama/Llama-3.3-70B-Instruct-Turbo"),
        ("perplexity-hbse", "sonar-pro"),
        ("azure-openai-hbse", "azure:gpt-5.4"),
    ] {
        let selected_provider = Arc::new(Mutex::new(None));
        let config = ProviderConfig {
            name: provider.to_string(),
            display_name: None,
            kind: "test".to_string(),
            api_key_env: None,
            base_url: None,
            auth_type: "none".to_string(),
            enabled: true,
            metadata: Default::default(),
        };
        let mut runner = ConversationRunner {
            provider: RecordingProvider {
                config,
                selected_provider: selected_provider.clone(),
            },
            models: ModelRegistry::default_catalog()?,
            tools: None,
            tool_executor: None,
            event_sink: None,
            cancel_token: None,
            steering_rx: None,
        };
        let workspace = tempdir()?;
        let mut session =
            vegvisir_rust::core::SessionState::new(workspace.path(), Vec::new(), Vec::new());
        session.current_provider = provider.to_string();
        session.current_model = model_name.to_string();

        let response = runner.send(&mut session, "test")?;

        assert_eq!(response, "ok");
        assert_eq!(selected_provider.lock().unwrap().as_deref(), Some(provider));
        assert_eq!(session.current_provider, provider);
    }
    Ok(())
}

#[test]
fn conversation_runner_does_not_send_saved_ui_notes_as_system_prompt() -> anyhow::Result<()> {
    let recorded_messages = Arc::new(Mutex::new(Vec::new()));
    let mut runner = ConversationRunner {
        provider: MessageRecordingProvider {
            config: ProviderConfig {
                name: "demo".to_string(),
                display_name: None,
                kind: "test".to_string(),
                api_key_env: None,
                base_url: None,
                auth_type: "none".to_string(),
                enabled: true,
                metadata: Default::default(),
            },
            messages: recorded_messages.clone(),
        },
        models: ModelRegistry::default_catalog()?,
        tools: None,
        tool_executor: None,
        event_sink: None,
        cancel_token: None,
        steering_rx: None,
    };
    let workspace = tempdir()?;
    let mut session =
        vegvisir_rust::core::SessionState::new(workspace.path(), Vec::new(), Vec::new());
    session.system_prompt = "real harness prompt".to_string();
    session.messages.push(ChatMessage {
        role: "system".to_string(),
        content: "note from /system show that should stay out of provider input".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    });

    runner.send(&mut session, "hello")?;

    let messages = recorded_messages.lock().unwrap();
    assert_eq!(messages[0].role, "system");
    assert_eq!(messages[0].content, "real harness prompt");
    assert!(
        !messages
            .iter()
            .any(|message| message.content.contains("note from /system show"))
    );
    Ok(())
}

#[test]
fn hbse_socket_resolution_matches_hbse_rust_service_defaults() -> anyhow::Result<()> {
    let _guard = env_lock().lock().unwrap();
    unsafe {
        std::env::remove_var("HBSE_BROKER_SOCKET");
        std::env::set_var("XDG_RUNTIME_DIR", "/tmp/vegvisir-runtime");
        std::env::set_var("HOME", "/tmp/vegvisir-home");
    }
    let mut provider = ProviderConfig {
        name: "openai-hbse".to_string(),
        display_name: None,
        kind: "hbse_openai_compatible".to_string(),
        api_key_env: None,
        base_url: Some("https://api.openai.com/v1".to_string()),
        auth_type: "hbse".to_string(),
        enabled: true,
        metadata: Default::default(),
    };

    assert_eq!(
        vegvisir_rust::provider::hbse_default_or_configured_socket(&provider),
        std::path::PathBuf::from("/tmp/vegvisir-runtime/hbse/broker.sock")
    );

    unsafe {
        std::env::set_var("HBSE_BROKER_SOCKET", "/tmp/explicit-hbse.sock");
    }
    assert_eq!(
        vegvisir_rust::provider::hbse_default_or_configured_socket(&provider),
        std::path::PathBuf::from("/tmp/explicit-hbse.sock")
    );

    provider.metadata.insert(
        "hbse_socket".to_string(),
        Value::String("/tmp/provider-hbse.sock".to_string()),
    );
    assert_eq!(
        vegvisir_rust::provider::hbse_default_or_configured_socket(&provider),
        std::path::PathBuf::from("/tmp/provider-hbse.sock")
    );

    unsafe {
        std::env::remove_var("HBSE_BROKER_SOCKET");
    }
    Ok(())
}

#[test]
fn provider_router_exposes_tool_calls_for_every_catalog_provider() -> anyhow::Result<()> {
    let providers = ProviderRegistry::default_catalog()?;
    let models = ModelRegistry::default_catalog()?;
    let router = ProviderRouter::from_registry(&providers);
    for provider in providers.list() {
        let Some(model) = models.default_for_provider(&provider.name) else {
            continue;
        };
        assert!(
            router
                .get(&provider.name)
                .unwrap()
                .supports_tool_calls(model, &provider.name),
            "{} should expose Vegvisir tools",
            provider.name
        );
    }
    Ok(())
}

#[test]
fn xai_models_are_not_seeded_in_static_catalog() -> anyhow::Result<()> {
    let models = ModelRegistry::default_catalog()?;

    assert!(models.default_for_provider("xai").is_none());
    assert!(models.default_for_provider("xai-hbse").is_none());
    assert!(
        models
            .list()
            .into_iter()
            .all(|model| model.provider != "xai"),
        "xAI models must be populated by provider discovery, not defaults/models.json"
    );
    Ok(())
}

#[test]
fn xai_model_discovery_reads_live_provider_models() -> anyhow::Result<()> {
    unsafe {
        std::env::set_var("VEGVISIR_TEST_XAI_DISCOVERY_KEY", "xai-discovery-key");
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            if String::from_utf8_lossy(&bytes).contains("\r\n\r\n") {
                break;
            }
        }
        let body =
            r#"{"data":[{"id":"xai-live-from-provider","display_name":"xAI Live From Provider"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    });

    let provider = ProviderConfig {
        name: "xai".to_string(),
        display_name: Some("xAI".to_string()),
        kind: "openai_compatible".to_string(),
        api_key_env: Some("VEGVISIR_TEST_XAI_DISCOVERY_KEY".to_string()),
        base_url: Some(format!("http://{}", addr)),
        auth_type: "api_key".to_string(),
        enabled: true,
        metadata: Default::default(),
    };

    let models = discover_provider_models(&provider)?;

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].name, "xai-live-from-provider");
    assert_eq!(
        models[0].display_name.as_deref(),
        Some("xAI Live From Provider")
    );
    assert_eq!(models[0].provider, "xai");
    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("GET /models HTTP/1.1"));
    assert!(request.contains("Authorization: Bearer xai-discovery-key"));
    Ok(())
}

#[test]
fn xai_hbse_model_discovery_routes_through_broker() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let socket_path = tmp.path().join("hbse-xai-discovery.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let server = thread::spawn(move || -> anyhow::Result<Value> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            if buffer[..n].contains(&b'\n') {
                break;
            }
        }
        let request: Value = serde_json::from_slice(
            bytes
                .split(|byte| *byte == b'\n')
                .next()
                .unwrap_or(bytes.as_slice()),
        )?;
        let response = json!({
            "ok": true,
            "status_code": 200,
            "body": r#"{"data":[{"id":"xai-hbse-live-from-provider","display_name":"xAI HBSE Live From Provider"}]}"#
        });
        stream.write_all(serde_json::to_string(&response)?.as_bytes())?;
        stream.write_all(b"\n")?;
        Ok(request)
    });
    let provider = ProviderConfig {
        name: "xai-hbse".to_string(),
        display_name: Some("xAI via HBSE".to_string()),
        kind: "hbse_openai_compatible".to_string(),
        api_key_env: None,
        base_url: Some("https://api.x.ai/v1".to_string()),
        auth_type: "hbse".to_string(),
        enabled: true,
        metadata: BTreeMap::from([
            (
                "hbse_socket".to_string(),
                Value::String(socket_path.display().to_string()),
            ),
            (
                "hbse_secret_ref".to_string(),
                Value::String("secret://vegvisir/providers/xai/test".to_string()),
            ),
            (
                "hbse_consumer".to_string(),
                Value::String("vegvisir.provider.xai-hbse.test".to_string()),
            ),
            (
                "hbse_model_discovery_purpose".to_string(),
                Value::String("model.discovery".to_string()),
            ),
        ]),
    };

    let models = discover_provider_models(&provider)?;

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].name, "xai-hbse-live-from-provider");
    assert_eq!(models[0].provider, "xai-hbse");
    let request = server.join().expect("server thread completed")?;
    assert_eq!(request["command"], "provider_http");
    assert_eq!(request["method"], "GET");
    assert_eq!(request["url"], "https://api.x.ai/v1/models");
    assert_eq!(
        request["secret_ref"],
        "secret://vegvisir/providers/xai/test"
    );
    assert_eq!(request["consumer"], "vegvisir.provider.xai-hbse.test");
    assert_eq!(request["purpose"], "model.discovery");
    assert_eq!(request["headers"]["Accept"], "application/json");
    Ok(())
}

#[test]
fn openai_compatible_model_discovery_reads_live_models() -> anyhow::Result<()> {
    unsafe {
        std::env::set_var("VEGVISIR_TEST_OPENAI_DISCOVERY_KEY", "discovery-key");
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            if String::from_utf8_lossy(&bytes).contains("\r\n\r\n") {
                break;
            }
        }
        let body = r#"{"data":[{"id":"gpt-live","display_name":"GPT Live"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    });

    let provider = ProviderConfig {
        name: "test-openai-discovery".to_string(),
        display_name: Some("Test Discovery".to_string()),
        kind: "openai_compatible".to_string(),
        api_key_env: Some("VEGVISIR_TEST_OPENAI_DISCOVERY_KEY".to_string()),
        base_url: Some(format!("http://{}", addr)),
        auth_type: "api_key".to_string(),
        enabled: true,
        metadata: Default::default(),
    };

    let models = discover_provider_models(&provider)?;

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].name, "gpt-live");
    assert_eq!(models[0].display_name.as_deref(), Some("GPT Live"));
    assert_eq!(models[0].provider, "test-openai-discovery");
    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("GET /models HTTP/1.1"));
    assert!(request.contains("Authorization: Bearer discovery-key"));
    Ok(())
}

#[test]
fn models_command_refreshes_selected_provider_models() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<()> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            if String::from_utf8_lossy(&bytes).contains("\r\n\r\n") {
                break;
            }
        }
        let body = r#"{"data":[{"id":"gpt-refreshed"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(())
    });

    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.provider_registry.register(ProviderConfig {
        name: "test-refresh".to_string(),
        display_name: Some("Test Refresh".to_string()),
        kind: "openai_compatible".to_string(),
        api_key_env: None,
        base_url: Some(format!("http://{}", addr)),
        auth_type: "none".to_string(),
        enabled: true,
        metadata: Default::default(),
    });
    app.session.current_provider = "test-refresh".to_string();
    app.session.current_model = "demo-local".to_string();

    let output = app.execute_command("/models")?.unwrap();

    assert!(output.contains("Available models for test-refresh [ready]:"));
    assert!(output.contains("Refreshed 1 model(s) from test-refresh."));
    assert!(output.contains("gpt-refreshed"));
    assert_eq!(app.session.current_model, "gpt-refreshed");
    server.join().expect("server thread completed")?;
    Ok(())
}

#[test]
fn model_selection_refreshes_before_rejecting_dynamic_models() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<()> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            if String::from_utf8_lossy(&bytes).contains("\r\n\r\n") {
                break;
            }
        }
        let body = r#"{"data":[{"id":"gpt-selected-live"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(())
    });

    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.provider_registry.register(ProviderConfig {
        name: "test-select-refresh".to_string(),
        display_name: Some("Test Select Refresh".to_string()),
        kind: "openai_compatible".to_string(),
        api_key_env: None,
        base_url: Some(format!("http://{}", addr)),
        auth_type: "none".to_string(),
        enabled: true,
        metadata: Default::default(),
    });
    app.session.current_provider = "test-select-refresh".to_string();
    app.session.current_model = "demo-local".to_string();

    let output = app.execute_command("/model gpt-selected-live")?.unwrap();

    assert!(output.contains("Selected model gpt-selected-live"));
    assert_eq!(app.session.current_model, "gpt-selected-live");
    server.join().expect("server thread completed")?;
    Ok(())
}

#[test]
fn startup_replaces_unknown_persisted_model_with_provider_default() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    fs::write(
        home.join("config.json"),
        json!({
            "current_provider": "openai-sso",
            "current_model": "gpt-future-dynamic"
        })
        .to_string(),
    )?;

    let app = TuiApplication::with_data_root(tmp.path(), &home)?;

    assert_eq!(app.session.current_provider, "openai-sso");
    assert_eq!(app.session.current_model, "gpt-5.4-mini");
    Ok(())
}

#[test]
fn openai_sso_model_discovery_uses_saved_tokens() -> anyhow::Result<()> {
    let _guard = env_lock().lock().unwrap();
    let tmp = tempdir()?;
    write_sso_auth(tmp.path(), "sso-access-token", "acct-test")?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            if String::from_utf8_lossy(&bytes).contains("\r\n\r\n") {
                break;
            }
        }
        let body = r#"{"models":[{"slug":"gpt-5.6-sol","display_name":"GPT-5.6-Sol","context_window":372000,"supported_in_api":true,"visibility":"list"},{"slug":"gpt-5.6-terra","display_name":"GPT-5.6-Terra","context_window":372000,"supported_in_api":true,"visibility":"list"},{"slug":"gpt-5.6-luna","display_name":"GPT-5.6-Luna","context_window":372000,"supported_in_api":true,"visibility":"list"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    });
    let provider = ProviderConfig {
        name: "openai-sso".to_string(),
        display_name: Some("OpenAI SSO".to_string()),
        kind: "openai_sso".to_string(),
        api_key_env: None,
        base_url: None,
        auth_type: "oauth".to_string(),
        enabled: true,
        metadata: [
            (
                "codex_base_url".to_string(),
                json!(format!("http://{}", addr)),
            ),
            (
                "auth_root".to_string(),
                json!(tmp.path().display().to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    };

    let models = discover_provider_models(&provider)?;

    assert_eq!(models.len(), 3);
    assert_eq!(models[0].name, "gpt-5.6-sol");
    assert_eq!(models[0].display_name.as_deref(), Some("GPT-5.6-Sol"));
    assert_eq!(models[0].context_window, Some(372000));
    assert_eq!(models[0].provider, "openai-sso");
    assert_eq!(models[1].name, "gpt-5.6-terra");
    assert_eq!(models[2].name, "gpt-5.6-luna");
    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("GET /models HTTP/1.1"));
    assert!(request.contains("Authorization: Bearer sso-access-token"));
    assert!(request.contains("ChatGPT-Account-ID: acct-test"));
    Ok(())
}

#[test]
fn app_openai_sso_availability_uses_data_root() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let workspace = tmp.path().join("workspace");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&workspace)?;
    write_sso_auth(&home, "rooted-access-token", "acct-rooted")?;

    let mut app = TuiApplication::with_data_root(&workspace, &home)?;

    let output = app.execute_command("/providers")?.unwrap();
    assert!(
        output
            .lines()
            .any(|line| line.starts_with("openai-sso") && line.contains("ready")),
        "{output}"
    );
    assert_eq!(
        app.provider_registry
            .get("openai-sso")
            .unwrap()
            .metadata
            .get("auth_root")
            .and_then(Value::as_str)
            .map(str::to_string),
        Some(home.display().to_string())
    );
    Ok(())
}

#[test]
fn openai_sso_provider_streams_responses_api_text() -> anyhow::Result<()> {
    let _guard = env_lock().lock().unwrap();
    let tmp = tempdir()?;
    write_sso_auth(tmp.path(), "sso-response-token", "acct-response")?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let body = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello \"}\n\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"sso\"}\n\ndata: [DONE]\n\n";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    });
    let adapter = OpenAISsoProfileAdapter {
        config: ProviderConfig {
            name: "openai-sso".to_string(),
            display_name: Some("OpenAI SSO".to_string()),
            kind: "openai_sso".to_string(),
            api_key_env: None,
            base_url: None,
            auth_type: "oauth".to_string(),
            enabled: true,
            metadata: [
                (
                    "codex_base_url".to_string(),
                    json!(format!("http://{}", addr)),
                ),
                (
                    "auth_root".to_string(),
                    json!(tmp.path().display().to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        },
    };
    let model = ModelInfo {
        name: "gpt-sso".to_string(),
        provider: "openai-sso".to_string(),
        display_name: None,
        context_window: Some(400000),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "answer tersely".to_string(),
            attachments: Vec::new(),
            created_at: Utc::now(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: "say hello".to_string(),
            attachments: Vec::new(),
            created_at: Utc::now(),
        },
    ];

    let response = adapter.complete(&messages, &model, "openai-sso")?;

    assert_eq!(response, "hello sso");
    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("POST /responses HTTP/1.1"));
    assert!(request.contains("Authorization: Bearer sso-response-token"));
    assert!(request.contains("ChatGPT-Account-ID: acct-response"));
    assert!(request.contains("\"instructions\":\"answer tersely\""));
    assert!(request.contains("\"input_text\""));
    assert!(request.contains("\"say hello\""));
    Ok(())
}

#[test]
fn openai_sso_provider_runs_responses_tool_loop() -> anyhow::Result<()> {
    let _guard = env_lock().lock().unwrap();
    let tmp = tempdir()?;
    write_sso_auth(tmp.path(), "sso-tool-token", "acct-tool")?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<Vec<String>> {
        let mut requests = Vec::new();
        for response_body in [
            [
                "data: ".to_string()
                    + &json!({
                        "type": "response.output_item.added",
                        "item": {
                            "id": "fc_1",
                            "type": "function_call",
                            "call_id": "call_1",
                            "name": "list_files",
                            "arguments": ""
                        }
                    })
                    .to_string(),
                "data: ".to_string()
                    + &json!({
                        "type": "response.function_call_arguments.delta",
                        "item_id": "fc_1",
                        "delta": "{\"path\":\".\",\"limit\":5}"
                    })
                    .to_string(),
                "data: ".to_string()
                    + &json!({
                        "type": "response.completed",
                        "response": {
                            "id": "resp_tool",
                            "output": []
                        }
                    })
                    .to_string(),
                "data: [DONE]".to_string(),
            ]
            .join("\n\n"),
            [
                "data: ".to_string()
                    + &json!({
                        "type": "response.completed",
                        "response": {
                            "id": "resp_done",
                            "error": null,
                            "output_text": "listed files",
                            "output": [{
                                "type": "message",
                                "content": [{"type": "output_text", "text": "listed files"}]
                            }]
                        }
                    })
                    .to_string(),
                "data: [DONE]".to_string(),
            ]
            .join("\n\n"),
        ] {
            let response_body = format!("{response_body}\n\n");
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let n = stream.read(&mut buffer)?;
                bytes.extend_from_slice(&buffer[..n]);
                let request = String::from_utf8_lossy(&bytes);
                let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                    continue;
                };
                let content_length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if body.len() >= content_length {
                    break;
                }
            }
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
                response_body.len(),
                response_body
            )?;
            requests.push(String::from_utf8_lossy(&bytes).to_string());
        }
        Ok(requests)
    });
    let adapter = OpenAISsoProfileAdapter {
        config: ProviderConfig {
            name: "openai-sso".to_string(),
            display_name: Some("OpenAI SSO".to_string()),
            kind: "openai_sso".to_string(),
            api_key_env: None,
            base_url: None,
            auth_type: "oauth".to_string(),
            enabled: true,
            metadata: [
                (
                    "codex_base_url".to_string(),
                    json!(format!("http://{}", addr)),
                ),
                (
                    "auth_root".to_string(),
                    json!(tmp.path().display().to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        },
    };
    let model = ModelInfo {
        name: "gpt-sso".to_string(),
        provider: "openai-sso".to_string(),
        display_name: None,
        context_window: Some(400000),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: "list files".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    }];
    let tools = vec![json!({
        "name": "list_files",
        "description": "List files",
        "parameters": {
            "required": ["path"],
            "properties": {"path": "string", "limit": "integer"}
        }
    })];
    let mut called = false;
    let mut streamed = String::new();
    let response = adapter.complete_with_tools_streaming(
        &messages,
        &model,
        &tools,
        &mut |name, args| {
            called = true;
            assert_eq!(name, "list_files");
            assert_eq!(args.get("path").and_then(Value::as_str), Some("."));
            "Cargo.toml\nsrc".to_string()
        },
        "openai-sso",
        &mut |delta| streamed.push_str(delta),
    )?;

    assert!(called);
    assert_eq!(response, "listed files");
    assert_eq!(streamed, "listed files");
    let requests = server.join().expect("server thread completed")?;
    assert!(requests[0].contains("\"tools\":[{\"description\":\"List files\""));
    assert!(requests[0].contains("\"tool_choice\":\"auto\""));
    assert!(requests[0].contains("\"stream\":true"));
    assert!(requests[1].contains("\"type\":\"function_call_output\""));
    assert!(!requests[1].contains("\"id\":\"fc_1\""));
    assert!(requests[1].contains("Cargo.toml"));
    Ok(())
}

#[test]
fn openai_sso_provider_sends_valid_input_for_system_only_envelopes() -> anyhow::Result<()> {
    let _guard = env_lock().lock().unwrap();
    let tmp = tempdir()?;
    write_sso_auth(tmp.path(), "sso-envelope-token", "acct-envelope")?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let body =
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\ndata: [DONE]\n\n";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    });
    let adapter = OpenAISsoProfileAdapter {
        config: ProviderConfig {
            name: "openai-sso".to_string(),
            display_name: Some("OpenAI SSO".to_string()),
            kind: "openai_sso".to_string(),
            api_key_env: None,
            base_url: None,
            auth_type: "oauth".to_string(),
            enabled: true,
            metadata: [
                (
                    "codex_base_url".to_string(),
                    json!(format!("http://{}", addr)),
                ),
                (
                    "auth_root".to_string(),
                    json!(tmp.path().display().to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        },
    };
    let model = ModelInfo {
        name: "gpt-sso".to_string(),
        provider: "openai-sso".to_string(),
        display_name: None,
        context_window: Some(400000),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let messages = vec![ChatMessage {
        role: "system".to_string(),
        content: "system-only prepared envelope text".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    }];

    let response = adapter.complete(&messages, &model, "openai-sso")?;

    assert_eq!(response, "ok");
    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("POST /responses HTTP/1.1"));
    assert!(request.contains("\"input\""));
    assert!(request.contains("\"input_text\""));
    assert!(request.contains("system-only prepared envelope text"));
    Ok(())
}

#[test]
fn openai_sso_exchange_code_extracts_account_and_tokens() -> anyhow::Result<()> {
    let id_token = unsigned_jwt(json!({"chatgpt_account_id": "acct-exchange"}));
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let body = json!({
            "id_token": id_token,
            "access_token": "access-exchange",
            "refresh_token": "refresh-exchange",
        })
        .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    });

    let tokens = vegvisir_rust::openai_sso::exchange_code_with_issuer(
        "callback-code",
        "http://localhost:1455/auth/callback",
        "verifier",
        &format!("http://{}", addr),
    )?;

    assert_eq!(tokens.account_id, "acct-exchange");
    assert_eq!(tokens.access_token, "access-exchange");
    assert_eq!(tokens.refresh_token, "refresh-exchange");
    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("POST /oauth/token HTTP/1.1"));
    assert!(request.contains("grant_type=authorization_code"));
    assert!(request.contains("code=callback-code"));
    assert!(request.contains("code_verifier=verifier"));
    Ok(())
}

#[test]
fn openai_compatible_provider_sends_image_attachments_as_data_urls() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let image_path = tmp.path().join("pixel.png");
    std::fs::write(&image_path, [0_u8, 1, 2, 3])?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let body = r#"{"choices":[{"message":{"content":"saw image"}}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    });
    let adapter = OpenAICompatibleProviderAdapter {
        config: ProviderConfig {
            name: "test-openai".to_string(),
            display_name: None,
            kind: "openai_compatible".to_string(),
            api_key_env: None,
            base_url: Some(format!("http://{}", addr)),
            auth_type: "none".to_string(),
            enabled: true,
            metadata: [("stream".to_string(), json!(false))].into_iter().collect(),
        },
    };
    let model = ModelInfo {
        name: "gpt-vision".to_string(),
        provider: "test-openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: false,
        enabled: true,
        metadata: Default::default(),
    };
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: "inspect this".to_string(),
        attachments: vec![Attachment {
            path: image_path.display().to_string(),
            kind: "image".to_string(),
            mime_type: Some("image/png".to_string()),
            name: Some("pixel.png".to_string()),
            size_bytes: Some(4),
        }],
        created_at: Utc::now(),
    }];

    let response = adapter.complete(&messages, &model, "test-openai")?;

    assert_eq!(response, "saw image");
    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("\"type\":\"image_url\""));
    assert!(request.contains("data:image/png;base64,AAECAw=="));
    assert!(request.contains("[attachment] image: pixel.png"));
    Ok(())
}

#[test]
fn retriever_returns_matching_documents() {
    let mut retriever = InMemoryRetriever::default();
    retriever.add(RetrievalDocument {
        id: "a".to_string(),
        text: "python harness checkpoint recovery".to_string(),
        metadata: Default::default(),
    });
    retriever.add(RetrievalDocument {
        id: "b".to_string(),
        text: "unrelated".to_string(),
        metadata: Default::default(),
    });

    assert_eq!(retriever.search("checkpoint", 5)[0].id, "a");
}

#[test]
fn attachments_extract_file_uri_and_keep_instruction() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let file_path = tmp.path().join("sample.md");
    std::fs::write(&file_path, "# Sample")?;

    let (content, attachments) = extract_attachments(
        &format!("Summarize file://{}", file_path.display()),
        tmp.path(),
    );

    assert_eq!(content, format!("Summarize file://{}", file_path.display()));
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].name.as_deref(), Some("sample.md"));
    Ok(())
}

#[test]
fn attachments_preserve_pasted_path_in_sentence() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let file_path = tmp.path().join("vegvisir-harness-developer.json");
    std::fs::write(&file_path, r#"{"prompt":"weak"}"#)?;

    let input = format!(
        "rewrite the system prompt in this file {} and preserve skills",
        file_path.display()
    );
    let (content, attachments) = extract_attachments(&input, tmp.path());

    assert_eq!(content, input);
    assert_eq!(attachments.len(), 1);
    assert_eq!(
        attachments[0].name.as_deref(),
        Some("vegvisir-harness-developer.json")
    );
    Ok(())
}

#[test]
fn startup_dashboard_renders_inventory() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut app = TuiApplication::new(tmp.path())?;
    let output = app.render();

    assert!(output.contains("Vegvisir Console 0.2.5"));
    assert!(output.contains("status"));
    assert!(output.contains("dashboard"));
    assert!(!output.contains("┌ status"));
    assert!(!output.contains("┌ dashboard"));
    assert!(output.contains(" chat "));
    assert!(!output.contains("┌ notice"));
    assert!(output.contains("tools"));
    assert!(output.contains("skills"));
    assert!(output.contains("read_file"));
    assert!(output.contains("code-review"));
    Ok(())
}

#[test]
fn tui_layout_renders_slash_suggestions_and_stays_narrow() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut app = TuiApplication::new(tmp.path())?;
    app.input.set_buffer("/");

    let output = app.render();

    assert!(output.contains("┌ select"));
    assert!(output.contains("/attach"));
    assert!(output.lines().all(|line| visible_width(line) <= 100));

    app.input.set_buffer("/rem");
    let output = app.render();
    assert!(output.contains("/remember"));
    Ok(())
}

#[test]
fn tui_renders_markdown_code_blocks_and_tables() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
    app.renderer.viewport = Some((100, 32));
    app.session.messages.push(ChatMessage {
        role: "assistant".to_string(),
        content: "# Result\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n| Name | Status |\n| --- | --- |\n| parser | ok |"
            .to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    });

    let output = app.render();
    assert!(output.contains("■ Result"));
    assert!(output.contains("┌ rust"));
    assert!(output.contains("fn main()"));
    assert!(output.contains("└"));
    assert!(output.contains("Name"));
    assert!(output.contains("Status"));
    assert!(output.contains("parser"));
    Ok(())
}

#[test]
fn tui_renders_common_language_code_fences() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
    app.renderer.viewport = Some((120, 48));
    app.session.messages.push(ChatMessage {
        role: "assistant".to_string(),
        content: [
            "```csharp\npublic class Demo { string Name => \"ok\"; }\n```",
            "```cpp\nconstexpr int value = 42;\n```",
            "```java\npublic record User(String name) {}\n```",
            "```javascript\nconst run = async () => true;\n```",
            "```typescript\ntype User = { name: string };\n```",
            "```python\ndef run():\n    return True\n```",
        ]
        .join("\n"),
        attachments: Vec::new(),
        created_at: Utc::now(),
    });

    let output = app.render();
    for language in [
        "csharp",
        "cpp",
        "java",
        "javascript",
        "typescript",
        "python",
    ] {
        assert!(output.contains(&format!("┌ {language}")), "{language}");
    }
    assert!(output.contains("public class Demo"));
    assert!(output.contains("constexpr int value"));
    assert!(output.contains("def run()"));
    Ok(())
}

#[test]
fn tui_renders_diff_fences_as_line_numbered_review_view() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
    app.renderer.viewport = Some((120, 36));
    app.session.messages.push(ChatMessage {
        role: "assistant".to_string(),
        content: "```diff\ndiff --git a/src/lib.rs b/src/lib.rs\n@@ -7,2 +7,3 @@ fn demo() {\n-    let old = true;\n+    let new = true;\n+    println!(\"ok\");\n }\n```"
            .to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    });

    let output = app.render();

    assert!(output.contains("diff --git a/src/lib.rs b/src/lib.rs"));
    assert!(output.contains("@@ -7,2 +7,3 @@ fn demo()"));
    assert!(output.contains("    7 -"));
    assert!(output.contains("    7 +"));
    assert!(output.contains("    8 +"));
    assert!(!output.contains("┌ diff"));
    assert!(output.lines().all(|line| visible_width(line) <= 120));
    Ok(())
}

#[test]
fn diff_command_returns_workspace_git_diff_fence() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .arg("init")
        .output()?;
    fs::write(tmp.path().join("sample.txt"), "old\n")?;
    std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["add", "sample.txt"])
        .output()?;
    std::process::Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args([
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "commit",
            "-m",
            "init",
        ])
        .output()?;
    fs::write(tmp.path().join("sample.txt"), "new\n")?;

    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    let output = app.execute_command("/diff sample.txt")?.unwrap();

    assert!(output.starts_with("Git diff"));
    assert!(output.contains("```diff"));
    assert!(output.contains("-old"));
    assert!(output.contains("+new"));
    let overlay = app
        .diff_overlay
        .as_ref()
        .expect("/diff opens review overlay");
    assert_eq!(overlay.files_changed, 1);
    assert_eq!(overlay.added_lines, 1);
    assert_eq!(overlay.removed_lines, 1);
    Ok(())
}

#[test]
fn inspector_commands_open_tui_info_overlay() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let output = app.execute_command("/models")?.unwrap();
    assert!(output.contains("Available models for"));
    let overlay = app
        .info_overlay
        .as_ref()
        .expect("/models opens inspector overlay");
    assert_eq!(overlay.title, "models");
    assert!(overlay.body.contains("Available models for"));

    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::PageUp,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(app.info_overlay.is_some());
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(app.info_overlay.is_none());
    Ok(())
}

#[test]
fn slash_command_submit_routes_overlay_output_without_chat_spam() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    let message_count = app.session.messages.len();

    app.input.set_buffer("/trace --limit 5");
    app.handle_submit();

    assert_eq!(app.session.messages.len(), message_count);
    let overlay = app
        .info_overlay
        .as_ref()
        .expect("/trace output should be constrained to the info overlay");
    assert_eq!(overlay.title, "trace");
    assert!(overlay.body.contains("command_start"));
    assert!(overlay.body.contains("/trace"));
    Ok(())
}

#[test]
fn work_command_opens_activity_overlay_and_mouse_scrolls_active_overlay() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.execute_command("/provider demo")?;
    app.execute_command("/model demo-local")?;

    let output = app.execute_command("/work")?.unwrap();
    assert!(output.contains("Work activity for session"));
    assert!(output.contains("Recent events"));
    let overlay = app
        .info_overlay
        .as_ref()
        .expect("/work opens activity overlay");
    assert_eq!(overlay.title, "work activity");
    assert!(overlay.body.contains("command_start"));

    app.handle_mouse_event(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::ScrollUp,
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    assert_eq!(app.info_scroll_offset, 3);
    assert_eq!(app.chat_scroll_offset, 0);
    Ok(())
}

#[test]
fn tui_approval_queue_selects_and_resolves_chosen_request() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.tool_executor
        .guardrails
        .approvals
        .enqueue(ApprovalRequest {
            id: "apr_a".to_string(),
            reason: "Risky tool requires human approval: write_file".to_string(),
            tool_name: "write_file".to_string(),
            args: json!({"path": "a.txt", "content": "a"})
                .as_object()
                .unwrap()
                .clone(),
            risk_label: "write".to_string(),
        });
    app.tool_executor
        .guardrails
        .approvals
        .enqueue(ApprovalRequest {
            id: "apr_b".to_string(),
            reason: "Risky tool requires human approval: run_command".to_string(),
            tool_name: "run_command".to_string(),
            args: json!({"command": ["echo", "ok"]})
                .as_object()
                .unwrap()
                .clone(),
            risk_label: "command".to_string(),
        });

    assert_eq!(app.approval_selected_index, 0);
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Down,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.approval_selected_index, 1);
    app.handle_mouse_event(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::ScrollUp,
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    assert_eq!(app.approval_selected_index, 0);
    app.handle_mouse_event(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::ScrollDown,
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    assert_eq!(app.approval_selected_index, 1);
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::NONE,
    ));

    let pending = app.tool_executor.guardrails.approvals.pending_ids();
    assert_eq!(pending, vec!["apr_a".to_string()]);
    assert_eq!(app.approval_selected_index, 0);
    assert!(
        app.ephemeral_notice
            .as_ref()
            .is_some_and(|notice| notice.text.contains("Denied approval apr_b"))
    );
    assert!(
        app.session
            .messages
            .iter()
            .all(|message| !message.content.contains("Denied approval apr_b"))
    );
    Ok(())
}

#[test]
fn registry_backed_help_commands_and_tools_have_json_discovery() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let help = app.execute_command("/help")?.unwrap();
    assert!(help.contains("/help [--json] [--context <name>]"));
    assert!(help.contains("/commands [--json]"));

    let commands = app.execute_command("/commands")?.unwrap();
    assert!(commands.contains("Command registry:"));
    assert!(commands.contains("Core session"));
    assert!(commands.contains("/commands"));
    assert!(commands.contains("aliases:"));

    let commands_json: Value =
        serde_json::from_str(&app.execute_command("/commands --json")?.unwrap())?;
    let command_names = commands_json["commands"]
        .as_array()
        .expect("commands array")
        .iter()
        .filter_map(|command| command["name"].as_str())
        .collect::<Vec<_>>();
    assert!(command_names.contains(&"/commands"));
    assert!(command_names.contains(&"/tools"));

    let subagent_json: Value = serde_json::from_str(
        &app.execute_command("/commands --json --context subagent")?
            .unwrap(),
    )?;
    let subagent_commands = subagent_json["commands"]
        .as_array()
        .expect("commands array");
    assert!(subagent_commands.iter().all(|command| {
        command["contexts"]
            .as_array()
            .expect("contexts array")
            .iter()
            .any(|context| context == "subagent")
    }));

    let tools = app.execute_command("/tools inventory")?.unwrap();
    assert!(tools.contains("Tool registry:"));
    assert!(tools.contains("read_file"));

    let tools_json: Value = serde_json::from_str(&app.execute_command("/tools --json")?.unwrap())?;
    let tool_names = tools_json["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|tool| tool["id"].as_str())
        .collect::<Vec<_>>();
    assert!(tool_names.contains(&"read_file"));
    assert!(tool_names.contains(&"write_file"));
    Ok(())
}

#[test]
fn command_registry_suggests_and_preserves_system_text() {
    let registry = CommandRegistry::with_defaults();

    assert!(registry.suggest("/mo").contains(&"/model".to_string()));
    assert!(
        registry
            .suggest("/system-p")
            .contains(&"/system-prompt".to_string())
    );
    assert_eq!(registry.canonical("/quit"), "/exit");
    let parsed = CommandRegistry::parse("/system set keep this prompt intact").unwrap();
    assert_eq!(parsed.0, "/system");
    assert_eq!(parsed.1, vec!["set", "keep this prompt intact"]);
    let mut aliased = CommandRegistry::default();
    aliased.register(vegvisir_rust::core::CommandDefinition {
        name: "/system".to_string(),
        description: "system".to_string(),
        usage: "/system".to_string(),
        aliases: vec!["/sys".to_string()],
        delegates_to_agent: false,
    });
    let parsed = aliased
        .parse_with_aliases("/sys set line one\nline two")
        .unwrap();
    assert_eq!(parsed.0, "/system");
    assert_eq!(parsed.1, vec!["set", "line one\nline two"]);
}

#[test]
fn application_executes_core_commands_and_demo_runner() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    assert!(app.execute_command("/help")?.unwrap().contains("/models"));
    assert!(app.session.system_prompt.contains("You are Vegvisir"));
    assert!(
        app.session
            .system_prompt
            .contains("contract VegvisirDefaultAgentContract")
    );
    assert!(
        app.execute_command("/system-prompt")?
            .unwrap()
            .contains("You are Vegvisir")
    );
    assert_eq!(
        app.execute_command("/system set answer tersely")?.unwrap(),
        "Harness system prompt updated."
    );
    assert_eq!(app.session.system_prompt, "answer tersely");
    assert_eq!(
        app.execute_command("/system set line one\nline two\nline three")?
            .unwrap(),
        "Harness system prompt updated."
    );
    assert_eq!(app.session.system_prompt, "line one\nline two\nline three");
    assert_eq!(
        app.execute_command("/system line four\nline five")?
            .unwrap(),
        "Harness system prompt updated."
    );
    assert_eq!(app.session.system_prompt, "line four\nline five");
    assert!(home.join("config.json").exists());
    assert_eq!(
        app.execute_command("/system append keep it brief")?
            .unwrap(),
        "Appended to harness system prompt."
    );
    assert!(app.session.system_prompt.contains("keep it brief"));
    assert_eq!(
        app.execute_command("/system clear")?.unwrap(),
        "Harness system prompt cleared."
    );
    assert_eq!(app.session.system_prompt, "");
    assert_eq!(
        app.execute_command("/system default")?.unwrap(),
        "Harness system prompt reset to the Vegvisir default."
    );
    assert_eq!(app.session.system_prompt, default_system_prompt());
    assert!(app.session.system_prompt.contains("trigger ProviderCall"));
    assert!(
        app.execute_command("/system show")?
            .unwrap()
            .contains("You are Vegvisir")
    );
    assert!(
        app.execute_command("/system print")?
            .unwrap()
            .contains("contract VegvisirDefaultAgentContract")
    );

    let response = app.send_demo("hello")?;
    assert!(response.contains("Demo response from demo-local"));
    assert!(app.session.last_prompt_cache_key.is_some());
    assert!(app.session.last_prompt_manifest_id.is_some());
    assert!(home.join("cms-v2.sqlite3").exists());
    let saved_session_id = app.session.session_id.clone();
    let saved_session = std::fs::read_to_string(
        home.join("sessions")
            .join(format!("{saved_session_id}.json")),
    )?;
    assert!(saved_session.contains("\"content\": \"hello\""));
    assert!(saved_session.contains("Demo response from demo-local"));
    assert!(
        app.execute_command("/sessions")?
            .unwrap()
            .contains(&saved_session_id)
    );
    assert!(
        app.execute_command("/history")?
            .unwrap()
            .contains("user: hello")
    );
    assert!(!app.execute_command("/help")?.unwrap().contains("/select"));
    assert!(app.execute_command("/help")?.unwrap().contains("/cancel"));
    assert_eq!(
        app.execute_command("/cancel")?.unwrap(),
        "No in-flight model response to cancel."
    );
    app.execute_command("/new scratch")?.unwrap();
    assert_ne!(app.session.session_id, saved_session_id);
    assert_eq!(
        app.execute_command(&format!("/load {saved_session_id}"))?
            .unwrap(),
        format!("Loaded session {saved_session_id} with 2 message(s).")
    );
    assert_eq!(app.session.session_id, saved_session_id);
    assert_eq!(app.session.messages.len(), 2);
    assert_eq!(
        app.execute_command("/undo")?.unwrap(),
        "Removed last exchange."
    );
    assert!(
        app.execute_command("/history")?
            .unwrap()
            .contains("No conversation history")
    );
    app.session.messages.push(ChatMessage {
        role: "user".to_string(),
        content: "we need to patch /compress because it loses the useful current objective"
            .to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    });
    app.session.messages.push(ChatMessage {
        role: "assistant".to_string(),
        content: "Implemented a structured context capsule and verified it with cargo test."
            .to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    });
    let compressed = app
        .execute_command("/compact context compression")?
        .unwrap();
    assert!(compressed.contains("Context Capsule: context compression"));
    assert!(compressed.contains("Current Objective:"));
    assert!(compressed.contains("Recent Actions / Evidence:"));
    assert!(compressed.contains("Continuity Instructions:"));
    assert!(
        app.session
            .messages
            .first()
            .is_some_and(|message| message.content.contains("Context Capsule"))
    );
    assert!(
        app.session
            .messages
            .iter()
            .any(|message| message.content.contains("we need to patch /compress"))
    );
    assert!(
        app.execute_command("/tools allow-risky")?
            .unwrap()
            .contains("enabled")
    );
    assert!(
        app.execute_command("/tools require-approval")?
            .unwrap()
            .contains("Human approval required")
    );
    assert!(
        app.execute_command("/tools status")?
            .unwrap()
            .contains("Human approval: required")
    );
    assert!(
        app.execute_command("/tool-limit")?
            .unwrap()
            .contains("Max tool-call rounds per turn")
    );
    assert!(
        app.execute_command("/tool-limit 64")?
            .unwrap()
            .contains("set to 64")
    );
    assert!(
        app.execute_command("/tools max-rounds 65")?
            .unwrap()
            .contains("set to 65")
    );
    assert!(
        app.execute_command("/tool-limit default")?
            .unwrap()
            .contains("reset to")
    );
    assert_eq!(
        app.execute_command("/approvals")?.unwrap(),
        "No pending approvals."
    );
    assert_eq!(
        app.execute_command("/approvals edit missing {\"command\":[\"pwd\"]}")?
            .unwrap(),
        "Unknown pending approval: missing"
    );
    let blocked = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["pwd"]}).as_object().unwrap().clone(),
    });
    assert!(!blocked.ok);
    let approval_id = app
        .tool_executor
        .guardrails
        .approvals
        .pending_ids()
        .first()
        .cloned()
        .expect("approval queued");
    let approval_detail = app
        .execute_command(&format!("/approvals show {approval_id}"))?
        .unwrap();
    assert!(approval_detail.contains("\"tool\": \"run_command\""));
    assert!(approval_detail.contains("\"approve_once\""));
    assert!(approval_detail.contains("\"approve_for_session\""));
    app.execute_command(&format!("/approvals deny {approval_id}"))?
        .unwrap();
    assert!(
        app.execute_command("/help")?
            .unwrap()
            .contains("/approvals")
    );
    assert!(app.execute_command("/help")?.unwrap().contains("/eval"));
    assert!(
        app.execute_command("/tools no-approval")?
            .unwrap()
            .contains("no longer required")
    );
    assert!(
        app.execute_command("/providers")?
            .unwrap()
            .contains("openai")
    );
    let models = app.execute_command("/models")?.unwrap();
    assert!(models.contains("Available models for demo"));
    assert_eq!(app.input.buffer, "/model ");
    assert!(
        app.execute_command("/provider open")?
            .unwrap()
            .contains("Close matches")
    );
    assert!(
        app.execute_command("/model demo")?
            .unwrap()
            .contains("Close matches")
    );
    assert!(
        app.execute_command("/auth")?
            .unwrap()
            .contains("openai-sso-status")
    );
    assert!(
        app.execute_command("/auth openai-sso-status")?
            .unwrap()
            .contains("not logged in")
    );
    let auth = app.execute_command("/auth openai")?.unwrap();
    assert!(auth.contains("Production auth"));
    assert!(auth.contains("/hbse provider openai"));
    assert!(!auth.contains("read -rsp"));
    assert!(!auth.contains(">> ~/.bashrc"));
    Ok(())
}

#[test]
fn hbse_command_generates_reference_only_secret_setup() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let status = app.execute_command("/hbse")?.unwrap();
    assert!(status.contains("HBSE is the auth/secrets layer"));
    assert!(status.contains("Vegvisir only handles secret refs"));
    assert!(status.contains("anthropic-hbse"));
    assert!(status.contains("google-hbse"));
    assert!(status.contains("azure-openai-hbse"));
    assert!(status.contains("openrouter-hbse"));
    assert!(status.contains("perplexity-hbse"));

    let provider = app.execute_command("/hbse provider openai")?.unwrap();
    assert!(provider.contains("hbse model-provider setup openai --stdin"));
    assert!(provider.contains("secret://vegvisir/providers/openai/default"));
    assert!(provider.contains("vegvisir.provider.openai-hbse"));
    assert!(!provider.contains("sk-"));
    let openrouter = app.execute_command("/hbse provider openrouter")?.unwrap();
    assert!(openrouter.contains("secret://vegvisir/providers/openrouter/default"));
    assert!(openrouter.contains("vegvisir.provider.openrouter-hbse"));
    let anthropic = app.execute_command("/hbse provider anthropic")?.unwrap();
    assert!(anthropic.contains("secret://vegvisir/providers/anthropic/default"));
    assert!(anthropic.contains("vegvisir.provider.anthropic-hbse"));
    let google = app.execute_command("/hbse provider google")?.unwrap();
    assert!(google.contains("secret://vegvisir/providers/google/default"));
    assert!(google.contains("vegvisir.provider.google-hbse"));
    let azure = app.execute_command("/hbse provider azure-openai")?.unwrap();
    assert!(azure.contains("secret://vegvisir/providers/azure-openai/default"));
    assert!(azure.contains("vegvisir.provider.azure-openai-hbse"));

    let service = app
        .execute_command("/hbse service postgres vegvisir.tool.db-query database.query")?
        .unwrap();
    assert!(
        service.contains("hbse secret put secret://vegvisir/services/postgres/default --stdin")
    );
    assert!(service.contains("\"allowed_consumers\": [\n    \"vegvisir.tool.db-query\""));
    assert!(service.contains("\"allowed_purposes\": [\n    \"database.query\""));
    assert!(!service.contains("password"));
    assert!(!service.contains("api_key"));

    let mcp = app
        .execute_command(
            "/hbse mcp github https://api.githubcopilot.com/mcp/ vegvisir.mcp.github mcp.tool.call",
        )?
        .unwrap();
    assert!(mcp.contains("hbse secret put secret://vegvisir/mcp/github/default --stdin"));
    assert!(mcp.contains("\"allowed_delivery_modes\": [\n    \"brokered_http\""));
    assert!(mcp.contains("\"allowed_http_hosts\": [\n    \"api.githubcopilot.com\""));
    assert!(mcp.contains("\"allowed_http_path_prefixes\": [\n    \"/mcp/\""));
    assert!(mcp.contains("/mcp add-http github <url> secret://vegvisir/mcp/github/default vegvisir.mcp.github mcp.tool.call"));
    assert!(!mcp.contains("secret://vegvisir/services/mcp"));

    let rejected = app
        .execute_command("/hbse service add db postgres://user:password@example/db")?
        .unwrap();
    assert!(rejected.contains("secret://"));
    let registered = app
        .execute_command("/hbse service add db secret://vegvisir/services/db/default vegvisir.tool.db database.query")?
        .unwrap();
    assert!(registered.contains("Registered HBSE service ref db"));
    let refs = app.execute_command("/hbse services")?.unwrap();
    assert!(refs.contains("db"));
    assert!(refs.contains("secret://vegvisir/services/db/default"));
    assert!(refs.contains("vegvisir.tool.db"));
    let shown = app.execute_command("/hbse service show db")?.unwrap();
    assert!(shown.contains("name=db"));
    assert!(shown.contains("enabled=true"));
    assert!(shown.contains("consumer=vegvisir.tool.db"));
    let disabled = app.execute_command("/hbse service disable db")?.unwrap();
    assert!(disabled.contains("Disabled HBSE service ref db"));
    assert!(
        app.execute_command("/hbse services")?
            .unwrap()
            .contains("enabled=false")
    );
    let enabled = app.execute_command("/hbse service enable db")?.unwrap();
    assert!(enabled.contains("Enabled HBSE service ref db"));
    let stored = std::fs::read_to_string(home.join("hbse-services.json"))?;
    assert!(stored.contains("secret://vegvisir/services/db/default"));
    assert!(!stored.contains("password"));
    let removed = app.execute_command("/hbse service remove db")?.unwrap();
    assert!(removed.contains("Removed HBSE service ref db"));
    Ok(())
}

#[test]
fn providers_command_marks_legacy_env_and_hbse_secret_refs() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    let providers = app.execute_command("/providers")?.unwrap();
    assert!(providers.contains("openai"));
    assert!(providers.contains("legacy_env=OPENAI_API_KEY"));
    assert!(providers.contains("hbse=/hbse provider openai"));
    assert!(providers.contains("openai-hbse"));
    assert!(providers.contains("secret_ref=secret://vegvisir/providers/openai/default"));
    assert!(providers.contains("anthropic-hbse"));
    assert!(providers.contains("secret_ref=secret://vegvisir/providers/anthropic/default"));
    assert!(providers.contains("google-hbse"));
    assert!(providers.contains("secret_ref=secret://vegvisir/providers/google/default"));
    assert!(providers.contains("azure-openai-hbse"));
    assert!(providers.contains("secret_ref=secret://vegvisir/providers/azure-openai/default"));
    assert!(providers.contains("openrouter-hbse"));
    assert!(providers.contains("secret_ref=secret://vegvisir/providers/openrouter/default"));
    assert!(providers.contains("perplexity-hbse"));
    Ok(())
}

#[test]
fn production_mode_blocks_direct_provider_auth() -> anyhow::Result<()> {
    let _guard = env_lock().lock().unwrap();
    unsafe {
        std::env::set_var("VEGVISIR_PRODUCTION", "1");
        std::env::remove_var("VEGVISIR_ALLOW_DIRECT_PROVIDER_AUTH");
        std::env::set_var("VEGVISIR_TEST_DIRECT_KEY", "direct-secret");
    }

    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    let providers = app.execute_command("/providers")?.unwrap();
    assert!(providers.contains("blocked_by_production"));
    let selected = app.execute_command("/provider openai")?.unwrap();
    assert!(selected.contains("Direct API-key provider auth is disabled in production mode"));
    assert!(selected.contains("/hbse provider openai"));
    let verify = app.execute_command("/verify auth")?.unwrap();
    assert!(verify.contains("direct env fallback blocked by production mode"));

    let adapter = OpenAICompatibleProviderAdapter {
        config: ProviderConfig {
            name: "test-direct".to_string(),
            display_name: Some("Test Direct".to_string()),
            kind: "openai_compatible".to_string(),
            api_key_env: Some("VEGVISIR_TEST_DIRECT_KEY".to_string()),
            base_url: Some("http://127.0.0.1:1/v1".to_string()),
            auth_type: "api_key".to_string(),
            enabled: true,
            metadata: Default::default(),
        },
    };
    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "test-direct".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let error = adapter
        .complete(
            &[ChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            }],
            &model,
            "test-direct",
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("Direct API-key provider auth is disabled in production mode"));

    unsafe {
        std::env::remove_var("VEGVISIR_PRODUCTION");
        std::env::remove_var("VEGVISIR_TEST_DIRECT_KEY");
    }
    Ok(())
}

#[test]
fn verify_command_reports_auth_mcp_agent_and_memory_readiness() -> anyhow::Result<()> {
    let _env_guard = env_lock().lock().unwrap();
    let _sandbox_env = EnvVarGuard::set(COMMAND_SANDBOX_ENV, "path-only");
    let _network_env = EnvVarGuard::set(COMMAND_SANDBOX_NETWORK_ENV, "inherit");
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    fs::write(
        home.join("mcp.json"),
        serde_json::to_string_pretty(&json!({
            "servers": [{
                "id": "remote",
                "transport": "http",
                "url": "https://mcp.example.test/rpc",
                "hbse_secret_refs": ["secret://vegvisir/mcp/remote/default"],
                "consumer": "vegvisir.mcp.remote",
                "purpose": "mcp.tool.call",
                "tools": [{"name": "lookup", "description": "Lookup", "schema": {"properties": {}}}]
            }]
        }))?,
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.execute_command("/agent create red | agent-red | Agent Red | Security work.")?
        .unwrap();
    app.execute_command("/agent allow-tool red read_file")?
        .unwrap();
    app.execute_command("/agent bind-usrl red red-contract")?
        .unwrap();
    app.execute_command("/agent use red")?.unwrap();

    let output = app.execute_command("/verify all")?.unwrap();
    assert!(output.contains("auth/hbse openai-hbse"));
    assert!(output.contains("warn auth/legacy openai"));
    assert!(output.contains("ok mcp/remote transport=Http tools=1 hbse_refs=1"));
    assert!(output.contains("warn mcp/active agent=red no MCP servers allowed"));
    assert!(output.contains("ok agent active=red mode=agent-red"));
    assert!(output.contains("ok agent/tools allowed=read_file"));
    assert!(output.contains("ok agent/usrl contracts=red-contract"));
    assert!(output.contains("ok memory cms_v2"));
    assert!(output.contains("ok runtime/approvals"));
    assert!(output.contains("ok runtime/traces"));
    assert!(output.contains("ok runtime/subagents"));
    assert!(output.contains("ok runtime/cancel command=/cancel"));
    assert!(output.contains("ok runtime/dangerous_bypass disabled startup_only=true"));
    assert!(output.contains("ok runtime/workspace_file_containment active"));
    assert!(output.contains("ok runtime/file_access_hardening "));
    #[cfg(target_os = "linux")]
    assert!(output.contains(
        "ok runtime/file_access_hardening openat2_existing_paths_plus_symlink_rejection"
    ));
    assert!(output.contains("ok runtime/command_os_sandbox unavailable/not_configured"));
    assert!(output.contains("ok runtime/bubblewrap_available "));
    assert!(output.contains("ok runtime/command_network_policy inherit"));
    assert!(output.contains(
        "ok runtime/command_mount_policy writable=not_applicable readonly=not_applicable"
    ));
    assert!(output.contains("ok runtime/user default=local-user active=agent:red"));
    assert!(output.contains("ok evals/golden passed=3 total=3"));
    Ok(())
}

#[test]
fn verify_runtime_reports_dangerous_bypass_startup_mode() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    let mut app = TuiApplication::with_data_root_and_dangerous_bypass(tmp.path(), &home, true)?;

    let output = app.execute_command("/verify runtime")?.unwrap();
    assert!(output.contains("ok runtime/dangerous_bypass enabled startup_only=true"));
    assert!(output.contains("ok runtime/workspace_file_containment disabled_by_dangerous_bypass"));
    assert!(output.contains("ok runtime/file_access_hardening disabled_by_dangerous_bypass"));
    assert!(output.contains("ok runtime/command_os_sandbox disabled_by_dangerous_bypass"));
    assert!(output.contains("ok runtime/approvals"));
    assert!(output.contains("ok runtime/user default=local-user active=local-user"));
    Ok(())
}

#[test]
fn verify_mcp_reports_active_agent_server_filtering() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    fs::write(
        home.join("mcp.json"),
        serde_json::to_string_pretty(&json!({
            "servers": [
                {
                    "id": "github",
                    "transport": "http",
                    "url": "https://mcp.example.test/github",
                    "hbse_secret_refs": ["secret://vegvisir/mcp/github/default"],
                    "consumer": "vegvisir.mcp.github",
                    "purpose": "mcp.tool.call",
                    "tools": [{"name": "search", "description": "Search", "schema": {"properties": {}}}]
                },
                {
                    "id": "linear",
                    "transport": "http",
                    "url": "https://mcp.example.test/linear",
                    "hbse_secret_refs": ["secret://vegvisir/mcp/linear/default"],
                    "consumer": "vegvisir.mcp.linear",
                    "purpose": "mcp.tool.call",
                    "tools": [{"name": "issue", "description": "Issue", "schema": {"properties": {}}}]
                }
            ]
        }))?,
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.execute_command("/agent create researcher | researcher | Researcher | Research.")?
        .unwrap();
    app.execute_command("/agent allow-mcp researcher github")?
        .unwrap();
    app.execute_command("/agent allow-mcp researcher missing")?
        .unwrap();
    app.execute_command("/agent use researcher")?.unwrap();

    let tools = app.execute_command("/mcp tools")?.unwrap();
    assert!(tools.contains("mcp::github::search"));
    assert!(!tools.contains("mcp::linear::issue"));

    let output = app.execute_command("/verify mcp")?.unwrap();
    assert!(output.contains("ok mcp/github transport=Http tools=1 hbse_refs=1"));
    assert!(output.contains("ok mcp/linear transport=Http tools=1 hbse_refs=1"));
    assert!(output.contains("ok mcp/active agent=researcher servers=github tools=1"));
    assert!(
        output.contains("fail mcp/active agent=researcher missing configured server(s): missing")
    );
    Ok(())
}

#[test]
fn multiline_system_prompt_config_is_repaired_on_startup() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home)?;
    std::fs::write(
        home.join("config.json"),
        "{\n  \"current_model\": \"demo-local\",\n  \"current_provider\": \"demo\",\n  \"system_prompt\": \"line one\n\nline three\"\n}\n",
    )?;

    let app = TuiApplication::with_data_root(tmp.path(), &home)?;

    assert_eq!(app.session.system_prompt, "line one\n\nline three");
    let repaired: Value =
        serde_json::from_str(&std::fs::read_to_string(home.join("config.json"))?)?;
    assert_eq!(repaired["system_prompt"], "line one\n\nline three");
    Ok(())
}

#[test]
fn tui_submit_command_appends_result_and_keeps_running() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    app.input.set_buffer("/help");
    app.handle_submit();

    assert!(app.running);
    assert!(app.input.buffer.is_empty(), "buffer={:?}", app.input.buffer);
    assert_eq!(app.session.messages.len(), 1);
    assert_eq!(app.session.messages[0].role, "system");
    assert!(app.session.messages[0].content.contains("/models"));
    assert!(
        app.session.input_history.is_empty(),
        "slash commands should not be saved to normal input history: {:?}",
        app.session.input_history
    );
    Ok(())
}

#[test]
fn system_prompt_persists_across_app_instances() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut first = TuiApplication::with_data_root(tmp.path(), &home)?;

    assert_eq!(
        first.execute_command("/system set Persistent prompt")?,
        Some("Harness system prompt updated.".to_string())
    );

    let second = TuiApplication::with_data_root(tmp.path(), &home)?;

    assert_eq!(second.session.system_prompt, "Persistent prompt");
    Ok(())
}

#[test]
fn new_and_branch_preserve_model_and_system_prompt() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    assert!(
        app.execute_command("/model demo-long-context")?
            .unwrap()
            .contains("Selected model demo-long-context")
    );
    assert_eq!(
        app.execute_command("/system set Persistent prompt")?,
        Some("Harness system prompt updated.".to_string())
    );
    let first_session = app.session.session_id.clone();

    let new_result = app.execute_command("/new next chat")?.unwrap();
    assert!(new_result.contains("Started new session"));
    assert_ne!(app.session.session_id, first_session);
    assert_eq!(app.session.current_provider, "demo");
    assert_eq!(app.session.current_model, "demo-long-context");
    assert_eq!(app.session.system_prompt, "Persistent prompt");

    let second_session = app.session.session_id.clone();
    let branch_result = app.execute_command("/branch alternate")?.unwrap();
    assert!(branch_result.contains(&format!("Branched {second_session}")));
    assert_ne!(app.session.session_id, second_session);
    assert_eq!(app.session.current_provider, "demo");
    assert_eq!(app.session.current_model, "demo-long-context");
    assert_eq!(app.session.system_prompt, "Persistent prompt");
    Ok(())
}

#[test]
fn startup_autoloads_latest_workspace_context() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let workspace = tmp.path().join("project");
    let home = tmp.path().join("home");
    fs::create_dir_all(&workspace)?;

    let first_session_id = {
        let mut app = TuiApplication::with_data_root(&workspace, &home)?;
        app.execute_command("/title startup restored context")?
            .unwrap();
        app.session.messages.push(vegvisir_rust::core::ChatMessage {
            role: "user".to_string(),
            content: "remember this startup context".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        app.execute_command("/save")?.unwrap();
        app.session.session_id.clone()
    };

    let app = TuiApplication::with_data_root(&workspace, &home)?;
    assert_eq!(app.session.session_id, first_session_id);
    assert_eq!(app.session.title, "startup restored context");
    assert!(
        app.session
            .messages
            .iter()
            .any(|message| message.content == "remember this startup context")
    );
    Ok(())
}

#[test]
fn cwd_alias_restores_previous_workspace_context() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let workspace_one = tmp.path().join("one");
    let workspace_two = tmp.path().join("two");
    let home = tmp.path().join("home");
    fs::create_dir_all(&workspace_one)?;
    fs::create_dir_all(&workspace_two)?;

    let mut app = TuiApplication::with_data_root(&workspace_one, &home)?;
    app.execute_command("/title workspace one previous context")?
        .unwrap();
    app.session.messages.push(vegvisir_rust::core::ChatMessage {
        role: "user".to_string(),
        content: "context from workspace one".to_string(),
        attachments: Vec::new(),
        created_at: chrono::Utc::now(),
    });
    let workspace_one_session = app.session.session_id.clone();

    let changed = app
        .execute_command(&format!("/cwd {}", workspace_two.display()))?
        .unwrap();
    assert!(changed.contains("Started new project session"));
    assert_ne!(app.session.session_id, workspace_one_session);

    let restored = app
        .execute_command(&format!("/cwd {}", workspace_one.display()))?
        .unwrap();
    assert!(restored.contains(&format!("Restored session {workspace_one_session}")));
    assert_eq!(app.session.session_id, workspace_one_session);
    assert!(
        app.session
            .messages
            .iter()
            .any(|message| message.content == "context from workspace one")
    );
    Ok(())
}

#[test]
fn workspace_command_retargets_filesystem_tools_and_sessions() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    unsafe {
        std::env::set_var("HOME", tmp.path());
    }
    let home = tmp.path().join("home");
    let workspace_one = tmp.path().join("one");
    let workspace_two = tmp.path().join("two");
    let home_workspace = tmp.path().join("Secret_Project");
    fs::create_dir_all(&workspace_one)?;
    fs::create_dir_all(&workspace_two)?;
    fs::create_dir_all(&home_workspace)?;
    fs::write(workspace_one.join("one.txt"), "one")?;
    fs::write(workspace_two.join("two.txt"), "two")?;
    fs::write(home_workspace.join("secret.txt"), "secret")?;

    let mut app = TuiApplication::with_data_root(&workspace_one, &home)?;
    let workspace_one_project = app.cms.config.project_id.clone();
    app.execute_command("/title workspace one session")?
        .unwrap();
    app.execute_command("/remember One project note | only visible in workspace one")?
        .unwrap();
    app.execute_command(
        "/remember --global User security preference | prefer threat models with mitigations",
    )?
    .unwrap();
    let workspace_one_session = app.session.session_id.clone();
    assert!(
        app.execute_command("/workspace")?
            .unwrap()
            .contains(&workspace_one.canonicalize()?.display().to_string())
    );
    let before = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "list_files".to_string(),
        args: json!({"path": "."}).as_object().unwrap().clone(),
    });
    assert!(before.ok, "{}", before.content);
    assert!(before.content.contains("one.txt"));
    assert!(!before.content.contains("two.txt"));

    let changed = app
        .execute_command(&format!("/workspace {}", workspace_two.display()))?
        .unwrap();
    assert!(changed.contains(&workspace_two.canonicalize()?.display().to_string()));
    assert!(changed.contains("Started new project session"));
    assert_ne!(app.session.session_id, workspace_one_session);
    assert_eq!(
        app.session.cwd,
        workspace_two.canonicalize()?.display().to_string()
    );
    let workspace_two_project = app.cms.config.project_id.clone();
    assert_ne!(workspace_one_project, workspace_two_project);
    assert_eq!(
        app.execute_command("/recall only visible in workspace one")?
            .unwrap(),
        "No CMS memories matched."
    );
    assert!(
        app.execute_command("/recall threat models mitigations")?
            .unwrap()
            .contains("User security preference")
    );
    app.execute_command("/title workspace two session")?
        .unwrap();
    app.execute_command("/remember Two project note | only visible in workspace two")?
        .unwrap();
    let after = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "list_files".to_string(),
        args: json!({"path": "."}).as_object().unwrap().clone(),
    });
    assert!(after.ok, "{}", after.content);
    assert!(after.content.contains("two.txt"));
    assert!(!after.content.contains("one.txt"));

    app.execute_command("/new retargeted")?.unwrap();
    assert_eq!(
        app.session.cwd,
        workspace_two.canonicalize()?.display().to_string()
    );
    let workspace_two_session = app.session.session_id.clone();
    let projects = app.execute_command("/projects")?.unwrap();
    assert!(projects.contains(&workspace_one.canonicalize()?.display().to_string()));
    assert!(projects.contains(&workspace_two.canonicalize()?.display().to_string()));
    assert!(projects.contains(&workspace_two_session));
    let named = app
        .execute_command(&format!("/projects name main {}", workspace_one.display()))?
        .unwrap();
    assert!(named.contains("Project alias main"));
    assert!(
        app.execute_command("/projects")?
            .unwrap()
            .contains("alias=main")
    );
    let restored_one = app.execute_command("/projects use main")?.unwrap();
    assert!(restored_one.contains(&format!("Restored session {workspace_one_session}")));
    assert_eq!(app.session.session_id, workspace_one_session);
    assert!(
        app.execute_command("/recall only visible in workspace one")?
            .unwrap()
            .contains("One project note")
    );
    assert!(
        app.execute_command("/memory recent --global --limit 10")?
            .unwrap()
            .contains("User security preference")
    );
    assert_eq!(
        app.execute_command("/recall Two project note")?.unwrap(),
        "No CMS memories matched."
    );
    assert!(
        app.execute_command("/recall --global --limit 10 Two project note")?
            .unwrap()
            .contains("Two project note")
    );
    assert!(
        app.execute_command("/memory recent --global --limit 10")?
            .unwrap()
            .contains("Two project note")
    );
    let restored_two = app
        .execute_command(&format!("/workspace {}", workspace_two.display()))?
        .unwrap();
    assert!(restored_two.contains(&format!("Restored session {workspace_two_session}")));
    assert_eq!(app.session.session_id, workspace_two_session);
    assert!(
        app.execute_command("/recall only visible in workspace two")?
            .unwrap()
            .contains("Two project note")
    );
    let restored_one_again = app
        .execute_command(&format!("/project {}", workspace_one.display()))?
        .unwrap();
    assert!(restored_one_again.contains(&format!("Restored session {workspace_one_session}")));
    let forgot = app.execute_command("/projects forget main")?.unwrap();
    assert!(forgot.contains("Forgot project alias main"));
    let home_switch = app.execute_command("/workspace ~/Secret_Project")?.unwrap();
    assert!(home_switch.contains(&home_workspace.canonicalize()?.display().to_string()));
    let home_files = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "list_files".to_string(),
        args: json!({"path": "."}).as_object().unwrap().clone(),
    });
    assert!(home_files.ok, "{}", home_files.content);
    assert!(home_files.content.contains("secret.txt"));
    Ok(())
}

#[test]
fn provider_defaults_are_global_with_workspace_overrides() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let workspace_one = tmp.path().join("one");
    let workspace_two = tmp.path().join("two");
    fs::create_dir_all(&workspace_one)?;
    fs::create_dir_all(&workspace_two)?;

    let mut app = TuiApplication::with_data_root(&workspace_one, &home)?;
    assert!(
        app.execute_command("/config provider openai-hbse")?
            .unwrap()
            .contains("global default")
    );
    assert_eq!(app.session.current_provider, "openai-hbse");

    app.execute_command(&format!("/workspace {}", workspace_two.display()))?
        .unwrap();
    assert_eq!(app.session.current_provider, "openai-hbse");

    assert!(
        app.execute_command("/provider demo")?
            .unwrap()
            .contains("project override")
    );
    assert_eq!(app.session.current_provider, "demo");

    app.execute_command(&format!("/workspace {}", workspace_one.display()))?
        .unwrap();
    assert_eq!(app.session.current_provider, "openai-hbse");

    app.execute_command(&format!("/workspace {}", workspace_two.display()))?
        .unwrap();
    assert_eq!(app.session.current_provider, "demo");
    Ok(())
}

#[test]
fn input_history_navigation_and_large_paste_rendering_match_python_tui() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    app.input.history = vec!["first".to_string(), "second".to_string()];
    assert!(app.input.history_move(-1));
    assert_eq!(app.input.buffer, "second");
    assert!(app.input.history_move(-1));
    assert_eq!(app.input.buffer, "first");
    assert!(app.input.history_move(1));
    assert_eq!(app.input.buffer, "second");
    assert!(app.input.history_move(1));
    assert_eq!(app.input.buffer, "");

    app.input.history = vec![
        "/agent list".to_string(),
        "normal request".to_string(),
        "/provider demo".to_string(),
        "latest request".to_string(),
    ];
    app.input.clear();
    assert!(app.input.history_move(-1));
    assert_eq!(app.input.buffer, "latest request");
    assert!(app.input.history_move(-1));
    assert_eq!(app.input.buffer, "normal request");
    assert!(app.input.history_move(-1));
    assert_eq!(app.input.buffer, "normal request");
    assert!(app.input.history_move(1));
    assert_eq!(app.input.buffer, "latest request");
    assert!(app.input.history_move(1));
    assert_eq!(app.input.buffer, "");

    app.input.history = vec!["first".to_string(), "second".to_string()];
    app.input.clear();
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Up,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.buffer, "second");
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Down,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.buffer, "");

    app.input.clear();
    app.input.append_text(&"x".repeat(200), true);
    let output = app.render();
    assert!(output.contains("[Pasted 200 characters]"));
    assert!(!output.contains(&"x".repeat(100)));
    Ok(())
}

#[test]
fn tui_input_expands_for_long_typed_messages() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.renderer.viewport = Some((64, 24));
    app.input.set_buffer(
        "This is a longer typed message that should wrap into multiple input rows instead of being truncated.",
    );

    let output = app.render();
    let input_lines = output
        .lines()
        .filter(|line| line.contains("› This is a longer") || line.contains("  input rows"))
        .count();
    assert!(input_lines >= 2, "{output}");
    assert!(output.contains("instead of being truncated."));
    Ok(())
}

#[test]
fn attach_command_queues_attachment_for_next_message() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let file = tmp.path().join("sample.md");
    std::fs::write(&file, "# Sample")?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let result = app
        .execute_command(&format!("/attach {}", file.display()))?
        .unwrap();

    assert!(result.contains("Attached sample.md"));
    assert_eq!(app.session.pending_attachments.len(), 1);
    assert_eq!(
        app.execute_command("/attach")?,
        Some(format!("file: {}", file.canonicalize()?.display()))
    );
    assert_eq!(
        app.execute_command("/attach clear")?,
        Some("Pending attachments cleared.".to_string())
    );
    assert!(app.session.pending_attachments.is_empty());
    Ok(())
}

#[test]
fn tui_keypress_requests_redraw_immediately() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::NONE,
    ));

    assert_eq!(app.input.buffer, "x");
    assert!(app.redraw_requested);
    Ok(())
}

#[test]
fn redraw_command_requests_full_terminal_clear() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let response = app.execute_command("/redraw")?.unwrap();

    assert_eq!(response, "Full redraw requested.");
    assert!(app.redraw_requested);
    assert!(app.clear_requested);
    Ok(())
}

#[test]
fn tui_arrow_keys_move_input_cursor_for_text_editing() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    app.input.set_buffer("helo");
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Left,
        crossterm::event::KeyModifiers::NONE,
    ));
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('l'),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.buffer, "hello");

    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Left,
        crossterm::event::KeyModifiers::NONE,
    ));
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Backspace,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.buffer, "helo");

    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Home,
        crossterm::event::KeyModifiers::NONE,
    ));
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('>'),
        crossterm::event::KeyModifiers::SHIFT,
    ));
    assert_eq!(app.input.buffer, ">helo");

    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::End,
        crossterm::event::KeyModifiers::NONE,
    ));
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('!'),
        crossterm::event::KeyModifiers::SHIFT,
    ));
    assert_eq!(app.input.buffer, ">helo!");
    Ok(())
}

#[test]
fn tui_command_palette_and_help_shortcuts_work() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('/'),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.buffer, "/");
    assert!(app.command_palette_open);
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));

    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(app.input.buffer, "/");
    assert!(app.command_palette_open);
    assert!(!app.input.suggestions.is_empty());
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('m'),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.buffer, "/m");
    assert!(app.command_palette_open);
    assert!(
        app.input
            .suggestions
            .iter()
            .any(|suggestion| suggestion.value == "/models")
    );
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::PageDown,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(app.input.selected_suggestion > 0);
    app.input.selected_suggestion = 0;
    app.handle_mouse_event(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::ScrollDown,
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    assert_eq!(app.input.selected_suggestion, 1);
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Home,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.selected_suggestion, 0);
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::End,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(
        app.input.selected_suggestion,
        app.input.suggestions.len().saturating_sub(1)
    );
    let models_index = app
        .input
        .suggestions
        .iter()
        .position(|suggestion| suggestion.value == "/models")
        .expect("/models suggestion");
    app.input.selected_suggestion = models_index;
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(!app.command_palette_open);
    assert!(
        app.info_overlay
            .as_ref()
            .is_some_and(|overlay| overlay.body.contains("Available models for"))
    );

    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(app.info_overlay.is_none());

    app.input.set_buffer("/models");
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(
        app.info_overlay
            .as_ref()
            .is_some_and(|overlay| overlay.body.contains("Available models for"))
    );
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));

    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(app.input.buffer.is_empty());
    assert!(!app.command_palette_open);

    app.input.clear();
    let message_count = app.session.messages.len();
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('?'),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(app.help_overlay_open);
    assert_eq!(app.session.messages.len(), message_count);
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(!app.help_overlay_open);

    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('a'),
        crossterm::event::KeyModifiers::NONE,
    ));
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::SHIFT,
    ));
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('b'),
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.buffer, "a\nb");
    Ok(())
}

#[test]
fn tui_up_down_preserve_input_cursor_column_across_wrapped_lines() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.renderer.viewport = Some((12, 24));
    app.input
        .set_buffer(format!("{}{}", "a".repeat(48), "b".repeat(12)));
    app.input.cursor = 6;
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Up,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.cursor, 6);

    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Down,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.cursor, 54);

    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Up,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.cursor, 6);

    app.input.history = vec!["previous".to_string()];
    app.input.set_buffer("current");
    app.input.cursor = 0;
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Up,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.buffer, "previous");
    assert_eq!(app.input.cursor, 0);

    app.input.set_buffer("abc\ndefghij\nxy");
    app.input.cursor = 6;
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Down,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.cursor, app.input.buffer.chars().count());
    app.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Up,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.input.cursor, 6);
    Ok(())
}

#[test]
fn tui_mouse_scroll_adjusts_chat_viewport_without_input_navigation() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.input.set_buffer("editable draft");
    app.input.cursor = app.input.buffer.chars().count();

    app.handle_mouse_event(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::ScrollUp,
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });

    assert_eq!(app.chat_scroll_offset, 3);
    assert_eq!(app.input.buffer, "editable draft");
    assert_eq!(app.input.cursor, "editable draft".chars().count());
    assert!(app.redraw_requested);

    app.handle_mouse_event(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::ScrollDown,
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });

    assert_eq!(app.chat_scroll_offset, 0);
    assert_eq!(app.input.buffer, "editable draft");
    assert_eq!(app.input.cursor, "editable draft".chars().count());
    Ok(())
}

#[test]
fn tui_submit_message_uses_cms_memory_and_demo_provider() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    app.input.set_buffer("hello from tui");
    app.handle_submit();
    for _ in 0..500 {
        if app.poll_pending_send() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(app.running);
    assert_eq!(app.session.messages.len(), 2);
    assert_eq!(app.session.messages[0].role, "user");
    assert_eq!(app.session.messages[0].content, "hello from tui");
    assert_eq!(app.session.messages[1].role, "assistant");
    assert!(
        app.session.messages[1]
            .content
            .contains("CMS-v2 model request")
    );
    assert!(app.session.last_prompt_cache_key.is_some());
    assert!(home.join("cms-v2.sqlite3").exists());

    let runs_dir = tmp.path().join(".vegvisir/runs");
    let run_dirs = fs::read_dir(&runs_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(run_dirs.len(), 1);
    let run_dir = &run_dirs[0];
    assert!(run_dir.join("manifest.json").exists());
    assert!(run_dir.join("request.json").exists());
    assert!(run_dir.join("context.md").exists());
    assert!(run_dir.join("context-sources.json").exists());
    assert!(run_dir.join("result.md").exists());
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(run_dir.join("manifest.json"))?)?;
    assert_eq!(manifest["status"], "completed");
    assert_eq!(manifest["session_id"], app.session.session_id);
    assert_eq!(manifest["provider"], "demo");
    assert_eq!(manifest["model"], "demo-local");
    let request: Value = serde_json::from_str(&fs::read_to_string(run_dir.join("request.json"))?)?;
    assert_eq!(request["mode"], "tui_turn");
    assert_eq!(request["goal"], "hello from tui");
    assert!(fs::read_to_string(run_dir.join("result.md"))?.contains("CMS-v2 model request"));
    let runtime_events = fs::read_to_string(run_dir.join("runtime-events.jsonl"))?;
    assert!(runtime_events.contains(r#""type":"run_started""#));
    assert!(runtime_events.contains(r#""type":"user_message""#));
    assert!(runtime_events.contains(r#""type":"memory_read""#));
    assert!(runtime_events.contains(r#""type":"context_prepared""#));
    assert!(runtime_events.contains(r#""type":"assistant_delta""#));
    assert!(runtime_events.contains(r#""type":"assistant_message_completed""#));
    assert!(runtime_events.contains(r#""type":"run_completed""#));
    let runtime_lines = runtime_events.lines().collect::<Vec<_>>();
    assert!(runtime_lines.len() >= 7);
    for (idx, line) in runtime_lines.iter().enumerate() {
        assert!(line.contains(&format!(r#""seq":{}"#, idx + 1)));
    }
    let file_changes: Value =
        serde_json::from_str(&fs::read_to_string(run_dir.join("file-changes.json"))?)?;
    assert_eq!(file_changes["status"], "unavailable");
    assert_eq!(file_changes["run_id"], manifest["run_id"]);
    Ok(())
}

#[test]
fn tui_failed_provider_send_keeps_user_message_and_shows_error() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.session.current_model = "missing-model".to_string();

    app.input.set_buffer("hello failing provider");
    app.handle_submit();
    for _ in 0..50 {
        if app.poll_pending_send() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(app.session.status, "ready");
    assert_eq!(app.session.messages[0].role, "user");
    assert_eq!(app.session.messages[0].content, "hello failing provider");
    assert_eq!(app.session.messages.last().unwrap().role, "system");
    assert!(
        app.session
            .messages
            .last()
            .unwrap()
            .content
            .contains("Unknown model: missing-model")
    );

    let runs_dir = tmp.path().join(".vegvisir/runs");
    let run_dirs = fs::read_dir(&runs_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(run_dirs.len(), 1);
    let run_dir = &run_dirs[0];
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(run_dir.join("manifest.json"))?)?;
    assert_eq!(manifest["status"], "failed");
    assert!(run_dir.join("file-changes.json").exists());
    let failure: Value = serde_json::from_str(&fs::read_to_string(run_dir.join("failure.json"))?)?;
    assert!(
        failure["message"]
            .as_str()
            .unwrap()
            .contains("Unknown model: missing-model")
    );
    Ok(())
}

#[test]
fn application_exposes_cms_commands() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let remembered = app.execute_command(
        "/remember Runtime memory note | Vegvisir command surface uses CMS-v2 recall.",
    )?;
    assert!(remembered.unwrap().contains("Remembered memory"));

    let recalled = app
        .execute_command("/recall command surface CMS-v2")?
        .unwrap();
    assert!(recalled.contains("Runtime memory note"));
    let memory_status = app.execute_command("/memory status")?.unwrap();
    assert!(memory_status.contains("CMS-v2 memory scope"));
    assert!(memory_status.contains("user_id=local-user"));
    assert!(memory_status.contains("project_id=workspace:"));
    assert!(
        app.execute_command("/config status")?
            .unwrap()
            .contains("default_user_id=local-user")
    );
    assert_eq!(
        app.execute_command("/config user user:alice")?.unwrap(),
        "Default user id set to user:alice."
    );
    assert_eq!(app.cms.config.user_id, "user:alice");
    assert!(
        app.sessions
            .store
            .root
            .ends_with("users/user-alice/sessions")
    );
    assert!(home.join("users/user-alice/sessions").exists());
    let user_status = app.execute_command("/memory status")?.unwrap();
    assert!(user_status.contains("user_id=user:alice"));
    assert!(
        app.execute_command("/recall command surface CMS-v2")?
            .unwrap()
            .contains("No CMS memories matched")
    );
    assert_eq!(
        app.execute_command("/config user bad/user")
            .unwrap_err()
            .to_string(),
        "User id must be 1-128 chars and contain only letters, numbers, '-', '_', '.', ':', or '@', with no secret-like material."
    );
    let recent = app.execute_command("/memory recent --limit 5")?.unwrap();
    assert!(recent.contains("No recent CMS memories"));

    let context = app
        .execute_command("/context Continue command surface work")?
        .unwrap();
    assert!(!context.contains("Runtime memory note"));
    let context_budget = app
        .execute_command("/context budget Continue command surface work")?
        .unwrap();
    assert!(context_budget.contains("Context budget"));
    assert!(context_budget.contains("remaining_tokens="));
    assert!(context_budget.contains("overflow_tokens="));
    assert!(context_budget.contains("action="));
    assert!(context_budget.contains("policy=warn@60% compact@80% block@95%"));
    assert!(context_budget.contains("categories:"));
    assert!(context_budget.contains("warnings:"));
    assert!(context_budget.contains("strategy=ECM prepared"));
    let context_budget_json: Value = serde_json::from_str(
        &app.execute_command("/context budget --json Continue command surface work")?
            .unwrap(),
    )?;
    assert_eq!(
        context_budget_json["model"].as_str(),
        Some(app.session.current_model.as_str())
    );
    assert!(context_budget_json["used_tokens"].as_u64().is_some());
    assert!(context_budget_json["remaining_tokens"].as_u64().is_some());
    assert_eq!(context_budget_json["overflow_tokens"].as_u64(), Some(0));
    assert!(matches!(
        context_budget_json["action"].as_str(),
        Some("ok" | "warn" | "compact_recommended" | "block")
    ));
    assert_eq!(
        context_budget_json["policy"]["warning_percent"].as_f64(),
        Some(60.0)
    );
    assert_eq!(
        context_budget_json["policy"]["compaction_percent"].as_f64(),
        Some(80.0)
    );
    assert_eq!(
        context_budget_json["policy"]["block_percent"].as_f64(),
        Some(95.0)
    );
    assert!(context_budget_json["categories"].as_array().is_some());
    assert!(context_budget_json["warnings"].as_array().is_some());
    let model_request = app
        .execute_command("/model-request Continue command surface work")?
        .unwrap();
    assert!(model_request.contains("prompt_cache_key"));
    let help = app.execute_command("/help")?.unwrap();
    assert!(help.contains("/remember"));
    assert!(help.contains("/memory"));
    assert!(help.contains("/trace"));

    let trace = app.execute_command("/trace --limit 5")?.unwrap();
    assert!(trace.contains("command_start"));
    assert!(trace.contains("/model-request") || trace.contains("/trace"));
    let trace_json = app.execute_command("/trace --limit 2 --json")?.unwrap();
    let parsed: Vec<vegvisir_rust::observability::Event> = serde_json::from_str(&trace_json)?;
    assert!(!parsed.is_empty());

    let reopened = TuiApplication::with_data_root(tmp.path(), &home)?;
    assert_eq!(reopened.cms.config.user_id, "user:alice");
    assert!(
        reopened
            .sessions
            .store
            .root
            .ends_with("users/user-alice/sessions")
    );
    Ok(())
}

#[test]
fn memory_import_chatgpt_uses_explicit_archive_database() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let workspace = tmp.path().join("workspace");
    let export_dir = tmp.path().join("chatgpt-export");
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&export_dir)?;
    fs::write(
        export_dir.join("conversations.json"),
        r#"[
          {
            "id": "conv_cms_import",
            "title": "CMS Import Planning",
            "create_time": 1700000000.0,
            "update_time": 1700000100.0,
            "mapping": {
              "m1": {
                "message": {
                  "id": "m1",
                  "author": {"role": "user"},
                  "create_time": 1700000000.0,
                  "content": {
                    "content_type": "text",
                    "parts": ["How should imported ChatGPT memories be scoped?"]
                  }
                },
                "parent": null,
                "children": ["m2"]
              },
              "m2": {
                "message": {
                  "id": "m2",
                  "author": {"role": "assistant"},
                  "create_time": 1700000001.0,
                  "content": {
                    "content_type": "text",
                    "parts": ["Use a separate explicit-only ChatGPT archive database. Preserve useful answer material in the chunk body so agents can quote the imported conversation when asked about old project ideas."]
                  }
                },
                "parent": "m1",
                "children": []
              }
            }
          }
        ]"#,
    )?;

    let mut app = TuiApplication::with_data_root(&workspace, &home)?;
    let imported = app
        .execute_command(&format!(
            "/memory import-chatgpt {} --messages-per-memory 20",
            export_dir.display()
        ))?
        .unwrap();
    assert!(imported.contains("Started ChatGPT archive import in background"));
    assert!(imported.contains("user_id=local-user"));
    assert!(imported.contains("corpus=chatgpt_archive"));
    assert!(imported.contains("retrieval_policy=explicit_only"));
    assert!(imported.contains("cms-v2-chatgpt-archive.sqlite3"));
    for _ in 0..50 {
        if app.poll_background_jobs() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        app.session
            .messages
            .last()
            .map(|message| message
                .content
                .contains("Imported 1 ChatGPT archive memory object"))
            .unwrap_or(false)
    );

    let recent = app.execute_command("/memory recent --limit 5")?.unwrap();
    assert!(!recent.contains("ChatGPT: CMS Import Planning"));

    let recalled = app
        .execute_command("/recall imported ChatGPT memories scoped")?
        .unwrap();
    assert!(!recalled.contains("ChatGPT: CMS Import Planning"));

    let archive_search = app
        .execute_command("/memory search-chatgpt imported ChatGPT memories scoped")?
        .unwrap();
    assert!(
        archive_search.contains("CMS Import Planning"),
        "{archive_search}"
    );
    assert!(archive_search.contains("chunk 1/1"), "{archive_search}");

    let tool_registry = build_builtin_registry_with_cms(&workspace, app.cms.config.clone())?;
    assert!(tool_registry.get("cms_search_chatgpt_archive").is_ok());
    let archive_tool = tool_registry.get("cms_search_chatgpt_archive")?;
    let observation = (archive_tool.handler)(serde_json::Map::from_iter([
        (
            "query".to_string(),
            serde_json::json!("imported ChatGPT memories scoped"),
        ),
        ("limit".to_string(), serde_json::json!(5)),
    ]));
    assert!(observation.ok, "{:?}", observation.error);
    assert!(
        observation.content.contains("CMS Import Planning"),
        "{}",
        observation.content
    );
    assert!(
        observation.content.contains("excerpt:"),
        "{}",
        observation.content
    );
    assert!(
        observation
            .content
            .contains("separate explicit-only ChatGPT archive database"),
        "{}",
        observation.content
    );
    let structured_results = observation
        .data
        .get("results")
        .and_then(serde_json::Value::as_array)
        .expect("structured archive results");
    let first_result = structured_results.first().expect("first archive result");
    assert!(
        first_result
            .get("excerpt")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("separate explicit-only ChatGPT archive database"),
        "{first_result:?}"
    );
    assert_eq!(
        first_result
            .get("conversation_title")
            .and_then(serde_json::Value::as_str),
        Some("CMS Import Planning")
    );
    assert_eq!(
        observation
            .data
            .get("corpus")
            .and_then(serde_json::Value::as_str),
        Some("chatgpt_archive")
    );
    assert_eq!(
        observation
            .data
            .get("retrieval_policy")
            .and_then(serde_json::Value::as_str),
        Some("explicit_only")
    );
    Ok(())
}

#[test]
fn application_runs_builtin_eval_harness() -> anyhow::Result<()> {
    let _env_guard = env_lock().lock().unwrap();
    let _sandbox_env = EnvVarGuard::set(COMMAND_SANDBOX_ENV, "path-only");
    let _network_env = EnvVarGuard::set(COMMAND_SANDBOX_NETWORK_ENV, "inherit");
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let output = app.execute_command("/eval security")?.unwrap();
    assert!(output.contains("eval summary:"));
    assert!(output.contains("pass eval/tools/approval_queue"));
    assert!(output.contains("pass eval/tools/command_bounds"));
    assert!(output.contains("pass eval/security/secret_injection_memory_write"));

    let memory = app.execute_command("/eval memory")?.unwrap();
    assert!(memory.contains("pass eval/memory/project_isolation"));
    assert!(memory.contains("pass eval/memory/secret_memory_rejection"));

    let golden = app.execute_command("/eval golden")?.unwrap();
    assert!(golden.contains("eval summary: passed=3 total=3"));
    assert!(golden.contains("pass eval/golden/workspace_memory_recall"));
    assert!(golden.contains("pass eval/golden/secret_memory_prompt_injection"));
    assert!(golden.contains("pass eval/golden/approval_queue_surface"));

    let eval_file = tmp.path().join("evals.json");
    fs::write(
        &eval_file,
        r#"[
          {
            "id": "custom_memory_case",
            "category": "custom",
            "description": "custom eval file case",
            "steps": [
              {
                "command": "/remember Custom Eval | File-backed eval marker.",
                "expect_contains": ["Remembered memory"]
              },
              {
                "command": "/recall File-backed eval marker",
                "expect_contains": ["Custom Eval"]
              }
            ]
          }
        ]"#,
    )?;
    let file_output = app
        .execute_command(&format!("/eval file {}", eval_file.display()))?
        .unwrap();
    assert!(file_output.contains("pass eval/custom/custom_memory_case"));
    Ok(())
}

#[test]
fn agent_popup_suggestions_show_selectable_agent_profiles() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    app.execute_command(
        "/agent create reviewer | review | Code Reviewer | Review code carefully.",
    )?;
    app.execute_command("/agent create coder | build | Builder Agent | Implement code changes.")?;
    app.execute_command("/agent use reviewer")?;

    app.input.set_buffer("/agent");
    let suggestions = app.build_suggestions();
    assert!(
        suggestions.iter().any(|suggestion| {
            suggestion.value == "reviewer"
                && suggestion.replacement.as_deref() == Some("/agent use reviewer")
                && suggestion.description.contains("active")
        }),
        "{suggestions:?}"
    );
    assert!(
        suggestions.iter().any(|suggestion| {
            suggestion.value == "coder"
                && suggestion.replacement.as_deref() == Some("/agent use coder")
        }),
        "{suggestions:?}"
    );

    app.input.set_buffer("/agent use rev");
    let suggestions = app.build_suggestions();
    assert_eq!(suggestions.len(), 1, "{suggestions:?}");
    assert_eq!(suggestions[0].value, "reviewer");
    assert_eq!(
        suggestions[0].replacement.as_deref(),
        Some("/agent use reviewer")
    );
    Ok(())
}

#[test]
fn new_harness_command_suggestions_are_discoverable() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;

    app.input.set_buffer("/runs ");
    let runs = app.build_suggestions();
    assert!(
        runs.iter()
            .any(|suggestion| suggestion.value == "replay-plan")
    );
    assert!(
        runs.iter()
            .any(|suggestion| suggestion.value == "memory-used")
    );

    app.input.set_buffer("/memory w");
    let memory = app.build_suggestions();
    assert!(memory.iter().any(|suggestion| suggestion.value == "why"));
    assert!(
        memory
            .iter()
            .any(|suggestion| suggestion.value == "writes-this-session")
    );

    app.input.set_buffer("/provider c");
    let providers = app.build_suggestions();
    assert!(
        providers
            .iter()
            .any(|suggestion| suggestion.value == "compare")
    );

    app.input.set_buffer("/model c");
    let models = app.build_suggestions();
    assert!(
        models
            .iter()
            .any(|suggestion| suggestion.value == "compare")
    );

    app.session.current_provider = "openai-sso".to_string();
    app.input.set_buffer("/model gpt-5.6");
    let openai_sso_models = app.build_suggestions();
    for model in ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
        assert!(
            openai_sso_models
                .iter()
                .any(|suggestion| suggestion.value == model),
            "TUI model suggestions should include {model}"
        );
    }

    app.input.set_buffer("/auto l");
    let auto = app.build_suggestions();
    assert!(auto.iter().any(|suggestion| suggestion.value == "level"));
    Ok(())
}

#[test]
fn agent_list_shows_valid_agents_when_one_profile_is_invalid() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let agents = home.join("agents");
    fs::create_dir_all(&agents)?;
    fs::write(
        agents.join("broken.json"),
        r#"{"id":"broken","display_name":"Broken"}"#,
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let created = app
        .execute_command("/agent create good-agent | tester | Good Agent | You are valid.")?
        .unwrap();
    assert!(created.contains("Created agent good-agent"));

    let listed = app.execute_command("/agent list")?.unwrap();
    assert!(listed.contains("good-agent"), "{listed}");
    assert!(listed.contains("mode=tester"), "{listed}");
    assert!(listed.contains("Warnings:"), "{listed}");
    assert!(listed.contains("broken.json"), "{listed}");
    Ok(())
}

#[test]
fn application_persists_custom_agents_with_dedicated_memory_scope() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let skill_dir = tmp
        .path()
        .join(".vegvisir")
        .join("skills")
        .join("review-style");
    fs::create_dir_all(&skill_dir)?;
    fs::write(
        skill_dir.join("SKILL.md"),
        "# Review Style\nFlag correctness risks before style notes.",
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let created = app
        .execute_command(
            "/agent create reviewer | tester | Code Reviewer | You are a dedicated code-review agent.",
        )?
        .unwrap();
    assert!(created.contains("Created agent reviewer"));
    assert!(created.contains("mode=tester"));
    assert!(home.join("agents").join("reviewer.json").exists());

    let mut reopened = TuiApplication::with_data_root(tmp.path(), &home)?;
    let listed = reopened.execute_command("/agent list")?.unwrap();
    assert!(listed.contains("reviewer"));
    assert!(listed.contains("mode=tester"));
    assert!(listed.contains("Code Reviewer"));

    assert!(
        reopened
            .execute_command("/agent enable-skill reviewer review-style")?
            .unwrap()
            .contains("Enabled skill review-style")
    );
    assert!(
        reopened
            .execute_command("/agent allow-tool reviewer read_file")?
            .unwrap()
            .contains("Allowed tool read_file")
    );
    assert!(
        reopened
            .execute_command("/agent bind-usrl reviewer review-contract")?
            .unwrap()
            .contains("Bound USRL contract(s) review-contract")
    );
    assert!(
        reopened
            .execute_command("/agent allow-mcp reviewer github")?
            .unwrap()
            .contains("Allowed MCP server github")
    );
    let shown = reopened.execute_command("/agent show reviewer")?.unwrap();
    assert!(shown.contains("| tools | read_file |"));
    assert!(shown.contains("| skills | review-style |"));
    assert!(shown.contains("| usrl_contracts | review-contract |"));
    assert!(shown.contains("| mcp_servers | github |"));

    let selected = reopened.execute_command("/agent use reviewer")?.unwrap();
    assert!(selected.contains("System prompt and CMS memory scope applied"));
    assert_eq!(
        reopened.tool_executor.runtime_policy.usrl_contracts,
        vec!["review-contract".to_string()]
    );
    assert_eq!(
        reopened.tool_executor.runtime_policy.allowed_tools,
        vec!["read_file".to_string()]
    );
    assert_eq!(
        reopened.session.active_agent_id.as_deref(),
        Some("reviewer")
    );
    assert_eq!(
        reopened.session.system_prompt,
        "You are a dedicated code-review agent.\n\nEnabled agent skills:\nSkill: review-style\n# Review Style\nFlag correctness risks before style notes."
    );
    assert!(
        reopened
            .execute_command("/agent revoke-tool reviewer read_file")?
            .unwrap()
            .contains("Revoked tool read_file")
    );
    assert!(
        reopened
            .tool_executor
            .runtime_policy
            .allowed_tools
            .is_empty()
    );
    assert!(
        reopened
            .execute_command("/agent disable-skill reviewer review-style")?
            .unwrap()
            .contains("Disabled skill review-style")
    );
    assert_eq!(
        reopened.session.system_prompt,
        "You are a dedicated code-review agent."
    );
    assert!(
        reopened
            .execute_command("/agent unbind-usrl reviewer review-contract")?
            .unwrap()
            .contains("Unbound USRL contract(s) review-contract")
    );
    assert!(
        reopened
            .tool_executor
            .runtime_policy
            .usrl_contracts
            .is_empty()
    );
    assert!(!reopened.tool_executor.runtime_policy.strict_usrl);
    assert!(
        reopened
            .execute_command("/agent revoke-mcp reviewer github")?
            .unwrap()
            .contains("Revoked MCP server github")
    );
    assert_eq!(reopened.cms.config.user_id, "agent:reviewer");
    assert_eq!(
        reopened.cms.config.project_id.as_deref(),
        Some("agent:reviewer")
    );

    reopened
        .execute_command("/remember Reviewer private note | Agent-only memory marker")?
        .unwrap();
    assert!(
        reopened
            .execute_command("/recall Agent-only memory marker")?
            .unwrap()
            .contains("Reviewer private note")
    );

    let cleared = reopened.execute_command("/agent clear")?.unwrap();
    assert!(cleared.contains("default Vegvisir memory scope"));
    assert_eq!(reopened.session.active_agent_id, None);
    assert_eq!(reopened.cms.config.user_id, "local-user");
    assert!(
        reopened
            .cms
            .config
            .project_id
            .as_deref()
            .unwrap_or("")
            .starts_with("workspace:")
    );
    assert_eq!(
        reopened
            .execute_command("/recall Agent-only memory marker")?
            .unwrap(),
        "No CMS memories matched."
    );
    Ok(())
}

#[test]
fn application_clones_exports_imports_and_deletes_agents() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    app.execute_command("/agent create planner | planner | Planner | Plan carefully.")?
        .unwrap();
    let cloned = app
        .execute_command("/agent clone planner researcher Researcher")
        .unwrap()
        .unwrap();
    assert!(cloned.contains("Cloned agent planner to researcher"));
    let researcher = app.execute_command("/agent show researcher")?.unwrap();
    assert!(researcher.contains("| name | Researcher |"));
    assert!(researcher.contains("| cms_user_id | `agent:researcher` |"));

    let export_path = tmp.path().join("planner-export.json");
    let exported = app
        .execute_command(&format!("/agent export planner {}", export_path.display()))?
        .unwrap();
    assert!(exported.contains("Exported agent planner"));
    assert!(export_path.exists());
    app.execute_command("/agent delete planner")?.unwrap();
    assert!(!home.join("agents").join("planner.json").exists());

    let imported = app
        .execute_command(&format!("/agent import {}", export_path.display()))?
        .unwrap();
    assert!(imported.contains("Imported agent planner"));
    assert!(home.join("agents").join("planner.json").exists());

    app.execute_command("/agent use planner")?.unwrap();
    assert_eq!(app.session.active_agent_id.as_deref(), Some("planner"));
    let deleted = app.execute_command("/agent delete planner")?.unwrap();
    assert!(deleted.contains("Deleted agent planner"));
    assert_eq!(app.session.active_agent_id, None);
    assert_eq!(app.cms.config.user_id, "local-user");
    assert!(
        app.cms
            .config
            .project_id
            .as_deref()
            .unwrap_or("")
            .starts_with("workspace:")
    );
    Ok(())
}

#[test]
fn application_creates_specialized_agents_from_templates() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let templates = app.execute_command("/agent templates")?.unwrap();
    assert!(templates.contains("planner"));
    assert!(templates.contains("agent-red"));
    assert!(templates.contains("tools=list_files,read_file"));

    let created = app
        .execute_command("/agent create-template agent-red red-team Red Team")?
        .unwrap();
    assert!(created.contains("Created agent red-team from template agent-red"));
    let shown = app.execute_command("/agent show red-team")?.unwrap();
    assert!(shown.contains("| mode | `agent-red` |"));
    assert!(shown.contains("| name | Red Team |"));
    assert!(shown.contains("Security-oriented review"));
    assert!(shown.contains("| tools | list_files, read_file, run_command"));
    assert!(shown.contains("cms_prepare_model_request"));
    assert!(shown.contains("spawn_subagent"));
    assert!(shown.contains("| skills | repo-orientation, code-review, test-repair, risk-check |"));
    assert!(shown.contains("You are Agent Red"));
    assert!(shown.contains("prompt/tool injection paths"));

    app.execute_command("/agent use red-team")?.unwrap();
    assert_eq!(app.session.active_agent_id.as_deref(), Some("red-team"));
    assert_eq!(app.cms.config.user_id, "agent:red-team");
    assert_eq!(
        app.tool_executor.runtime_policy.allowed_tools,
        vec![
            "list_files",
            "read_file",
            "run_command",
            "run_tests",
            "cms_recall",
            "cms_recent",
            "cms_search_chatgpt_archive",
            "cms_remember",
            "cms_prepare_context",
            "cms_prepare_model_request",
            "spawn_subagent",
            "audit_log"
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn natural_agent_template_request_creates_agent_without_provider_roundtrip() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    app.input.set_buffer("using the agent-red template make an adversarial offensive security auditing agent. specializing in owasp and ai-owasp security auditing, bug discovery, vulnerability assessments.");
    app.handle_submit();

    assert!(app.pending_send.is_none());
    assert!(
        home.join("agents")
            .join("agent-red-security-auditor.json")
            .exists()
    );
    assert_eq!(app.session.messages[0].role, "user");
    assert_eq!(app.session.messages[1].role, "system");
    assert!(
        app.session.messages[1]
            .content
            .contains("Created agent agent-red-security-auditor")
    );
    let shown = app
        .execute_command("/agent show agent-red-security-auditor")?
        .unwrap();
    assert!(shown.contains("| mode | `agent-red` |"));
    assert!(shown.contains("owasp and ai-owasp"));
    Ok(())
}

#[test]
fn agent_command_typo_reports_unknown_command_or_agent() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let result = app.execute_command("/agent tempates")?.unwrap();

    assert!(result.contains("Unknown /agent command or agent id: tempates"));
    assert!(result.contains("/agent templates"));
    assert!(!result.contains("os error"));
    Ok(())
}

#[test]
fn application_designs_custom_agents_with_bindings() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let skill_root = tmp.path().join(".vegvisir").join("skills");
    fs::create_dir_all(&skill_root)?;
    fs::write(
        skill_root.join("review-style.md"),
        "# Review Style\nFocus on correctness.",
    )?;
    fs::write(
        skill_root.join("release.usrl"),
        "contract release_gate {\nrule require_tests\nconstraint no_secret_output\n}",
    )?;
    fs::create_dir_all(&home)?;
    fs::write(
        home.join("mcp.json"),
        serde_json::to_string_pretty(&json!({
            "servers": [{
                "id": "github",
                "transport": "http",
                "url": "https://mcp.example.test/rpc",
                "hbse_secret_refs": ["secret://vegvisir/mcp/github/default"],
                "consumer": "vegvisir.mcp.github",
                "purpose": "mcp.tool.call",
                "tools": [{"name": "search", "description": "Search", "schema": {"properties": {}}}]
            }]
        }))?,
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let designed = app
        .execute_command("/agent design reviewer2 | reviewer | Reviewer Two | Review carefully. | tools=list_files,read_file skills=review-style mcp=github usrl=release provider=openai-hbse model=gpt-5.5 use=true")?
        .unwrap();

    assert!(designed.contains("Designed agent reviewer2"));
    assert_eq!(app.session.active_agent_id.as_deref(), Some("reviewer2"));
    let shown = app.execute_command("/agent show reviewer2")?.unwrap();
    assert!(shown.contains("| mode | `reviewer` |"));
    assert!(shown.contains("| name | Reviewer Two |"));
    assert!(shown.contains("| provider | `openai-hbse` |"));
    assert!(shown.contains("| model | `gpt-5.5` |"));
    assert!(shown.contains("| tools | list_files, read_file |"));
    assert!(shown.contains("| skills | review-style |"));
    assert!(shown.contains("| mcp_servers | github |"));
    assert!(shown.contains("| usrl_contracts | release_gate |"));
    assert_eq!(
        app.tool_executor.runtime_policy.allowed_tools,
        vec!["list_files".to_string(), "read_file".to_string()]
    );
    assert_eq!(
        app.tool_executor.runtime_policy.usrl_contracts,
        vec!["release_gate".to_string()]
    );
    assert_eq!(
        app.tool_executor.runtime_policy.usrl_rules,
        vec!["require_tests".to_string()]
    );
    assert_eq!(
        app.tool_executor.runtime_policy.usrl_constraints,
        vec!["no_secret_output".to_string()]
    );
    Ok(())
}

#[test]
fn application_edits_agent_prompt_description_and_model_defaults() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    app.execute_command("/agent create coder | coder | Coder | Initial prompt.")?
        .unwrap();
    assert!(
        app.execute_command("/agent describe coder Writes focused patches.")?
            .unwrap()
            .contains("Updated agent coder description")
    );
    assert!(
        app.execute_command(
            "/agent prompt coder You write Rust changes only after reading context."
        )?
        .unwrap()
        .contains("Updated agent coder system prompt")
    );
    assert!(
        app.execute_command("/agent provider coder openai-hbse")?
            .unwrap()
            .contains("Set agent coder provider to openai-hbse")
    );
    assert!(
        app.execute_command("/agent model coder gpt-5.4-mini")?
            .unwrap()
            .contains("Set agent coder model to gpt-5.4-mini")
    );

    let shown = app.execute_command("/agent show coder")?.unwrap();
    assert!(shown.contains("| description | Writes focused patches. |"));
    assert!(shown.contains("| provider | `openai-hbse` |"));
    assert!(shown.contains("| model | `gpt-5.4-mini` |"));
    assert!(shown.contains("You write Rust changes only after reading context."));

    app.execute_command("/agent use coder")?.unwrap();
    assert_eq!(app.session.current_provider, "openai-hbse");
    assert_eq!(app.session.current_model, "gpt-5.4-mini");
    assert_eq!(
        app.session.system_prompt,
        "You write Rust changes only after reading context."
    );

    assert!(
        app.execute_command("/agent model coder claude-sonnet-4-5")?
            .unwrap()
            .contains("not available for agent provider openai-hbse")
    );
    assert!(
        app.execute_command("/agent provider coder -")?
            .unwrap()
            .contains("Set agent coder provider to -")
    );
    let cleared = app.execute_command("/agent show coder")?.unwrap();
    assert!(cleared.contains("| provider | `-` |"));
    assert!(cleared.contains("| model | `-` |"));
    Ok(())
}

#[test]
fn application_loads_markdown_and_usrl_skills_from_filesystem() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let skill_dir = tmp.path().join(".vegvisir").join("skills");
    fs::create_dir_all(skill_dir.join("planner"))?;
    fs::write(
        skill_dir.join("planner").join("SKILL.md"),
        "# Planning Mode\nBreak work into verifiable phases.",
    )?;
    fs::write(
        skill_dir.join("regulated-release.usrl"),
        "contract regulated_release {\n  require approvals >= 2\n}",
    )?;
    fs::write(
        skill_dir.join("cryptography.lsl"),
        r#"
        library cryptography {
            meta { id: "cryptography"; name: "Cryptography"; version: "1.0.0"; }
            subskill cryptography.secure_randomness {
                id: cryptography.secure_randomness;
                title: "Secure Randomness";
                summary: "CSPRNG and nonce safety.";
                type: procedure;
                tags: [rng, nonce];
                load {
                    card: """Use a CSPRNG.""";
                    body: """Use operating-system cryptographic randomness.""";
                }
                verification: ["Random source is cryptographic."];
                eval_refs: [cryptography.secure_randomness.eval.basic];
            }
            eval cryptography.secure_randomness.eval.basic {
                target: cryptography.secure_randomness;
                task: "Check secure randomness guidance.";
                expected: ["CSPRNG"];
                forbidden: ["math.random"];
            }
        }
        "#,
    )?;

    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    let skills = app.execute_command("/skills")?.unwrap();
    assert!(skills.contains("filesystem: planner - Planning Mode"));
    assert!(skills.contains("regulated-workflow: regulated-release"));
    assert!(skills.contains("[contracts: regulated_release]"));
    assert!(skills.contains("linked-skill/procedure: cryptography.secure_randomness"));

    let compiled = app.execute_command("/skills compile")?.unwrap();
    assert!(compiled.contains("Compiled 1 LSL libraries, 1 sub-skills"));
    assert!(
        tmp.path()
            .join(".vegvisir/compiled/index/subskills.json")
            .exists()
    );
    let status = app.execute_command("/skills status")?.unwrap();
    assert!(status.contains("compiled_exists: true"));
    assert!(status.contains("fresh: true"));

    let routed = app
        .execute_command("/skills route cryptographic randomness nonce")?
        .unwrap();
    assert!(routed.contains("cryptography.secure_randomness"));

    let loaded = app
        .execute_command("/skills load --tokens 200 cryptographic randomness")?
        .unwrap();
    assert!(loaded.contains("Loaded 1 sub-skills"));
    assert!(loaded.contains("Use operating-system cryptographic randomness."));
    let evals = app.execute_command("/skills eval")?.unwrap();
    assert!(evals.contains("cryptography.secure_randomness.eval.basic"));
    assert!(evals.contains("passed"));

    let forged = app
        .execute_command(
            "/skills forge software_engineering.rust_nextest | Rust Nextest | Use cargo nextest for Rust test execution. | Use cargo nextest for Rust test execution. Run cargo nextest and inspect failing test output. | tags=rust,nextest",
        )?
        .unwrap();
    assert!(forged.contains("Forged candidate sub-skill software_engineering.rust_nextest"));
    let forged_status = fs::read_to_string(tmp.path().join("skills/software_engineering.lsl"))?;
    assert!(forged_status.contains("status: candidate;"));
    let patched = app
        .execute_command(
            "/skills patch software_engineering.rust_nextest | append_list_items | verification | Nextest output is inspected",
        )?
        .unwrap();
    assert!(patched.contains("Patched software_engineering.rust_nextest"));
    let promoted = app
        .execute_command("/skills promote software_engineering.rust_nextest")?
        .unwrap();
    assert!(promoted.contains("status to active"));
    let forged_status = fs::read_to_string(tmp.path().join("skills/software_engineering.lsl"))?;
    assert!(forged_status.contains("status: active;"));
    assert!(forged_status.contains("Nextest output is inspected"));
    app.execute_command("/skills route no-such-obscure-framework")?;
    app.execute_command("/skills route no-such-obscure-framework")?;
    let detected = app.execute_command("/skills detect")?.unwrap();
    assert!(detected.contains("no-such-obscure-framework"));
    let curated = app.execute_command("/skills curate")?.unwrap();
    assert!(curated.contains("total_subskills:"));
    let trace = app.execute_command("/skills trace")?.unwrap();
    assert!(trace.contains("route no-such-obscure-framework"));

    let planner = app
        .session
        .enabled_skills
        .iter()
        .find(|skill| skill.name == "planner")
        .expect("planner markdown skill should load");
    assert_eq!(
        planner.metadata.get("format").and_then(Value::as_str),
        Some("markdown")
    );
    assert!(
        planner
            .metadata
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .contains("Break work into verifiable phases")
    );

    let usrl = app
        .session
        .enabled_skills
        .iter()
        .find(|skill| skill.name == "regulated-release")
        .expect("USRL contract skill should load");
    assert_eq!(
        usrl.metadata.get("format").and_then(Value::as_str),
        Some("usrl")
    );
    assert!(
        usrl.metadata
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .contains("contract regulated_release")
    );
    assert_eq!(
        usrl.metadata
            .get("usrl_contracts")
            .and_then(Value::as_array)
            .and_then(|values| values.first())
            .and_then(Value::as_str),
        Some("regulated_release")
    );
    assert_eq!(
        usrl.metadata.get("usrl_validator").and_then(Value::as_str),
        Some("cms-v2-lightweight")
    );
    assert_eq!(
        usrl.metadata
            .get("usrl_validation_status")
            .and_then(Value::as_str),
        Some("not_requested")
    );
    app.execute_command("/agent create release | orchestrator | Release Agent | Govern releases.")?
        .unwrap();
    let bound = app
        .execute_command("/agent bind-usrl release regulated-release")?
        .unwrap();
    assert!(bound.contains("regulated_release"));
    let profile = app.execute_command("/agent show release")?.unwrap();
    assert!(profile.contains("| usrl_contracts | regulated_release |"));
    Ok(())
}

#[test]
fn runtime_usrl_gate_blocks_risky_tools_without_contract() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let registry = build_builtin_registry(tmp.path())?;
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy {
            active_agent_id: Some("agent-red".to_string()),
            active_agent_mode: Some("agent-red".to_string()),
            allowed_tools: Vec::new(),
            usrl_contracts: Vec::new(),
            strict_usrl: true,
            ..vegvisir_rust::policy::RuntimePolicy::default()
        },
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let blocked = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["pwd"]}).as_object().unwrap().clone(),
    });
    assert!(!blocked.ok);
    assert!(
        blocked
            .content
            .contains("risky operation requires an active USRL contract")
    );

    executor.runtime_policy.usrl_contracts = vec!["agent-red-baseline".to_string()];
    let allowed = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["pwd"]}).as_object().unwrap().clone(),
    });
    assert!(allowed.ok, "{}", allowed.content);

    executor.runtime_policy.usrl_constraints = vec!["no_secret_output".to_string()];
    let denied_secret = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["pwd"], "note": "token=secret-value"})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(!denied_secret.ok);
    assert!(
        denied_secret
            .content
            .contains("violates no-secret constraint")
    );
    Ok(())
}

#[test]
fn runtime_usrl_gate_enforces_stage_and_evidence_requirements() -> anyhow::Result<()> {
    let policy = vegvisir_rust::policy::RuntimePolicy {
        active_agent_id: Some("release".to_string()),
        active_agent_mode: Some("orchestrator".to_string()),
        usrl_contracts: vec!["regulated-release".to_string()],
        usrl_constraints: vec!["require_stage".to_string(), "require_evidence".to_string()],
        usrl_stages: vec!["plan".to_string(), "verify".to_string()],
        strict_usrl: true,
        ..vegvisir_rust::policy::RuntimePolicy::default()
    };

    let missing_stage = policy.gate(vegvisir_rust::policy::RuntimeGateRequest {
        operation: "run_command".to_string(),
        target: "run_command".to_string(),
        args_summary: json!({"command": ["pwd"]}),
        tool_metadata: None,
    });
    assert!(!missing_stage.allowed);
    assert!(missing_stage.reason.contains("stage evidence is required"));

    let bad_stage = policy.gate(vegvisir_rust::policy::RuntimeGateRequest {
        operation: "run_command".to_string(),
        target: "run_command".to_string(),
        args_summary: json!({"command": ["pwd"], "usrl_stage": "deploy", "evidence": "manual check"}),
        tool_metadata: None,
    });
    assert!(!bad_stage.allowed);
    assert!(bad_stage.reason.contains("not in contract stages"));

    let missing_evidence = policy.gate(vegvisir_rust::policy::RuntimeGateRequest {
        operation: "run_command".to_string(),
        target: "run_command".to_string(),
        args_summary: json!({"command": ["pwd"], "usrl_stage": "verify"}),
        tool_metadata: None,
    });
    assert!(!missing_evidence.allowed);
    assert!(
        missing_evidence
            .reason
            .contains("evidence or justification is required")
    );

    let allowed = policy.gate(vegvisir_rust::policy::RuntimeGateRequest {
        operation: "run_command".to_string(),
        target: "run_command".to_string(),
        args_summary: json!({"command": ["pwd"], "usrl_stage": "verify", "evidence": "test run observed"}),
        tool_metadata: None,
    });
    assert!(allowed.allowed, "{}", allowed.reason);
    Ok(())
}

#[test]
fn active_agent_tool_allow_list_blocks_unlisted_tools() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let registry = build_builtin_registry(tmp.path())?;
    fs::write(tmp.path().join("note.txt"), "allowed")?;
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: true,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy {
            active_agent_id: Some("reader".to_string()),
            active_agent_mode: Some("researcher".to_string()),
            allowed_tools: vec!["read_file".to_string()],
            usrl_contracts: Vec::new(),
            strict_usrl: false,
            ..vegvisir_rust::policy::RuntimePolicy::default()
        },
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let allowed = executor.execute(vegvisir_rust::types::ToolCall {
        name: "read_file".to_string(),
        args: json!({"path": "note.txt"}).as_object().unwrap().clone(),
    });
    assert!(allowed.ok, "{}", allowed.content);

    let blocked = executor.execute(vegvisir_rust::types::ToolCall {
        name: "run_command".to_string(),
        args: json!({"command": ["pwd"]}).as_object().unwrap().clone(),
    });
    assert!(!blocked.ok);
    assert!(
        blocked
            .content
            .contains("tool is not enabled for active agent: run_command")
    );
    Ok(())
}

#[test]
fn mcp_config_registers_namespaced_tools_and_enforces_hbse_boundary() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    fs::write(
        home.join("mcp.json"),
        serde_json::to_string_pretty(&json!({
            "servers": [{
                "id": "github",
                "display_name": "GitHub MCP",
                "transport": "http",
                "url": "https://mcp.example.test",
                "hbse_secret_refs": ["secret://vegvisir/mcp/github/default"],
                "consumer": "vegvisir.mcp.github",
                "purpose": "mcp.tool.call",
                "tools": [{
                    "name": "search_issues",
                    "description": "Search issues",
                    "schema": {"required": ["query"], "properties": {"query": "string"}}
                }]
            }]
        }))?,
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let servers = app.execute_command("/mcp list")?.unwrap();
    assert!(servers.contains("github"));
    assert!(servers.contains("hbse_refs=1"));
    let tools = app.execute_command("/mcp tools")?.unwrap();
    assert!(tools.contains("mcp::github::search_issues"));

    let denied_config = vegvisir_rust::core::McpServerConfig {
        id: "bad".to_string(),
        display_name: String::new(),
        transport: vegvisir_rust::core::McpTransport::Http,
        command: None,
        args: Vec::new(),
        working_dir: None,
        url: Some("https://bad.example.test".to_string()),
        enabled: true,
        hbse_secret_refs: Vec::new(),
        consumer: String::new(),
        purpose: String::new(),
        tools: Vec::new(),
        metadata: [(
            "api_secret".to_string(),
            Value::String("plaintext-token".to_string()),
        )]
        .into_iter()
        .collect(),
        discovery_error: None,
    };
    assert!(vegvisir_rust::mcp::validate_hbse_boundary(&denied_config).is_err());
    Ok(())
}

#[test]
fn mcp_commands_manage_config_without_plaintext_secrets() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    let rejected = app
        .execute_command("/mcp add-http bad https://mcp.example.test?token=plain secret://vegvisir/mcp/bad/default")?
        .unwrap();
    assert!(rejected.contains("must not contain plaintext secrets"));
    assert!(!home.join("mcp.json").exists());

    let added = app
        .execute_command("/mcp add-http github https://mcp.example.test/rpc secret://vegvisir/mcp/github/default vegvisir.mcp.github mcp.tool.call")?
        .unwrap();
    assert!(added.contains("Configured HTTP MCP server github"));
    let config = std::fs::read_to_string(home.join("mcp.json"))?;
    assert!(config.contains("secret://vegvisir/mcp/github/default"));
    assert!(!config.contains("token=plain"));

    let tool = app
        .execute_command("/mcp add-tool github search_issues Search GitHub issues")?
        .unwrap();
    assert!(tool.contains("Added MCP tool search_issues"));
    let shown = app.execute_command("/mcp show github")?.unwrap();
    assert!(shown.contains("id=github"));
    assert!(shown.contains("secret://vegvisir/mcp/github/default"));
    assert!(shown.contains("consumer=vegvisir.mcp.github"));
    assert!(shown.contains("tools=search_issues"));
    let status = app.execute_command("/mcp status")?.unwrap();
    assert!(status.contains("ok mcp/github transport=Http tools=1 hbse_refs=1"));
    let tools = app.execute_command("/mcp tools")?.unwrap();
    assert!(tools.contains("mcp::github::search_issues"));

    let removed_tool = app
        .execute_command("/mcp remove-tool github search_issues")?
        .unwrap();
    assert!(removed_tool.contains("Removed MCP tool search_issues"));
    assert_eq!(
        app.execute_command("/mcp tools")?.unwrap(),
        "No MCP tools are registered."
    );
    app.execute_command("/mcp add-tool github search_issues Search GitHub issues")?
        .unwrap();

    let disabled = app.execute_command("/mcp disable github")?.unwrap();
    assert!(disabled.contains("Disabled MCP server github"));
    assert_eq!(
        app.execute_command("/mcp tools")?.unwrap(),
        "No MCP tools are registered."
    );
    let enabled = app.execute_command("/mcp enable github")?.unwrap();
    assert!(enabled.contains("Enabled MCP server github"));
    assert!(
        app.execute_command("/mcp tools")?
            .unwrap()
            .contains("mcp::github::search_issues")
    );

    let stdio = app
        .execute_command("/mcp add-stdio local python3 /tmp/mock_mcp.py")?
        .unwrap();
    assert!(stdio.contains("Configured stdio MCP server local"));
    assert!(app.execute_command("/mcp list")?.unwrap().contains("local"));

    let removed = app.execute_command("/mcp remove github")?.unwrap();
    assert!(removed.contains("Removed MCP server github"));
    let list = app.execute_command("/mcp list")?.unwrap();
    assert!(!list.contains("github"));
    assert!(list.contains("local"));
    Ok(())
}

#[test]
fn mcp_http_can_use_registered_hbse_service_ref() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;

    app.execute_command("/hbse service add github-mcp secret://vegvisir/mcp/github/default vegvisir.mcp.github mcp.tool.call")?
        .unwrap();
    let added = app
        .execute_command("/mcp add-http-service github https://mcp.example.test/rpc github-mcp")?
        .unwrap();
    assert!(added.contains("Configured HTTP MCP server github from HBSE service ref github-mcp"));
    let shown = app.execute_command("/mcp show github")?.unwrap();
    assert!(shown.contains("hbse_secret_refs=secret://vegvisir/mcp/github/default"));
    assert!(shown.contains("consumer=vegvisir.mcp.github"));
    assert!(shown.contains("purpose=mcp.tool.call"));
    let stored = std::fs::read_to_string(home.join("mcp.json"))?;
    assert!(stored.contains("\"hbse_service_ref\": \"github-mcp\""));
    assert!(!stored.contains("password"));

    app.execute_command("/hbse service disable github-mcp")?
        .unwrap();
    let rejected = app
        .execute_command("/mcp add-http-service blocked https://mcp.example.test/rpc github-mcp")?
        .unwrap();
    assert!(rejected.contains("is disabled"));
    Ok(())
}

#[test]
fn mcp_stdio_tool_executes_json_rpc_call() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    let server_script = tmp.path().join("mock_mcp.py");
    fs::write(
        &server_script,
        r#"
import json
import sys

def read_message():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, _, value = line.decode().partition(":")
        if name.lower() == "content-length":
            length = int(value.strip())
    if length is None:
        raise RuntimeError("missing content length")
    return json.loads(sys.stdin.buffer.read(length).decode())

def write_message(payload):
    body = json.dumps(payload).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    sys.stdout.buffer.flush()

while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "initialize":
        write_message({"jsonrpc": "2.0", "id": message["id"], "result": {"protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": {"name": "mock", "version": "1"}}})
    elif method == "notifications/initialized":
        continue
    elif method == "tools/call":
        params = message.get("params", {})
        args = params.get("arguments", {})
        write_message({"jsonrpc": "2.0", "id": message["id"], "result": {"content": [{"type": "text", "text": f"echo:{args.get('query', '')}"}]}})
"#,
    )?;
    fs::write(
        home.join("mcp.json"),
        serde_json::to_string_pretty(&json!({
            "servers": [{
                "id": "local",
                "transport": "stdio",
                "command": "python3",
                "args": [server_script.display().to_string()],
                "tools": [{
                    "name": "echo",
                    "description": "Echo input",
                    "schema": {"required": ["query"], "properties": {"query": "string"}}
                }]
            }]
        }))?,
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.tool_executor.guardrails.policy.allow_risky_tools = true;
    let observation = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "mcp::local::echo".to_string(),
        args: json!({"query": "hello"}).as_object().unwrap().clone(),
    });
    assert!(observation.ok, "{}", observation.content);
    assert_eq!(observation.content, "echo:hello");
    assert_eq!(
        observation.data.get("server").and_then(Value::as_str),
        Some("local")
    );
    Ok(())
}

#[test]
fn mcp_stdio_tool_reuses_persistent_session_between_calls() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    let server_script = tmp.path().join("stateful_mcp.py");
    fs::write(
        &server_script,
        r#"
import json
import sys

count = 0

def read_message():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, _, value = line.decode().partition(":")
        if name.lower() == "content-length":
            length = int(value.strip())
    if length is None:
        raise RuntimeError("missing content length")
    return json.loads(sys.stdin.buffer.read(length).decode())

def write_message(payload):
    body = json.dumps(payload).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    sys.stdout.buffer.flush()

while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "initialize":
        write_message({"jsonrpc": "2.0", "id": message["id"], "result": {"protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": {"name": "stateful", "version": "1"}}})
    elif method == "notifications/initialized":
        continue
    elif method == "tools/call":
        count += 1
        write_message({"jsonrpc": "2.0", "id": message["id"], "result": {"content": [{"type": "text", "text": f"count:{count}"}]}})
"#,
    )?;
    fs::write(
        home.join("mcp.json"),
        serde_json::to_string_pretty(&json!({
            "servers": [{
                "id": "stateful",
                "transport": "stdio",
                "command": "python3",
                "args": [server_script.display().to_string()],
                "tools": [{
                    "name": "count",
                    "description": "Return call count",
                    "schema": {"properties": {}}
                }]
            }]
        }))?,
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.tool_executor.guardrails.policy.allow_risky_tools = true;
    let first = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "mcp::stateful::count".to_string(),
        args: Map::new(),
    });
    let second = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "mcp::stateful::count".to_string(),
        args: Map::new(),
    });

    assert!(first.ok, "{}", first.content);
    assert!(second.ok, "{}", second.content);
    assert_eq!(first.content, "count:1");
    assert_eq!(second.content, "count:2");
    Ok(())
}

#[test]
fn mcp_stdio_discovers_tools_when_config_omits_manual_tool_list() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    let server_script = tmp.path().join("discover_mcp.py");
    fs::write(
        &server_script,
        r#"
import json
import sys

def read_message():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, _, value = line.decode().partition(":")
        if name.lower() == "content-length":
            length = int(value.strip())
    return json.loads(sys.stdin.buffer.read(length).decode())

def write_message(payload):
    body = json.dumps(payload).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    sys.stdout.buffer.flush()

while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "initialize":
        write_message({"jsonrpc": "2.0", "id": message["id"], "result": {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}}, "serverInfo": {"name": "mock", "version": "1"}}})
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        write_message({"jsonrpc": "2.0", "id": message["id"], "result": {"tools": [{"name": "dynamic_echo", "description": "Dynamic echo", "inputSchema": {"required": ["query"], "properties": {"query": {"type": "string"}}}}]}})
    elif method == "tools/call":
        args = message.get("params", {}).get("arguments", {})
        write_message({"jsonrpc": "2.0", "id": message["id"], "result": {"content": [{"type": "text", "text": f"dynamic:{args.get('query', '')}"}]}})
"#,
    )?;
    fs::write(
        home.join("mcp.json"),
        serde_json::to_string_pretty(&json!({
            "servers": [{
                "id": "dynamic",
                "transport": "stdio",
                "command": "python3",
                "args": [server_script.display().to_string()]
            }]
        }))?,
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    let tools = app.execute_command("/mcp tools")?.unwrap();
    assert!(tools.contains("mcp::dynamic::dynamic_echo"));
    app.tool_executor.guardrails.policy.allow_risky_tools = true;
    let observation = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "mcp::dynamic::dynamic_echo".to_string(),
        args: json!({"query": "ok"}).as_object().unwrap().clone(),
    });
    assert!(observation.ok, "{}", observation.content);
    assert_eq!(observation.content, "dynamic:ok");
    Ok(())
}

#[test]
fn mcp_http_tool_routes_json_rpc_through_hbse_provider_http() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    let socket_path = tmp.path().join("hbse-mcp.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let captured = Arc::new(Mutex::new(None::<Value>));
    let captured_thread = Arc::clone(&captured);
    let handle = thread::spawn(move || -> anyhow::Result<()> {
        let (mut stream, _) = listener.accept()?;
        let mut line = String::new();
        std::io::BufReader::new(stream.try_clone()?).read_line(&mut line)?;
        let request: Value = serde_json::from_str(line.trim_end())?;
        *captured_thread.lock().unwrap() = Some(request);
        writeln!(
            stream,
            "{}",
            serde_json::to_string(&json!({
                "ok": true,
                "status_code": 200,
                "headers": {},
                "body": serde_json::to_string(&json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {
                        "content": [{"type": "text", "text": "remote:mcp"}]
                    }
                }))?
            }))?
        )?;
        Ok(())
    });

    fs::write(
        home.join("mcp.json"),
        serde_json::to_string_pretty(&json!({
            "servers": [{
                "id": "remote",
                "transport": "http",
                "url": "https://mcp.example.test/rpc",
                "hbse_secret_refs": ["secret://vegvisir/mcp/remote/default"],
                "consumer": "vegvisir.mcp.remote",
                "purpose": "mcp.tool.call",
                "metadata": {"hbse_socket": socket_path.display().to_string()},
                "tools": [{
                    "name": "remote_echo",
                    "description": "Remote echo",
                    "schema": {"required": ["query"], "properties": {"query": "string"}}
                }]
            }]
        }))?,
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.tool_executor.guardrails.policy.allow_risky_tools = true;
    let observation = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "mcp::remote::remote_echo".to_string(),
        args: json!({"query": "hello"}).as_object().unwrap().clone(),
    });
    assert!(observation.ok, "{}", observation.content);
    assert_eq!(observation.content, "remote:mcp");
    handle.join().unwrap()?;
    let request = captured.lock().unwrap().clone().expect("captured request");
    assert_eq!(request["command"], "provider_http");
    assert_eq!(
        request["secret_ref"],
        "secret://vegvisir/mcp/remote/default"
    );
    assert_eq!(request["consumer"], "vegvisir.mcp.remote");
    assert_eq!(request["purpose"], "mcp.tool.call");
    assert_eq!(request["method"], "POST");
    assert_eq!(request["url"], "https://mcp.example.test/rpc");
    let body: Value = serde_json::from_str(request["body"].as_str().unwrap())?;
    assert_eq!(body["method"], "tools/call");
    assert_eq!(body["params"]["name"], "remote_echo");
    assert_eq!(body["params"]["arguments"]["query"], "hello");
    Ok(())
}

#[test]
fn mcp_http_reuses_session_id_returned_through_hbse_headers() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    let socket_path = tmp.path().join("hbse-mcp-session.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured_thread = Arc::clone(&captured);
    let handle = thread::spawn(move || -> anyhow::Result<()> {
        for index in 0..2 {
            let (mut stream, _) = listener.accept()?;
            let mut line = String::new();
            std::io::BufReader::new(stream.try_clone()?).read_line(&mut line)?;
            let request: Value = serde_json::from_str(line.trim_end())?;
            captured_thread.lock().unwrap().push(request);
            writeln!(
                stream,
                "{}",
                serde_json::to_string(&json!({
                    "ok": true,
                    "status_code": 200,
                    "headers": if index == 0 {
                        json!({"Mcp-Session-Id": "session-123"})
                    } else {
                        json!({})
                    },
                    "body": serde_json::to_string(&json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": {
                            "content": [{"type": "text", "text": format!("remote:{index}")}]
                        }
                    }))?
                }))?
            )?;
        }
        Ok(())
    });

    fs::write(
        home.join("mcp.json"),
        serde_json::to_string_pretty(&json!({
            "servers": [{
                "id": "sessioned",
                "transport": "http",
                "url": "https://mcp.example.test/rpc",
                "hbse_secret_refs": ["secret://vegvisir/mcp/sessioned/default"],
                "consumer": "vegvisir.mcp.sessioned",
                "purpose": "mcp.tool.call",
                "metadata": {"hbse_socket": socket_path.display().to_string()},
                "tools": [{
                    "name": "ping",
                    "description": "Ping",
                    "schema": {"properties": {}}
                }]
            }]
        }))?,
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.tool_executor.guardrails.policy.allow_risky_tools = true;
    for _ in 0..2 {
        let observation = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
            name: "mcp::sessioned::ping".to_string(),
            args: Map::new(),
        });
        assert!(observation.ok, "{}", observation.content);
    }
    handle.join().unwrap()?;
    let captured = captured.lock().unwrap().clone();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0]["headers"]["MCP-Protocol-Version"], "2024-11-05");
    assert!(captured[0]["headers"].get("Mcp-Session-Id").is_none());
    assert_eq!(captured[1]["headers"]["Mcp-Session-Id"], "session-123");
    Ok(())
}

#[test]
fn mcp_http_discovers_tools_through_hbse_provider_http() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    let socket_path = tmp.path().join("hbse-mcp-discover.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured_thread = Arc::clone(&captured);
    let handle = thread::spawn(move || -> anyhow::Result<()> {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept()?;
            let mut line = String::new();
            std::io::BufReader::new(stream.try_clone()?).read_line(&mut line)?;
            let request: Value = serde_json::from_str(line.trim_end())?;
            let body: Value = serde_json::from_str(request["body"].as_str().unwrap())?;
            captured_thread.lock().unwrap().push(body.clone());
            let result = if body["method"] == "tools/list" {
                json!({
                    "tools": [{
                        "name": "remote_dynamic",
                        "description": "Remote dynamic",
                        "inputSchema": {"required": ["query"], "properties": {"query": {"type": "string"}}}
                    }]
                })
            } else {
                json!({"content": [{"type": "text", "text": "remote:dynamic"}]})
            };
            writeln!(
                stream,
                "{}",
                serde_json::to_string(&json!({
                    "ok": true,
                    "status_code": 200,
                    "headers": {},
                    "body": serde_json::to_string(&json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": result
                    }))?
                }))?
            )?;
        }
        Ok(())
    });

    fs::write(
        home.join("mcp.json"),
        serde_json::to_string_pretty(&json!({
            "servers": [{
                "id": "remote-dynamic",
                "transport": "http",
                "url": "https://mcp.example.test/rpc",
                "hbse_secret_refs": ["secret://vegvisir/mcp/remote-dynamic/default"],
                "consumer": "vegvisir.mcp.remote-dynamic",
                "purpose": "mcp.tool.call",
                "metadata": {"hbse_socket": socket_path.display().to_string()}
            }]
        }))?,
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    let tools = app.execute_command("/mcp tools")?.unwrap();
    assert!(tools.contains("mcp::remote-dynamic::remote_dynamic"));
    app.tool_executor.guardrails.policy.allow_risky_tools = true;
    let observation = app.tool_executor.execute(vegvisir_rust::types::ToolCall {
        name: "mcp::remote-dynamic::remote_dynamic".to_string(),
        args: json!({"query": "hello"}).as_object().unwrap().clone(),
    });
    assert!(observation.ok, "{}", observation.content);
    assert_eq!(observation.content, "remote:dynamic");
    handle.join().unwrap()?;
    let captured = captured.lock().unwrap();
    assert_eq!(captured[0]["method"], "tools/list");
    assert_eq!(captured[1]["method"], "tools/call");
    assert_eq!(captured[1]["params"]["name"], "remote_dynamic");
    Ok(())
}

#[test]
fn builtin_registry_exposes_cms_tools() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut registry = build_builtin_registry(tmp.path())?;
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry: registry.clone(),
        guardrails: GuardrailEngine::default(),
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let remembered = executor.execute(vegvisir_rust::types::ToolCall {
        name: "cms_remember".to_string(),
        args: json!({
            "memory_type": "decision",
            "title": "CMS tool memory",
            "content": "Vegvisir tool execution writes to CMS-v2."
        })
        .as_object()
        .unwrap()
        .clone(),
    });
    assert!(remembered.ok, "{}", remembered.content);

    let recalled = executor.execute(vegvisir_rust::types::ToolCall {
        name: "cms_recall".to_string(),
        args: json!({"query": "tool execution CMS-v2", "limit": 5})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(recalled.ok, "{}", recalled.content);
    assert!(recalled.content.contains("CMS tool memory"));

    let recent = executor.execute(vegvisir_rust::types::ToolCall {
        name: "cms_recent".to_string(),
        args: json!({"limit": 5}).as_object().unwrap().clone(),
    });
    assert!(recent.ok, "{}", recent.content);
    assert!(recent.content.contains("CMS tool memory"));

    let chatgpt_archive_search = executor.execute(vegvisir_rust::types::ToolCall {
        name: "cms_search_chatgpt_archive".to_string(),
        args: json!({"query": "tool execution CMS-v2", "limit": 5})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(
        chatgpt_archive_search.ok,
        "{}",
        chatgpt_archive_search.content
    );
    assert_eq!(
        chatgpt_archive_search
            .data
            .get("corpus")
            .and_then(Value::as_str),
        Some("chatgpt_archive")
    );

    let prepared = executor.execute(vegvisir_rust::types::ToolCall {
        name: "cms_prepare_context".to_string(),
        args: json!({"message": "Continue CMS tool memory and tool execution CMS-v2 work"})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(prepared.ok, "{}", prepared.content);
    assert!(prepared.content.contains("CMS tool memory"));

    let legacy_context = executor.execute(vegvisir_rust::types::ToolCall {
        name: "eternium_prepare_context".to_string(),
        args: json!({"user_message": "Continue CMS tool memory and tool execution CMS-v2 work", "mode": "balanced"})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(legacy_context.ok, "{}", legacy_context.content);
    assert!(legacy_context.content.contains("context_prompt"));
    assert!(
        legacy_context
            .data
            .get("context_prompt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .contains("CMS tool memory")
    );

    let private_context = executor.execute(vegvisir_rust::types::ToolCall {
        name: "eternium_prepare_context".to_string(),
        args: json!({"user_message": "Answer without long-term memory", "mode": "private"})
            .as_object()
            .unwrap()
            .clone(),
    });
    assert!(private_context.ok, "{}", private_context.content);
    assert!(!private_context.content.contains("CMS tool memory"));
    assert_eq!(
        private_context
            .data
            .get("included_memory_ids")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    let model_request = executor.execute(vegvisir_rust::types::ToolCall {
        name: "cms_prepare_model_request".to_string(),
        args: json!({
            "message": "Continue tool execution memory work",
            "provider": "openai",
            "model": "gpt-test"
        })
        .as_object()
        .unwrap()
        .clone(),
    });
    assert!(model_request.ok, "{}", model_request.content);
    assert!(model_request.data.contains_key("manifest"));

    registry.register(Tool::new(
        "extra",
        "extra",
        Arc::new(|_| Observation::ok("ok")),
        json!({}),
        false,
    ))?;
    Ok(())
}

#[test]
fn cms_v2_is_the_runtime_memory_system() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut cms = VegvisirCms::open(VegvisirCmsConfig {
        db_path: tmp.path().join("cms.sqlite3"),
        user_id: "tester".to_string(),
        project_id: Some("Vegvisir".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })?;

    let committed = cms.remember(
        "decision",
        "CMS v2 integration",
        "Vegvisir uses CMS-v2 as its runtime memory system.",
    )?;
    assert!(committed.created_new || committed.updated_existing);

    let bundle = cms.retrieve("runtime memory system", 5)?;
    assert!(
        bundle
            .results
            .iter()
            .any(|result| result.memory.title == "CMS v2 integration")
    );

    let prepared = cms.prepare_context("Continue the Vegvisir runtime memory integration")?;
    assert!(prepared.packed_text.contains("CMS v2 integration"));
    let envelope = cms.prepare_cached_prompt(
        "Continue the Vegvisir runtime memory integration",
        "openai",
        "gpt-test",
    )?;
    assert!(!envelope.manifest.prompt_cache_key.is_empty());
    assert!(envelope.model_request.prompt.contains("CMS v2 integration"));
    Ok(())
}

#[test]
fn cms_context_private_mode_uses_no_long_term_memory() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut cms = VegvisirCms::open(VegvisirCmsConfig {
        db_path: tmp.path().join("cms.sqlite3"),
        user_id: "tester".to_string(),
        project_id: Some("Vegvisir".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })?;
    cms.remember(
        "preference",
        "Detailed specifications",
        "User prefers detailed specifications in all responses.",
    )?;

    let prepared = cms.prepare_context_with_options(
        "Answer this without using memory.",
        ContextPrepareOptions {
            mode: cms_v2::ecm::ContextMode::Minimal,
            metadata: [("memory_mode".to_string(), json!("none"))]
                .into_iter()
                .collect(),
            budget: None,
        },
    )?;

    assert!(prepared.included_memory_ids.is_empty());
    assert!(!prepared.packed_text.contains("Detailed specifications"));
    assert_eq!(
        prepared.metadata.get("memory_mode").and_then(Value::as_str),
        Some("none")
    );
    Ok(())
}

#[test]
fn cms_model_request_skips_ambient_memory_for_trivial_short_messages() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut cms = VegvisirCms::open(VegvisirCmsConfig {
        db_path: tmp.path().join("cms.sqlite3"),
        user_id: "tester".to_string(),
        project_id: Some("Vegvisir".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })?;
    cms.remember(
        "note",
        "Large imported note",
        "This imported memory should not be consulted for a trivial test message.",
    )?;

    let trivial = cms.prepare_cached_prompt("this is a test message", "openai", "gpt-test")?;
    assert!(!trivial.model_request.prompt.contains("Large imported note"));
    assert!(
        trivial
            .model_request
            .prompt
            .contains("this is a test message")
    );

    let relevant = cms.prepare_cached_prompt(
        "Continue with the memory work from this project",
        "openai",
        "gpt-test",
    )?;
    assert!(
        relevant
            .model_request
            .prompt
            .contains("Large imported note")
    );
    Ok(())
}

#[test]
fn cms_v2_blocks_secret_like_memory() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut cms = VegvisirCms::open(VegvisirCmsConfig {
        db_path: tmp.path().join("cms.sqlite3"),
        user_id: "tester".to_string(),
        project_id: Some("Vegvisir".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })?;

    let error = cms
        .remember("note", "secret", "api_key=abcdefghijklmnop")
        .unwrap_err()
        .to_string();

    assert!(error.contains("sensitive secret-like content"));
    Ok(())
}

#[test]
fn cms_v2_retrieval_isolates_configured_users() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let db_path = tmp.path().join("cms.sqlite3");
    let mut alpha = VegvisirCms::open(VegvisirCmsConfig {
        db_path: db_path.clone(),
        user_id: "alpha".to_string(),
        project_id: Some("Vegvisir".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })?;
    let mut beta = VegvisirCms::open(VegvisirCmsConfig {
        db_path,
        user_id: "beta".to_string(),
        project_id: Some("Vegvisir".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })?;

    alpha.remember("note", "Alpha memory", "Alpha only memory content.")?;

    assert!(
        alpha
            .retrieve("Alpha only", 5)?
            .results
            .iter()
            .any(|result| result.memory.title == "Alpha memory")
    );
    assert!(beta.retrieve("Alpha only", 5)?.results.is_empty());
    Ok(())
}

#[test]
fn openai_compatible_provider_consumes_cms_envelope() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut cms = VegvisirCms::open(VegvisirCmsConfig {
        db_path: tmp.path().join("cms.sqlite3"),
        user_id: "tester".to_string(),
        project_id: Some("Vegvisir".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })?;
    cms.remember(
        "decision",
        "Provider envelope memory",
        "Provider adapters consume CMS-v2 prompt cache envelopes.",
    )?;
    let envelope =
        cms.prepare_cached_prompt("Continue provider envelope work", "openai", "gpt-test")?;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let request = String::from_utf8_lossy(&bytes).to_string();
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"provider saw \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"cms envelope\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(request)
    });

    let adapter = OpenAICompatibleProviderAdapter {
        config: ProviderConfig {
            name: "test-openai".to_string(),
            display_name: Some("Test OpenAI".to_string()),
            kind: "openai_compatible".to_string(),
            api_key_env: None,
            base_url: Some(format!("http://{}", addr)),
            auth_type: "none".to_string(),
            enabled: true,
            metadata: Default::default(),
        },
    };
    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "test-openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: false,
        enabled: true,
        metadata: Default::default(),
    };

    let response = adapter.complete_envelope(&envelope, &model, "test-openai")?;
    assert_eq!(response, "provider saw cms envelope");
    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("POST /chat/completions"));
    assert!(request.contains("Provider envelope memory"));
    assert!(!request.contains("\"metadata\""));
    assert!(request.contains("\"stream\":true"));
    Ok(())
}

#[test]
fn openai_envelope_stream_emits_deltas() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut cms = VegvisirCms::open(VegvisirCmsConfig {
        db_path: tmp.path().join("cms.sqlite3"),
        user_id: "tester".to_string(),
        project_id: Some("Vegvisir".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })?;
    let envelope = cms.prepare_cached_prompt("stream this", "test-openai", "gpt-test")?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<()> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"part \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"two\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(())
    });
    let adapter = OpenAICompatibleProviderAdapter {
        config: ProviderConfig {
            name: "test-openai".to_string(),
            display_name: Some("Test OpenAI".to_string()),
            kind: "openai_compatible".to_string(),
            api_key_env: None,
            base_url: Some(format!("http://{}", addr)),
            auth_type: "none".to_string(),
            enabled: true,
            metadata: Default::default(),
        },
    };
    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "test-openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let mut deltas = Vec::new();
    let response = adapter.stream_envelope(&envelope, &model, "test-openai", &mut |delta| {
        deltas.push(delta.to_string());
    })?;

    server.join().expect("server thread completed")?;
    assert_eq!(response, "part two");
    assert_eq!(deltas, vec!["part ", "two"]);
    Ok(())
}

#[test]
fn openai_provider_routes_through_responses_api() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"responses \"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"api\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"output_text\":\"responses api\",\"output\":[]}}\n\n",
            "data: [DONE]\n\n"
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    });
    let adapter = OpenAICompatibleProviderAdapter {
        config: ProviderConfig {
            name: "openai".to_string(),
            display_name: Some("OpenAI".to_string()),
            kind: "openai".to_string(),
            api_key_env: None,
            base_url: Some(format!("http://{}", addr)),
            auth_type: "none".to_string(),
            enabled: true,
            metadata: Default::default(),
        },
    };
    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: "hello".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    }];
    let response = adapter.complete(&messages, &model, "openai")?;
    assert_eq!(response, "responses api");
    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("POST /responses HTTP/1.1"));
    assert!(request.contains("\"input\""));
    assert!(!request.contains("/chat/completions"));
    Ok(())
}

#[test]
fn cms_envelope_send_includes_harness_system_prompt() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut cms = VegvisirCms::open(VegvisirCmsConfig {
        db_path: tmp.path().join("cms.sqlite3"),
        user_id: "tester".to_string(),
        project_id: Some("Vegvisir".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })?;
    let envelope = cms.prepare_cached_prompt("user turn", "test-openai", "gpt-test")?;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let request = String::from_utf8_lossy(&bytes).to_string();
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(request)
    });
    let adapter = OpenAICompatibleProviderAdapter {
        config: ProviderConfig {
            name: "test-openai".to_string(),
            display_name: Some("Test OpenAI".to_string()),
            kind: "openai_compatible".to_string(),
            api_key_env: None,
            base_url: Some(format!("http://{}", addr)),
            auth_type: "none".to_string(),
            enabled: true,
            metadata: Default::default(),
        },
    };
    let mut models = ModelRegistry::default();
    models.register(ModelInfo {
        name: "gpt-test".to_string(),
        provider: "test-openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    });
    let mut session = vegvisir_rust::core::SessionState::new(tmp.path(), Vec::new(), Vec::new());
    session.current_provider = "test-openai".to_string();
    session.current_model = "gpt-test".to_string();
    session.system_prompt = "Always answer in uppercase.".to_string();
    session.messages.push(ChatMessage {
        role: "user".to_string(),
        content: "prior private turn not sent by default".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    });
    let mut runner = ConversationRunner {
        provider: adapter,
        models,
        tools: None,
        tool_executor: None,
        event_sink: None,
        cancel_token: None,
        steering_rx: None,
    };

    runner.send_with_envelope(&mut session, "user turn", envelope)?;

    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("Harness system prompt"));
    assert!(request.contains("Always answer in uppercase."));
    assert!(request.contains("prior private turn not sent by default"));
    Ok(())
}

#[test]
fn provider_payloads_preserve_system_context_across_wire_formats() {
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "Harness prompt.

ECM memory: blue lantern"
                .to_string(),
            attachments: Vec::new(),
            created_at: Utc::now(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: "What is the codename?".to_string(),
            attachments: Vec::new(),
            created_at: Utc::now(),
        },
    ];
    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };

    let openai = vegvisir_rust::provider::test_support::openai_messages_for_test(&messages);
    assert_eq!(openai[0]["role"], "system");
    assert!(
        openai[0]["content"]
            .as_str()
            .unwrap()
            .contains("ECM memory: blue lantern")
    );

    let responses =
        vegvisir_rust::provider::test_support::responses_payload_for_test(&messages, &model);
    assert!(
        responses["instructions"]
            .as_str()
            .unwrap()
            .contains("ECM memory: blue lantern")
    );
    assert!(responses["input"].as_array().unwrap().iter().any(|item| {
        item["role"] == "user" && item["content"][0]["text"] == "What is the codename?"
    }));

    let anthropic = vegvisir_rust::provider::test_support::anthropic_messages_payload_for_test(
        &messages, &model,
    );
    let anthropic_system = anthropic["system"].as_array().unwrap();
    assert!(
        anthropic_system[0]["text"]
            .as_str()
            .unwrap()
            .contains("ECM memory: blue lantern")
    );
    assert_eq!(
        anthropic_system[0]["cache_control"],
        serde_json::json!({"type": "ephemeral"})
    );
    assert_eq!(anthropic["messages"][0]["role"], "user");
    assert!(
        openai
            .iter()
            .all(|message| message.get("cache_control").is_none())
    );
    assert!(responses.get("cache_control").is_none());

    let google = vegvisir_rust::provider::test_support::google_generate_content_payload_for_test(
        &messages, &model,
    );
    assert!(
        google["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("ECM memory: blue lantern")
    );
    assert_eq!(google["contents"][0]["role"], "user");
}

#[test]
fn provider_payloads_preserve_image_attachments_across_wire_formats() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let image_path = tmp.path().join("pixel.png");
    fs::write(&image_path, b"not-a-real-png-but-provider-payload-test")?;
    let attachment = Attachment {
        path: image_path.display().to_string(),
        kind: "image".to_string(),
        mime_type: Some("image/png".to_string()),
        name: Some("pixel.png".to_string()),
        size_bytes: Some(36),
    };
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: "Describe this image.".to_string(),
        attachments: vec![attachment],
        created_at: Utc::now(),
    }];
    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };

    let openai = vegvisir_rust::provider::test_support::openai_messages_for_test(&messages);
    assert!(openai[0]["content"].as_array().unwrap().iter().any(|part| {
        part["type"] == "image_url"
            && part["image_url"]["url"]
                .as_str()
                .is_some_and(|url| url.starts_with("data:image/png;base64,"))
    }));

    let responses =
        vegvisir_rust::provider::test_support::responses_payload_for_test(&messages, &model);
    assert!(
        responses["input"][0]["content"]
            .as_array()
            .unwrap()
            .iter()
            .any(|part| {
                part["type"] == "input_image"
                    && part["image_url"]
                        .as_str()
                        .is_some_and(|url| url.starts_with("data:image/png;base64,"))
            })
    );

    let anthropic = vegvisir_rust::provider::test_support::anthropic_messages_payload_for_test(
        &messages, &model,
    );
    assert!(
        anthropic["messages"][0]["content"]
            .as_array()
            .unwrap()
            .iter()
            .any(|part| {
                part["type"] == "image"
                    && part["source"]["media_type"] == "image/png"
                    && part["source"]["data"]
                        .as_str()
                        .is_some_and(|data| !data.is_empty())
            })
    );

    let google = vegvisir_rust::provider::test_support::google_generate_content_payload_for_test(
        &messages, &model,
    );
    assert!(
        google["contents"][0]["parts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|part| {
                part["inlineData"]["mimeType"] == "image/png"
                    && part["inlineData"]["data"]
                        .as_str()
                        .is_some_and(|data| !data.is_empty())
            })
    );

    Ok(())
}

#[test]
fn provider_stream_parsers_return_same_final_text() -> anyhow::Result<()> {
    let expected = "Hello parity.";
    let openai = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello \"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"parity.\"}}]}\n\n",
        "data: [DONE]\n\n",
    );
    let responses = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello \"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"parity.\"}\n\n",
        "data: [DONE]\n\n",
    );
    let anthropic = concat!(
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Hello \"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"parity.\"}}\n\n",
        "data: [DONE]\n\n",
    );
    let google = concat!(
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello \"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"parity.\"}]}}]}\n\n",
    );

    assert_eq!(
        vegvisir_rust::provider::test_support::parse_openai_sse_for_test(openai)?,
        expected
    );
    assert_eq!(
        vegvisir_rust::provider::test_support::parse_responses_sse_for_test(responses)?,
        expected
    );
    assert_eq!(
        vegvisir_rust::provider::test_support::parse_anthropic_sse_for_test(anthropic)?,
        expected
    );
    assert_eq!(
        vegvisir_rust::provider::test_support::parse_google_stream_for_test(google)?,
        expected
    );
    Ok(())
}

#[test]
fn native_tool_argument_repair_is_provider_independent() {
    let args = vec![
        r#"{"path":"alpha.txt"}"#,
        "```json\n{\"path\":\"alpha.txt\"}\n```",
        "please call with {\"path\":\"alpha.txt\"}",
    ];
    for raw in args {
        let parsed = vegvisir_rust::provider::test_support::parse_tool_arguments_for_test(Some(
            &Value::String(raw.to_string()),
        ));
        assert_eq!(
            parsed.get("path").and_then(Value::as_str),
            Some("alpha.txt")
        );
    }
    let object = json!({"path":"beta.txt"});
    let parsed =
        vegvisir_rust::provider::test_support::parse_tool_arguments_for_test(Some(&object));
    assert_eq!(parsed.get("path").and_then(Value::as_str), Some("beta.txt"));
}

#[test]
fn provider_tool_round_limit_result_is_consistent() -> anyhow::Result<()> {
    let observations = vec![
        ("read_file".to_string(), "alpha".to_string()),
        ("run_tests".to_string(), "failure at tail".to_string()),
    ];
    let message =
        vegvisir_rust::provider::test_support::tool_round_limit_result_for_test(&observations, 2)?;
    assert!(message.contains("Tool-call round limit reached"));
    assert!(message.contains(
        "[read_file]
alpha"
    ));
    assert!(message.contains(
        "[run_tests]
failure at tail"
    ));
    Ok(())
}

#[test]
fn provider_stream_parsers_surface_provider_errors_consistently() {
    let openai = "data: {\"error\":{\"message\":\"rate limit parity\"}}\n\n";
    let responses = "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"rate limit parity\"}}}\n\n";
    let anthropic = "data: {\"type\":\"error\",\"error\":{\"message\":\"rate limit parity\"}}\n\n";
    let google = "data: {\"error\":{\"message\":\"rate limit parity\"}}\n\n";

    let errors = [
        vegvisir_rust::provider::test_support::parse_openai_sse_for_test(openai),
        vegvisir_rust::provider::test_support::parse_responses_sse_for_test(responses),
        vegvisir_rust::provider::test_support::parse_anthropic_sse_for_test(anthropic),
        vegvisir_rust::provider::test_support::parse_google_stream_for_test(google),
    ];
    for error in errors {
        let message = error
            .expect_err("provider stream error should be surfaced")
            .to_string();
        assert!(
            message.contains("rate limit parity"),
            "unexpected error: {message}"
        );
    }
}

#[test]
fn provider_tool_schema_conversion_preserves_complex_schema_across_wire_formats() {
    let tool = json!({
        "name": "inspect_files",
        "description": "Inspect workspace files",
        "parameters": {
            "type": "object",
            "properties": {
                "paths": {"type": "array"},
                "options": {
                    "type": "object",
                    "properties": {
                        "include_hidden": "boolean",
                        "limit": {"type": "integer"}
                    }
                }
            },
            "required": ["paths"]
        }
    });

    let openai = openai_tool_schema(&tool);
    let openai_params = &openai["function"]["parameters"];
    assert_eq!(
        openai_params["properties"]["paths"]["items"]["type"],
        "string"
    );
    assert_eq!(
        openai_params["properties"]["options"]["additionalProperties"],
        false
    );
    assert_eq!(
        openai_params["properties"]["options"]["properties"]["include_hidden"]["type"],
        "boolean"
    );

    let responses = vegvisir_rust::provider::test_support::responses_tool_schema_for_test(&tool);
    assert_eq!(responses["parameters"], *openai_params);

    let anthropic = vegvisir_rust::provider::test_support::anthropic_tool_schema_for_test(&tool);
    assert_eq!(anthropic["input_schema"], *openai_params);

    let google = vegvisir_rust::provider::test_support::google_tool_schema_for_test(&tool);
    assert_eq!(google["parameters"], *openai_params);
}

#[test]
fn active_agent_markdown_skills_reach_cms_model_request_prompt() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let skill_dir = tmp
        .path()
        .join(".vegvisir")
        .join("skills")
        .join("planner-mode");
    fs::create_dir_all(&skill_dir)?;
    fs::write(
        skill_dir.join("SKILL.md"),
        "# Planner Mode\nAlways return numbered implementation phases.",
    )?;
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    app.execute_command("/agent create planner | planner | Planner | You plan work.")?
        .unwrap();
    app.execute_command("/agent enable-skill planner planner-mode")?
        .unwrap();
    app.execute_command("/agent use planner")?.unwrap();

    let request = app
        .execute_command("/model-request Build the next feature")?
        .unwrap();
    assert!(request.contains("Harness system prompt"));
    assert!(request.contains("You plan work."));
    assert!(request.contains("Skill: planner-mode"));
    assert!(request.contains("Always return numbered implementation phases."));
    Ok(())
}

#[test]
fn openai_compatible_provider_streams_regular_chat_messages_by_default() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let request = String::from_utf8_lossy(&bytes).to_string();
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"regular \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"stream\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(request)
    });
    let adapter = OpenAICompatibleProviderAdapter {
        config: ProviderConfig {
            name: "test-openai".to_string(),
            display_name: Some("Test OpenAI".to_string()),
            kind: "openai_compatible".to_string(),
            api_key_env: None,
            base_url: Some(format!("http://{}", addr)),
            auth_type: "none".to_string(),
            enabled: true,
            metadata: Default::default(),
        },
    };
    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "test-openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let response = adapter.complete(
        &[ChatMessage {
            role: "user".to_string(),
            content: "hello provider".to_string(),
            attachments: Vec::new(),
            created_at: Utc::now(),
        }],
        &model,
        "test-openai",
    )?;
    assert_eq!(response, "regular stream");
    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("\"content\":\"hello provider\""));
    assert!(request.contains("\"stream\":true"));
    Ok(())
}

#[test]
fn providers_accept_successful_empty_sse_streams() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<()> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            if String::from_utf8_lossy(&bytes).contains("\r\n\r\n") {
                break;
            }
        }
        let body = "data: [DONE]\n\n";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(())
    });
    let adapter = OpenAICompatibleProviderAdapter {
        config: ProviderConfig {
            name: "test-openai-empty".to_string(),
            display_name: None,
            kind: "openai_compatible".to_string(),
            api_key_env: None,
            base_url: Some(format!("http://{}", addr)),
            auth_type: "none".to_string(),
            enabled: true,
            metadata: Default::default(),
        },
    };
    let model = ModelInfo {
        name: "gpt-empty".to_string(),
        provider: "test-openai-empty".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };

    let response = adapter.complete(
        &[ChatMessage {
            role: "system".to_string(),
            content: "custom prompt".to_string(),
            attachments: Vec::new(),
            created_at: Utc::now(),
        }],
        &model,
        "test-openai-empty",
    )?;

    server.join().expect("server thread completed")?;
    assert_eq!(response, "");
    Ok(())
}

#[test]
fn openai_compatible_provider_includes_http_error_body() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<()> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let body = r#"{"error":{"message":"Incorrect API key provided"}}"#;
        write!(
            stream,
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(())
    });
    let adapter = OpenAICompatibleProviderAdapter {
        config: ProviderConfig {
            name: "test-openai".to_string(),
            display_name: Some("Test OpenAI".to_string()),
            kind: "openai_compatible".to_string(),
            api_key_env: None,
            base_url: Some(format!("http://{}", addr)),
            auth_type: "none".to_string(),
            enabled: true,
            metadata: Default::default(),
        },
    };
    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "test-openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };

    let error = adapter
        .complete(
            &[ChatMessage {
                role: "user".to_string(),
                content: "hello provider".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            }],
            &model,
            "test-openai",
        )
        .unwrap_err()
        .to_string();

    server.join().expect("server thread completed")?;
    assert!(error.contains("test-openai request failed: 401"));
    assert!(error.contains("Incorrect API key provided"));
    Ok(())
}

#[test]
fn anthropic_provider_streams_messages_with_system_prompt() -> anyhow::Result<()> {
    unsafe {
        std::env::set_var("VEGVISIR_TEST_ANTHROPIC_KEY", "anthropic-test-key");
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let request = String::from_utf8_lossy(&bytes).to_string();
        let body = concat!(
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"anthropic \"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"stream\"}}\n\n"
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(request)
    });
    let adapter = AnthropicProviderAdapter {
        config: ProviderConfig {
            name: "test-anthropic".to_string(),
            display_name: Some("Test Anthropic".to_string()),
            kind: "anthropic".to_string(),
            api_key_env: Some("VEGVISIR_TEST_ANTHROPIC_KEY".to_string()),
            base_url: Some(format!("http://{}", addr)),
            auth_type: "api_key".to_string(),
            enabled: true,
            metadata: Default::default(),
        },
    };
    let model = ModelInfo {
        name: "claude-test".to_string(),
        provider: "test-anthropic".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };

    let response = adapter.complete(
        &[
            ChatMessage {
                role: "system".to_string(),
                content: "Be terse".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            },
        ],
        &model,
        "test-anthropic",
    )?;

    assert_eq!(response, "anthropic stream");
    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("POST /messages"));
    assert!(request.contains("x-api-key: anthropic-test-key"));
    assert!(request.contains("\"system\":[{\"cache_control\":{\"type\":\"ephemeral\"}"));
    assert!(request.contains("\"text\":\"Be terse\""));
    assert!(request.contains("\"role\":\"user\""));
    Ok(())
}

#[test]
fn hbse_anthropic_provider_routes_messages_through_broker_socket() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let socket_path = tmp.path().join("hbse-anthropic.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let server = thread::spawn(move || -> anyhow::Result<Value> {
        let (mut stream, _) = listener.accept()?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            if buffer[..n].contains(&b'\n') {
                break;
            }
        }
        let request: Value = serde_json::from_slice(
            bytes
                .split(|byte| *byte == b'\n')
                .next()
                .unwrap_or(bytes.as_slice()),
        )?;
        let body = concat!(
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"hbse anthropic\"}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let response = json!({
            "ok": true,
            "status_code": 200,
            "body": body
        });
        stream.write_all(serde_json::to_string(&response)?.as_bytes())?;
        stream.write_all(b"\n")?;
        Ok(request)
    });
    let adapter = HBSEAnthropicProviderAdapter {
        config: ProviderConfig {
            name: "test-anthropic-hbse".to_string(),
            display_name: Some("Test Anthropic HBSE".to_string()),
            kind: "hbse_anthropic".to_string(),
            api_key_env: None,
            base_url: Some("https://api.anthropic.example/v1".to_string()),
            auth_type: "hbse".to_string(),
            enabled: true,
            metadata: BTreeMap::from([
                (
                    "hbse_socket".to_string(),
                    Value::String(socket_path.display().to_string()),
                ),
                (
                    "hbse_secret_ref".to_string(),
                    Value::String("secret://vegvisir/anthropic/test".to_string()),
                ),
                (
                    "credential_header".to_string(),
                    Value::String("x-api-key".to_string()),
                ),
                (
                    "credential_prefix".to_string(),
                    Value::String(String::new()),
                ),
            ]),
        },
    };
    let model = ModelInfo {
        name: "claude-test".to_string(),
        provider: "anthropic".to_string(),
        display_name: None,
        context_window: Some(200000),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };

    let response = adapter.complete(
        &[
            ChatMessage {
                role: "system".to_string(),
                content: "Be exact".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            },
        ],
        &model,
        "test-anthropic-hbse",
    )?;

    assert_eq!(response, "hbse anthropic");
    let request = server.join().expect("server thread completed")?;
    assert_eq!(request["command"], "provider_http");
    assert_eq!(request["secret_ref"], "secret://vegvisir/anthropic/test");
    assert_eq!(request["credential_header"], "x-api-key");
    assert_eq!(request["credential_prefix"], "");
    assert_eq!(request["url"], "https://api.anthropic.example/v1/messages");
    assert_eq!(request["headers"]["anthropic-version"], "2023-06-01");
    let body = request["body"].as_str().unwrap();
    let body_json: Value = serde_json::from_str(body)?;
    assert_eq!(body_json["model"], "claude-test");
    assert_eq!(body_json["system"][0]["type"], "text");
    assert_eq!(body_json["system"][0]["text"], "Be exact");
    Ok(())
}

#[test]
fn google_provider_streams_generate_content() -> anyhow::Result<()> {
    unsafe {
        std::env::set_var("VEGVISIR_TEST_GOOGLE_KEY", "google-test-key");
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let request = String::from_utf8_lossy(&bytes).to_string();
        let body = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"gemini \"}]}}]}\n\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"stream\"}]}}]}\n\n"
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(request)
    });
    let adapter = GoogleProviderAdapter {
        config: ProviderConfig {
            name: "test-google".to_string(),
            display_name: Some("Test Google".to_string()),
            kind: "google".to_string(),
            api_key_env: Some("VEGVISIR_TEST_GOOGLE_KEY".to_string()),
            base_url: Some(format!("http://{}", addr)),
            auth_type: "api_key".to_string(),
            enabled: true,
            metadata: Default::default(),
        },
    };
    let model = ModelInfo {
        name: "gemini-test".to_string(),
        provider: "test-google".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };

    let response = adapter.complete(
        &[ChatMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
            attachments: Vec::new(),
            created_at: Utc::now(),
        }],
        &model,
        "test-google",
    )?;

    assert_eq!(response, "gemini stream");
    let request = server.join().expect("server thread completed")?;
    assert!(
        request
            .contains("POST /models/gemini-test:streamGenerateContent?alt=sse&key=google-test-key")
    );
    assert!(request.contains("\"role\":\"user\""));
    assert!(request.contains("\"parts\":[{\"text\":\"hello\"}]"));
    Ok(())
}

#[test]
fn hbse_google_provider_routes_generate_content_through_broker_socket() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let socket_path = tmp.path().join("hbse-google.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let server = thread::spawn(move || -> anyhow::Result<Value> {
        let (mut stream, _) = listener.accept()?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            if buffer[..n].contains(&b'\n') {
                break;
            }
        }
        let request: Value = serde_json::from_slice(
            bytes
                .split(|byte| *byte == b'\n')
                .next()
                .unwrap_or(bytes.as_slice()),
        )?;
        let body = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hbse \"}]}}]}\n\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"google\"}]}}]}\n\n"
        );
        let response = json!({
            "ok": true,
            "status_code": 200,
            "body": body
        });
        stream.write_all(serde_json::to_string(&response)?.as_bytes())?;
        stream.write_all(b"\n")?;
        Ok(request)
    });
    let adapter = HBSEGoogleProviderAdapter {
        config: ProviderConfig {
            name: "test-google-hbse".to_string(),
            display_name: Some("Test Google HBSE".to_string()),
            kind: "hbse_google".to_string(),
            api_key_env: None,
            base_url: Some("https://gemini.example/v1beta".to_string()),
            auth_type: "hbse".to_string(),
            enabled: true,
            metadata: BTreeMap::from([
                (
                    "hbse_socket".to_string(),
                    Value::String(socket_path.display().to_string()),
                ),
                (
                    "hbse_secret_ref".to_string(),
                    Value::String("secret://vegvisir/google/test".to_string()),
                ),
                (
                    "credential_header".to_string(),
                    Value::String("x-goog-api-key".to_string()),
                ),
                (
                    "credential_prefix".to_string(),
                    Value::String(String::new()),
                ),
            ]),
        },
    };
    let model = ModelInfo {
        name: "gemini-test".to_string(),
        provider: "google".to_string(),
        display_name: None,
        context_window: Some(1000000),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };

    let response = adapter.complete(
        &[ChatMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
            attachments: Vec::new(),
            created_at: Utc::now(),
        }],
        &model,
        "test-google-hbse",
    )?;

    assert_eq!(response, "hbse google");
    let request = server.join().expect("server thread completed")?;
    assert_eq!(request["command"], "provider_http");
    assert_eq!(request["secret_ref"], "secret://vegvisir/google/test");
    assert_eq!(request["credential_header"], "x-goog-api-key");
    assert_eq!(request["credential_prefix"], "");
    assert_eq!(
        request["url"],
        "https://gemini.example/v1beta/models/gemini-test:streamGenerateContent?alt=sse"
    );
    assert!(
        request["body"]
            .as_str()
            .unwrap()
            .contains("\"role\":\"user\"")
    );
    Ok(())
}

#[test]
fn hbse_azure_openai_provider_routes_deployment_chat_through_broker_socket() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let socket_path = tmp.path().join("hbse-azure.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let server = thread::spawn(move || -> anyhow::Result<Value> {
        let (mut stream, _) = listener.accept()?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            if buffer[..n].contains(&b'\n') {
                break;
            }
        }
        let request: Value = serde_json::from_slice(
            bytes
                .split(|byte| *byte == b'\n')
                .next()
                .unwrap_or(bytes.as_slice()),
        )?;
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"azure \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"hbse\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let response = json!({
            "ok": true,
            "status_code": 200,
            "body": body
        });
        stream.write_all(serde_json::to_string(&response)?.as_bytes())?;
        stream.write_all(b"\n")?;
        Ok(request)
    });
    let adapter = HBSEAzureOpenAIProviderAdapter {
        config: ProviderConfig {
            name: "test-azure-hbse".to_string(),
            display_name: Some("Test Azure HBSE".to_string()),
            kind: "hbse_azure_openai".to_string(),
            api_key_env: None,
            base_url: Some("https://example-resource.openai.azure.com".to_string()),
            auth_type: "hbse".to_string(),
            enabled: true,
            metadata: BTreeMap::from([
                (
                    "hbse_socket".to_string(),
                    Value::String(socket_path.display().to_string()),
                ),
                (
                    "hbse_secret_ref".to_string(),
                    Value::String("secret://vegvisir/azure/test".to_string()),
                ),
                (
                    "credential_header".to_string(),
                    Value::String("api-key".to_string()),
                ),
                (
                    "credential_prefix".to_string(),
                    Value::String(String::new()),
                ),
                (
                    "azure_deployment".to_string(),
                    Value::String("gpt-prod".to_string()),
                ),
                (
                    "api_version".to_string(),
                    Value::String("2024-10-21".to_string()),
                ),
            ]),
        },
    };
    let model = ModelInfo {
        name: "azure:gpt-5.4".to_string(),
        provider: "azure-openai".to_string(),
        display_name: None,
        context_window: Some(400000),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };

    let response = adapter.complete(
        &[ChatMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
            attachments: Vec::new(),
            created_at: Utc::now(),
        }],
        &model,
        "test-azure-hbse",
    )?;

    assert_eq!(response, "azure hbse");
    let request = server.join().expect("server thread completed")?;
    assert_eq!(request["command"], "provider_http");
    assert_eq!(request["secret_ref"], "secret://vegvisir/azure/test");
    assert_eq!(request["credential_header"], "api-key");
    assert_eq!(request["credential_prefix"], "");
    assert_eq!(
        request["url"],
        "https://example-resource.openai.azure.com/openai/deployments/gpt-prod/chat/completions?api-version=2024-10-21"
    );
    let body = request["body"].as_str().unwrap();
    assert!(body.contains("\"stream\":true"));
    assert!(!body.contains("\"model\""));
    Ok(())
}

#[test]
fn hbse_provider_routes_chat_completion_through_broker_socket() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let socket_path = tmp.path().join("hbse.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let server = thread::spawn(move || -> anyhow::Result<Value> {
        let (mut stream, _) = listener.accept()?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            if buffer[..n].contains(&b'\n') {
                break;
            }
        }
        let request: Value = serde_json::from_slice(
            bytes
                .split(|byte| *byte == b'\n')
                .next()
                .unwrap_or(bytes.as_slice()),
        )?;
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hbse \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"stream\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let response = json!({
            "ok": true,
            "status_code": 200,
            "body": body
        });
        stream.write_all(serde_json::to_string(&response)?.as_bytes())?;
        stream.write_all(b"\n")?;
        Ok(request)
    });
    let adapter = HBSEOpenAICompatibleProviderAdapter {
        config: ProviderConfig {
            name: "test-hbse".to_string(),
            display_name: Some("Test HBSE".to_string()),
            kind: "hbse_openai_compatible".to_string(),
            api_key_env: None,
            base_url: Some("https://api.example.test/v1".to_string()),
            auth_type: "hbse".to_string(),
            enabled: true,
            metadata: BTreeMap::from([
                (
                    "hbse_socket".to_string(),
                    Value::String(socket_path.display().to_string()),
                ),
                (
                    "hbse_secret_ref".to_string(),
                    Value::String("secret://vegvisir/test".to_string()),
                ),
                (
                    "hbse_consumer".to_string(),
                    Value::String("vegvisir.provider.test-hbse".to_string()),
                ),
            ]),
        },
    };
    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };

    let response = adapter.complete(
        &[ChatMessage {
            role: "user".to_string(),
            content: "hello hbse".to_string(),
            attachments: Vec::new(),
            created_at: Utc::now(),
        }],
        &model,
        "test-hbse",
    )?;

    assert_eq!(response, "hbse stream");
    let request = server.join().expect("server thread completed")?;
    assert_eq!(request["command"], "provider_http");
    assert_eq!(request["secret_ref"], "secret://vegvisir/test");
    assert_eq!(request["consumer"], "vegvisir.provider.test-hbse");
    assert_eq!(request["purpose"], "model.chat");
    assert_eq!(request["credential_header"], "Authorization");
    assert_eq!(request["credential_prefix"], "Bearer ");
    assert_eq!(
        request["url"],
        "https://api.example.test/v1/chat/completions"
    );
    assert_eq!(request["headers"]["Accept"], "text/event-stream");
    assert!(
        request["body"]
            .as_str()
            .unwrap()
            .contains("\"stream\":true")
    );
    Ok(())
}

#[test]
fn hbse_provider_surfaces_broker_denial() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let socket_path = tmp.path().join("hbse-deny.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let server = thread::spawn(move || -> anyhow::Result<()> {
        let (mut stream, _) = listener.accept()?;
        let mut buffer = [0_u8; 4096];
        let _ = stream.read(&mut buffer)?;
        let response = json!({
            "ok": false,
            "error": {"message": "secret access denied"}
        });
        stream.write_all(serde_json::to_string(&response)?.as_bytes())?;
        stream.write_all(b"\n")?;
        Ok(())
    });
    let adapter = HBSEOpenAICompatibleProviderAdapter {
        config: ProviderConfig {
            name: "test-hbse".to_string(),
            display_name: Some("Test HBSE".to_string()),
            kind: "hbse_openai_compatible".to_string(),
            api_key_env: None,
            base_url: Some("https://api.example.test/v1".to_string()),
            auth_type: "hbse".to_string(),
            enabled: true,
            metadata: BTreeMap::from([
                (
                    "hbse_socket".to_string(),
                    Value::String(socket_path.display().to_string()),
                ),
                (
                    "hbse_secret_ref".to_string(),
                    Value::String("secret://vegvisir/test".to_string()),
                ),
            ]),
        },
    };
    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };

    let error = adapter
        .complete(
            &[ChatMessage {
                role: "user".to_string(),
                content: "hello hbse".to_string(),
                attachments: Vec::new(),
                created_at: Utc::now(),
            }],
            &model,
            "test-hbse",
        )
        .unwrap_err()
        .to_string();

    server.join().expect("server thread completed")?;
    assert!(error.contains("HBSE broker denied provider request"));
    assert!(error.contains("secret access denied"));
    Ok(())
}

#[test]
fn openai_tool_schema_adds_array_items_for_run_command() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let run_command = build_builtin_registry(tmp.path())?
        .schemas()
        .into_iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some("run_command"))
        .expect("run_command schema");

    let schema = openai_tool_schema(&run_command);

    assert_eq!(
        schema.pointer("/function/parameters/properties/command"),
        Some(&json!({"type": "array", "items": {"type": "string"}}))
    );
    Ok(())
}

#[test]
fn openai_tool_loop_returns_last_observations_on_round_limit() -> anyhow::Result<()> {
    let mut calls = 0;
    let mut post = |_: Value| -> anyhow::Result<Value> {
        calls += 1;
        Ok(json!({
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "id": format!("call-{calls}"),
                        "type": "function",
                        "function": {
                            "name": "list_files",
                            "arguments": "{\"path\":\".\"}"
                        }
                    }]
                }
            }]
        }))
    };
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: "list files".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    }];
    let tools = vec![json!({
        "name": "list_files",
        "description": "List files.",
        "parameters": {"properties": {"path": "string"}}
    })];
    let mut execute_tool = |_: &str, _: Map<String, Value>| "alpha.txt".to_string();

    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let result = openai_tool_loop(&model, &messages, &tools, &mut execute_tool, &mut post, 2)?;

    assert!(result.contains("Tool-call round limit reached"));
    assert!(result.contains("alpha.txt"));
    Ok(())
}

#[test]
fn openai_tool_loop_executes_read_only_sibling_tool_calls_same_round() -> anyhow::Result<()> {
    let payloads = Arc::new(Mutex::new(Vec::<Value>::new()));
    let payloads_for_post = Arc::clone(&payloads);
    let mut posts = 0;
    let mut post = move |payload: Value| -> anyhow::Result<Value> {
        posts += 1;
        payloads_for_post.lock().unwrap().push(payload);
        if posts == 1 {
            return Ok(json!({
                "choices": [{
                    "message": {
                        "content": "",
                        "tool_calls": [
                            {
                                "id": "call-read",
                                "type": "function",
                                "function": {
                                    "name": "read_file",
                                    "arguments": "{\"path\":\"src/lib.rs\"}"
                                }
                            },
                            {
                                "id": "call-list",
                                "type": "function",
                                "function": {
                                    "name": "list_files",
                                    "arguments": "{\"path\":\"src\"}"
                                }
                            }
                        ]
                    }
                }]
            }));
        }
        Ok(json!({
            "choices": [{
                "message": {
                    "content": "done"
                }
            }]
        }))
    };
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: "inspect files".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    }];
    let tools = vec![
        json!({"name": "read_file", "description": "Read file.", "parameters": {}}),
        json!({"name": "list_files", "description": "List files.", "parameters": {}}),
    ];
    let mut executed = Vec::<String>::new();
    let mut execute_tool = |name: &str, _: Map<String, Value>| {
        executed.push(name.to_string());
        format!("{name} output")
    };

    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let result = openai_tool_loop(&model, &messages, &tools, &mut execute_tool, &mut post, 4)?;

    assert_eq!(result, "done");
    assert_eq!(
        executed,
        vec!["read_file".to_string(), "list_files".to_string()]
    );
    let payloads = payloads.lock().unwrap();
    assert_eq!(payloads[0]["parallel_tool_calls"], true);
    let followup_messages = payloads[1]
        .get("messages")
        .and_then(Value::as_array)
        .unwrap();
    let read_result = followup_messages
        .iter()
        .find(|message| message["tool_call_id"] == "call-read")
        .and_then(|message| message["content"].as_str())
        .unwrap_or("");
    let list_result = followup_messages
        .iter()
        .find(|message| message["tool_call_id"] == "call-list")
        .and_then(|message| message["content"].as_str())
        .unwrap_or("");
    assert!(read_result.contains("[Vegvisir tool completed]"));
    assert!(read_result.contains("read_file output"));
    assert!(list_result.contains("[Vegvisir tool completed]"));
    assert!(list_result.contains("list_files output"));
    Ok(())
}

#[test]
fn openai_tool_loop_defers_risky_sibling_tool_calls() -> anyhow::Result<()> {
    let payloads = Arc::new(Mutex::new(Vec::<Value>::new()));
    let payloads_for_post = Arc::clone(&payloads);
    let mut posts = 0;
    let mut post = move |payload: Value| -> anyhow::Result<Value> {
        posts += 1;
        payloads_for_post.lock().unwrap().push(payload);
        if posts == 1 {
            return Ok(json!({
                "choices": [{
                    "message": {
                        "content": "",
                        "tool_calls": [
                            {
                                "id": "call-read",
                                "type": "function",
                                "function": {
                                    "name": "read_file",
                                    "arguments": "{\"path\":\"src/lib.rs\"}"
                                }
                            },
                            {
                                "id": "call-run",
                                "type": "function",
                                "function": {
                                    "name": "run_command",
                                    "arguments": "{\"command\":[\"git\",\"status\"]}"
                                }
                            }
                        ]
                    }
                }]
            }));
        }
        Ok(json!({"choices": [{"message": {"content": "done"}}]}))
    };
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: "inspect and run".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    }];
    let tools = vec![
        json!({"name": "read_file", "description": "Read file.", "parameters": {}}),
        json!({"name": "run_command", "description": "Run command.", "parameters": {}, "risky": true}),
    ];
    let mut executed = Vec::<String>::new();
    let mut execute_tool = |name: &str, _: Map<String, Value>| {
        executed.push(name.to_string());
        format!("{name} output")
    };

    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let result = openai_tool_loop(&model, &messages, &tools, &mut execute_tool, &mut post, 4)?;

    assert_eq!(result, "done");
    assert_eq!(executed, vec!["read_file".to_string()]);
    let payloads = payloads.lock().unwrap();
    let followup_messages = payloads[1]
        .get("messages")
        .and_then(Value::as_array)
        .unwrap();
    let deferred_result = followup_messages
        .iter()
        .find(|message| message["tool_call_id"] == "call-run")
        .and_then(|message| message["content"].as_str())
        .unwrap_or("");
    assert!(deferred_result.contains("[Vegvisir tool deferred]"));
    assert!(deferred_result.contains("read-only"));
    Ok(())
}

#[test]
fn builtin_file_tools_bound_list_files_and_read_full_file_outputs() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    for index in 0..550 {
        std::fs::write(tmp.path().join(format!("file-{index:03}.txt")), "x")?;
    }
    std::fs::write(tmp.path().join("large.txt"), "a".repeat(80 * 1024))?;
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry: build_builtin_registry(tmp.path())?,
        guardrails: GuardrailEngine::default(),
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let listed = executor.execute(vegvisir_rust::types::ToolCall {
        name: "list_files".to_string(),
        args: Map::new(),
    });
    assert!(listed.ok);
    assert!(listed.content.contains("list_files truncated"));
    assert!(
        listed.content.lines().count() <= 505,
        "list_files returned too many lines"
    );

    let read = executor.execute(vegvisir_rust::types::ToolCall {
        name: "read_file".to_string(),
        args: json!({"path": "large.txt"}).as_object().unwrap().clone(),
    });
    assert!(read.ok);
    assert_eq!(read.content.len(), 80 * 1024);
    assert_eq!(
        read.data.get("output_truncated").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        read.data.get("bytes").and_then(Value::as_u64),
        Some(80 * 1024)
    );
    Ok(())
}

#[test]
fn openai_tool_loop_truncates_tool_observation_before_resend() -> anyhow::Result<()> {
    let payloads = Arc::new(Mutex::new(Vec::<Value>::new()));
    let payloads_for_post = Arc::clone(&payloads);
    let mut calls = 0;
    let mut post = move |payload: Value| -> anyhow::Result<Value> {
        calls += 1;
        payloads_for_post.lock().unwrap().push(payload);
        if calls == 1 {
            Ok(json!({
                "choices": [{
                    "message": {
                        "content": "",
                        "tool_calls": [{
                            "id": "call-1",
                            "type": "function",
                            "function": {
                                "name": "read_file",
                                "arguments": "{\"path\":\"large.txt\"}"
                            }
                        }]
                    }
                }]
            }))
        } else {
            Ok(json!({"choices": [{"message": {"content": "done"}}]}))
        }
    };
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: "read".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    }];
    let tools = vec![json!({
        "name": "read_file",
        "description": "Read file.",
        "parameters": {"properties": {"path": "string"}}
    })];
    let mut execute_tool = |_: &str, _: Map<String, Value>| "z".repeat(200 * 1024);

    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let result = openai_tool_loop(&model, &messages, &tools, &mut execute_tool, &mut post, 2)?;

    assert_eq!(result, "done");
    let captured = payloads.lock().unwrap();
    let tool_content = captured[1]
        .pointer("/messages/2/content")
        .and_then(Value::as_str)
        .expect("tool observation in second request");
    assert!(tool_content.contains("tool observation compacted"));
    assert!(tool_content.len() < 70 * 1024);
    Ok(())
}

#[test]
fn tool_executor_normalizes_safe_argument_shapes_and_classifies_errors() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    std::fs::write(tmp.path().join("alpha.txt"), "alpha")?;
    let mut executor = vegvisir_rust::tools::ToolExecutor {
        registry: build_builtin_registry(tmp.path())?,
        guardrails: GuardrailEngine::default(),
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };

    let read = executor.execute(vegvisir_rust::types::ToolCall {
        name: "read_file".to_string(),
        args: json!({"path": 123}).as_object().unwrap().clone(),
    });
    assert!(!read.ok);
    assert_eq!(read.error.as_deref(), Some("ReadError"));

    let listed = executor.execute(vegvisir_rust::types::ToolCall {
        name: "list_files".to_string(),
        args: json!({"limit": "1"}).as_object().unwrap().clone(),
    });
    assert!(listed.ok, "{}", listed.content);
    assert_eq!(listed.data.get("output_truncated"), Some(&json!(false)));

    let unknown = executor.execute(vegvisir_rust::types::ToolCall {
        name: "not_a_tool".to_string(),
        args: Map::new(),
    });
    assert!(!unknown.ok);
    assert_eq!(unknown.error.as_deref(), Some("UnknownTool"));
    Ok(())
}

#[test]
fn openai_tool_loop_repairs_markdown_wrapped_tool_arguments_and_compacts_observation()
-> anyhow::Result<()> {
    let payloads = Arc::new(Mutex::new(Vec::<Value>::new()));
    let payloads_for_post = Arc::clone(&payloads);
    let mut calls = 0;
    let mut post = move |payload: Value| -> anyhow::Result<Value> {
        calls += 1;
        payloads_for_post.lock().unwrap().push(payload);
        if calls == 1 {
            Ok(json!({
                "choices": [{
                    "message": {
                        "content": "",
                        "tool_calls": [{
                            "id": "call-1",
                            "type": "function",
                            "function": {
                                "name": "read_file",
                                "arguments": "```json\n{\"path\":\"alpha.txt\"}\n```"
                            }
                        }]
                    }
                }]
            }))
        } else {
            Ok(json!({"choices": [{"message": {"content": "done"}}]}))
        }
    };
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: "read".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    }];
    let tools = vec![json!({
        "name": "read_file",
        "description": "Read file.",
        "parameters": {"properties": {"path": "string"}}
    })];
    let mut seen_args = Map::new();
    let mut execute_tool = |_: &str, args: Map<String, Value>| {
        seen_args = args;
        format!("{}ERROR: important tail", "z".repeat(80 * 1024))
    };

    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let result = openai_tool_loop(&model, &messages, &tools, &mut execute_tool, &mut post, 2)?;

    assert_eq!(result, "done");
    assert_eq!(seen_args.get("path"), Some(&json!("alpha.txt")));
    let captured = payloads.lock().unwrap();
    let tool_content = captured[1]
        .pointer("/messages/2/content")
        .and_then(Value::as_str)
        .expect("tool observation in second request");
    assert!(tool_content.contains("tool observation compacted"));
    assert!(tool_content.contains("ERROR: important tail"));
    assert!(tool_content.len() < 30 * 1024);
    Ok(())
}

#[test]
fn conversation_runner_injects_mid_run_steering_after_tool_call() -> anyhow::Result<()> {
    let workspace = tempdir()?;
    let observation = Arc::new(Mutex::new(None));
    let (steering_tx, steering_rx) = std::sync::mpsc::channel();
    steering_tx.send("stop exploring and summarize now".to_string())?;
    drop(steering_tx);

    let mut runner = ConversationRunner {
        provider: SteeringToolProvider {
            config: ProviderConfig {
                name: "demo".to_string(),
                display_name: None,
                kind: "test".to_string(),
                api_key_env: None,
                base_url: None,
                auth_type: "none".to_string(),
                enabled: true,
                metadata: Default::default(),
            },
            observation: observation.clone(),
        },
        models: ModelRegistry::default_catalog()?,
        tools: Some(build_builtin_registry(workspace.path())?),
        tool_executor: Some(vegvisir_rust::tools::ToolExecutor {
            registry: build_builtin_registry(workspace.path())?,
            guardrails: GuardrailEngine::default(),
            runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
            logger: vegvisir_rust::observability::EventLogger::default(),
        }),
        event_sink: None,
        cancel_token: None,
        steering_rx: Some(steering_rx),
    };
    let mut session =
        vegvisir_rust::core::SessionState::new(workspace.path(), Vec::new(), Vec::new());
    session.current_provider = "demo".to_string();
    session.current_model = "demo-local".to_string();

    runner.send(&mut session, "inspect")?;

    let observed = observation.lock().unwrap().clone().unwrap_or_default();
    assert!(observed.contains("User steering received"), "{observed}");
    assert!(
        observed.contains("stop exploring and summarize now"),
        "{observed}"
    );
    assert!(session.messages.iter().any(|message| {
        message.role == "user" && message.content.contains("[mid-run steering]")
    }));
    Ok(())
}

#[test]
fn native_tool_call_executes_through_conversation_runner_executor() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    std::fs::write(tmp.path().join("alpha.txt"), "alpha")?;
    let tool_registry = build_builtin_registry(tmp.path())?;
    let tool_executor = vegvisir_rust::tools::ToolExecutor {
        registry: tool_registry.clone(),
        guardrails: GuardrailEngine::default(),
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };
    let model = ModelInfo {
        name: "tool-model".to_string(),
        provider: "test-tools".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let mut models = ModelRegistry::default();
    models.register(model);
    let mut session = vegvisir_rust::core::SessionState::new(tmp.path(), Vec::new(), Vec::new());
    session.current_provider = "test-tools".to_string();
    session.current_model = "tool-model".to_string();
    let mut runner = ConversationRunner {
        provider: TestToolCallingProvider {
            config: ProviderConfig {
                name: "test-tools".to_string(),
                display_name: None,
                kind: "test".to_string(),
                api_key_env: None,
                base_url: None,
                auth_type: "none".to_string(),
                enabled: true,
                metadata: Default::default(),
            },
        },
        models,
        tools: Some(tool_registry),
        tool_executor: Some(tool_executor),
        event_sink: None,
        cancel_token: None,
        steering_rx: None,
    };

    let response = runner.send(&mut session, "what files are here?")?;

    assert!(response.contains("alpha.txt"));
    assert_eq!(session.messages.last().unwrap().role, "assistant");
    Ok(())
}

#[test]
fn cms_envelope_path_exposes_tool_schemas_to_tool_capable_models() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    std::fs::write(tmp.path().join("alpha.txt"), "alpha")?;
    let mut cms = VegvisirCms::open(VegvisirCmsConfig {
        db_path: tmp.path().join("cms.sqlite3"),
        user_id: "tester".to_string(),
        project_id: Some("Vegvisir".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })?;
    let envelope = cms.prepare_cached_prompt("what files are here?", "test-tools", "tool-model")?;
    let tool_registry = build_builtin_registry(tmp.path())?;
    let tool_executor = vegvisir_rust::tools::ToolExecutor {
        registry: tool_registry.clone(),
        guardrails: GuardrailEngine::default(),
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };
    let model = ModelInfo {
        name: "tool-model".to_string(),
        provider: "test-tools".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    };
    let mut models = ModelRegistry::default();
    models.register(model);
    let mut session = vegvisir_rust::core::SessionState::new(tmp.path(), Vec::new(), Vec::new());
    session.current_provider = "test-tools".to_string();
    session.current_model = "tool-model".to_string();
    let mut runner = ConversationRunner {
        provider: TestToolCallingProvider {
            config: ProviderConfig {
                name: "test-tools".to_string(),
                display_name: None,
                kind: "test".to_string(),
                api_key_env: None,
                base_url: None,
                auth_type: "none".to_string(),
                enabled: true,
                metadata: Default::default(),
            },
        },
        models,
        tools: Some(tool_registry),
        tool_executor: Some(tool_executor),
        event_sink: None,
        cancel_token: None,
        steering_rx: None,
    };

    let response = runner.send_with_envelope(&mut session, "what files are here?", envelope)?;

    assert!(response.contains("alpha.txt"));
    assert_eq!(session.messages.last().unwrap().role, "assistant");
    Ok(())
}

#[test]
fn cms_envelope_path_preserves_recent_chat_history() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut cms = VegvisirCms::open(VegvisirCmsConfig {
        db_path: tmp.path().join("cms.sqlite3"),
        user_id: "tester".to_string(),
        project_id: Some("Vegvisir".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })?;
    let envelope =
        cms.prepare_cached_prompt("what did I ask you to remember?", "demo", "demo-local")?;
    let recorded_messages = Arc::new(Mutex::new(Vec::new()));
    let mut runner = ConversationRunner {
        provider: MessageRecordingProvider {
            config: ProviderConfig {
                name: "demo".to_string(),
                display_name: None,
                kind: "test".to_string(),
                api_key_env: None,
                base_url: None,
                auth_type: "none".to_string(),
                enabled: true,
                metadata: Default::default(),
            },
            messages: recorded_messages.clone(),
        },
        models: ModelRegistry::default_catalog()?,
        tools: None,
        tool_executor: None,
        event_sink: None,
        cancel_token: None,
        steering_rx: None,
    };
    let mut session = vegvisir_rust::core::SessionState::new(tmp.path(), Vec::new(), Vec::new());
    session.messages.push(ChatMessage {
        role: "user".to_string(),
        content: "remember the codename is blue lantern".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    });
    session.messages.push(ChatMessage {
        role: "assistant".to_string(),
        content: "I will remember that for this chat.".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    });

    runner.send_with_envelope(&mut session, "what did I ask you to remember?", envelope)?;

    let messages = recorded_messages.lock().unwrap();
    assert_eq!(messages[0].role, "system");
    assert!(
        messages
            .iter()
            .any(|message| message.content.contains("blue lantern"))
    );
    assert!(
        messages
            .iter()
            .any(|message| message.content == "what did I ask you to remember?")
    );
    Ok(())
}

#[test]
fn approval_required_tool_call_stops_runner_and_leaves_pending_request() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let tool_registry = build_builtin_registry(tmp.path())?;
    let tool_executor = vegvisir_rust::tools::ToolExecutor {
        registry: tool_registry.clone(),
        guardrails: GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools: false,
                require_human_approval: true,
                bypass_approvals_and_sandbox: false,
                allowed_commands: Default::default(),
                denied_tools: Default::default(),
            },
            approvals: Default::default(),
        },
        runtime_policy: vegvisir_rust::policy::RuntimePolicy::default(),
        logger: vegvisir_rust::observability::EventLogger::default(),
    };
    let mut models = ModelRegistry::default();
    models.register(ModelInfo {
        name: "tool-model".to_string(),
        provider: "test-tools".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: true,
        enabled: true,
        metadata: Default::default(),
    });
    let mut session = vegvisir_rust::core::SessionState::new(tmp.path(), Vec::new(), Vec::new());
    session.current_provider = "test-tools".to_string();
    session.current_model = "tool-model".to_string();
    let mut runner = ConversationRunner {
        provider: RiskyToolCallingProvider {
            config: ProviderConfig {
                name: "test-tools".to_string(),
                display_name: None,
                kind: "test".to_string(),
                api_key_env: None,
                base_url: None,
                auth_type: "none".to_string(),
                enabled: true,
                metadata: Default::default(),
            },
        },
        models,
        tools: Some(tool_registry),
        tool_executor: Some(tool_executor),
        event_sink: None,
        cancel_token: None,
        steering_rx: None,
    };

    let error = runner
        .send(&mut session, "write the file")
        .expect_err("approval should stop the model turn");

    assert!(error.to_string().contains("approval_id="));
    let pending = runner
        .tool_executor
        .as_ref()
        .unwrap()
        .guardrails
        .approvals
        .pending();
    assert_eq!(pending.len(), 1);
    assert_eq!(session.messages.last().unwrap().role, "user");
    Ok(())
}

#[test]
fn openai_compatible_provider_can_opt_out_of_streaming() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut cms = VegvisirCms::open(VegvisirCmsConfig {
        db_path: tmp.path().join("cms.sqlite3"),
        user_id: "tester".to_string(),
        project_id: Some("Vegvisir".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    })?;
    cms.remember(
        "decision",
        "Provider nonstream memory",
        "Provider adapters can opt out of streaming.",
    )?;
    let envelope =
        cms.prepare_cached_prompt("Continue provider nonstream work", "openai", "gpt-test")?;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let server = thread::spawn(move || -> anyhow::Result<String> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let n = stream.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..n]);
            let request = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = request.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if body.len() >= content_length {
                break;
            }
        }
        let request = String::from_utf8_lossy(&bytes).to_string();
        let body = r#"{"choices":[{"message":{"content":"provider nonstream response"}}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )?;
        Ok(request)
    });

    let adapter = OpenAICompatibleProviderAdapter {
        config: ProviderConfig {
            name: "test-openai".to_string(),
            display_name: Some("Test OpenAI".to_string()),
            kind: "openai_compatible".to_string(),
            api_key_env: None,
            base_url: Some(format!("http://{}", addr)),
            auth_type: "none".to_string(),
            enabled: true,
            metadata: [("stream".to_string(), json!(false))].into_iter().collect(),
        },
    };
    let model = ModelInfo {
        name: "gpt-test".to_string(),
        provider: "test-openai".to_string(),
        display_name: None,
        context_window: Some(8192),
        supports_streaming: false,
        enabled: true,
        metadata: Default::default(),
    };

    let response = adapter.complete_envelope(&envelope, &model, "test-openai")?;
    assert_eq!(response, "provider nonstream response");
    let request = server.join().expect("server thread completed")?;
    assert!(request.contains("\"stream\":false"));
    Ok(())
}

#[test]
fn config_audit_and_environment_helpers_work() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let config = ConfigStore::new(tmp.path().join("config.json"));
    let data = [("current_model".to_string(), json!("demo-local"))]
        .into_iter()
        .collect();
    config.save(&data)?;
    assert_eq!(config.load()?.get("current_model").unwrap(), "demo-local");

    let audit = AuditLog::new(tmp.path().join("audit.jsonl"))?;
    audit.append(&AuditEvent::new(
        "test.event",
        "recorded",
        Some("session".to_string()),
        Default::default(),
    ))?;
    assert!(std::fs::read_to_string(tmp.path().join("audit.jsonl"))?.contains("test.event"));

    assert_eq!(
        parse_environment_line("export OPENAI_API_KEY='abc123'"),
        Some(("OPENAI_API_KEY".to_string(), "abc123".to_string()))
    );
    let env_root = tmp.path().join("environment.d");
    std::fs::create_dir_all(&env_root)?;
    std::fs::write(env_root.join("vegvisir.conf"), "VEGVISIR_TEST=value\n")?;
    assert_eq!(
        load_environment_d(Some(&env_root))?
            .get("VEGVISIR_TEST")
            .unwrap(),
        "value"
    );
    Ok(())
}

#[test]
fn checkpoint_store_round_trips_snapshot() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut context = ContextManager::default();
    context.add(Message::new(Role::User, "hello"));
    let state = vegvisir_rust::state::RunState::new("inspect");
    let store = CheckpointStore::new(tmp.path());
    let path = store.save(&vegvisir_rust::checkpoints::RunSnapshot {
        state: state.clone(),
        context,
        cms_root: None,
    })?;

    assert!(path.exists());
    let loaded = store.load(&state.run_id)?;
    assert_eq!(loaded.state.run_id, state.run_id);
    assert!(!std::fs::read_to_string(path)?.contains("memory_root"));
    Ok(())
}

#[test]
fn checkpoint_store_loads_legacy_memory_root_snapshot() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let store = CheckpointStore::new(tmp.path());
    let state = vegvisir_rust::state::RunState::new("legacy");
    let run_id = state.run_id.clone();
    let context = ContextManager::default();
    let path = store.path_for(&run_id);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "state": state,
            "context": context,
            "memory_root": "/tmp/old-cms-root"
        }))?,
    )?;

    let loaded = store.load(&run_id)?;

    assert_eq!(loaded.cms_root.as_deref(), Some("/tmp/old-cms-root"));
    Ok(())
}

#[test]
fn simple_runtime_plugin_installs_tool() -> anyhow::Result<()> {
    struct ExamplePlugin;
    impl vegvisir_rust::runtime::RuntimePlugin for ExamplePlugin {
        fn name(&self) -> &str {
            "example"
        }

        fn register(&self, registry: &mut ToolRegistry) -> anyhow::Result<()> {
            registry.register(Tool::new(
                "ping",
                "ping",
                Arc::new(|_| Observation::ok("pong")),
                json!({}),
                false,
            ))
        }
    }

    let mut runtime = vegvisir_rust::runtime::Runtime::default();
    runtime.install(ExamplePlugin)?;

    assert_eq!(runtime.registry.get("ping")?.name, "ping");
    runtime.require("tools")?;
    Ok(())
}

#[test]
fn parallel_subagents_return_results() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let model = ScriptedModel::new(vec![
        AgentDecision::final_decision("child one", "one"),
        AgentDecision::final_decision("child two", "two"),
    ]);
    let mut supervisor =
        vegvisir_rust::subagents::SubAgentSupervisor::new(model, ToolRegistry::default());
    supervisor.max_children = 2;

    let results = supervisor.run_parallel(
        [
            ("one".to_string(), AgentTask::new("one", tmp.path())),
            ("two".to_string(), AgentTask::new("two", tmp.path())),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>(),
    )?;

    assert_eq!(
        results.keys().cloned().collect::<Vec<_>>(),
        vec!["one".to_string(), "two".to_string()]
    );
    assert_eq!(results["one"].final_answer.as_deref(), Some("one"));
    assert_eq!(results["two"].final_answer.as_deref(), Some("two"));
    Ok(())
}

#[test]
fn parallel_subagents_can_run_concurrently_with_durable_board() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let board_path = tmp.path().join("parallel-subagents.json");
    let mut supervisor = vegvisir_rust::subagents::SubAgentSupervisor::with_board_path(
        ScriptedModel::default(),
        ToolRegistry::default(),
        &board_path,
    )?;
    supervisor.max_children = 2;

    let results = supervisor.run_parallel_with_models(
        [
            (
                "one".to_string(),
                (
                    ScriptedModel::new(vec![AgentDecision::final_decision("one done", "one")]),
                    AgentTask::new("one", tmp.path()),
                ),
            ),
            (
                "two".to_string(),
                (
                    ScriptedModel::new(vec![AgentDecision::final_decision("two done", "two")]),
                    AgentTask::new("two", tmp.path()),
                ),
            ),
        ]
        .into_iter()
        .collect::<BTreeMap<_, _>>(),
    )?;

    assert_eq!(results["one"].final_answer.as_deref(), Some("one"));
    assert_eq!(results["two"].final_answer.as_deref(), Some("two"));
    assert_eq!(supervisor.task_records().len(), 2);
    assert!(
        supervisor
            .task_records()
            .iter()
            .all(|record| record.status == vegvisir_rust::subagents::SubAgentStatus::Completed)
    );
    let persisted = fs::read_to_string(board_path)?;
    assert!(persisted.contains("\"name\": \"one\""));
    assert!(persisted.contains("\"name\": \"two\""));
    let started_events = supervisor
        .logger
        .events()
        .into_iter()
        .filter(|event| event.name == "subagent.started")
        .count();
    assert_eq!(started_events, 2);
    Ok(())
}

#[test]
fn application_exposes_subagent_task_board_commands() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    let mut app = TuiApplication::with_data_root(tmp.path(), &home)?;
    assert_eq!(
        app.execute_command("/subagents")?.unwrap(),
        "No subagent task records."
    );

    let records = vec![vegvisir_rust::subagents::SubAgentTaskRecord {
        id: "task-1".to_string(),
        name: "planner".to_string(),
        workspace: tmp.path().to_path_buf(),
        goal: "Plan the release".to_string(),
        parent_run_id: None,
        child_run_id: None,
        artifact_dir: None,
        ownership: None,
        provider: None,
        model: None,
        file_scope: Vec::new(),
        work_budget: vegvisir_rust::subagents::SubAgentWorkBudget::default(),
        status: vegvisir_rust::subagents::SubAgentStatus::Running,
        created_at: Utc::now(),
        started_at: Some(Utc::now()),
        finished_at: None,
        checkpoint: None,
        final_answer: None,
        error: None,
        observability: vegvisir_rust::subagents::SubAgentObservability::default(),
    }];
    fs::create_dir_all(&home)?;
    fs::write(
        home.join("subagents.json"),
        serde_json::to_string_pretty(&records)?,
    )?;

    let listed = app.execute_command("/subagents list")?.unwrap();
    assert!(listed.contains("task-1"));
    assert!(listed.contains("name=planner"));
    let shown = app
        .execute_command("/subagents show planner --json")?
        .unwrap();
    assert!(shown.contains("\"goal\": \"Plan the release\""));
    let cancelled = app.execute_command("/subagents cancel planner")?.unwrap();
    assert!(cancelled.contains("Cancelled subagent task task-1"));
    let shown = app
        .execute_command("/subagents show task-1 --json")?
        .unwrap();
    assert!(shown.contains("\"status\": \"cancelled\""));
    let second_cancel = app.execute_command("/subagents cancel task-1")?.unwrap();
    assert!(second_cancel.contains("already Cancelled"));
    Ok(())
}

#[test]
fn builtin_registry_exposes_spawn_subagent_tool() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let registry = build_builtin_registry(tmp.path())?;
    let tool = registry.get("spawn_subagent")?;

    assert!(!tool.risky);
    assert!(tool.description.contains("background Vegvisir child agent"));
    assert!(
        tool.schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "goal")
    );
    Ok(())
}

#[test]
fn spawn_subagent_enforces_three_active_task_limit() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    let active_records = (0..3)
        .map(|index| vegvisir_rust::subagents::SubAgentTaskRecord {
            id: format!("task-{index}"),
            name: format!("worker-{index}"),
            workspace: tmp.path().to_path_buf(),
            goal: "bounded review".to_string(),
            parent_run_id: None,
            child_run_id: None,
            artifact_dir: None,
            ownership: None,
            provider: None,
            model: None,
            file_scope: Vec::new(),
            work_budget: vegvisir_rust::subagents::SubAgentWorkBudget::default(),
            status: vegvisir_rust::subagents::SubAgentStatus::Running,
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            finished_at: None,
            checkpoint: None,
            final_answer: None,
            error: None,
            observability: vegvisir_rust::subagents::SubAgentObservability::default(),
        })
        .collect::<Vec<_>>();
    fs::write(
        home.join("subagents.json"),
        serde_json::to_string_pretty(&active_records)?,
    )?;

    let cms_config = VegvisirCmsConfig {
        db_path: home.join("cms-v2.sqlite3"),
        user_id: "test-user".to_string(),
        project_id: Some("test-project".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    };
    let mut executor = ToolExecutor {
        registry: vegvisir_rust::tools::build_builtin_registry_with_cms_and_mode(
            tmp.path(),
            cms_config,
            true,
        )?,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy::default(),
            approvals: Default::default(),
        },
        runtime_policy: Default::default(),
        logger: Default::default(),
    };

    let observation = executor.execute(ToolCall {
        name: "spawn_subagent".to_string(),
        args: serde_json::from_value(json!({
            "goal": "another bounded review",
            "name": "worker-overflow"
        }))?,
    });

    assert!(!observation.ok);
    assert_eq!(observation.error.as_deref(), Some("SubagentLimit"));
    assert!(
        observation
            .content
            .contains("Maximum active subagents reached (3)")
    );
    Ok(())
}

#[test]
fn spawn_subagent_rejects_overlapping_active_file_scope() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let home = tmp.path().join("home");
    fs::create_dir_all(&home)?;
    let active_records = vec![vegvisir_rust::subagents::SubAgentTaskRecord {
        id: "task-1".to_string(),
        name: "app-owner".to_string(),
        workspace: tmp.path().to_path_buf(),
        goal: "Work on app runtime".to_string(),
        parent_run_id: None,
        child_run_id: None,
        artifact_dir: None,
        ownership: None,
        provider: None,
        model: None,
        file_scope: vec![tmp.path().join("vegvisir/src/app")],
        work_budget: vegvisir_rust::subagents::SubAgentWorkBudget::default(),
        status: vegvisir_rust::subagents::SubAgentStatus::Running,
        created_at: Utc::now(),
        started_at: Some(Utc::now()),
        finished_at: None,
        checkpoint: None,
        final_answer: None,
        error: None,
        observability: vegvisir_rust::subagents::SubAgentObservability::default(),
    }];
    fs::write(
        home.join("subagents.json"),
        serde_json::to_string_pretty(&active_records)?,
    )?;

    let cms_config = VegvisirCmsConfig {
        db_path: home.join("cms-v2.sqlite3"),
        user_id: "test-user".to_string(),
        project_id: Some("test-project".to_string()),
        context_mode: cms_v2::ecm::ContextMode::Project,
        commit_writebacks: true,
    };
    let mut executor = ToolExecutor {
        registry: vegvisir_rust::tools::build_builtin_registry_with_cms_and_mode(
            tmp.path(),
            cms_config,
            true,
        )?,
        guardrails: GuardrailEngine {
            policy: PermissionPolicy::default(),
            approvals: Default::default(),
        },
        runtime_policy: Default::default(),
        logger: Default::default(),
    };

    let observation = executor.execute(ToolCall {
        name: "spawn_subagent".to_string(),
        args: serde_json::from_value(json!({
            "goal": "work on app runtime child",
            "name": "app-conflict",
            "file_scope": ["vegvisir/src/app/runtime.rs"]
        }))?,
    });

    assert!(!observation.ok);
    assert_eq!(observation.error.as_deref(), Some("SubagentScopeConflict"));
    assert!(
        observation
            .content
            .contains("file scope overlaps active task")
    );
    Ok(())
}

#[test]
fn subagent_supervisor_persists_task_board_and_events() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let board_path = tmp.path().join("subagents.json");
    let model = ScriptedModel::new(vec![AgentDecision::final_decision(
        "child done",
        "durable child result",
    )]);
    let mut supervisor = vegvisir_rust::subagents::SubAgentSupervisor::with_board_path(
        model,
        ToolRegistry::default(),
        &board_path,
    )?;

    let result = supervisor.run_child("durable", AgentTask::new("persist me", tmp.path()))?;
    assert_eq!(result.final_answer.as_deref(), Some("durable child result"));

    let record = supervisor.task_record("durable").expect("subagent record");
    assert_eq!(
        record.status,
        vegvisir_rust::subagents::SubAgentStatus::Completed
    );
    assert_eq!(record.final_answer.as_deref(), Some("durable child result"));
    assert!(record.started_at.is_some());
    assert!(record.finished_at.is_some());

    let events = supervisor.logger.events();
    assert!(events.iter().any(|event| event.name == "subagent.queued"));
    assert!(events.iter().any(|event| event.name == "subagent.started"));
    assert!(
        events
            .iter()
            .any(|event| event.name == "subagent.completed")
    );

    let reopened = vegvisir_rust::subagents::SubAgentSupervisor::with_board_path(
        ScriptedModel::default(),
        ToolRegistry::default(),
        &board_path,
    )?;
    let persisted = reopened.task_record("durable").expect("persisted record");
    assert_eq!(
        persisted.status,
        vegvisir_rust::subagents::SubAgentStatus::Completed
    );
    assert_eq!(
        persisted.final_answer.as_deref(),
        Some("durable child result")
    );
    Ok(())
}

#[test]
fn app_server_bridge_accepts_codex_thread_turn_flow() -> anyhow::Result<()> {
    let workspace = tempdir()?;
    let data_root = tempdir()?;
    let input = [
        json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": { "name": "vegvisir-test" },
                "capabilities": {}
            }
        })
        .to_string(),
        json!({
            "method": "initialized",
            "params": {}
        })
        .to_string(),
        json!({
            "id": 2,
            "method": "thread/start",
            "params": {
                "cwd": workspace.path(),
                "modelProvider": "demo",
                "model": "demo-local",
                "ephemeral": true
            }
        })
        .to_string(),
        json!({
            "id": 3,
            "method": "turn/start",
            "params": {
                "threadId": "current",
                "input": [{ "type": "text", "text": "hello codex bridge" }]
            }
        })
        .to_string(),
    ];

    let full_input = input.join("\n");
    let mut output = Vec::new();
    run_app_server_with_io(
        std::io::Cursor::new(full_input),
        &mut output,
        BridgeOptions {
            workspace: workspace.path().to_path_buf(),
            data_root: Some(data_root.path().to_path_buf()),
            provider: Some("demo".to_string()),
            model: Some("demo-local".to_string()),
            agent: None,
            dangerously_bypass_approvals_and_sandbox: false,
        },
    )?;

    let output = String::from_utf8(output)?;
    let events = output
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(events[1]["id"], 1);
    assert!(
        events[1]["result"]["userAgent"]
            .as_str()
            .unwrap()
            .starts_with("vegvisir/")
    );
    assert!(
        events
            .iter()
            .any(|event| event["method"] == "thread/started")
    );
    assert!(events.iter().any(|event| event["method"] == "turn/started"));
    assert!(events.iter().any(|event| {
        event["method"] == "item/agentMessage/delta"
            && event["params"]["delta"]
                .as_str()
                .unwrap_or_default()
                .contains("Demo response from demo-local")
    }));
    assert!(events.iter().any(|event| {
        event["method"] == "turn/completed"
            && event["params"]["turn"]["status"] == "completed"
            && event["params"]["turn"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["type"] == "agentMessage")
    }));
    Ok(())
}

#[test]
fn app_server_bridge_streams_demo_turn_and_reports_status() -> anyhow::Result<()> {
    let workspace = tempdir()?;
    let data_root = tempdir()?;
    let input = [
        json!({
            "id": "start",
            "method": "session.start",
            "params": {
                "workspace": workspace.path(),
                "provider": "demo",
                "model": "demo-local"
            }
        })
        .to_string(),
        json!({
            "id": "turn",
            "method": "turn.send",
            "params": {
                "content": "hello bridge"
            }
        })
        .to_string(),
        json!({
            "id": "tools",
            "method": "tools.list",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "providers",
            "method": "providers.list",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "models",
            "method": "models.list",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "messages",
            "method": "session.messages",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "export",
            "method": "session.exportMarkdown",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "memory",
            "method": "memory.status",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "prompt",
            "method": "system.prompt",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "status",
            "method": "session.status",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "bye",
            "method": "shutdown",
            "params": {}
        })
        .to_string(),
    ]
    .join("\n");
    let mut output = Vec::new();
    run_app_server_with_io(
        std::io::Cursor::new(input),
        &mut output,
        BridgeOptions {
            workspace: workspace.path().to_path_buf(),
            data_root: Some(data_root.path().to_path_buf()),
            provider: Some("demo".to_string()),
            model: Some("demo-local".to_string()),
            agent: None,
            dangerously_bypass_approvals_and_sandbox: false,
        },
    )?;

    let output = String::from_utf8(output)?;
    let events = output
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert!(events.iter().any(|event| event["type"] == "server.ready"));
    assert!(
        events
            .iter()
            .any(|event| event["type"] == "session.started")
    );
    assert!(events.iter().any(|event| {
        event["type"] == "content.delta"
            && event["payload"]["text"]
                .as_str()
                .unwrap_or_default()
                .contains("Demo response from demo-local")
    }));
    let completed = events
        .iter()
        .find(|event| event["type"] == "turn.completed")
        .expect("turn completed event");
    assert_eq!(completed["payload"]["session"]["provider"], "demo");
    assert_eq!(completed["payload"]["session"]["model"], "demo-local");
    let status = events
        .iter()
        .find(|event| event["type"] == "session.status" && event["id"] == "status")
        .expect("status event");
    assert_eq!(status["payload"]["messages"], 2);
    let tools = events
        .iter()
        .find(|event| event["type"] == "tools.list")
        .expect("tools list event");
    assert!(
        tools["payload"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "read_file")
    );
    let providers = events
        .iter()
        .find(|event| event["type"] == "providers.list")
        .expect("providers list event");
    assert!(
        providers["payload"]["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|provider| provider["name"] == "demo")
    );
    let models = events
        .iter()
        .find(|event| event["type"] == "models.list")
        .expect("models list event");
    assert!(
        models["payload"]["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["name"] == "demo-local")
    );
    let messages = events
        .iter()
        .find(|event| event["type"] == "session.messages")
        .expect("session messages event");
    assert_eq!(messages["payload"]["messages"].as_array().unwrap().len(), 2);
    let exported = events
        .iter()
        .find(|event| event["type"] == "session.exportMarkdown")
        .expect("session export event");
    assert!(
        exported["payload"]["markdown"]
            .as_str()
            .unwrap()
            .contains("hello bridge")
    );
    let memory = events
        .iter()
        .find(|event| event["type"] == "memory.status")
        .expect("memory status event");
    assert_eq!(memory["payload"]["cms"]["user_id"], "local-user");
    let prompt = events
        .iter()
        .find(|event| event["type"] == "system.prompt")
        .expect("system prompt event");
    assert!(
        prompt["payload"]["prompt"]
            .as_str()
            .unwrap()
            .contains("Vegvisir")
    );
    Ok(())
}

#[test]
fn bridge_models_list_filters_requested_provider_aliases() -> anyhow::Result<()> {
    let workspace = tempdir()?;
    let data_root = tempdir()?;
    let input = [
        json!({
            "id": "init",
            "method": "initialize",
            "params": {
                "clientInfo": { "name": "test-client" },
                "capabilities": {}
            }
        })
        .to_string(),
        json!({
            "id": "initialized",
            "method": "initialized",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "models-provider",
            "method": "models.list",
            "params": { "provider": "openrouter-hbse" }
        })
        .to_string(),
        json!({
            "id": "models-model-provider",
            "method": "models.list",
            "params": { "modelProvider": "openrouter-hbse" }
        })
        .to_string(),
        json!({
            "id": "codex-model-list",
            "method": "model/list",
            "params": { "modelProvider": "openrouter-hbse" }
        })
        .to_string(),
        json!({
            "id": "bye",
            "method": "shutdown",
            "params": {}
        })
        .to_string(),
    ]
    .join("\n");
    let mut output = Vec::new();
    run_app_server_with_io(
        std::io::Cursor::new(input),
        &mut output,
        BridgeOptions {
            workspace: workspace.path().to_path_buf(),
            data_root: Some(data_root.path().to_path_buf()),
            provider: Some("demo".to_string()),
            model: Some("demo-local".to_string()),
            agent: None,
            dangerously_bypass_approvals_and_sandbox: false,
        },
    )?;

    let output = String::from_utf8(output)?;
    let events = output
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;

    for id in ["models-provider", "models-model-provider"] {
        let event = events
            .iter()
            .find(|event| event["type"] == "models.list" && event["id"] == id)
            .expect("models.list event");
        assert_eq!(event["payload"]["requested_provider"], "openrouter-hbse");
        let models = event["payload"]["models"].as_array().unwrap();
        assert!(
            models
                .iter()
                .any(|model| model["id"] == "openai/gpt-5.4"
                    && model["provider"] == "openrouter-hbse"),
            "provider-filtered models should include OpenRouter-compatible catalog entries: {event:?}"
        );
        assert!(
            !models.iter().any(|model| model["id"] == "demo-local"),
            "provider-filtered models must not be the global catalog: {event:?}"
        );
        assert!(event["payload"]["all_models"].as_array().unwrap().len() >= models.len());
    }

    let response = events
        .iter()
        .find(|event| event["id"] == "codex-model-list")
        .expect("model/list response");
    let data = response["result"]["data"]
        .as_array()
        .unwrap_or_else(|| panic!("model/list response missing data array: {response:?}"));
    assert!(data.iter().any(|model| model["id"] == "openai/gpt-5.4"));
    assert!(!data.iter().any(|model| model["id"] == "demo-local"));
    Ok(())
}

#[test]
fn fast_command_toggles_supported_openai_and_rejects_unsupported_models() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
    app.session.current_provider = "openai".to_string();
    app.session.current_model = "gpt-5.5".to_string();

    let status = app.execute_command("/fast")?.unwrap();
    assert!(status.contains("Fast mode: off"));
    assert!(status.contains("Supported for active model: yes"));

    let enabled = app.execute_command("/fast on")?.unwrap();
    assert!(enabled.contains("Fast mode enabled"));
    assert!(app.session.fast_mode);

    let disabled = app.execute_command("/fast off")?.unwrap();
    assert!(disabled.contains("Fast mode disabled"));
    assert!(!app.session.fast_mode);

    app.session.current_provider = "demo".to_string();
    app.session.current_model = "demo-local".to_string();
    let rejected = app.execute_command("/fast on")?.unwrap();
    assert!(rejected.contains("Fast mode is not supported"));
    assert!(!app.session.fast_mode);
    Ok(())
}

#[test]
fn effort_command_sets_selectable_reasoning_override() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
    app.session.current_provider = "openai".to_string();
    app.session.current_model = "gpt-5.5".to_string();

    let status = app.execute_command("/effort")?.unwrap();
    assert!(status.contains("Selectable values: minimal, low, medium, high, xhigh, max"));

    let selected = app.execute_command("/effort xhigh")?.unwrap();
    assert!(selected.contains("Reasoning effort set to xhigh"));
    assert_eq!(
        app.session.current_reasoning_level.as_deref(),
        Some("xhigh")
    );

    let selected = app.execute_command("/effort max")?.unwrap();
    assert!(selected.contains("Reasoning effort set to max"));
    assert_eq!(app.session.current_reasoning_level.as_deref(), Some("max"));

    let selected = app.execute_command("/effort low")?.unwrap();
    assert!(selected.contains("Reasoning effort set to low"));
    assert_eq!(app.session.current_reasoning_level.as_deref(), Some("low"));

    let models = app.execute_command("/models")?.unwrap();
    assert!(models.contains("reasoning=low"));

    let invalid = app.execute_command("/effort extreme")?.unwrap();
    assert!(invalid.contains("Unknown reasoning effort"));

    let reset = app.execute_command("/effort default")?.unwrap();
    assert!(reset.contains("Reasoning effort reset to model default"));
    assert_eq!(app.session.current_reasoning_level, None);
    Ok(())
}

#[test]
fn conversation_runner_applies_session_reasoning_override_to_provider_model() -> anyhow::Result<()>
{
    let recorded_models = Arc::new(Mutex::new(Vec::new()));
    let mut runner = ConversationRunner {
        provider: ModelRecordingProvider {
            config: ProviderConfig {
                name: "openai".to_string(),
                display_name: None,
                kind: "test".to_string(),
                api_key_env: None,
                base_url: None,
                auth_type: "none".to_string(),
                enabled: true,
                metadata: Default::default(),
            },
            models: recorded_models.clone(),
        },
        models: ModelRegistry::default_catalog()?,
        tools: None,
        tool_executor: None,
        event_sink: None,
        cancel_token: None,
        steering_rx: None,
    };
    let workspace = tempdir()?;
    let mut session =
        vegvisir_rust::core::SessionState::new(workspace.path(), Vec::new(), Vec::new());
    session.current_provider = "openai".to_string();
    session.current_model = "gpt-5.5".to_string();
    session.current_reasoning_level = Some("minimal".to_string());

    runner.send(&mut session, "hello")?;

    let models = recorded_models.lock().unwrap();
    assert_eq!(
        models[0]
            .metadata
            .get("reasoning_level")
            .and_then(Value::as_str),
        Some("minimal")
    );
    Ok(())
}

#[test]
fn provider_payloads_apply_model_reasoning_settings_across_wire_formats() -> anyhow::Result<()> {
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: "solve carefully".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    }];

    let openai_model = test_model_with_metadata(
        "openai",
        "gpt-test",
        BTreeMap::from([
            ("reasoning_level".to_string(), json!("xhigh")),
            ("reasoning_summary".to_string(), json!(true)),
        ]),
    );
    let responses =
        vegvisir_rust::provider::test_support::responses_payload_for_test(&messages, &openai_model);
    assert_eq!(
        responses.pointer("/reasoning/effort"),
        Some(&json!("xhigh"))
    );
    assert_eq!(
        responses.pointer("/reasoning/summary"),
        Some(&json!("auto"))
    );

    let anthropic_model = test_model_with_metadata(
        "anthropic",
        "claude-test",
        BTreeMap::from([("reasoning_level".to_string(), json!("medium"))]),
    );
    let anthropic = vegvisir_rust::provider::test_support::anthropic_messages_payload_for_test(
        &messages,
        &anthropic_model,
    );
    assert_eq!(anthropic.pointer("/thinking/type"), Some(&json!("enabled")));
    assert_eq!(
        anthropic.pointer("/thinking/budget_tokens"),
        Some(&json!(8192))
    );

    let google_model = test_model_with_metadata(
        "google",
        "gemini-test",
        BTreeMap::from([("reasoning_level".to_string(), json!("low"))]),
    );
    let google = vegvisir_rust::provider::test_support::google_generate_content_payload_for_test(
        &messages,
        &google_model,
    );
    assert_eq!(
        google.pointer("/generationConfig/thinkingConfig/thinkingBudget"),
        Some(&json!(1024))
    );

    Ok(())
}

#[test]
fn openai_tool_loop_applies_model_reasoning_effort() -> anyhow::Result<()> {
    let payloads = Arc::new(Mutex::new(Vec::<Value>::new()));
    let payloads_for_post = Arc::clone(&payloads);
    let mut post = move |payload: Value| -> anyhow::Result<Value> {
        payloads_for_post.lock().unwrap().push(payload);
        Ok(json!({"choices": [{"message": {"content": "done"}}]}))
    };
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: "inspect".to_string(),
        attachments: Vec::new(),
        created_at: Utc::now(),
    }];
    let model = test_model_with_metadata(
        "openai",
        "gpt-test",
        BTreeMap::from([("reasoning_effort".to_string(), json!("max"))]),
    );
    let mut execute_tool = |_: &str, _: Map<String, Value>| "unused".to_string();

    let result = openai_tool_loop(&model, &messages, &[], &mut execute_tool, &mut post, 1)?;

    assert_eq!(result, "done");
    let captured = payloads.lock().unwrap();
    assert_eq!(captured[0].get("reasoning_effort"), Some(&json!("max")));
    Ok(())
}

#[test]
fn app_server_bridge_exposes_full_command_and_openai_control_surface() -> anyhow::Result<()> {
    let workspace = tempdir()?;
    let data_root = tempdir()?;
    let input = [
        json!({
            "id": "commands",
            "method": "commands.list",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "suggest",
            "method": "commands.suggest",
            "params": { "prefix": "/pro" }
        })
        .to_string(),
        json!({
            "id": "describe",
            "method": "commands.describe",
            "params": { "name": "providers" }
        })
        .to_string(),
        json!({
            "id": "capabilities",
            "method": "bridge.capabilities",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "memory-recent",
            "method": "memory.recent",
            "params": { "limit": 2 }
        })
        .to_string(),
        json!({
            "id": "skills-status",
            "method": "skills.status",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "mcp-list",
            "method": "mcp.list",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "verify-runtime",
            "method": "verify.run",
            "params": { "scope": "runtime" }
        })
        .to_string(),
        json!({
            "id": "provider",
            "method": "provider.select",
            "params": { "provider": "demo" }
        })
        .to_string(),
        json!({
            "id": "model",
            "method": "model.select",
            "params": { "model": "demo-local" }
        })
        .to_string(),
        json!({
            "id": "effort",
            "method": "effort.set",
            "params": { "effort": "default" }
        })
        .to_string(),
        json!({
            "id": "fast",
            "method": "fast.status",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "limit",
            "method": "toolLimit.status",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "runtime",
            "method": "runtime.status",
            "params": {}
        })
        .to_string(),
        json!({
            "id": "openai",
            "method": "openai.compat.info",
            "params": { "host": "127.0.0.1", "port": 11435 }
        })
        .to_string(),
        json!({
            "id": "invoke",
            "method": "command.invoke",
            "params": { "command": "/providers" }
        })
        .to_string(),
        json!({
            "id": "bye",
            "method": "shutdown",
            "params": {}
        })
        .to_string(),
    ]
    .join("\n");
    let mut output = Vec::new();
    run_app_server_with_io(
        std::io::Cursor::new(input),
        &mut output,
        BridgeOptions {
            workspace: workspace.path().to_path_buf(),
            data_root: Some(data_root.path().to_path_buf()),
            provider: Some("demo".to_string()),
            model: Some("demo-local".to_string()),
            agent: None,
            dangerously_bypass_approvals_and_sandbox: false,
        },
    )?;

    let output = String::from_utf8(output)?;
    let events = output
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;

    let commands = events
        .iter()
        .find(|event| event["type"] == "commands.list")
        .expect("commands.list event");
    let command_list = commands["payload"]["commands"].as_array().unwrap();
    assert!(
        command_list
            .iter()
            .any(|command| command["name"] == "/subagents")
    );
    assert!(command_list.iter().any(|command| command["name"] == "/mcp"));
    assert!(
        command_list
            .iter()
            .any(|command| command["name"] == "/skills")
    );

    let suggest = events
        .iter()
        .find(|event| event["type"] == "commands.suggest")
        .expect("commands.suggest event");
    assert!(
        suggest["payload"]["suggestions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "/providers")
    );

    let describe = events
        .iter()
        .find(|event| event["type"] == "commands.describe")
        .expect("commands.describe event");
    assert_eq!(describe["payload"]["canonical"], "/providers");
    assert_eq!(describe["payload"]["command"]["name"], "/providers");

    let capabilities = events
        .iter()
        .find(|event| event["type"] == "bridge.capabilities")
        .expect("bridge.capabilities event");
    assert!(
        capabilities["payload"]["native_methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|method| method == "command.invoke")
    );
    assert!(
        capabilities["payload"]["command_backed_methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|method| method["method"] == "skills.status" && method["command"] == "/skills")
    );

    for (event_type, command_prefix) in [
        ("memory.recent", "/memory recent"),
        ("skills.status", "/skills status"),
        ("mcp.list", "/mcp list"),
        ("verify.run", "/verify runtime"),
    ] {
        let event = events
            .iter()
            .find(|event| event["type"] == event_type)
            .unwrap_or_else(|| panic!("{event_type} event"));
        assert!(
            event["payload"]["command"]
                .as_str()
                .unwrap()
                .starts_with(command_prefix),
            "unexpected command-backed bridge command for {event_type}: {event:?}"
        );
        assert!(event["payload"]["output"].is_string());
    }

    let provider = events
        .iter()
        .find(|event| event["type"] == "provider.selected")
        .expect("provider.selected event");
    assert_eq!(provider["payload"]["current_provider"], "demo");

    let model = events
        .iter()
        .find(|event| event["type"] == "model.selected")
        .expect("model.selected event");
    assert_eq!(model["payload"]["current_model"], "demo-local");

    assert!(events.iter().any(|event| event["type"] == "effort.updated"));
    assert!(events.iter().any(|event| event["type"] == "fast.status"));
    assert!(
        events
            .iter()
            .any(|event| event["type"] == "toolLimit.status")
    );

    let runtime = events
        .iter()
        .find(|event| event["type"] == "runtime.status")
        .expect("runtime.status event");
    assert_eq!(runtime["payload"]["session"]["provider"], "demo");
    assert!(
        runtime["payload"]["status_output"]
            .as_str()
            .unwrap()
            .contains("Session")
    );

    let openai = events
        .iter()
        .find(|event| event["type"] == "openai.compat.info")
        .expect("openai.compat.info event");
    assert_eq!(openai["payload"]["base_url"], "http://127.0.0.1:11435/v1");
    assert!(openai["payload"]["note"].as_str().unwrap().contains("HBSE"));

    let invoked = events
        .iter()
        .find(|event| event["type"] == "command.completed" && event["id"] == "invoke")
        .expect("command.invoke event");
    assert_eq!(invoked["payload"]["command"], "/providers");
    assert!(
        invoked["payload"]["output"]
            .as_str()
            .unwrap()
            .contains("demo")
    );
    Ok(())
}
