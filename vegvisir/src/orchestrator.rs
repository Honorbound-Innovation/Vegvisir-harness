use std::{any::type_name, collections::BTreeMap, path::PathBuf, sync::Arc};

use cms_v2::cms_api::CommitResult;
use serde_json::{Value, json};

use crate::{
    checkpoints::{CheckpointStore, RunSnapshot},
    context::{ContextCompactionSummary, ContextManager},
    guardrails::{GuardrailEngine, PermissionPolicy},
    hooks::HookManager,
    memory::{VegvisirCms, VegvisirCmsConfig},
    model::Model,
    observability::EventLogger,
    planning::Plan,
    policy::RuntimePolicy,
    prompts::PromptAssembler,
    provider::ProviderRunEvent,
    retrieval::{InMemoryRetriever, RetrievalDocument},
    run_artifacts::{RunArtifactManager, RunStatus},
    state::RunState,
    tools::{
        CommandOutputSink, ToolExecutor, ToolRegistry, build_builtin_registry,
        build_builtin_registry_with_cms_and_mode, with_command_output_sink,
    },
    types::{Message, Role, ToolCall},
};

#[derive(Clone, Debug)]
pub struct AgentTask {
    pub goal: String,
    pub workspace: PathBuf,
    pub max_steps: usize,
    pub checkpoint_dir: Option<PathBuf>,
    pub resume_run_id: Option<String>,
}

