use std::path::PathBuf;

use serde_json::{Value, json};

use crate::{
    checkpoints::{CheckpointStore, RunSnapshot},
    context::ContextManager,
    guardrails::{GuardrailEngine, PermissionPolicy},
    hooks::HookManager,
    memory::{VegvisirCms, VegvisirCmsConfig},
    model::Model,
    observability::EventLogger,
    planning::Plan,
    policy::RuntimePolicy,
    prompts::PromptAssembler,
    retrieval::{InMemoryRetriever, RetrievalDocument},
    state::RunState,
    tools::{
        ToolExecutor, ToolRegistry, build_builtin_registry,
        build_builtin_registry_with_cms_and_mode,
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
            state
        };
        let checkpoint = self.checkpoint_path(&task, &state);
        self.logger.emit(
            "run_start",
            json!({"run_id": state.run_id, "goal": task.goal}),
        );
        self.inject_cms_context(&state.goal);

        let mut final_answer = None;
        while state.step < task.max_steps {
            state.step += 1;
            self.logger.emit("step_start", json!({"step": state.step}));
            for document in self.retriever.search(&state.goal, 5) {
                self.context.add(Message::named(
                    Role::System,
                    document.text,
                    format!("retrieval:{}", document.id),
                ));
            }
            let messages = self
                .prompts
                .assemble(&state, self.context.visible_messages());
            let messages = self.hooks.before_model(&state, messages);
            let decision = self.model.decide(&messages, &self.tools.schemas());
            let decision = self.hooks.after_model(&state, decision);
            self.context
                .add(Message::new(Role::Assistant, &decision.thought));
            self.logger.emit(
                "model_decision",
                json!({"step": state.step, "decision": decision}),
            );

            if decision.is_final() {
                final_answer = decision.final_answer.clone();
                state.status = "completed".to_string();
                complete_plan(&mut state);
                if let Some(answer) = &final_answer {
                    self.commit_cms_turn(&state.goal, answer);
                }
                break;
            }

            let Some(action) = decision.action.clone() else {
                self.context
                    .add(Message::named(Role::Tool, "No action selected.", "error"));
                continue;
            };
            let call = self.hooks.before_tool(
                &state,
                ToolCall {
                    name: action,
                    args: decision.args.clone(),
                },
            );
            let observation = self.executor.execute(call.clone());
            let observation = self.hooks.after_tool(&state, &call, observation);
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
        })
    }

    fn checkpoint_path(&self, task: &AgentTask, state: &RunState) -> PathBuf {
        task.checkpoint_dir
            .clone()
            .unwrap_or_else(|| task.workspace.join(".vegvisir").join("runs"))
            .join(format!("{}.json", state.run_id))
    }

    fn inject_cms_context(&mut self, goal: &str) {
        match self.cms.prepare_context(goal) {
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
            }
            Err(error) => {
                self.logger
                    .emit("cms_context_error", json!({"error": error.to_string()}));
            }
        }
    }

    fn commit_cms_turn(&mut self, user_message: &str, assistant_response: &str) {
        match self.cms.complete_turn(user_message, assistant_response) {
            Ok(results) => self.logger.emit(
                "cms_writeback",
                json!({
                    "committed": results.len(),
                    "memory_ids": results.into_iter().map(|result| result.memory_id.0).collect::<Vec<_>>()
                }),
            ),
            Err(error) => self
                .logger
                .emit("cms_writeback_error", json!({"error": error.to_string()})),
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
        if item.get("status").and_then(Value::as_str) == Some("in_progress") {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("status".to_string(), Value::String("passed".to_string()));
            }
        }
    }
}