impl AgentTask {
    pub fn new(goal: impl Into<String>, workspace: impl Into<PathBuf>) -> Self {
        Self {
            goal: goal.into(),
            workspace: workspace.into(),
            max_steps: 12,
            checkpoint_dir: None,
            resume_run_id: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AgentResult {
    pub run_id: String,
    pub status: String,
    pub final_answer: Option<String>,
    pub steps: usize,
    pub checkpoint: Option<PathBuf>,
    pub snapshot: Option<PathBuf>,
    pub artifact_dir: Option<PathBuf>,
}

pub struct AgentHarness<M: Model> {
    pub model: M,
    pub tools: ToolRegistry,
    pub executor: ToolExecutor,
    pub context: ContextManager,
    pub prompts: PromptAssembler,
    pub hooks: HookManager,
    pub logger: EventLogger,
    pub retriever: InMemoryRetriever,
    pub cms: VegvisirCms,
}

impl<M: Model> AgentHarness<M> {
    pub fn default(model: M, workspace: impl Into<PathBuf>) -> anyhow::Result<Self> {
        Self::with_options(model, workspace, None, false, false, None)
    }

    pub fn with_dangerous_bypass(model: M, workspace: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let workspace = workspace.into();
        let cms_config = VegvisirCmsConfig::for_workspace(&workspace);
        let tools = build_builtin_registry_with_cms_and_mode(&workspace, cms_config, true)?;
        let mut harness = Self::with_options(model, workspace, Some(tools), true, false, None)?;
        harness
            .executor
            .guardrails
            .policy
            .bypass_approvals_and_sandbox = true;
        Ok(harness)
    }

    pub fn with_options(
        model: M,
        workspace: impl Into<PathBuf>,
        tools: Option<ToolRegistry>,
        allow_risky_tools: bool,
        require_human_approval: bool,
        trace_path: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let workspace = workspace.into();
        let registry = match tools {
            Some(tools) => tools,
            None => build_builtin_registry(&workspace)?,
        };
        let logger = EventLogger::new(trace_path);
        let guardrails = GuardrailEngine {
            policy: PermissionPolicy {
                allow_risky_tools,
                require_human_approval,
                ..PermissionPolicy::default()
            },
            ..GuardrailEngine::default()
        };
        let executor = ToolExecutor {
            registry: registry.clone(),
            guardrails,
            runtime_policy: RuntimePolicy::default(),
            logger: logger.clone(),
        };
        let cms = VegvisirCms::open(VegvisirCmsConfig::for_workspace(&workspace))?;
        Ok(Self {
            model,
            tools: registry,
            executor,
            context: ContextManager::default(),
            prompts: PromptAssembler::default(),
            hooks: HookManager::default(),
            logger,
            retriever: InMemoryRetriever::default(),
            cms,
        })
    }

    pub fn with_cms(mut self, cms: VegvisirCms) -> Self {
        self.cms = cms;
        self
    }

    pub fn run(&mut self, task: AgentTask) -> anyhow::Result<AgentResult> {
        let checkpoint_store = CheckpointStore::new(
            task.checkpoint_dir
                .clone()
                .unwrap_or_else(|| task.workspace.join(".vegvisir").join("runs")),
        );
        let mut state = if let Some(run_id) = &task.resume_run_id {
            let snapshot = checkpoint_store.load(run_id)?;
            self.context = snapshot.context;
            snapshot.state
        } else {
            let mut state = RunState::new(&task.goal);
            state.metadata.insert(
                "plan".to_string(),
                serde_json::to_value(Plan::from_goal(&task.goal))?,
            );
            self.context.add(Message::new(Role::User, &task.goal));
            self.persist_pending_context_compactions(&state);
            state
        };
        let checkpoint = self.checkpoint_path(&task, &state);
        let (artifact_manager, mut artifact_manifest) = RunArtifactManager::start_with_run_id(
            &task.workspace,
            task.workspace.join(".vegvisir"),
            state.run_id.clone(),
            Option::<PathBuf>::None,
            "headless",
            "agent-harness",
            type_name::<M>(),
            Some("AgentHarness".to_string()),
        )?;
        artifact_manager.write_request(&json!({
            "goal": task.goal,
            "workspace": task.workspace,
            "max_steps": task.max_steps,
            "resume_run_id": task.resume_run_id,
        }))?;
        self.logger.emit(
            "run_start",
            json!({"run_id": state.run_id, "goal": task.goal}),
        );
        self.inject_cms_context(&state);

        let mut final_answer = None;
        let mut memory_writeback: Option<Result<Vec<CommitResult>, String>> = None;
        while state.step < task.max_steps {
            state.step += 1;
            self.logger.emit("step_start", json!({"step": state.step}));
            for document in self.retriever.search(&state.goal, 5) {
                self.context.add(Message::named(
                    Role::System,
                    document.text,
                    format!("retrieval:{}", document.id),
                ));
                self.persist_pending_context_compactions(&state);
            }
            let messages = self
                .prompts
                .assemble(&state, self.context.visible_messages());
            let messages = self.hooks.before_model(&state, messages);
            let decision = self.model.decide(&messages, &self.tools.schemas());
            let decision = self.hooks.after_model(&state, decision);
            self.context
                .add(Message::new(Role::Assistant, &decision.thought));
            self.persist_pending_context_compactions(&state);
            self.logger.emit(
                "model_decision",
                json!({"step": state.step, "decision": decision}),
            );

            if decision.is_final() {
                final_answer = decision.final_answer.clone();
                state.status = "completed".to_string();
                complete_plan(&mut state);
                if let Some(answer) = &final_answer {
                    memory_writeback = Some(self.commit_cms_turn(&state.goal, answer));
                }
                break;
            }

            let Some(action) = decision.action.clone() else {
                self.context
                    .add(Message::named(Role::Tool, "No action selected.", "error"));
                self.persist_pending_context_compactions(&state);
                continue;
            };
            let call = self.hooks.before_tool(
                &state,
                ToolCall {
                    name: action,
                    args: decision.args.clone(),
                },
            );
            artifact_manager.append_observed_provider_event(&ProviderRunEvent::ToolStart {
                name: call.name.clone(),
                args: serde_json::to_string(&call.args).unwrap_or_default(),
            })?;
            let command_output_sink = headless_command_output_sink(&artifact_manager, &call.name);
            let observation = with_command_output_sink(Some(command_output_sink), || {
                self.executor.execute(call.clone())
            });
            let observation = self.hooks.after_tool(&state, &call, observation);
            artifact_manager.append_observed_provider_event(&ProviderRunEvent::ToolEnd {
                name: call.name.clone(),
                ok: observation.ok,
                summary: observation.content.clone(),
                detail: observation.error.clone(),
            })?;
            let mut message =
                Message::named(Role::Tool, observation.content.clone(), call.name.clone());
            message
                .metadata
                .insert("ok".to_string(), Value::Bool(observation.ok));
            if let Some(error) = &observation.error {
                message
                    .metadata
                    .insert("error".to_string(), Value::String(error.clone()));
            }
            self.context.add(message);
            self.persist_pending_context_compactions(&state);
            self.retriever.add(RetrievalDocument {
                id: format!("{}:{}", state.run_id, state.step),
                text: observation.content.clone(),
                metadata: [("tool".to_string(), call.name.clone())]
                    .into_iter()
                    .collect(),
            });
            record_plan_evidence(&mut state, &observation.content);
            self.remember_cms_observation(&state, &call.name, &observation.content);
            state.checkpoint(&checkpoint)?;
            checkpoint_store.save(&RunSnapshot {
                state: state.clone(),
                context: self.context.clone(),
                cms_root: Some(self.cms.config.db_path.display().to_string()),
            })?;
            self.logger.emit(
                "step_end",
                json!({"step": state.step, "ok": observation.ok}),
            );
        }

        if final_answer.is_none() {
            state.status = "max_steps_exceeded".to_string();
        }
        state.checkpoint(&checkpoint)?;
        let snapshot_path = Some(checkpoint_store.save(&RunSnapshot {
            state: state.clone(),
            context: self.context.clone(),
            cms_root: Some(self.cms.config.db_path.display().to_string()),
        })?);
        let artifact_status = if state.status == "completed" {
            if let Some(answer) = &final_answer {
                artifact_manager.write_result(answer)?;
            }
            match &memory_writeback {
                Some(Ok(results)) => {
                    artifact_manager.write_memory_written_from_outcome(results, None)?
                }
                Some(Err(error)) => {
                    artifact_manager.write_memory_written_from_outcome(&[], Some(error))?
                }
                None => artifact_manager.write_memory_written_from_outcome(&[], None)?,
            }
            artifact_manager
                .write_approvals_from_pending(&self.executor.guardrails.approvals.pending())?;
            artifact_manager.write_subagents_from_board()?;
            artifact_manager.write_workspace_change_artifacts()?;
            RunStatus::Completed
        } else {
            artifact_manager.fail(
                &mut artifact_manifest,
                format!("headless run ended with status {}", state.status),
                true,
            )?;
            RunStatus::Failed
        };
        if artifact_status == RunStatus::Completed {
            artifact_manager.finish(&mut artifact_manifest, artifact_status)?;
        }
        self.logger.emit(
            "run_end",
            json!({"run_id": state.run_id, "status": state.status}),
        );

        Ok(AgentResult {
            run_id: state.run_id,
            status: state.status,
            final_answer,
            steps: state.step,
            checkpoint: Some(checkpoint),
            snapshot: snapshot_path,
            artifact_dir: Some(artifact_manager.run_dir.clone()),
        })
    }

    fn checkpoint_path(&self, task: &AgentTask, state: &RunState) -> PathBuf {
        task.checkpoint_dir
            .clone()
            .unwrap_or_else(|| task.workspace.join(".vegvisir").join("runs"))
            .join(format!("{}.json", state.run_id))
    }

    fn persist_pending_context_compactions(&mut self, state: &RunState) {
        for summary in self.context.take_pending_compactions() {
            self.persist_context_compaction(state, summary);
        }
    }

    fn persist_context_compaction(&mut self, state: &RunState, summary: ContextCompactionSummary) {
        let body = summary.render();
        let title = format!(
            "Context compaction {} for run {}",
            summary.sequence, state.run_id
        );
        let mut metadata = BTreeMap::new();
        metadata.insert("run_id".to_string(), Value::String(state.run_id.clone()));
        metadata.insert("goal".to_string(), Value::String(state.goal.clone()));
        metadata.insert(
            "compaction_sequence".to_string(),
            Value::String(summary.sequence.to_string()),
        );
        metadata.insert(
            "message_count".to_string(),
            Value::String(summary.message_count.to_string()),
        );
        match self
            .cms
            .remember_with_metadata("context-compaction", title, body, metadata)
        {
            Ok(result) => {
                let memory_id = result.memory_id.0;
                self.context
                    .mark_compaction_persisted(summary.sequence, memory_id.clone());
                self.logger.emit(
                    "context_compaction_persisted",
                    json!({
                        "run_id": state.run_id,
                        "sequence": summary.sequence,
                        "message_count": summary.message_count,
                        "memory_id": memory_id,
                    }),
                );
            }
            Err(error) => self.logger.emit(
                "context_compaction_persist_error",
                json!({
                    "run_id": state.run_id,
                    "sequence": summary.sequence,
                    "error": error.to_string(),
                }),
            ),
        }
    }

    fn inject_cms_context(&mut self, state: &RunState) {
        match self.cms.prepare_context(&state.goal) {
            Ok(prepared) => {
                if prepared.packed_text.trim().is_empty() {
                    return;
                }
                let mut message =
                    Message::named(Role::System, prepared.packed_text, "cms_v2_context");
                message
                    .metadata
                    .insert("trace_id".to_string(), Value::String(prepared.trace_id));
                message.metadata.insert(
                    "included_memory_ids".to_string(),
                    json!(
                        prepared
                            .included_memory_ids
                            .into_iter()
                            .map(|memory_id| memory_id.0)
                            .collect::<Vec<_>>()
                    ),
                );
                self.context.add(message);
                self.persist_pending_context_compactions(state);
            }
            Err(error) => {
                self.logger
                    .emit("cms_context_error", json!({"error": error.to_string()}));
            }
        }
    }

    fn commit_cms_turn(
        &mut self,
        user_message: &str,
        assistant_response: &str,
    ) -> Result<Vec<CommitResult>, String> {
        match self.cms.complete_turn(user_message, assistant_response) {
            Ok(results) => {
                self.logger.emit(
                    "cms_writeback",
                    json!({
                        "committed": results.len(),
                        "memory_ids": results.iter().map(|result| result.memory_id.0.clone()).collect::<Vec<_>>()
                    }),
                );
                Ok(results)
            }
            Err(error) => {
                let error = error.to_string();
                self.logger
                    .emit("cms_writeback_error", json!({"error": error}));
                Err(error)
            }
        }
    }

    fn remember_cms_observation(&mut self, state: &RunState, tool_name: &str, content: &str) {
        let title = format!("Tool observation {} step {}", tool_name, state.step);
        if let Err(error) = self.cms.remember("tool-observation", title, content) {
            self.logger.emit(
                "cms_observation_error",
                json!({"tool": tool_name, "error": error.to_string()}),
            );
        }
    }
}

fn headless_command_output_sink(
    artifact_manager: &RunArtifactManager,
    tool_name: &str,
) -> CommandOutputSink {
    let artifact_manager = artifact_manager.clone();
    let tool_name = tool_name.to_string();
    Arc::new(move |chunk| {
        let _ = artifact_manager.append_observed_provider_event(&ProviderRunEvent::ToolOutput {
            name: tool_name.clone(),
            stream: chunk.stream,
            chunk: chunk.chunk,
            truncated: chunk.truncated,
        });
    })
}

fn record_plan_evidence(state: &mut RunState, evidence: &str) {
    let Some(plan) = state
        .metadata
        .get_mut("plan")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let Some(items) = plan.get_mut("items").and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        if item.get("status").and_then(Value::as_str) == Some("in_progress") {
            let short: String = evidence.chars().take(1000).collect();
            let Some(obj) = item.as_object_mut() else {
                continue;
            };
            let evidence = obj
                .entry("evidence")
                .or_insert_with(|| Value::Array(Vec::new()));
            if !evidence.is_array() {
                *evidence = Value::Array(Vec::new());
            }
            if let Some(entries) = evidence.as_array_mut() {
                entries.push(Value::String(short));
            }
            return;
        }
    }
}

fn complete_plan(state: &mut RunState) {
    let Some(plan) = state
        .metadata
        .get_mut("plan")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let Some(items) = plan.get_mut("items").and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        if item.get("status").and_then(Value::as_str) == Some("in_progress")
            && let Some(obj) = item.as_object_mut()
        {
            obj.insert("status".to_string(), Value::String("passed".to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        context::ContextManager,
        model::ScriptedModel,
        run_artifacts::{
            RunArtifactCompletenessEvidence, RunArtifactCompletenessOverall, RunManifest,
        },
        types::{AgentDecision, Message, Role},
    };
    use serde_json::{Map, Value, json};

    #[test]
    fn headless_harness_writes_run_artifact_bundle_for_final_answer() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace)?;
        let model = ScriptedModel::new(vec![AgentDecision::final_decision(
            "I can answer immediately.",
            "Headless artifact capture works.",
        )]);
        let mut harness = AgentHarness::default(model, &workspace)?;

        let result = harness.run(AgentTask::new("write a final answer", &workspace))?;

        assert_eq!(result.status, "completed");
        let artifact_dir = result
            .artifact_dir
            .expect("artifact dir should be recorded");
        assert!(artifact_dir.exists());
        assert_eq!(
            std::fs::read_to_string(artifact_dir.join("result.md"))?,
            "Headless artifact capture works."
        );
        let manifest: RunManifest = serde_json::from_str(&std::fs::read_to_string(
            artifact_dir.join("manifest.json"),
        )?)?;
        assert_eq!(manifest.run_id, result.run_id);
        assert_eq!(manifest.provider, "agent-harness");
        assert_eq!(manifest.model, std::any::type_name::<ScriptedModel>());
        assert!(manifest.finished_at.is_some());
        assert!(artifact_dir.join("request.json").exists());
        assert!(artifact_dir.join("verification.json").exists());
        assert!(artifact_dir.join("file-changes.json").exists());
        assert!(artifact_dir.join("diff.patch").exists());
        assert!(artifact_dir.join("approvals.json").exists());
        assert!(artifact_dir.join("subagents.json").exists());
        assert!(artifact_dir.join("memory-written.json").exists());

        let completeness: RunArtifactCompletenessEvidence = serde_json::from_str(
            &std::fs::read_to_string(artifact_dir.join("artifact-completeness.json"))?,
        )?;
        assert_eq!(
            completeness.overall,
            RunArtifactCompletenessOverall::Complete
        );
        Ok(())
    }

    #[test]
    fn headless_harness_records_tool_events_in_run_artifacts() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace)?;
        std::fs::write(workspace.join("example.txt"), "hello")?;
        let mut args = Map::new();
        args.insert("path".to_string(), json!("."));
        let model = ScriptedModel::new(vec![
            AgentDecision {
                thought: "List files for evidence.".to_string(),
                action: Some("list_files".to_string()),
                args,
                final_answer: None,
            },
            AgentDecision::final_decision("Done.", "Listed files."),
        ]);
        let mut harness = AgentHarness::default(model, &workspace)?;

        let result = harness.run(AgentTask::new("inspect workspace", &workspace))?;

        let artifact_dir = result
            .artifact_dir
            .expect("artifact dir should be recorded");
        let provider_events = std::fs::read_to_string(artifact_dir.join("provider-events.jsonl"))?;
        assert!(provider_events.contains(r#""kind":"tool_start""#));
        assert!(provider_events.contains(r#""kind":"tool_end""#));
        assert!(provider_events.contains("list_files"));
        let tool_events = std::fs::read_to_string(artifact_dir.join("tool-events.jsonl"))?;
        assert!(tool_events.contains("list_files"));
        let runtime_events = std::fs::read_to_string(artifact_dir.join("runtime-events.jsonl"))?;
        assert!(runtime_events.contains(r#""type":"tool_started""#));
        assert!(runtime_events.contains(r#""type":"tool_completed""#));
        Ok(())
    }

    #[test]
    fn harness_persists_pending_context_compactions_to_cms() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut cms_config = VegvisirCmsConfig::for_workspace(tmp.path());
        cms_config.db_path = tmp.path().join("cms.sqlite3");
        let cms = VegvisirCms::open(cms_config)?;

        let mut harness =
            AgentHarness::default(ScriptedModel::default(), tmp.path())?.with_cms(cms);
        harness.context = ContextManager::new(4);
        let state = RunState::new("persist compacted context summaries");

        harness.context.add(Message::new(
            Role::User,
            "Inspect vegvisir/src/context.rs and run cargo test -p vegvisir-rust context",
        ));
        harness.context.add(Message::new(
            Role::Assistant,
            "Implemented structured compaction and decided to persist it through CMS.",
        ));
        harness.context.add(Message::named(
            Role::Tool,
            "cargo test -p vegvisir-rust context passed after fixing failures",
            "run_command",
        ));
        harness
            .context
            .add(Message::new(Role::User, "What remains?"));
        harness.context.add(Message::new(
            Role::Assistant,
            "Follow-up: verify metadata and ECM retrieval.",
        ));

        assert_eq!(harness.context.compacted_summaries.len(), 1);
        assert!(
            harness.context.compacted_summaries[0]
                .cms_memory_id
                .is_none()
        );

        harness.persist_pending_context_compactions(&state);

        let memory_id = harness.context.compacted_summaries[0]
            .cms_memory_id
            .clone()
            .expect("compaction should be marked with CMS memory id");
        let bundle = harness.cms.retrieve("structured compaction CMS", 10)?;
        let persisted = bundle
            .results
            .iter()
            .find(|result| result.memory.id.0 == memory_id)
            .expect("persisted compaction memory should be retrievable");

        assert_eq!(persisted.memory.memory_type, "context-compaction");
        assert!(persisted.memory.body.contains("## Commands Run"));
        assert_eq!(
            persisted
                .memory
                .metadata
                .get("run_id")
                .and_then(Value::as_str),
            Some(state.run_id.as_str())
        );
        assert_eq!(
            persisted
                .memory
                .metadata
                .get("compaction_sequence")
                .and_then(Value::as_str),
            Some("1")
        );
        assert_eq!(
            persisted
                .memory
                .metadata
                .get("visibility")
                .and_then(Value::as_str),
            Some("private")
        );

        Ok(())
    }
}
