use std::{
    collections::BTreeMap,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use serde_json::{Value, json};

use crate::{
    attachments::{attachment_for, extract_attachments},
    command_registry::CommandRegistry,
    core::{
        AgentProfile, AgentProfileStore, ChatMessage, ConfigStore, HbseServiceRef,
        HbseServiceRefStore, McpConfigStore, McpServerConfig, McpToolConfig, McpTransport,
        ModelRegistry, ProviderConfig, ProviderRegistry, SessionManager, SessionState,
        SessionStore, default_system_prompt, default_tool_definitions, load_skill_definitions,
    },
    environment::get_env,
    guardrails::{GuardrailEngine, PermissionPolicy},
    mcp::{load_mcp_servers, register_mcp_tools},
    memory::{VegvisirCms, VegvisirCmsConfig, default_vegvisir_data_root},
    model_discovery::discover_provider_models,
    observability::EventLogger,
    policy::RuntimePolicy,
    provider::{ConversationRunner, ProviderRouter, direct_provider_auth_allowed},
    subagents::{SubAgentStatus, SubAgentTaskRecord},
    tools::{ToolExecutor, ToolRegistry, build_builtin_registry_with_cms_and_mode},
    ui::{
        input::{InputState, Suggestion},
        layout::LayoutRenderer,
    },
};

pub struct TuiApplication {
    pub cwd: PathBuf,
    pub data_root: PathBuf,
    pub session: SessionState,
    pub sessions: SessionManager,
    pub agents: AgentProfileStore,
    pub config: ConfigStore,
    pub commands: CommandRegistry,
    pub provider_registry: ProviderRegistry,
    pub models: ModelRegistry,
    pub cms: VegvisirCms,
    pub tool_registry: ToolRegistry,
    pub tool_executor: ToolExecutor,
    pub logger: EventLogger,
    pub input: InputState,
    pub renderer: LayoutRenderer,
    pub chat_scroll_offset: usize,
    pub running: bool,
    pub clear_requested: bool,
    pub redraw_requested: bool,
    pub risky_tools_enabled: bool,
    pub dangerously_bypass_approvals_and_sandbox: bool,
    pub mcp_servers: Vec<crate::core::McpServerConfig>,
    pub hbse_services: Vec<HbseServiceRef>,
    pub pending_send: Option<JoinHandle<anyhow::Result<SessionState>>>,
    pending_background_jobs: Vec<JoinHandle<anyhow::Result<String>>>,
    pending_stream: Option<Receiver<StreamEvent>>,
    pending_cancel: Option<Arc<AtomicBool>>,
}

enum StreamEvent {
    Delta(String),
}

struct ProjectListEntry {
    cwd: String,
    latest_session_id: String,
    latest_created_at: chrono::DateTime<chrono::Utc>,
    session_count: usize,
    message_count: usize,
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct WorkspaceIndex {
    #[serde(default)]
    active_sessions: BTreeMap<String, String>,
    #[serde(default)]
    aliases: BTreeMap<String, String>,
    #[serde(default)]
    provider_overrides: BTreeMap<String, ProviderSelection>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct ProviderSelection {
    provider: String,
    model: String,
}

impl TuiApplication {
    pub fn new(cwd: impl AsRef<Path>) -> anyhow::Result<Self> {
        Self::new_with_dangerous_bypass(cwd, false)
    }

    pub fn new_with_dangerous_bypass(
        cwd: impl AsRef<Path>,
        dangerously_bypass_approvals_and_sandbox: bool,
    ) -> anyhow::Result<Self> {
        let cwd = cwd.as_ref().to_path_buf();
        let data_root = default_vegvisir_data_root();
        Self::with_data_root_and_dangerous_bypass(
            cwd,
            data_root,
            dangerously_bypass_approvals_and_sandbox,
        )
    }

    pub fn with_data_root(
        cwd: impl AsRef<Path>,
        data_root: impl AsRef<Path>,
    ) -> anyhow::Result<Self> {
        Self::with_data_root_and_dangerous_bypass(cwd, data_root, false)
    }

    pub fn with_data_root_and_dangerous_bypass(
        cwd: impl AsRef<Path>,
        data_root: impl AsRef<Path>,
        dangerously_bypass_approvals_and_sandbox: bool,
    ) -> anyhow::Result<Self> {
        let cwd = cwd.as_ref().to_path_buf();
        let data_root = data_root.as_ref().to_path_buf();
        let config = ConfigStore::new(data_root.join("config.json"));
        let defaults = config.load().unwrap_or_default();
        let default_user_id = configured_user_id(&defaults);
        let tools = default_tool_definitions()?;
        let skills = load_skill_definitions(&cwd, &data_root)?;
        let sessions = SessionManager::new(
            SessionStore::new(session_root_for_user(&data_root, &default_user_id))?,
            &cwd,
        );
        let agents = AgentProfileStore::new(data_root.join("agents"))?;
        let cms_project_id = workspace_project_id(&cwd);
        let cms = VegvisirCms::open(VegvisirCmsConfig {
            db_path: data_root.join("cms-v2.sqlite3"),
            user_id: default_user_id.clone(),
            project_id: Some(cms_project_id.clone()),
            context_mode: cms_v2::ecm::ContextMode::Project,
            commit_writebacks: true,
        })?;
        let cms_config = VegvisirCmsConfig {
            db_path: data_root.join("cms-v2.sqlite3"),
            user_id: default_user_id,
            project_id: Some(cms_project_id),
            context_mode: cms_v2::ecm::ContextMode::Project,
            commit_writebacks: true,
        };
        let tool_registry = build_builtin_registry_with_cms_and_mode(
            &cwd,
            cms_config,
            dangerously_bypass_approvals_and_sandbox,
        )?;
        let mcp_servers = load_mcp_servers(&data_root)?;
        let hbse_services =
            HbseServiceRefStore::new(data_root.join("hbse-services.json")).load()?;
        let logger = EventLogger::new(Some(data_root.join("traces").join("tui.jsonl")));
        let approvals =
            crate::guardrails::ApprovalLedger::new_persisted(data_root.join("approvals.json"))
                .unwrap_or_default();
        let tool_executor = ToolExecutor {
            registry: tool_registry.clone(),
            guardrails: GuardrailEngine {
                policy: PermissionPolicy {
                    allow_risky_tools: dangerously_bypass_approvals_and_sandbox,
                    bypass_approvals_and_sandbox: dangerously_bypass_approvals_and_sandbox,
                    ..PermissionPolicy::default()
                },
                approvals,
            },
            runtime_policy: RuntimePolicy::default(),
            logger: logger.clone(),
        };
        let mut session = SessionState::new(&cwd, tools, skills);
        session.system_prompt = default_system_prompt();
        if let Some(provider) = defaults.get("current_provider").and_then(Value::as_str) {
            session.current_provider = provider.to_string();
        }
        if let Some(model) = defaults.get("current_model").and_then(Value::as_str) {
            session.current_model = model.to_string();
        }
        if let Some(prompt) = defaults.get("system_prompt").and_then(Value::as_str) {
            session.system_prompt = prompt.to_string();
        }
        let mut provider_registry = ProviderRegistry::default_catalog()?;
        set_openai_sso_auth_root(&mut provider_registry, &data_root);
        let models = ModelRegistry::default_catalog()?;
        if provider_registry.get(&session.current_provider).is_none() {
            session.current_provider = "demo".to_string();
        }
        if model_known_but_invalid(&models, &session.current_provider, &session.current_model)
            && let Some(default) = models.default_for_provider(&session.current_provider)
        {
            session.current_model = default.name.clone();
            if let Some(context_window) = default.context_window {
                session.context_limit = context_window;
            }
        }
        let input_history = session.input_history.clone();
        let mut app = Self {
            session,
            sessions,
            agents,
            config,
            commands: CommandRegistry::with_defaults(),
            provider_registry,
            models,
            cms,
            tool_registry,
            tool_executor,
            logger,
            input: InputState {
                history: input_history,
                ..InputState::default()
            },
            renderer: LayoutRenderer::default(),
            chat_scroll_offset: 0,
            cwd,
            data_root,
            running: true,
            clear_requested: false,
            redraw_requested: false,
            risky_tools_enabled: false,
            dangerously_bypass_approvals_and_sandbox,
            mcp_servers,
            hbse_services,
            pending_send: None,
            pending_background_jobs: Vec::new(),
            pending_stream: None,
            pending_cancel: None,
        };
        app.rebuild_tooling_for_cms()?;
        let provider = app.session.current_provider.clone();
        let _ = app.refresh_provider_models(&provider);
        Ok(app)
    }

    pub fn render(&mut self) -> String {
        let suggestions = self.build_suggestions();
        self.input.update_suggestions(suggestions);
        self.renderer.render_startup(
            &self.session,
            &self.commands,
            &self.input,
            &self.input.suggestions,
            self.input.selected_suggestion,
            self.chat_scroll_offset,
        )
    }

    pub fn build_suggestions(&self) -> Vec<Suggestion> {
        let raw = &self.input.buffer;
        if !raw.starts_with('/') {
            return Vec::new();
        }
        let parts = raw.split_whitespace().collect::<Vec<_>>();
        let trailing_space = raw.ends_with(' ');
        if raw.starts_with("/provider ") || raw == "/provider " {
            if trailing_space && parts.len() >= 2 {
                return Vec::new();
            }
            let prefix = if trailing_space {
                ""
            } else {
                parts.get(1).copied().unwrap_or("")
            };
            return self
                .provider_registry
                .list()
                .into_iter()
                .filter(|provider| provider.name.starts_with(prefix))
                .map(|provider| {
                    Suggestion::new(
                        provider.name.clone(),
                        provider
                            .display_name
                            .clone()
                            .unwrap_or_else(|| provider.name.clone()),
                        Some(format!("/provider {}", provider.name)),
                    )
                })
                .collect();
        }
        if raw.starts_with("/model ")
            || raw == "/model "
            || raw.starts_with("/models ")
            || raw == "/models "
        {
            if trailing_space && parts.len() >= 2 {
                return Vec::new();
            }
            let prefix = if trailing_space {
                ""
            } else {
                parts.get(1).copied().unwrap_or("")
            };
            let command = if raw.starts_with("/models") {
                "/models"
            } else {
                "/model"
            };
            return self
                .models
                .by_provider(&self.session.current_provider)
                .into_iter()
                .filter(|model| model.name.starts_with(prefix))
                .map(|model| {
                    Suggestion::new(
                        model.name.clone(),
                        format!(
                            "{} · {} ctx",
                            model.provider,
                            model
                                .context_window
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "unknown".to_string())
                        ),
                        Some(if command == "/models" {
                            format!("/models {}", model.name)
                        } else {
                            format!("/model {}", model.name)
                        }),
                    )
                })
                .collect();
        }
        self.commands
            .all()
            .into_iter()
            .filter(|command| command.name.starts_with(raw))
            .map(|command| {
                Suggestion::new(
                    command.name.clone(),
                    command.description.clone(),
                    Some(command.name.clone()),
                )
            })
            .collect()
    }

    pub fn execute_command(&mut self, raw: &str) -> anyhow::Result<Option<String>> {
        let Some((command, args)) = self.commands.parse_with_aliases(raw) else {
            return Ok(None);
        };
        self.logger.emit(
            "command_start",
            json!({
                "command": command.clone(),
                "args": args.clone(),
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
            }),
        );
        let response = match command.as_str() {
            "/new" => self.new_session(&args),
            "/sessions" => self.sessions_command()?,
            "/load" => self.load_session_command(&args)?,
            "/workspace" => self.workspace_command(&args)?,
            "/projects" => self.projects_command(&args)?,
            "/reset" => {
                self.sessions.reset(&mut self.session);
                "Conversation state reset.".to_string()
            }
            "/clear" => {
                self.clear_requested = true;
                "Screen cleared.".to_string()
            }
            "/redraw" => {
                self.redraw_requested = true;
                "Redraw requested.".to_string()
            }
            "/cancel" => self.cancel_pending_response(),
            "/history" => self.history(),
            "/save" => format!(
                "Saved session to {}",
                self.sessions.save(&self.session)?.display()
            ),
            "/retry" => self.retry()?,
            "/undo" => {
                self.sessions.undo(&mut self.session);
                "Removed last exchange.".to_string()
            }
            "/title" => {
                if !args.is_empty() {
                    self.session.title = args.join(" ");
                }
                format!("Session title: {}", self.session.title)
            }
            "/branch" | "/fork" => self.branch(&args),
            "/compress" => self.compress(&args),
            "/system" => self.system_command(&args)?,
            "/system-prompt" => self.system_command(&[])?,
            "/agent" => self.agent_command(&args)?,
            "/attach" => self.attach_command(&args)?,
            "/help" => self.help(),
            "/tools" => self.tools_command(&args),
            "/approvals" => self.approvals_command(&args),
            "/skills" => self.skills(),
            "/recall" => self.recall_command(&args)?,
            "/memory" => self.memory_command(&args)?,
            "/remember" => self.remember_command(&args)?,
            "/context" => self.context_command(&args)?,
            "/model-request" => self.model_request_command(&args)?,
            "/models" => self.models_command(&args)?,
            "/model" => self.select_model(&args)?,
            "/provider" => self.provider_command(&args)?,
            "/providers" => self.providers_command(),
            "/auth" => self.auth_command(&args),
            "/verify" => self.verify_command(&args),
            "/eval" => self.eval_command(&args)?,
            "/trace" => self.trace_command(&args)?,
            "/subagents" => self.subagents_command(&args)?,
            "/mcp" => self.mcp_command(&args)?,
            "/hbse" => self.hbse_command(&args),
            "/config" => self.config_command(&args)?,
            "/exit" => {
                self.running = false;
                "Exiting.".to_string()
            }
            _ => format!("Unknown command: {command}"),
        };
        self.logger.emit(
            "command_finish",
            json!({
                "command": command.clone(),
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
            }),
        );
        Ok(Some(response))
    }

    fn trace_command(&self, args: &[String]) -> anyhow::Result<String> {
        let json_output = args.iter().any(|arg| arg == "--json" || arg == "json");
        let limit = parse_limit(args, 20);
        let mut events = self.logger.events();
        let total = events.len();
        if total > limit {
            events = events.split_off(total - limit);
        }
        if json_output {
            return Ok(serde_json::to_string_pretty(&events)?);
        }
        if events.is_empty() {
            return Ok("No trace events recorded.".to_string());
        }
        Ok(events
            .iter()
            .map(|event| {
                format!(
                    "{} {} {}",
                    event.timestamp.to_rfc3339(),
                    event.name,
                    compact_json(&event.payload)
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn config_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("status") | Some("show") => {
                let defaults = self.config.load().unwrap_or_default();
                Ok(format!(
                    "Vegvisir configuration\npath={}\nsessions={}\ndefault_user_id={}\nactive_cms_user_id={}\nprovider={}\nmodel={}\nworkspace={}",
                    self.config.path.display(),
                    self.sessions.store.root.display(),
                    configured_user_id(&defaults),
                    self.cms.config.user_id,
                    self.session.current_provider,
                    self.session.current_model,
                    self.cwd.display()
                ))
            }
            Some("user") | Some("set-user") => {
                let Some(user_id) = args.get(1) else {
                    return Ok(format!("Default user id: {}", self.default_user_id()));
                };
                validate_user_id(user_id)?;
                let mut defaults = self.config.load().unwrap_or_default();
                defaults.insert(
                    "current_user_id".to_string(),
                    Value::String(user_id.clone()),
                );
                self.config.save(&defaults)?;
                self.autosave_session();
                self.sessions.store =
                    SessionStore::new(session_root_for_user(&self.data_root, user_id))?;
                self.sessions.cwd = self.cwd.clone();
                if self.session.active_agent_id.is_none() {
                    let previous = self.session.clone();
                    let mut config = self.cms.config.clone();
                    config.user_id = user_id.clone();
                    config.project_id = Some(workspace_project_id(&self.cwd));
                    self.cms = VegvisirCms::open(config)?;
                    self.rebuild_tooling_for_cms()?;
                    if let Some(restored) = self.session_for_workspace(&self.cwd)? {
                        self.session = restored;
                    } else {
                        let mut next = self.sessions.create(
                            workspace_title(&self.cwd),
                            previous.current_provider,
                            previous.current_model,
                            previous.enabled_tools,
                            previous.enabled_skills,
                        );
                        next.system_prompt = previous.system_prompt;
                        next.context_limit = previous.context_limit;
                        self.session = next;
                    }
                    self.session.cwd = self.cwd.display().to_string();
                    self.input.history = self.session.input_history.clone();
                    self.autosave_session();
                }
                Ok(format!("Default user id set to {user_id}."))
            }
            Some("provider") => {
                let Some(provider) = args.get(1) else {
                    return Ok("Usage: /config provider <provider>".to_string());
                };
                self.provider_command(&["--global".to_string(), provider.clone()])
            }
            Some("model") => {
                let Some(model) = args.get(1) else {
                    return Ok("Usage: /config model <model>".to_string());
                };
                self.select_model(&["--global".to_string(), model.clone()])
            }
            Some("path") => Ok(self.config.path.display().to_string()),
            Some(other) => Ok(format!("Unknown /config command: {other}")),
        }
    }

    fn eval_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.first().map(String::as_str) == Some("file") {
            let path = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("Usage: /eval file <path>"))?;
            let results = crate::evals::run_eval_file(path)?;
            return Ok(crate::evals::format_eval_results(&results));
        }
        let scope = args.first().map(String::as_str).unwrap_or("all");
        let results = crate::evals::run_builtin_evals(scope)?;
        Ok(crate::evals::format_eval_results(&results))
    }

    fn subagents_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("list") | Some("tasks") => {
                let records = self.load_subagent_records()?;
                if records.is_empty() {
                    return Ok("No subagent task records.".to_string());
                }
                Ok(records
                    .iter()
                    .map(|record| {
                        format!(
                            "{}  name={} status={:?} workspace={} goal={}",
                            record.id,
                            record.name,
                            record.status,
                            record.workspace.display(),
                            record.goal
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            Some("show") => {
                let Some(id_or_name) = args.get(1) else {
                    return Ok("Usage: /subagents show <id-or-name>".to_string());
                };
                let Some(record) = self.find_subagent_record(id_or_name)? else {
                    return Ok(format!("Unknown subagent task: {id_or_name}"));
                };
                Ok(serde_json::to_string_pretty(&record)?)
            }
            Some("cancel") => {
                let Some(id_or_name) = args.get(1) else {
                    return Ok("Usage: /subagents cancel <id-or-name>".to_string());
                };
                let mut records = self.load_subagent_records()?;
                let Some(record) = records
                    .iter_mut()
                    .find(|record| record.id == *id_or_name || record.name == *id_or_name)
                else {
                    return Ok(format!("Unknown subagent task: {id_or_name}"));
                };
                if matches!(
                    record.status,
                    SubAgentStatus::Completed | SubAgentStatus::Failed | SubAgentStatus::Cancelled
                ) {
                    return Ok(format!(
                        "Subagent task {} is already {:?}.",
                        record.id, record.status
                    ));
                }
                record.status = SubAgentStatus::Cancelled;
                record.finished_at = Some(chrono::Utc::now());
                let id = record.id.clone();
                let name = record.name.clone();
                self.save_subagent_records(&records)?;
                self.logger.emit(
                    "subagent.cancelled",
                    json!({
                        "id": id,
                        "name": name,
                        "source": "tui-command",
                    }),
                );
                Ok(format!("Cancelled subagent task {id}."))
            }
            Some(other) => Ok(format!("Unknown /subagents command: {other}")),
        }
    }

    fn subagent_board_path(&self) -> PathBuf {
        self.data_root.join("subagents.json")
    }

    fn load_subagent_records(&self) -> anyhow::Result<Vec<SubAgentTaskRecord>> {
        let path = self.subagent_board_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    fn save_subagent_records(&self, records: &[SubAgentTaskRecord]) -> anyhow::Result<()> {
        let path = self.subagent_board_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(records)?)?;
        Ok(())
    }

    fn find_subagent_record(&self, id_or_name: &str) -> anyhow::Result<Option<SubAgentTaskRecord>> {
        Ok(self
            .load_subagent_records()?
            .into_iter()
            .find(|record| record.id == id_or_name || record.name == id_or_name))
    }

    pub fn send_demo(&mut self, content: &str) -> anyhow::Result<String> {
        let mut runner = ConversationRunner {
            provider: ProviderRouter::from_registry(&self.provider_registry)
                .get(&self.session.current_provider)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("Unknown provider: {}", self.session.current_provider)
                })?,
            models: self.models.clone(),
            tools: None,
            tool_executor: None,
        };
        let envelope = self.cms.prepare_cached_prompt(
            content,
            self.session.current_provider.clone(),
            self.session.current_model.clone(),
        )?;
        let response = runner.send_with_envelope(&mut self.session, content, envelope)?;
        let _ = self.cms.complete_turn(content, &response);
        self.autosave_session();
        Ok(response)
    }

    pub fn send_headless(&mut self, content: &str) -> anyhow::Result<String> {
        let mut runner = ConversationRunner {
            provider: ProviderRouter::from_registry(&self.provider_registry)
                .get(&self.session.current_provider)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("Unknown provider: {}", self.session.current_provider)
                })?,
            models: self.models.clone(),
            tools: Some(self.tool_registry.clone()),
            tool_executor: Some(self.tool_executor.clone()),
        };
        let envelope = self.cms.prepare_cached_prompt(
            content,
            self.session.current_provider.clone(),
            self.session.current_model.clone(),
        )?;
        let response = runner.send_with_envelope(&mut self.session, content, envelope)?;
        let _ = self.cms.complete_turn(content, &response);
        self.autosave_session();
        Ok(response)
    }

    pub fn handle_submit(&mut self) {
        let raw = self.input.buffer.trim().to_string();
        if raw.is_empty() {
            self.input.clear();
            self.input.update_suggestions(Vec::new());
            return;
        }
        self.input.push_history(raw.clone());
        self.session.input_history = self.input.history.clone();
        self.input.clear();
        self.input.update_suggestions(Vec::new());

        if raw.starts_with('/') {
            if self
                .commands
                .parse_with_aliases(&raw)
                .map(|(command, _)| command)
                == Some("/select".to_string())
            {
                if let Err(error) = self.open_selection_view() {
                    self.push_system_message(format!("Command failed: {error}"));
                    self.autosave_session();
                }
                self.redraw_requested = true;
                return;
            }
            match self.execute_command(&raw) {
                Ok(Some(response)) if !response.is_empty() => {
                    self.push_system_message(response);
                    self.autosave_session();
                }
                Ok(_) => {
                    self.autosave_session();
                }
                Err(error) => {
                    self.session.status = "ready".to_string();
                    self.session.activity.clear();
                    self.push_system_message(format!("Command failed: {error}"));
                    self.autosave_session();
                }
            }
            return;
        }

        let (mut content, mut attachments) = extract_attachments(&raw, &self.cwd);
        let pending = std::mem::take(&mut self.session.pending_attachments);
        attachments = pending.into_iter().chain(attachments).collect();
        if content.trim().is_empty() && !attachments.is_empty() {
            content = "Please review the attached file(s).".to_string();
        }

        self.start_background_send(content, attachments);
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        let suggestions = self.build_suggestions();
        self.input.update_suggestions(suggestions);
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.running = false;
            }
            KeyCode::Enter => {
                if !self.input.accept_suggestion() {
                    self.handle_submit();
                }
            }
            KeyCode::Tab => {
                self.input.accept_suggestion();
            }
            KeyCode::Esc => {
                self.input.update_suggestions(Vec::new());
            }
            KeyCode::Backspace => {
                self.input.backspace();
            }
            KeyCode::Up => {
                if !self.input.move_selection(-1) {
                    let input_width = self.input_edit_width();
                    if self.input.cursor == 0 {
                        self.input.history_move(-1);
                    } else if self.input.visual_line_count(input_width) > 1 {
                        self.input.move_cursor_vertical(-1, input_width);
                    }
                }
            }
            KeyCode::Down => {
                if !self.input.move_selection(1) {
                    let input_width = self.input_edit_width();
                    if self.input.cursor == 0 {
                        self.input.history_move(1);
                    } else if self.input.visual_line_count(input_width) > 1 {
                        self.input.move_cursor_vertical(1, input_width);
                    }
                }
            }
            KeyCode::Left => {
                self.input.move_cursor(-1);
            }
            KeyCode::Right => {
                self.input.move_cursor(1);
            }
            KeyCode::PageUp => {
                self.chat_scroll_offset = self
                    .chat_scroll_offset
                    .saturating_add(self.chat_page_size());
            }
            KeyCode::PageDown => {
                self.chat_scroll_offset = self
                    .chat_scroll_offset
                    .saturating_sub(self.chat_page_size());
            }
            KeyCode::Home => {
                if self.input.buffer.is_empty() {
                    self.chat_scroll_offset = usize::MAX / 2;
                } else {
                    self.input.move_cursor_home();
                }
            }
            KeyCode::End => {
                if self.input.buffer.is_empty() {
                    self.chat_scroll_offset = 0;
                } else {
                    self.input.move_cursor_end();
                }
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.input.append_text(&ch.to_string(), false);
                self.chat_scroll_offset = 0;
            }
            _ => {}
        }
        self.redraw_requested = true;
    }

    pub fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.chat_scroll_offset = self.chat_scroll_offset.saturating_add(3);
            }
            MouseEventKind::ScrollDown => {
                self.chat_scroll_offset = self.chat_scroll_offset.saturating_sub(3);
            }
            _ => return,
        }
        self.redraw_requested = true;
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        let _terminal = TerminalGuard::enter()?;
        self.paint()?;
        while self.running {
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) => self.handle_key_event(key),
                    Event::Mouse(mouse) => self.handle_mouse_event(mouse),
                    Event::Paste(text) => {
                        self.input.append_text(&text, true);
                        self.redraw_requested = true;
                    }
                    Event::Resize(_, _) => {
                        self.redraw_requested = true;
                    }
                    _ => {}
                }
            }
            self.poll_stream_events();
            self.poll_pending_send();
            self.poll_background_jobs();
            self.pulse_activity();
            if self.clear_requested {
                self.chat_scroll_offset = 0;
                self.clear_requested = false;
            }
            if self.redraw_requested
                || self.pending_send.is_some()
                || !self.pending_background_jobs.is_empty()
            {
                self.redraw_requested = false;
                self.paint()?;
            }
        }
        Ok(())
    }

    fn paint(&mut self) -> anyhow::Result<()> {
        let mut stdout = io::stdout();
        let rendered = self.render();
        let cursor = self.input_cursor_position(&rendered);
        execute!(stdout, Hide, MoveTo(0, 0))?;
        write!(stdout, "{}", terminal_frame(&rendered))?;
        execute!(
            stdout,
            Clear(ClearType::FromCursorDown),
            MoveTo(cursor.0, cursor.1),
            Show
        )?;
        stdout.flush()?;
        Ok(())
    }

    fn input_edit_width(&self) -> usize {
        let width = self
            .renderer
            .viewport
            .map(|(width, _)| width)
            .or_else(|| {
                crossterm::terminal::size()
                    .ok()
                    .map(|(width, _)| width as usize)
            })
            .unwrap_or(80)
            .max(50);
        width.saturating_sub(2).max(1)
    }

    fn input_cursor_position(&self, rendered: &str) -> (u16, u16) {
        let input_width = self.input_edit_width();
        let visual_lines = self.input.visual_line_count(input_width);
        let visible_lines = visual_lines.min(6).max(1);
        let hidden_lines = visual_lines.saturating_sub(visible_lines);
        let autocomplete_height =
            if self.input.buffer.starts_with('/') && !self.input.suggestions.is_empty() {
                self.input.suggestions.len().min(8) + 2
            } else {
                0
            };
        let rendered_lines = rendered.lines().count();
        let input_start = rendered_lines.saturating_sub(autocomplete_height + visible_lines);
        let (line, column) = self.input.visual_cursor_position(input_width);
        let visible_line = line
            .saturating_sub(hidden_lines)
            .min(visible_lines.saturating_sub(1));
        (
            (2 + column.min(input_width)) as u16,
            (input_start + visible_line) as u16,
        )
    }

    fn push_system_message(&mut self, content: impl Into<String>) {
        self.session.messages.push(ChatMessage {
            role: "system".to_string(),
            content: content.into(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
    }

    fn autosave_session(&self) {
        let _ = self.sessions.save(&self.session);
        self.remember_workspace_session(&self.cwd, &self.session.session_id);
    }

    fn workspace_index_path(&self) -> PathBuf {
        workspace_index_path_for_user(&self.data_root, &self.default_user_id())
    }

    fn load_workspace_index(&self) -> WorkspaceIndex {
        std::fs::read_to_string(self.workspace_index_path())
            .ok()
            .and_then(|text| serde_json::from_str::<WorkspaceIndex>(&text).ok())
            .unwrap_or_default()
    }

    fn save_workspace_index(&self, index: &WorkspaceIndex) -> anyhow::Result<()> {
        if let Some(parent) = self.workspace_index_path().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            self.workspace_index_path(),
            serde_json::to_string_pretty(index)?,
        )?;
        Ok(())
    }

    fn load_workspace_session_index(&self) -> BTreeMap<String, String> {
        self.load_workspace_index().active_sessions
    }

    fn provider_selection_for_workspace(&self, workspace: &Path) -> ProviderSelection {
        let key = workspace.display().to_string();
        if let Some(selection) = self.load_workspace_index().provider_overrides.get(&key) {
            return selection.clone();
        }
        let defaults = self.config.load().unwrap_or_default();
        ProviderSelection {
            provider: defaults
                .get("current_provider")
                .and_then(Value::as_str)
                .unwrap_or("demo")
                .to_string(),
            model: defaults
                .get("current_model")
                .and_then(Value::as_str)
                .unwrap_or("demo-local")
                .to_string(),
        }
    }

    fn apply_provider_selection_for_workspace(&mut self) {
        let selection = self.provider_selection_for_workspace(&self.cwd);
        if self.provider_registry.get(&selection.provider).is_some() {
            self.session.current_provider = selection.provider;
        }
        match self.models.get(&selection.model) {
            Some(model)
                if self
                    .models
                    .is_model_allowed_for_provider(model, &self.session.current_provider) =>
            {
                self.session.current_model = selection.model;
                if let Some(context_window) = model.context_window {
                    self.session.context_limit = context_window;
                }
            }
            Some(_) => {
                if let Some(default) = self
                    .models
                    .default_for_provider(&self.session.current_provider)
                {
                    self.session.current_model = default.name.clone();
                    if let Some(context_window) = default.context_window {
                        self.session.context_limit = context_window;
                    }
                }
            }
            None if !selection.model.trim().is_empty() => {
                self.session.current_model = selection.model;
            }
            None => {
                if let Some(default) = self
                    .models
                    .default_for_provider(&self.session.current_provider)
                {
                    self.session.current_model = default.name.clone();
                    if let Some(context_window) = default.context_window {
                        self.session.context_limit = context_window;
                    }
                }
            }
        }
    }

    fn save_workspace_provider_override(&self) -> anyhow::Result<()> {
        let mut index = self.load_workspace_index();
        index.provider_overrides.insert(
            self.cwd.display().to_string(),
            ProviderSelection {
                provider: self.session.current_provider.clone(),
                model: self.session.current_model.clone(),
            },
        );
        self.save_workspace_index(&index)
    }

    fn clear_workspace_provider_override(&self) -> anyhow::Result<()> {
        let mut index = self.load_workspace_index();
        index
            .provider_overrides
            .remove(&self.cwd.display().to_string());
        self.save_workspace_index(&index)
    }

    fn remember_workspace_session(&self, workspace: &Path, session_id: &str) {
        let mut index = self.load_workspace_index();
        index
            .active_sessions
            .insert(workspace.display().to_string(), session_id.to_string());
        let _ = self.save_workspace_index(&index);
    }

    fn start_background_send(
        &mut self,
        content: String,
        attachments: Vec<crate::core::Attachment>,
    ) {
        if self.pending_send.is_some() {
            self.push_system_message("A model response is already in progress.");
            return;
        }
        let display_content = if content.trim().is_empty() && !attachments.is_empty() {
            "Please review the attached file(s).".to_string()
        } else {
            content.clone()
        };
        self.session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: display_content.clone(),
            attachments: attachments.clone(),
            created_at: chrono::Utc::now(),
        });
        self.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: String::new(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        self.session.status = "streaming".to_string();
        self.session.activity = "using CMS-v2 prepared model request".to_string();
        self.session.activity_tick = 0;
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;

        let mut worker_session = self.session.clone();
        worker_session.messages.pop();
        worker_session.messages.pop();
        worker_session.pending_attachments = attachments;
        let provider_registry = self.provider_registry.clone();
        let models = self.models.clone();
        let tool_registry = self.tool_registry.clone();
        let tool_executor = self.tool_executor.clone();
        let mut cms_config = self.cms.config.clone();
        let (stream_tx, stream_rx) = mpsc::channel();
        let cancel_token = Arc::new(AtomicBool::new(false));
        let worker_cancel_token = Arc::clone(&cancel_token);
        self.pending_stream = Some(stream_rx);
        let handle = thread::spawn(move || -> anyhow::Result<SessionState> {
            let mut cms = VegvisirCms::open({
                cms_config.commit_writebacks = true;
                cms_config
            })?;
            let mut runner = ConversationRunner {
                provider: ProviderRouter::from_registry(&provider_registry)
                    .get(&worker_session.current_provider)
                    .cloned()
                    .ok_or_else(|| {
                        anyhow::anyhow!("Unknown provider: {}", worker_session.current_provider)
                    })?,
                models,
                tools: Some(tool_registry),
                tool_executor: Some(tool_executor),
            };
            let envelope = cms.prepare_cached_prompt(
                &display_content,
                worker_session.current_provider.clone(),
                worker_session.current_model.clone(),
            )?;
            let mut on_delta = |delta: &str| {
                if !worker_cancel_token.load(Ordering::SeqCst) {
                    let _ = stream_tx.send(StreamEvent::Delta(delta.to_string()));
                }
            };
            let response = runner.send_with_envelope_streaming(
                &mut worker_session,
                &display_content,
                envelope,
                &mut on_delta,
            )?;
            if worker_cancel_token.load(Ordering::SeqCst) {
                anyhow::bail!("Cancelled");
            }
            let _ = cms.complete_turn(&display_content, &response);
            Ok(worker_session)
        });
        self.pending_send = Some(handle);
        self.pending_cancel = Some(cancel_token);
    }

    pub fn poll_pending_send(&mut self) -> bool {
        let Some(handle) = self.pending_send.take() else {
            return false;
        };
        if !handle.is_finished() {
            self.pending_send = Some(handle);
            return false;
        }
        match handle.join() {
            Ok(Ok(session)) => {
                self.session = session;
                self.pending_stream = None;
                self.pending_cancel = None;
                self.autosave_session();
            }
            Ok(Err(error)) => {
                self.session.status = "ready".to_string();
                self.session.activity.clear();
                self.pending_stream = None;
                self.pending_cancel = None;
                self.pop_empty_assistant_placeholder();
                if error.to_string() == "Cancelled" {
                    self.pop_last_assistant_response();
                    self.push_system_message("Cancelled in-flight model response.");
                } else {
                    self.push_system_message(format!("Error: {error}"));
                }
                self.autosave_session();
            }
            Err(_) => {
                self.session.status = "ready".to_string();
                self.session.activity.clear();
                self.pending_stream = None;
                self.pending_cancel = None;
                self.pop_empty_assistant_placeholder();
                self.push_system_message("Error: provider worker panicked.");
                self.autosave_session();
            }
        }
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;
        true
    }

    pub fn poll_background_jobs(&mut self) -> bool {
        let mut changed = false;
        let mut index = 0usize;
        while index < self.pending_background_jobs.len() {
            if !self.pending_background_jobs[index].is_finished() {
                index += 1;
                continue;
            }
            let handle = self.pending_background_jobs.remove(index);
            match handle.join() {
                Ok(Ok(message)) => self.push_system_message(message),
                Ok(Err(error)) => self.push_system_message(format!("Error: {error}")),
                Err(_) => self.push_system_message("Error: background job panicked."),
            }
            changed = true;
        }
        if changed {
            self.autosave_session();
            self.chat_scroll_offset = 0;
            self.redraw_requested = true;
        }
        changed
    }

    fn cancel_pending_response(&mut self) -> String {
        let Some(handle) = self.pending_send.take() else {
            return "No in-flight model response to cancel.".to_string();
        };
        if let Some(cancel_token) = &self.pending_cancel {
            cancel_token.store(true, Ordering::SeqCst);
        }
        drop(handle);
        self.pending_stream = None;
        self.pending_cancel = None;
        self.session.status = "ready".to_string();
        self.session.activity.clear();
        self.pop_last_assistant_response();
        self.push_system_message("Cancelled in-flight model response.");
        self.autosave_session();
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;
        self.logger.emit(
            "provider_cancelled",
            json!({
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
            }),
        );
        "Cancelled in-flight model response.".to_string()
    }

    fn poll_stream_events(&mut self) {
        let mut deltas = Vec::new();
        if let Some(receiver) = &self.pending_stream {
            while let Ok(event) = receiver.try_recv() {
                match event {
                    StreamEvent::Delta(delta) => deltas.push(delta),
                }
            }
        }
        if deltas.is_empty() {
            return;
        }
        let assistant_index = self
            .session
            .messages
            .iter()
            .rposition(|message| message.role == "assistant")
            .unwrap_or_else(|| {
                self.session.messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: String::new(),
                    attachments: Vec::new(),
                    created_at: chrono::Utc::now(),
                });
                self.session.messages.len() - 1
            });
        for delta in deltas {
            self.session.messages[assistant_index]
                .content
                .push_str(&delta);
        }
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;
    }

    fn pop_empty_assistant_placeholder(&mut self) {
        if self
            .session
            .messages
            .last()
            .map(|message| message.role == "assistant" && message.content.is_empty())
            .unwrap_or(false)
        {
            self.session.messages.pop();
        }
    }

    fn pop_last_assistant_response(&mut self) {
        if self
            .session
            .messages
            .last()
            .map(|message| message.role == "assistant")
            .unwrap_or(false)
        {
            self.session.messages.pop();
        }
    }

    fn chat_page_size(&self) -> usize {
        self.renderer
            .viewport
            .map(|(_, lines)| lines / 2)
            .or_else(|| {
                crossterm::terminal::size()
                    .ok()
                    .map(|(_, lines)| usize::from(lines) / 2)
            })
            .unwrap_or(16)
            .max(5)
    }

    fn pulse_activity(&mut self) {
        if self.session.status != "streaming" {
            return;
        }
        self.session.activity_tick = self.session.activity_tick.saturating_add(1);
        self.redraw_requested = true;
    }

    fn new_session(&mut self, args: &[String]) -> String {
        self.autosave_session();
        let title = join_or(args, "untitled");
        let mut next = self.sessions.create(
            title,
            self.session.current_provider.clone(),
            self.session.current_model.clone(),
            self.session.enabled_tools.clone(),
            self.session.enabled_skills.clone(),
        );
        next.system_prompt = self.session.system_prompt.clone();
        next.active_agent_id = self.session.active_agent_id.clone();
        next.active_agent_name = self.session.active_agent_name.clone();
        next.cwd = self.cwd.display().to_string();
        next.context_limit = self.session.context_limit;
        self.session = next;
        let _ = self.apply_session_workspace_state();
        format!("Started new session {}", self.session.session_id)
    }

    fn sessions_command(&self) -> anyhow::Result<String> {
        let sessions = self.sessions.list()?;
        if sessions.is_empty() {
            return Ok("No saved sessions.".to_string());
        }
        Ok(sessions
            .iter()
            .map(|session| {
                format!(
                    "{}  {}  messages={}  title={}  cwd={}",
                    session.session_id,
                    session.created_at.format("%Y-%m-%d %H:%M:%S"),
                    session.messages.len(),
                    session.title,
                    session.cwd
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn load_session_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(session_id) = args.first() else {
            return Ok("Usage: /load <session-id>".to_string());
        };
        self.autosave_session();
        let loaded = self.sessions.load(session_id)?;
        let loaded_cwd = PathBuf::from(&loaded.cwd);
        self.session = loaded;
        if loaded_cwd.exists() && loaded_cwd.is_dir() {
            self.set_workspace_root(loaded_cwd)?;
        }
        self.apply_session_workspace_state()?;
        self.input.history = self.session.input_history.clone();
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;
        Ok(format!(
            "Loaded session {} with {} message(s).",
            self.session.session_id,
            self.session.messages.len()
        ))
    }

    fn projects_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("list") => self.projects_list_command(),
            Some("name") | Some("alias") => self.projects_name_command(args),
            Some("forget") | Some("remove") => self.projects_forget_command(args),
            Some("use") | Some("switch") => {
                let Some(path) = args.get(1..) else {
                    return Ok("Usage: /projects use <path>".to_string());
                };
                if path.is_empty() {
                    return Ok("Usage: /projects use <path>".to_string());
                }
                self.switch_workspace_or_alias(&path.join(" "))
            }
            Some(_) => self.switch_workspace_or_alias(&args.join(" ")),
        }
    }

    fn projects_list_command(&self) -> anyhow::Result<String> {
        let mut projects = BTreeMap::<String, ProjectListEntry>::new();
        for session in self.sessions.list()? {
            let entry = projects
                .entry(session.cwd.clone())
                .or_insert_with(|| ProjectListEntry {
                    cwd: session.cwd.clone(),
                    latest_session_id: session.session_id.clone(),
                    latest_created_at: session.created_at,
                    session_count: 0,
                    message_count: 0,
                });
            entry.session_count += 1;
            entry.message_count += session.messages.len();
            if session.created_at > entry.latest_created_at {
                entry.latest_created_at = session.created_at;
                entry.latest_session_id = session.session_id.clone();
            }
        }
        let active = self.cwd.display().to_string();
        projects
            .entry(active.clone())
            .and_modify(|entry| {
                entry.latest_session_id = self.session.session_id.clone();
                entry.latest_created_at = self.session.created_at;
            })
            .or_insert_with(|| ProjectListEntry {
                cwd: active.clone(),
                latest_session_id: self.session.session_id.clone(),
                latest_created_at: self.session.created_at,
                session_count: 1,
                message_count: self.session.messages.len(),
            });
        if projects.is_empty() {
            return Ok("No project workspaces are known yet.".to_string());
        }
        let index = self.load_workspace_index();
        let alias_by_path = index
            .aliases
            .iter()
            .map(|(alias, path)| (path.clone(), alias.clone()))
            .collect::<BTreeMap<_, _>>();
        Ok(projects
            .values()
            .map(|project| {
                let marker = if project.cwd == active { "*" } else { " " };
                let remembered = if project.cwd == active {
                    self.session.session_id.clone()
                } else {
                    index
                        .active_sessions
                        .get(&project.cwd)
                        .cloned()
                        .unwrap_or_else(|| project.latest_session_id.clone())
                };
                let alias = alias_by_path
                    .get(&project.cwd)
                    .map(|alias| format!(" alias={alias}"))
                    .unwrap_or_default();
                format!(
                    "{marker} {}{}  sessions={} messages={} last_session={} path={}",
                    workspace_title(Path::new(&project.cwd)),
                    alias,
                    project.session_count,
                    project.message_count,
                    remembered,
                    project.cwd
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn projects_name_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(alias) = args.get(1) else {
            return Ok("Usage: /projects name <alias> [path]".to_string());
        };
        let alias = crate::core::normalize_agent_id(alias);
        if alias.is_empty() {
            return Ok("Project alias must contain at least one letter or number.".to_string());
        }
        let requested = if args.len() > 2 {
            args[2..].join(" ")
        } else {
            self.cwd.display().to_string()
        };
        let path = self.resolve_project_path(&requested)?;
        let mut index = self.load_workspace_index();
        index
            .aliases
            .insert(alias.clone(), path.display().to_string());
        self.save_workspace_index(&index)?;
        Ok(format!("Project alias {alias} -> {}.", path.display()))
    }

    fn projects_forget_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(alias) = args.get(1) else {
            return Ok("Usage: /projects forget <alias>".to_string());
        };
        let alias = crate::core::normalize_agent_id(alias);
        let mut index = self.load_workspace_index();
        if index.aliases.remove(&alias).is_none() {
            return Ok(format!("Unknown project alias: {alias}"));
        }
        self.save_workspace_index(&index)?;
        Ok(format!("Forgot project alias {alias}."))
    }

    fn workspace_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok(format!("Workspace: {}", self.cwd.display()));
        }
        let path = self.resolve_project_path(&args.join(" "))?;
        self.switch_workspace(path)
    }

    fn switch_workspace_or_alias(&mut self, raw: &str) -> anyhow::Result<String> {
        let alias = crate::core::normalize_agent_id(raw);
        if let Some(path) = self.load_workspace_index().aliases.get(&alias) {
            return self.switch_workspace(path);
        }
        let path = self.resolve_project_path(raw)?;
        self.switch_workspace(path)
    }

    fn resolve_project_path(&self, raw: &str) -> anyhow::Result<PathBuf> {
        let requested = expand_workspace_path(raw);
        let path = if requested.is_absolute() {
            requested
        } else {
            self.cwd.join(requested)
        };
        canonical_workspace(&path)
    }

    fn switch_workspace(&mut self, path: impl AsRef<Path>) -> anyhow::Result<String> {
        self.autosave_session();
        let target = canonical_workspace(path.as_ref())?;
        let restored = self.session_for_workspace(&target)?;
        let restored_id = restored.as_ref().map(|session| session.session_id.clone());
        self.set_workspace_root(&target)?;
        if let Some(session) = restored {
            self.session = session;
        } else {
            self.session = self.sessions.create(
                workspace_title(&target),
                self.provider_selection_for_workspace(&target).provider,
                self.provider_selection_for_workspace(&target).model,
                self.session.enabled_tools.clone(),
                Vec::new(),
            );
            self.session.system_prompt = self
                .config
                .load()
                .ok()
                .and_then(|defaults| {
                    defaults
                        .get("system_prompt")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(default_system_prompt);
        }
        self.session.cwd = self.cwd.display().to_string();
        if self.session.active_agent_id.is_none() {
            self.apply_provider_selection_for_workspace();
        }
        self.apply_session_workspace_state()?;
        self.input.history = self.session.input_history.clone();
        self.chat_scroll_offset = 0;
        self.redraw_requested = true;
        self.autosave_session();
        Ok(match restored_id {
            Some(session_id) => format!(
                "Workspace set to {}. Restored session {}.",
                self.cwd.display(),
                session_id
            ),
            None => format!(
                "Workspace set to {}. Started new project session {}.",
                self.cwd.display(),
                self.session.session_id
            ),
        })
    }

    fn set_workspace_root(&mut self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = canonical_workspace(path.as_ref())?;
        self.cwd = path.clone();
        self.sessions.cwd = path;
        Ok(())
    }

    fn apply_session_workspace_state(&mut self) -> anyhow::Result<()> {
        self.session.cwd = self.cwd.display().to_string();
        self.session.enabled_skills = load_skill_definitions(&self.cwd, &self.data_root)?;
        if let Some(agent_id) = self.session.active_agent_id.clone()
            && let Ok(profile) = self.agents.load(&agent_id)
        {
            self.apply_agent_profile(&profile)?;
            return Ok(());
        }
        self.apply_provider_selection_for_workspace();
        let mut config = self.cms.config.clone();
        config.user_id = self.default_user_id();
        config.project_id = Some(workspace_project_id(&self.cwd));
        self.cms = VegvisirCms::open(config)?;
        self.tool_executor.runtime_policy = RuntimePolicy::default();
        self.rebuild_tooling_for_cms()?;
        Ok(())
    }

    fn latest_session_for_workspace(
        &self,
        workspace: &Path,
    ) -> anyhow::Result<Option<SessionState>> {
        let target = workspace.display().to_string();
        Ok(self
            .sessions
            .list()?
            .into_iter()
            .find(|session| session.cwd == target))
    }

    fn session_for_workspace(&self, workspace: &Path) -> anyhow::Result<Option<SessionState>> {
        let target = workspace.display().to_string();
        if let Some(session_id) = self.load_workspace_session_index().get(&target)
            && let Ok(session) = self.sessions.load(session_id)
            && session.cwd == target
        {
            return Ok(Some(session));
        }
        self.latest_session_for_workspace(workspace)
    }

    fn history(&self) -> String {
        self.transcript_text()
    }

    fn transcript_text(&self) -> String {
        if self.session.messages.is_empty() {
            return "No conversation history.".to_string();
        }
        self.session
            .messages
            .iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn open_selection_view(&mut self) -> anyhow::Result<()> {
        if !io::stdout().is_terminal() {
            self.push_system_message(self.transcript_text());
            return Ok(());
        }
        disable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            Show,
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
            Clear(ClearType::All),
            MoveTo(0, 0)
        )?;
        writeln!(
            stdout,
            "Vegvisir transcript selection view\nSession: {}\nWorkspace: {}\n\n{}\n\n-- Select/copy with your terminal. Press Enter to return to Vegvisir. --",
            self.session.session_id,
            self.cwd.display(),
            self.transcript_text()
        )?;
        stdout.flush()?;
        let mut line = String::new();
        let _ = io::stdin().read_line(&mut line);
        enable_raw_mode()?;
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableMouseCapture
        )?;
        stdout.flush()?;
        self.redraw_requested = true;
        Ok(())
    }

    fn retry(&mut self) -> anyhow::Result<String> {
        let Some(last_user) = self
            .session
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.clone())
        else {
            return Ok("No user message to retry.".to_string());
        };
        self.send_demo(&last_user)
    }

    fn branch(&mut self, args: &[String]) -> String {
        let old_id = self.session.session_id.clone();
        let mut next = self.sessions.create(
            join_or(args, &format!("{}-branch", self.session.title)),
            self.session.current_provider.clone(),
            self.session.current_model.clone(),
            self.session.enabled_tools.clone(),
            self.session.enabled_skills.clone(),
        );
        next.messages = self.session.messages.clone();
        next.system_prompt = self.session.system_prompt.clone();
        next.active_agent_id = self.session.active_agent_id.clone();
        next.active_agent_name = self.session.active_agent_name.clone();
        next.context_limit = self.session.context_limit;
        self.session = next;
        format!("Branched {old_id} into {}", self.session.session_id)
    }

    fn compress(&mut self, args: &[String]) -> String {
        let topic = join_or(args, "conversation");
        let summary = format!(
            "Compressed context for {topic}: {} messages retained as summary.",
            self.session.messages.len()
        );
        self.session.messages = vec![ChatMessage {
            role: "system".to_string(),
            content: summary.clone(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        }];
        summary
    }

    fn system_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() || matches!(args[0].as_str(), "show" | "print" | "view") {
            return Ok(if self.session.system_prompt.is_empty() {
                "No harness system prompt is set.".to_string()
            } else {
                self.session.system_prompt.clone()
            });
        }
        match args[0].as_str() {
            "default" | "reset-default" => {
                self.session.system_prompt = default_system_prompt();
                self.save_config_defaults()?;
                return Ok("Harness system prompt reset to the Vegvisir default.".to_string());
            }
            "clear" => {
                self.session.system_prompt.clear();
                self.save_config_defaults()?;
                return Ok("Harness system prompt cleared.".to_string());
            }
            "append" => {
                let content = args.get(1).cloned().unwrap_or_default();
                if content.is_empty() {
                    return Ok("Usage: /system append <text>".to_string());
                }
                if !self.session.system_prompt.is_empty() {
                    self.session.system_prompt.push('\n');
                }
                self.session.system_prompt.push_str(&content);
                self.save_config_defaults()?;
                return Ok("Appended to harness system prompt.".to_string());
            }
            "set" => {
                let content = args.get(1).cloned().unwrap_or_default();
                if content.is_empty() {
                    return Ok("Usage: /system set <text>".to_string());
                }
                self.session.system_prompt = content;
            }
            other => self.session.system_prompt = other.to_string(),
        }
        self.save_config_defaults()?;
        Ok("Harness system prompt updated.".to_string())
    }

    fn agent_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("list") => {
                let profiles = self.agents.list()?;
                if profiles.is_empty() {
                    return Ok("No custom agents are defined. Use /agent create <id> | <mode> | <display name> | <system prompt>.".to_string());
                }
                Ok(profiles
                    .into_iter()
                    .map(|profile| {
                        let active = if self.session.active_agent_id.as_deref() == Some(&profile.id)
                        {
                            "*"
                        } else {
                            " "
                        };
                        format!(
                            "{active} {:<20} mode={:<14} {} cms={}/{}",
                            profile.id,
                            profile.mode,
                            profile.display_name,
                            profile.cms_user_id,
                            profile.cms_project_id
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            Some("templates") | Some("modes") => Ok(agent_templates()
                .into_iter()
                .map(|template| {
                    format!(
                        "{:<14} {:<22} tools={}",
                        template.mode,
                        template.display_name,
                        template.enabled_tools.join(",")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")),
            Some("design") | Some("designer") => self.agent_design_command(args),
            Some("create-template") | Some("from-template") => {
                let Some(mode) = args.get(1) else {
                    return Ok(
                        "Usage: /agent create-template <mode> <id> [display name]".to_string()
                    );
                };
                let Some(id) = args.get(2) else {
                    return Ok(
                        "Usage: /agent create-template <mode> <id> [display name]".to_string()
                    );
                };
                let Some(template) = agent_template(mode) else {
                    return Ok(format!(
                        "Unknown agent template: {mode}\nAvailable templates:\n{}",
                        agent_templates()
                            .into_iter()
                            .map(|template| template.mode)
                            .collect::<Vec<_>>()
                            .join("\n")
                    ));
                };
                let display_name = if args.len() > 3 {
                    args[3..].join(" ")
                } else {
                    template.display_name.clone()
                };
                let mut profile = AgentProfile::new(id, &display_name, &template.system_prompt)?;
                profile.mode = template.mode.clone();
                profile.description = template.description.clone();
                profile.enabled_tools = template.enabled_tools.clone();
                profile.enabled_skills = template.enabled_skills.clone();
                profile.usrl_contracts = template.usrl_contracts.clone();
                profile.memory_policy = template.memory_policy.clone();
                profile
                    .metadata
                    .insert("template".to_string(), Value::String(template.mode.clone()));
                let path = self.agents.save(&profile)?;
                Ok(format!(
                    "Created agent {} from template {} at {}",
                    profile.id,
                    template.mode,
                    path.display()
                ))
            }
            Some("create") | Some("new") => {
                let raw = args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
                let parts = raw.split('|').map(str::trim).collect::<Vec<_>>();
                if parts.len() < 3 || parts.iter().any(|part| part.is_empty()) {
                    return Ok(
                        "Usage: /agent create <id> | <mode> | <display name> | <system prompt>"
                            .to_string(),
                    );
                }
                let mut profile = if parts.len() >= 4 {
                    let mut profile =
                        AgentProfile::new(parts[0], parts[2], parts[3..].join(" | "))?;
                    profile.mode = parts[1].to_string();
                    profile
                } else {
                    AgentProfile::new(parts[0], parts[1], parts[2..].join(" | "))?
                };
                if profile.mode.trim().is_empty() {
                    profile.mode = "custom".to_string();
                }
                let path = self.agents.save(&profile)?;
                Ok(format!(
                    "Created agent {} ({}, mode={}) at {}",
                    profile.id,
                    profile.display_name,
                    profile.mode,
                    path.display()
                ))
            }
            Some("use") | Some("select") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent use <id>".to_string());
                };
                let profile = self.agents.load(id)?;
                self.apply_agent_profile(&profile)
            }
            Some("clone") => {
                let Some(source_id) = args.get(1) else {
                    return Ok(
                        "Usage: /agent clone <source-id> <new-id> [display name]".to_string()
                    );
                };
                let Some(new_id) = args.get(2) else {
                    return Ok(
                        "Usage: /agent clone <source-id> <new-id> [display name]".to_string()
                    );
                };
                let mut profile = self.agents.load(source_id)?;
                let normalized = crate::core::normalize_agent_id(new_id);
                if normalized.is_empty() {
                    return Ok(
                        "New agent id must contain at least one letter or number.".to_string()
                    );
                }
                profile.id = normalized;
                if args.len() > 3 {
                    profile.display_name = args[3..].join(" ");
                }
                let cms_scope = format!("agent:{}", profile.id);
                profile.cms_user_id = cms_scope.clone();
                profile.cms_project_id = cms_scope;
                profile.created_at = chrono::Utc::now();
                profile.updated_at = profile.created_at;
                let path = self.agents.save(&profile)?;
                Ok(format!(
                    "Cloned agent {source_id} to {} at {}",
                    profile.id,
                    path.display()
                ))
            }
            Some("export") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent export <id> [path]".to_string());
                };
                let profile = self.agents.load(id)?;
                if let Some(path) = args.get(2) {
                    let path = self.resolve_workspace_path(path);
                    std::fs::write(&path, serde_json::to_string_pretty(&profile)?)?;
                    Ok(format!(
                        "Exported agent {} to {}",
                        profile.id,
                        path.display()
                    ))
                } else {
                    Ok(serde_json::to_string_pretty(&profile)?)
                }
            }
            Some("import") => {
                let Some(path) = args.get(1) else {
                    return Ok("Usage: /agent import <path>".to_string());
                };
                let path = self.resolve_workspace_path(path);
                let mut profile: AgentProfile =
                    serde_json::from_str(&std::fs::read_to_string(&path)?)?;
                profile.id = crate::core::normalize_agent_id(&profile.id);
                if profile.id.is_empty() {
                    return Ok(
                        "Imported agent id must contain at least one letter or number.".to_string(),
                    );
                }
                profile.updated_at = chrono::Utc::now();
                let saved = self.agents.save(&profile)?;
                Ok(format!(
                    "Imported agent {} to {}",
                    profile.id,
                    saved.display()
                ))
            }
            Some("show") | Some("view") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent show <id>".to_string());
                };
                let profile = self.agents.load(id)?;
                Ok(format!(
                    "agent: {}\nmode: {}\nname: {}\ndescription: {}\ncms_user_id: {}\ncms_project_id: {}\nprovider: {}\nmodel: {}\ntools: {}\nskills: {}\nmcp_servers: {}\nusrl_contracts: {}\nsystem_prompt:\n{}",
                    profile.id,
                    profile.mode,
                    profile.display_name,
                    if profile.description.is_empty() {
                        "-"
                    } else {
                        &profile.description
                    },
                    profile.cms_user_id,
                    profile.cms_project_id,
                    profile.current_provider.as_deref().unwrap_or("-"),
                    profile.current_model.as_deref().unwrap_or("-"),
                    list_or_dash(&profile.enabled_tools),
                    list_or_dash(&profile.enabled_skills),
                    list_or_dash(&profile.enabled_mcp_servers),
                    list_or_dash(&profile.usrl_contracts),
                    profile.system_prompt
                ))
            }
            Some("mode") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent mode <id> <mode>".to_string());
                };
                let Some(mode) = args.get(2) else {
                    return Ok("Usage: /agent mode <id> <mode>".to_string());
                };
                let mut profile = self.agents.load(id)?;
                profile.mode = mode.to_string();
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Set agent {} mode to {}.",
                    profile.id, profile.mode
                ))
            }
            Some("provider") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent provider <id> <provider|->".to_string());
                };
                let Some(provider) = args.get(2) else {
                    return Ok("Usage: /agent provider <id> <provider|->".to_string());
                };
                let mut profile = self.agents.load(id)?;
                if provider == "-" || provider == "clear" {
                    profile.current_provider = None;
                    profile.current_model = None;
                } else {
                    let Some(config) = self.provider_registry.get(provider) else {
                        return Ok(format!("Unknown provider: {provider}"));
                    };
                    profile.current_provider = Some(config.name.clone());
                    if profile
                        .current_model
                        .as_deref()
                        .and_then(|model| self.models.get(model))
                        .filter(|model| self.models.is_model_allowed_for_provider(model, provider))
                        .is_none()
                    {
                        profile.current_model = self
                            .models
                            .default_for_provider(provider)
                            .map(|model| model.name.clone());
                    }
                }
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Set agent {} provider to {}.",
                    profile.id,
                    profile.current_provider.as_deref().unwrap_or("-")
                ))
            }
            Some("model") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent model <id> <model|->".to_string());
                };
                let Some(model) = args.get(2) else {
                    return Ok("Usage: /agent model <id> <model|->".to_string());
                };
                let mut profile = self.agents.load(id)?;
                if model == "-" || model == "clear" {
                    profile.current_model = None;
                } else {
                    let Some(model_info) = self.models.get(model) else {
                        return Ok(format!("Unknown model: {model}"));
                    };
                    let provider = profile
                        .current_provider
                        .as_deref()
                        .unwrap_or(&self.session.current_provider);
                    if !self
                        .models
                        .is_model_allowed_for_provider(model_info, provider)
                    {
                        return Ok(format!(
                            "Model {} is not available for agent provider {}. Set /agent provider {} {} first.",
                            model_info.name, provider, profile.id, model_info.provider
                        ));
                    }
                    profile.current_model = Some(model_info.name.clone());
                    if profile.current_provider.is_none() {
                        profile.current_provider = Some(provider.to_string());
                    }
                }
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Set agent {} model to {}.",
                    profile.id,
                    profile.current_model.as_deref().unwrap_or("-")
                ))
            }
            Some("prompt") | Some("system") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent prompt <id> <system prompt>".to_string());
                };
                if args.len() < 3 {
                    return Ok("Usage: /agent prompt <id> <system prompt>".to_string());
                }
                let mut profile = self.agents.load(id)?;
                profile.system_prompt = args[2..].join(" ");
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!("Updated agent {} system prompt.", profile.id))
            }
            Some("describe") | Some("description") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent describe <id> <description>".to_string());
                };
                if args.len() < 3 {
                    return Ok("Usage: /agent describe <id> <description>".to_string());
                }
                let mut profile = self.agents.load(id)?;
                profile.description = args[2..].join(" ");
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!("Updated agent {} description.", profile.id))
            }
            Some("bind-usrl") | Some("usrl") => {
                let Some(id) = args.get(1) else {
                    return Ok(
                        "Usage: /agent bind-usrl <id> <contract-id-or-skill-name>".to_string()
                    );
                };
                let Some(contract) = args.get(2) else {
                    return Ok(
                        "Usage: /agent bind-usrl <id> <contract-id-or-skill-name>".to_string()
                    );
                };
                let mut profile = self.agents.load(id)?;
                let contracts = self.resolve_usrl_contract_refs(contract);
                for resolved in &contracts {
                    if !profile.usrl_contracts.contains(resolved) {
                        profile.usrl_contracts.push(resolved.clone());
                    }
                }
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Bound USRL contract(s) {} to agent {}.",
                    contracts.join(","),
                    profile.id
                ))
            }
            Some("unbind-usrl") => {
                let Some(id) = args.get(1) else {
                    return Ok(
                        "Usage: /agent unbind-usrl <id> <contract-id-or-skill-name>".to_string()
                    );
                };
                let Some(contract) = args.get(2) else {
                    return Ok(
                        "Usage: /agent unbind-usrl <id> <contract-id-or-skill-name>".to_string()
                    );
                };
                let mut profile = self.agents.load(id)?;
                let contracts = self.resolve_usrl_contract_refs(contract);
                profile
                    .usrl_contracts
                    .retain(|item| !contracts.iter().any(|contract| contract == item));
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Unbound USRL contract(s) {} from agent {}.",
                    contracts.join(","),
                    profile.id
                ))
            }
            Some("allow-mcp") | Some("mcp") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent allow-mcp <id> <server-id>".to_string());
                };
                let Some(server) = args.get(2) else {
                    return Ok("Usage: /agent allow-mcp <id> <server-id>".to_string());
                };
                let mut profile = self.agents.load(id)?;
                if !profile.enabled_mcp_servers.contains(server) {
                    profile.enabled_mcp_servers.push(server.to_string());
                }
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Allowed MCP server {server} for agent {}.",
                    profile.id
                ))
            }
            Some("revoke-mcp") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent revoke-mcp <id> <server-id>".to_string());
                };
                let Some(server) = args.get(2) else {
                    return Ok("Usage: /agent revoke-mcp <id> <server-id>".to_string());
                };
                let mut profile = self.agents.load(id)?;
                profile.enabled_mcp_servers.retain(|item| item != server);
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Revoked MCP server {server} from agent {}.",
                    profile.id
                ))
            }
            Some("enable-skill") | Some("skill") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent enable-skill <id> <skill-name>".to_string());
                };
                let Some(skill) = args.get(2) else {
                    return Ok("Usage: /agent enable-skill <id> <skill-name>".to_string());
                };
                if !self
                    .session
                    .enabled_skills
                    .iter()
                    .any(|item| item.name == *skill)
                {
                    return Ok(format!("Unknown skill: {skill}"));
                }
                let mut profile = self.agents.load(id)?;
                if !profile.enabled_skills.contains(skill) {
                    profile.enabled_skills.push(skill.to_string());
                }
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!("Enabled skill {skill} for agent {}.", profile.id))
            }
            Some("disable-skill") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent disable-skill <id> <skill-name>".to_string());
                };
                let Some(skill) = args.get(2) else {
                    return Ok("Usage: /agent disable-skill <id> <skill-name>".to_string());
                };
                let mut profile = self.agents.load(id)?;
                profile.enabled_skills.retain(|item| item != skill);
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!("Disabled skill {skill} for agent {}.", profile.id))
            }
            Some("allow-tool") | Some("tool") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent allow-tool <id> <tool-name>".to_string());
                };
                let Some(tool) = args.get(2) else {
                    return Ok("Usage: /agent allow-tool <id> <tool-name>".to_string());
                };
                if tool != "*" && self.tool_registry.get(tool).is_err() {
                    return Ok(format!("Unknown tool: {tool}"));
                }
                let mut profile = self.agents.load(id)?;
                if !profile.enabled_tools.contains(tool) {
                    profile.enabled_tools.push(tool.to_string());
                }
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!("Allowed tool {tool} for agent {}.", profile.id))
            }
            Some("revoke-tool") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent revoke-tool <id> <tool-name>".to_string());
                };
                let Some(tool) = args.get(2) else {
                    return Ok("Usage: /agent revoke-tool <id> <tool-name>".to_string());
                };
                let mut profile = self.agents.load(id)?;
                profile.enabled_tools.retain(|item| item != tool);
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!("Revoked tool {tool} from agent {}.", profile.id))
            }
            Some("clear") | Some("default") => {
                self.clear_active_agent()?;
                Ok("Custom agent cleared. Using default Vegvisir memory scope.".to_string())
            }
            Some("delete") | Some("remove") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent delete <id>".to_string());
                };
                let path = self.agents.delete(id)?;
                if self.session.active_agent_id.as_deref() == Some(id) {
                    self.clear_active_agent()?;
                }
                Ok(format!("Deleted agent {id} at {}", path.display()))
            }
            Some(other) => {
                let profile = self.agents.load(other)?;
                self.apply_agent_profile(&profile)
            }
        }
    }

    fn agent_design_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let raw = args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
        let parts = raw.split('|').map(str::trim).collect::<Vec<_>>();
        if parts.len() < 4 || parts[..4].iter().any(|part| part.is_empty()) {
            return Ok("Usage: /agent design <id> | <mode> | <display name> | <system prompt> | tools=a,b skills=s,b mcp=server usrl=contract provider=id model=id use=true".to_string());
        }
        let id = parts[0];
        let mode = crate::core::normalize_agent_id(parts[1]);
        if mode.is_empty() {
            return Ok("Agent mode must contain at least one letter or number.".to_string());
        }
        let display_name = parts[2];
        let system_prompt = parts[3];
        let mut profile = AgentProfile::new(id, display_name, system_prompt)?;
        profile.mode = mode.clone();
        profile.description = format!("Designed {} agent.", mode);
        if let Some(template) = agent_template(&mode) {
            profile.description = template.description;
            profile.enabled_tools = template.enabled_tools;
            profile.enabled_skills = template.enabled_skills;
            profile.usrl_contracts = template.usrl_contracts;
            profile.memory_policy = template.memory_policy;
            profile
                .metadata
                .insert("template".to_string(), Value::String(template.mode));
        }

        let options = parts
            .get(4..)
            .map(|items| items.join(" "))
            .unwrap_or_default();
        let mut activate = false;
        for token in options.split_whitespace() {
            if matches!(token, "use" | "activate" | "use=true" | "activate=true") {
                activate = true;
                continue;
            }
            let Some((key, value)) = token.split_once('=') else {
                continue;
            };
            match key {
                "description" | "desc" => {
                    profile.description = value.replace('_', " ");
                }
                "tools" => {
                    let tools = comma_items(value);
                    for tool in &tools {
                        if tool != "*" && self.tool_registry.get(tool).is_err() {
                            return Ok(format!("Unknown tool for designed agent: {tool}"));
                        }
                    }
                    profile.enabled_tools = tools;
                }
                "skills" => {
                    let skills = comma_items(value);
                    for skill in &skills {
                        if !self
                            .session
                            .enabled_skills
                            .iter()
                            .any(|item| item.name == *skill)
                        {
                            return Ok(format!("Unknown skill for designed agent: {skill}"));
                        }
                    }
                    profile.enabled_skills = skills;
                }
                "mcp" | "mcp_servers" => {
                    let servers = comma_items(value);
                    for server in &servers {
                        if !self.mcp_servers.iter().any(|item| item.id == *server) {
                            return Ok(format!("Unknown MCP server for designed agent: {server}"));
                        }
                    }
                    profile.enabled_mcp_servers = servers;
                }
                "usrl" | "contracts" => {
                    let mut contracts = Vec::new();
                    for item in comma_items(value) {
                        for resolved in self.resolve_usrl_contract_refs(&item) {
                            if !contracts.contains(&resolved) {
                                contracts.push(resolved);
                            }
                        }
                    }
                    profile.usrl_contracts = contracts;
                }
                "provider" => {
                    if value != "-" && self.provider_registry.get(value).is_none() {
                        return Ok(format!("Unknown provider for designed agent: {value}"));
                    }
                    profile.current_provider = (value != "-").then(|| value.to_string());
                }
                "model" => {
                    if value != "-" && self.models.get(value).is_none() {
                        return Ok(format!("Unknown model for designed agent: {value}"));
                    }
                    profile.current_model = (value != "-").then(|| value.to_string());
                }
                "memory" | "memory_policy" => {
                    profile.memory_policy = value.to_string();
                }
                _ => {}
            }
        }
        if let (Some(provider), Some(model)) = (&profile.current_provider, &profile.current_model)
            && self_model_invalid(&self.models, provider, model)
        {
            return Ok(format!(
                "Model {model} is not available for designed agent provider {provider}."
            ));
        }
        profile
            .metadata
            .insert("designed".to_string(), Value::Bool(true));
        profile.updated_at = chrono::Utc::now();
        let path = self.agents.save(&profile)?;
        if activate {
            let message = self.apply_agent_profile(&profile)?;
            Ok(format!(
                "Designed agent {} at {}\n{}",
                profile.id,
                path.display(),
                message
            ))
        } else {
            Ok(format!(
                "Designed agent {} ({}, mode={}) at {}",
                profile.id,
                profile.display_name,
                profile.mode,
                path.display()
            ))
        }
    }

    fn apply_agent_profile(&mut self, profile: &AgentProfile) -> anyhow::Result<String> {
        self.session.active_agent_id = Some(profile.id.clone());
        self.session.active_agent_name = Some(profile.display_name.clone());
        self.session.system_prompt = self.effective_agent_system_prompt(profile);
        if let Some(provider) = &profile.current_provider
            && self.provider_registry.get(provider).is_some()
        {
            self.session.current_provider = provider.clone();
        }
        if let Some(model) = &profile.current_model
            && let Some(model_info) = self.models.get(model)
            && self
                .models
                .is_model_allowed_for_provider(model_info, &self.session.current_provider)
        {
            self.session.current_model = model.clone();
            if let Some(context_window) = model_info.context_window {
                self.session.context_limit = context_window;
            }
        }
        let mut config = self.cms.config.clone();
        config.user_id = profile.cms_user_id.clone();
        config.project_id = Some(profile.cms_project_id.clone());
        self.cms = VegvisirCms::open(config)?;
        self.tool_executor.runtime_policy = RuntimePolicy {
            active_agent_id: Some(profile.id.clone()),
            active_agent_mode: Some(profile.mode.clone()),
            allowed_tools: profile.enabled_tools.clone(),
            usrl_contracts: profile.usrl_contracts.clone(),
            usrl_rules: self.usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_rules"),
            usrl_constraints: self
                .usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_constraints"),
            usrl_stages: self.usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_stages"),
            usrl_triggers: self
                .usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_triggers"),
            strict_usrl: !profile.usrl_contracts.is_empty(),
        };
        self.rebuild_tooling_for_cms()?;
        Ok(format!(
            "Using agent {} ({}, mode={}). System prompt and CMS memory scope applied.",
            profile.id, profile.display_name, profile.mode
        ))
    }

    fn clear_active_agent(&mut self) -> anyhow::Result<()> {
        self.session.active_agent_id = None;
        self.session.active_agent_name = None;
        self.session.system_prompt = self
            .config
            .load()
            .ok()
            .and_then(|defaults| {
                defaults
                    .get("system_prompt")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(default_system_prompt);
        let mut config = self.cms.config.clone();
        config.user_id = self.default_user_id();
        config.project_id = Some(workspace_project_id(&self.cwd));
        self.cms = VegvisirCms::open(config)?;
        self.tool_executor.runtime_policy = RuntimePolicy::default();
        self.rebuild_tooling_for_cms()?;
        Ok(())
    }

    fn default_user_id(&self) -> String {
        self.config
            .load()
            .ok()
            .map(|defaults| configured_user_id(&defaults))
            .unwrap_or_else(|| "local-user".to_string())
    }

    fn save_agent_profile_and_refresh_if_active(
        &mut self,
        profile: &AgentProfile,
    ) -> anyhow::Result<()> {
        self.agents.save(profile)?;
        if self.session.active_agent_id.as_deref() == Some(&profile.id) {
            self.refresh_active_agent_profile(profile)?;
        }
        Ok(())
    }

    fn refresh_active_agent_profile(&mut self, profile: &AgentProfile) -> anyhow::Result<()> {
        self.session.active_agent_name = Some(profile.display_name.clone());
        self.session.system_prompt = self.effective_agent_system_prompt(profile);
        self.tool_executor.runtime_policy = RuntimePolicy {
            active_agent_id: Some(profile.id.clone()),
            active_agent_mode: Some(profile.mode.clone()),
            allowed_tools: profile.enabled_tools.clone(),
            usrl_contracts: profile.usrl_contracts.clone(),
            usrl_rules: self.usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_rules"),
            usrl_constraints: self
                .usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_constraints"),
            usrl_stages: self.usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_stages"),
            usrl_triggers: self
                .usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_triggers"),
            strict_usrl: !profile.usrl_contracts.is_empty(),
        };
        self.rebuild_tooling_for_cms()?;
        Ok(())
    }

    fn effective_agent_system_prompt(&self, profile: &AgentProfile) -> String {
        let mut sections = vec![profile.system_prompt.trim().to_string()]
            .into_iter()
            .filter(|section| !section.is_empty())
            .collect::<Vec<_>>();
        let skill_sections = profile
            .enabled_skills
            .iter()
            .filter_map(|name| {
                self.session
                    .enabled_skills
                    .iter()
                    .find(|skill| skill.name == *name)
            })
            .filter(|skill| {
                skill.kind == "markdown"
                    || skill.metadata.get("format").and_then(Value::as_str) == Some("markdown")
            })
            .filter_map(|skill| {
                let body = skill.metadata.get("body").and_then(Value::as_str)?;
                let body = body.trim();
                if body.is_empty() {
                    None
                } else {
                    Some(format!("Skill: {}\n{}", skill.name, body))
                }
            })
            .collect::<Vec<_>>();
        if !skill_sections.is_empty() {
            sections.push(format!(
                "Enabled agent skills:\n{}",
                skill_sections.join("\n\n")
            ));
        }
        sections.join("\n\n")
    }

    fn resolve_usrl_contract_refs(&self, value: &str) -> Vec<String> {
        let Some(skill) = self.session.enabled_skills.iter().find(|skill| {
            skill.name == value
                && (skill.kind == "usrl_contract"
                    || skill.metadata.get("format").and_then(Value::as_str) == Some("usrl"))
        }) else {
            return vec![value.to_string()];
        };
        let contracts = skill
            .metadata
            .get("usrl_contracts")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if contracts.is_empty() {
            vec![value.to_string()]
        } else {
            contracts
        }
    }

    fn usrl_metadata_for_contracts(&self, contracts: &[String], key: &str) -> Vec<String> {
        let mut values = Vec::new();
        for skill in &self.session.enabled_skills {
            if skill.kind != "usrl_contract"
                && skill.metadata.get("format").and_then(Value::as_str) != Some("usrl")
            {
                continue;
            }
            let skill_contracts = skill
                .metadata
                .get("usrl_contracts")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_default();
            if !contracts
                .iter()
                .any(|contract| skill_contracts.iter().any(|item| item == contract))
            {
                continue;
            }
            if let Some(items) = skill.metadata.get(key).and_then(Value::as_array) {
                for item in items.iter().filter_map(Value::as_str) {
                    let item = item.to_string();
                    if !values.contains(&item) {
                        values.push(item);
                    }
                }
            }
        }
        values
    }

    fn rebuild_tooling_for_cms(&mut self) -> anyhow::Result<()> {
        let allow_risky_tools = self.tool_executor.guardrails.policy.allow_risky_tools;
        let bypass = self
            .tool_executor
            .guardrails
            .policy
            .bypass_approvals_and_sandbox;
        let mut tool_registry =
            build_builtin_registry_with_cms_and_mode(&self.cwd, self.cms.config.clone(), bypass)?;
        let mcp_servers = self.active_mcp_servers();
        register_mcp_tools(
            &mut tool_registry,
            &mcp_servers,
            self.tool_executor.runtime_policy.clone(),
        )?;
        self.tool_registry = tool_registry.clone();
        self.tool_executor.registry = tool_registry;
        self.tool_executor.guardrails.policy.allow_risky_tools = allow_risky_tools;
        self.tool_executor
            .guardrails
            .policy
            .bypass_approvals_and_sandbox = bypass;
        Ok(())
    }

    fn active_mcp_servers(&self) -> Vec<crate::core::McpServerConfig> {
        let Some(agent_id) = &self.session.active_agent_id else {
            return self.mcp_servers.clone();
        };
        let Ok(profile) = self.agents.load(agent_id) else {
            return Vec::new();
        };
        self.mcp_servers
            .iter()
            .filter(|server| profile.enabled_mcp_servers.contains(&server.id))
            .cloned()
            .collect()
    }

    fn resolve_workspace_path(&self, path: &str) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            self.cwd.join(path)
        }
    }

    fn attach_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            if self.session.pending_attachments.is_empty() {
                return Ok(
                    "No pending attachments. Drag/drop a file path into the input or use /attach <path>."
                        .to_string(),
                );
            }
            return Ok(self
                .session
                .pending_attachments
                .iter()
                .map(|item| format!("{}: {}", item.kind, item.path))
                .collect::<Vec<_>>()
                .join("\n"));
        }
        if args[0] == "clear" {
            self.session.pending_attachments.clear();
            return Ok("Pending attachments cleared.".to_string());
        }
        let mut path = PathBuf::from(args.join(" "));
        if !path.is_absolute() {
            path = self.cwd.join(path);
        }
        if !path.exists() || !path.is_file() {
            return Ok(format!("Attachment not found: {}", path.display()));
        }
        let attachment = attachment_for(&path)?;
        let name = attachment
            .name
            .clone()
            .unwrap_or_else(|| path.display().to_string());
        self.session.pending_attachments.push(attachment);
        Ok(format!("Attached {name}"))
    }

    fn help(&self) -> String {
        self.commands
            .all()
            .into_iter()
            .map(|cmd| format!("{:<28} {}", cmd.usage, cmd.description))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn tools_command(&mut self, args: &[String]) -> String {
        if let Some(action) = args.first().map(|arg| arg.as_str()) {
            match action {
                "allow-risky" | "enable-risky" | "deny-risky" | "disable-risky"
                | "require-approval" | "approval" | "no-approval" | "disable-approval"
                    if self
                        .tool_executor
                        .guardrails
                        .policy
                        .bypass_approvals_and_sandbox =>
                {
                    return "Dangerous bypass was enabled at startup and cannot be changed from the TUI.".to_string();
                }
                "allow-risky" | "enable-risky" => {
                    self.risky_tools_enabled = true;
                    self.tool_executor.guardrails.policy.allow_risky_tools = true;
                    return "Risky tools enabled for this running session.".to_string();
                }
                "deny-risky" | "disable-risky" => {
                    self.risky_tools_enabled = false;
                    self.tool_executor.guardrails.policy.allow_risky_tools = false;
                    return "Risky tools disabled for this running session.".to_string();
                }
                "status" => {
                    return format!(
                        "Risky tools: {}\nHuman approval: {}\nDangerous bypass: {}\nPending approvals: {}",
                        if self.tool_executor.guardrails.policy.allow_risky_tools {
                            "enabled"
                        } else {
                            "disabled"
                        },
                        if self.tool_executor.guardrails.policy.require_human_approval {
                            "required"
                        } else {
                            "not required"
                        },
                        if self
                            .tool_executor
                            .guardrails
                            .policy
                            .bypass_approvals_and_sandbox
                        {
                            "enabled at startup"
                        } else {
                            "disabled"
                        },
                        self.tool_executor.guardrails.approvals.pending_len()
                    );
                }
                "require-approval" | "approval" => {
                    self.tool_executor.guardrails.policy.require_human_approval = true;
                    self.tool_executor.guardrails.policy.allow_risky_tools = false;
                    self.risky_tools_enabled = false;
                    return "Human approval required for risky tools.".to_string();
                }
                "no-approval" | "disable-approval" => {
                    self.tool_executor.guardrails.policy.require_human_approval = false;
                    return "Human approval is no longer required for risky tools.".to_string();
                }
                _ => {}
            }
        }
        let inventory = self
            .session
            .enabled_tools
            .iter()
            .map(|tool| format!("{}: {} - {}", tool.category, tool.name, tool.description))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "{inventory}\nRisky tools: {}\nHuman approval: {}\nDangerous bypass: {}\nPending approvals: {}",
            if self.tool_executor.guardrails.policy.allow_risky_tools {
                "enabled"
            } else {
                "disabled"
            },
            if self.tool_executor.guardrails.policy.require_human_approval {
                "required"
            } else {
                "not required"
            },
            if self
                .tool_executor
                .guardrails
                .policy
                .bypass_approvals_and_sandbox
            {
                "enabled at startup"
            } else {
                "disabled"
            },
            self.tool_executor.guardrails.approvals.pending_len()
        )
    }

    fn approvals_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("list") | Some("pending") => {
                let pending = self.tool_executor.guardrails.approvals.pending();
                if pending.is_empty() {
                    return "No pending approvals.".to_string();
                }
                pending
                    .values()
                    .map(|request| {
                        format!(
                            "{}  tool={} risk={} reason={} args={}",
                            request.id,
                            request.tool_name,
                            request.risk_label,
                            request.reason,
                            serde_json::to_string(&request.args).unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Some("show") | Some("view") | Some("inspect") => {
                let Some(id) = args.get(1) else {
                    return "Usage: /approvals show <id>".to_string();
                };
                let pending = self.tool_executor.guardrails.approvals.pending();
                let Some(request) = pending.get(id) else {
                    return format!("Unknown pending approval: {id}");
                };
                serde_json::to_string_pretty(&json!({
                    "id": request.id,
                    "tool": request.tool_name,
                    "risk": request.risk_label,
                    "reason": request.reason,
                    "args": request.args,
                    "actions": {
                        "approve_once": format!("/approvals approve {}", request.id),
                        "approve_pattern": format!("/approvals approve-pattern {}", request.id),
                        "edit": format!("/approvals edit {} <json-args>", request.id),
                        "deny": format!("/approvals deny {}", request.id),
                    }
                }))
                .unwrap_or_else(|error| format!("Failed to render approval {id}: {error}"))
            }
            Some("approve") | Some("allow") => {
                let Some(id) = args.get(1) else {
                    return "Usage: /approvals approve <id>".to_string();
                };
                if self.tool_executor.guardrails.approvals.approve_once(id) {
                    format!("Approved approval {id} for one matching tool call.")
                } else {
                    format!("Unknown pending approval: {id}")
                }
            }
            Some("approve-pattern") | Some("allow-pattern") => {
                let Some(id) = args.get(1) else {
                    return "Usage: /approvals approve-pattern <id>".to_string();
                };
                match self.tool_executor.guardrails.approvals.approve_pattern(id) {
                    Some(tool) => format!("Approved future risky calls for tool pattern {tool}."),
                    None => format!("Unknown pending approval: {id}"),
                }
            }
            Some("edit") => {
                let Some(id) = args.get(1) else {
                    return "Usage: /approvals edit <id> <json-args>".to_string();
                };
                let Some(raw_json) = args.get(2) else {
                    return "Usage: /approvals edit <id> <json-args>".to_string();
                };
                let args = match serde_json::from_str::<serde_json::Value>(raw_json) {
                    Ok(serde_json::Value::Object(args)) => args,
                    Ok(_) => return "Approval args must be a JSON object.".to_string(),
                    Err(error) => return format!("Invalid approval args JSON: {error}"),
                };
                match self.tool_executor.guardrails.approvals.edit(id, args) {
                    Some(request) => format!(
                        "Edited approval {id}; new approval id is {} args={}",
                        request.id,
                        serde_json::to_string(&request.args).unwrap_or_default()
                    ),
                    None => format!("Unknown pending approval: {id}"),
                }
            }
            Some("deny") | Some("reject") => {
                let Some(id) = args.get(1) else {
                    return "Usage: /approvals deny <id>".to_string();
                };
                if self.tool_executor.guardrails.approvals.deny(id) {
                    format!("Denied approval {id}.")
                } else {
                    format!("Unknown pending approval: {id}")
                }
            }
            Some(other) => format!("Unknown /approvals command: {other}"),
        }
    }

    fn skills(&self) -> String {
        self.session
            .enabled_skills
            .iter()
            .map(|skill| {
                let usrl = skill
                    .metadata
                    .get("usrl_contracts")
                    .and_then(Value::as_array)
                    .map(|contracts| {
                        contracts
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                    })
                    .filter(|contracts| !contracts.is_empty())
                    .map(|contracts| format!(" [contracts: {}]", contracts.join(",")))
                    .unwrap_or_default();
                format!(
                    "{}: {} - {}{}",
                    skill.category, skill.name, skill.description, usrl
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn recall_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok("Usage: /recall [--limit N] [--global] <query>".to_string());
        }
        let mut limit = 8_usize;
        let mut global = false;
        let mut query = Vec::new();
        let mut iter = args.iter().peekable();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--global" | "--all" => global = true,
                "--project" | "--local" => global = false,
                "--limit" | "-n" => {
                    let Some(value) = iter.next() else {
                        return Ok("Usage: /recall [--limit N] [--global] <query>".to_string());
                    };
                    limit = value.parse::<usize>().unwrap_or(8).clamp(1, 50);
                }
                value if value.starts_with("--limit=") => {
                    limit = value
                        .trim_start_matches("--limit=")
                        .parse::<usize>()
                        .unwrap_or(8)
                        .clamp(1, 50);
                }
                value => query.push(value.to_string()),
            }
        }
        if query.is_empty() {
            return Ok("Usage: /recall [--limit N] [--global] <query>".to_string());
        }
        let query = query.join(" ");
        let bundle = if global {
            self.cms.retrieve_global(query, limit)?
        } else {
            self.cms.retrieve(query, limit)?
        };
        if bundle.results.is_empty() {
            return Ok("No CMS memories matched.".to_string());
        }
        Ok(bundle
            .results
            .into_iter()
            .map(|result| {
                format!(
                    "{} [{}]: {}",
                    result.memory.title, result.memory.id.0, result.memory.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn memory_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("status") | Some("scope") => Ok(format!(
                "CMS-v2 memory scope\nmode={:?}\ndb={}\nuser_id={}\nproject_id={}\nactive_agent={}\nworkspace={}",
                self.cms.config.context_mode,
                self.cms.config.db_path.display(),
                self.cms.config.user_id,
                self.cms.config.project_id.as_deref().unwrap_or("none"),
                self.session.active_agent_id.as_deref().unwrap_or("default"),
                self.cwd.display()
            )),
            Some("recent") | Some("list") => {
                let (limit, global) = parse_limit_and_global(&args[1..], 8);
                let memories = self.cms.recent(limit, global)?;
                if memories.is_empty() {
                    return Ok(if global {
                        "No recent CMS memories are available for this user.".to_string()
                    } else {
                        "No recent CMS memories are available for this project scope.".to_string()
                    });
                }
                Ok(memories
                    .into_iter()
                    .map(|memory| {
                        format!(
                            "{}  {}  type={} project={} title={} summary={}",
                            memory.id,
                            memory.updated_at.format("%Y-%m-%d %H:%M:%S"),
                            memory.memory_type,
                            memory.project_id.as_deref().unwrap_or("none"),
                            memory.title,
                            memory.summary
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            Some("import-chatgpt") => {
                let (path, messages_per_memory, max_chars_per_memory) =
                    parse_chatgpt_import_args(&args[1..])?;
                if !path.exists() {
                    anyhow::bail!("ChatGPT export path does not exist: {}", path.display());
                }
                let config = self.cms.config.clone();
                let db_path = config.db_path.clone();
                let user_id = config.user_id.clone();
                let project_id = config.project_id.clone();
                let import_path = path.clone();
                let handle = thread::spawn(move || {
                    let mut cms = VegvisirCms::open(config)?;
                    let summary = cms.import_chatgpt(
                        &import_path,
                        messages_per_memory,
                        max_chars_per_memory,
                    )?;
                    Ok(format!(
                        "Imported {} ChatGPT memory object(s) into active CMS scope.\ndb={}\nuser_id={}\nproject_id={}",
                        summary.imported,
                        summary.db_path.display(),
                        summary.user_id,
                        summary.project_id.as_deref().unwrap_or("none")
                    ))
                });
                self.pending_background_jobs.push(handle);
                Ok(format!(
                    "Started ChatGPT import in background.\npath={}\ndb={}\nuser_id={}\nproject_id={}\nUse /memory recent after the completion note appears.",
                    path.display(),
                    db_path.display(),
                    user_id,
                    project_id.as_deref().unwrap_or("none")
                ))
            }
            Some(other) => Ok(format!("Unknown /memory command: {other}")),
        }
    }

    fn remember_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let global = args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--global" | "--user" | "--profile"));
        let raw = args
            .iter()
            .filter(|arg| !matches!(arg.as_str(), "--global" | "--user" | "--profile"))
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        let Some((title, content)) = raw.split_once('|') else {
            return Ok("Usage: /remember [--global] <title> | <content>".to_string());
        };
        let result = if global {
            self.cms
                .remember_global("note", title.trim(), content.trim())?
        } else {
            self.cms.remember("note", title.trim(), content.trim())?
        };
        Ok(format!(
            "Remembered {}memory {}",
            if global { "global " } else { "" },
            result.memory_id.0
        ))
    }

    fn context_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok("Usage: /context <message>".to_string());
        }
        Ok(self.cms.prepare_context(args.join(" "))?.packed_text)
    }

    fn model_request_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok("Usage: /model-request <message>".to_string());
        }
        let envelope = self.cms.prepare_cached_prompt(
            args.join(" "),
            self.session.current_provider.clone(),
            self.session.current_model.clone(),
        )?;
        let prompt = if self.session.system_prompt.trim().is_empty() {
            envelope.model_request.prompt.clone()
        } else {
            format!(
                "Harness system prompt:\n{}\n\n{}",
                self.session.system_prompt.trim(),
                envelope.model_request.prompt
            )
        };
        Ok(format!(
            "prompt_cache_key: {}\ncacheable_prefix_tokens: {}\ntotal_prompt_tokens: {}\n\n{}",
            envelope.manifest.prompt_cache_key,
            envelope.manifest.cacheable_prefix_tokens,
            envelope.manifest.total_prompt_tokens,
            prompt
        ))
    }

    fn models_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if !args.is_empty() {
            return self.select_model(args);
        }
        let refresh_note = self.refresh_provider_models(&self.session.current_provider.clone());
        let models = self.models.by_provider(&self.session.current_provider);
        let availability = self.provider_registry.availability();
        let state = if availability
            .get(&self.session.current_provider)
            .copied()
            .unwrap_or(false)
        {
            "ready"
        } else {
            "needs-auth"
        };
        let mut lines = vec![format!(
            "Available models for {} [{}]:",
            self.session.current_provider, state
        )];
        if let Some(refresh_note) = refresh_note {
            lines.push(format!("  {refresh_note}"));
        }
        for model in models {
            let active = if model.name == self.session.current_model {
                "*"
            } else {
                " "
            };
            let context = model
                .context_window
                .map(|value| format!("{value} ctx"))
                .unwrap_or_else(|| "ctx unknown".to_string());
            lines.push(format!("  {active} {:<34} {context}", model.name));
        }
        lines.push(
            "Use /provider <name> to switch provider. Use /model <name> to switch model."
                .to_string(),
        );
        self.input.set_buffer("/model ");
        self.input.paste_char_count = 0;
        self.input.selected_suggestion = 0;
        Ok(lines.join("\n"))
    }

    fn select_model(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok(format!("Current model: {}", self.session.current_model));
        }
        let global = args
            .iter()
            .any(|arg| arg == "--global" || arg == "--default");
        let name = args
            .iter()
            .filter(|arg| !matches!(arg.as_str(), "--global" | "--default"))
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        if self.models.get(&name).is_none() {
            let _ = self.refresh_provider_models(&self.session.current_provider.clone());
        }
        let Some(model) = self.models.get(&name) else {
            let matches = close_model_matches(&self.models, &self.session.current_provider, &name);
            if !matches.is_empty() {
                return Ok(format!(
                    "Unknown model for provider {}. Close matches:\n{}",
                    self.session.current_provider,
                    matches.join("\n")
                ));
            }
            return Ok(format!(
                "Unknown model for provider {}: {name}",
                self.session.current_provider
            ));
        };
        if !self
            .models
            .is_model_allowed_for_provider(model, &self.session.current_provider)
        {
            return Ok(format!(
                "Model {} belongs to provider {}, but selected provider is {}.\nRun /provider {} first, then /model {}.",
                model.name,
                model.provider,
                self.session.current_provider,
                model.provider,
                model.name
            ));
        }
        self.session.current_model = model.name.clone();
        if let Some(context_window) = model.context_window {
            self.session.context_limit = context_window;
        }
        if global {
            self.save_global_model_defaults()?;
            self.clear_workspace_provider_override()?;
        } else {
            self.save_workspace_provider_override()?;
        }
        Ok(format!(
            "Selected model {} via provider {} ({}).",
            model.name,
            model.provider,
            if global {
                "global default"
            } else {
                "project override"
            }
        ))
    }

    fn provider_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok(format!(
                "Current provider: {}",
                self.session.current_provider
            ));
        }
        let global = args
            .iter()
            .any(|arg| arg == "--global" || arg == "--default");
        let filtered = args
            .iter()
            .filter(|arg| !matches!(arg.as_str(), "--global" | "--default"))
            .cloned()
            .collect::<Vec<_>>();
        let Some(name) = filtered.first() else {
            return Ok(format!(
                "Current provider: {}",
                self.session.current_provider
            ));
        };
        let Some(provider) = self.provider_registry.get(name).cloned() else {
            let matches = close_provider_matches(&self.provider_registry, name);
            if !matches.is_empty() {
                return Ok(format!(
                    "Unknown provider. Close matches:\n{}",
                    matches.join("\n")
                ));
            }
            return Ok(format!("Unknown provider: {name}"));
        };
        if provider.auth_type == "api_key" && !direct_provider_auth_allowed() {
            return Ok(format!(
                "Direct API-key provider auth is disabled in production mode for {}.\nConfigure the secret in HBSE with /hbse provider {}, then select the HBSE-routed provider when available, for example /provider {}-hbse.",
                provider.display_name.as_deref().unwrap_or(&provider.name),
                canonical_hbse_provider_id(&provider.name),
                canonical_hbse_provider_id(&provider.name)
            ));
        }
        self.session.current_provider = provider.name.clone();
        let refresh_note = self.refresh_provider_models(&provider.name);
        if let Some(model) = self.models.default_for_provider(&provider.name) {
            self.session.current_model = model.name.clone();
            if let Some(context_window) = model.context_window {
                self.session.context_limit = context_window;
            }
        }
        if global {
            self.save_global_model_defaults()?;
            self.clear_workspace_provider_override()?;
        } else {
            self.save_workspace_provider_override()?;
        }
        if provider.name == "openai-sso" {
            let selected = format!(
                "Selected provider {}; active model is {} ({}).",
                provider.name,
                self.session.current_model,
                if global {
                    "global default"
                } else {
                    "project override"
                }
            );
            if self
                .openai_sso_status()
                .starts_with("OpenAI SSO is logged in")
            {
                return Ok(selected);
            }
            let auth_result = self.openai_sso_login();
            return Ok(format!("{selected}\n{auth_result}"));
        }
        let notice = api_key_notice(&provider);
        Ok([
            format!(
                "Selected provider {}; active model is {} ({}).",
                provider.name,
                self.session.current_model,
                if global {
                    "global default"
                } else {
                    "project override"
                }
            ),
            refresh_note.unwrap_or_default(),
            notice,
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n"))
    }

    fn providers_command(&self) -> String {
        let availability = self.provider_registry.availability();
        let mut lines = vec!["Providers:".to_string()];
        for provider in self.provider_registry.list() {
            let status = if availability.get(&provider.name).copied().unwrap_or(false) {
                "ready"
            } else {
                "needs-auth"
            };
            let auth_detail = provider_auth_detail(provider);
            lines.push(format!(
                "{:<14} {:<10} auth={}{}",
                provider.name, status, provider.auth_type, auth_detail
            ));
        }
        lines.join("\n")
    }

    fn refresh_provider_models(&mut self, provider_name: &str) -> Option<String> {
        let provider = self.provider_registry.get(provider_name)?.clone();
        if provider.kind == "demo" {
            return None;
        }
        match discover_provider_models(&provider) {
            Ok(discovered) if discovered.is_empty() => {
                Some(format!("No models returned by {provider_name}."))
            }
            Ok(discovered) => {
                let count = discovered.len();
                self.models
                    .replace_provider_models(provider_name, discovered);
                if provider_name == self.session.current_provider
                    && self
                        .models
                        .get(&self.session.current_model)
                        .filter(|model| {
                            self.models
                                .is_model_allowed_for_provider(model, provider_name)
                        })
                        .is_none()
                {
                    if let Some(default) = self.models.default_for_provider(provider_name) {
                        self.session.current_model = default.name.clone();
                        if let Some(context_window) = default.context_window {
                            self.session.context_limit = context_window;
                        }
                    }
                }
                Some(format!("Refreshed {count} model(s) from {provider_name}."))
            }
            Err(error) => Some(error.to_string()),
        }
    }

    fn auth_command(&self, args: &[String]) -> String {
        let Some(provider_name) = args.first() else {
            return "Usage: /auth <provider>. Example: /auth openai, /auth openai-sso, /auth openai-sso-status, or /auth openai-sso-logout".to_string();
        };
        if provider_name == "openai-sso-status" {
            return self.openai_sso_status();
        }
        if provider_name == "openai-sso-logout" {
            let _ =
                crate::openai_sso::OpenAISsoAuthStore::new(Some(self.data_root.clone())).clear();
            return "OpenAI SSO auth removed from Vegvisir.".to_string();
        }
        let Some(provider) = self.provider_registry.get(provider_name) else {
            return format!("Unknown provider: {provider_name}");
        };
        if provider.auth_type == "none" {
            return format!(
                "{} does not require authentication.",
                provider.display_name.as_deref().unwrap_or(&provider.name)
            );
        }
        if provider.name == "openai-sso" {
            return self.openai_sso_login();
        }
        api_key_notice(provider)
    }

    fn verify_command(&self, args: &[String]) -> String {
        let scope = args.first().map(String::as_str).unwrap_or("all");
        let mut checks = Vec::new();
        if matches!(scope, "all" | "auth") {
            checks.extend(self.verify_auth_checks());
        }
        if matches!(scope, "all" | "mcp") {
            checks.extend(self.verify_mcp_checks());
        }
        if matches!(scope, "all" | "agent") {
            checks.extend(self.verify_agent_checks());
        }
        if matches!(scope, "all" | "memory") {
            checks.extend(self.verify_memory_checks());
        }
        if matches!(scope, "all" | "runtime") {
            checks.extend(self.verify_runtime_checks());
        }
        if matches!(scope, "all" | "evals") {
            checks.extend(self.verify_eval_checks());
        }
        if checks.is_empty() {
            return "Usage: /verify [all|auth|mcp|agent|memory|runtime|evals]".to_string();
        }
        checks.join("\n")
    }

    fn verify_auth_checks(&self) -> Vec<String> {
        let mut checks = Vec::new();
        let hbse = self
            .provider_registry
            .list()
            .into_iter()
            .filter(|provider| provider.auth_type == "hbse")
            .collect::<Vec<_>>();
        for provider in hbse {
            let socket = crate::provider::hbse_default_or_configured_socket(provider);
            let secret_ref = provider
                .metadata
                .get("hbse_secret_ref")
                .and_then(Value::as_str)
                .unwrap_or("");
            let status = if socket.exists() && !secret_ref.is_empty() {
                "ok"
            } else {
                "warn"
            };
            checks.push(format!(
                "{status} auth/hbse {} socket={} secret_ref={}",
                provider.name,
                socket.display(),
                if secret_ref.is_empty() {
                    "missing"
                } else {
                    secret_ref
                }
            ));
        }
        for provider in self
            .provider_registry
            .list()
            .into_iter()
            .filter(|provider| provider.auth_type == "api_key")
        {
            if direct_provider_auth_allowed() {
                checks.push(format!(
                    "warn auth/legacy {} uses direct env fallback {}; production should use /hbse provider {}",
                    provider.name,
                    provider.api_key_env.as_deref().unwrap_or("unknown"),
                    canonical_hbse_provider_id(&provider.name)
                ));
            } else {
                checks.push(format!(
                    "ok auth/legacy {} direct env fallback blocked by production mode; use /hbse provider {}",
                    provider.name,
                    canonical_hbse_provider_id(&provider.name)
                ));
            }
        }
        if self.hbse_services.is_empty() {
            checks.push(
                "warn auth/services no HBSE service refs registered; use /hbse service add for tool/service credentials"
                    .to_string(),
            );
        } else {
            checks.push(format!(
                "ok auth/services registered_refs={}",
                self.hbse_services.len()
            ));
        }
        checks
    }

    fn verify_mcp_checks(&self) -> Vec<String> {
        if self.mcp_servers.is_empty() {
            return vec!["warn mcp no servers configured".to_string()];
        }
        let mut checks = self
            .mcp_servers
            .iter()
            .map(|server| {
                if let Some(error) = &server.discovery_error {
                    format!("warn mcp/{} discovery_error={error}", server.id)
                } else if server.transport == crate::core::McpTransport::Http
                    && server.hbse_secret_refs.is_empty()
                {
                    format!(
                        "fail mcp/{} http transport missing HBSE secret ref",
                        server.id
                    )
                } else {
                    format!(
                        "ok mcp/{} transport={:?} tools={} hbse_refs={}",
                        server.id,
                        server.transport,
                        server.tools.len(),
                        server.hbse_secret_refs.len()
                    )
                }
            })
            .collect::<Vec<_>>();
        checks.extend(self.verify_active_agent_mcp_checks());
        checks
    }

    fn verify_active_agent_mcp_checks(&self) -> Vec<String> {
        let Some(agent_id) = &self.session.active_agent_id else {
            return vec![
                "ok mcp/active all configured servers available; no active agent filter"
                    .to_string(),
            ];
        };
        let Ok(profile) = self.agents.load(agent_id) else {
            return vec![format!(
                "fail mcp/active active agent {agent_id} profile could not be loaded"
            )];
        };
        if profile.enabled_mcp_servers.is_empty() {
            return vec![format!(
                "warn mcp/active agent={agent_id} no MCP servers allowed"
            )];
        }
        let configured = self
            .mcp_servers
            .iter()
            .map(|server| server.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let missing = profile
            .enabled_mcp_servers
            .iter()
            .filter(|server| !configured.contains(*server))
            .cloned()
            .collect::<Vec<_>>();
        let active = self.active_mcp_servers();
        let active_tools = active
            .iter()
            .map(|server| server.tools.len())
            .sum::<usize>();
        let mut checks = vec![format!(
            "ok mcp/active agent={agent_id} servers={} tools={active_tools}",
            active
                .iter()
                .map(|server| server.id.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )];
        if !missing.is_empty() {
            checks.push(format!(
                "fail mcp/active agent={agent_id} missing configured server(s): {}",
                missing.join(",")
            ));
        }
        checks
    }

    fn verify_agent_checks(&self) -> Vec<String> {
        let Some(agent_id) = &self.session.active_agent_id else {
            return vec!["warn agent no active custom agent".to_string()];
        };
        let mut checks = Vec::new();
        checks.push(format!(
            "ok agent active={} mode={}",
            agent_id,
            self.tool_executor
                .runtime_policy
                .active_agent_mode
                .as_deref()
                .unwrap_or("custom")
        ));
        checks.push(format!(
            "{} agent/tools allowed={}",
            if self.tool_executor.runtime_policy.allowed_tools.is_empty() {
                "warn"
            } else {
                "ok"
            },
            if self.tool_executor.runtime_policy.allowed_tools.is_empty() {
                "unrestricted".to_string()
            } else {
                self.tool_executor.runtime_policy.allowed_tools.join(",")
            }
        ));
        checks.push(format!(
            "{} agent/usrl contracts={}",
            if self.tool_executor.runtime_policy.usrl_contracts.is_empty() {
                "warn"
            } else {
                "ok"
            },
            if self.tool_executor.runtime_policy.usrl_contracts.is_empty() {
                "none".to_string()
            } else {
                self.tool_executor.runtime_policy.usrl_contracts.join(",")
            }
        ));
        checks.push(format!(
            "{} agent/usrl constraints={}",
            if self
                .tool_executor
                .runtime_policy
                .usrl_constraints
                .is_empty()
            {
                "warn"
            } else {
                "ok"
            },
            if self
                .tool_executor
                .runtime_policy
                .usrl_constraints
                .is_empty()
            {
                "none".to_string()
            } else {
                self.tool_executor.runtime_policy.usrl_constraints.join(",")
            }
        ));
        checks
    }

    fn verify_memory_checks(&self) -> Vec<String> {
        vec![format!(
            "ok memory cms_v2 db={} user_id={} project_id={}",
            self.cms.config.db_path.display(),
            self.cms.config.user_id,
            self.cms.config.project_id.as_deref().unwrap_or("none")
        )]
    }

    fn verify_runtime_checks(&self) -> Vec<String> {
        let approval_path = self.data_root.join("approvals.json");
        let trace_path = self.data_root.join("traces").join("tui.jsonl");
        let subagent_path = self.subagent_board_path();
        let subagents = self.load_subagent_records().unwrap_or_default();
        vec![
            format!(
                "ok runtime/approvals path={} pending={}",
                approval_path.display(),
                self.tool_executor.guardrails.approvals.pending_len()
            ),
            format!(
                "ok runtime/traces path={} events={}",
                trace_path.display(),
                self.logger.events().len()
            ),
            format!(
                "ok runtime/subagents path={} tasks={}",
                subagent_path.display(),
                subagents.len()
            ),
            "ok runtime/cancel command=/cancel".to_string(),
            format!(
                "ok runtime/dangerous_bypass {} startup_only=true",
                if self.dangerously_bypass_approvals_and_sandbox {
                    "enabled"
                } else {
                    "disabled"
                }
            ),
            format!(
                "ok runtime/user default={} active={} sessions={}",
                self.default_user_id(),
                self.cms.config.user_id,
                self.sessions.store.root.display()
            ),
        ]
    }

    fn verify_eval_checks(&self) -> Vec<String> {
        match crate::evals::run_builtin_evals("golden") {
            Ok(results) => {
                let passed = results.iter().filter(|result| result.passed).count();
                let total = results.len();
                let status = if passed == total { "ok" } else { "fail" };
                vec![format!(
                    "{} evals/golden passed={} total={}",
                    status, passed, total
                )]
            }
            Err(error) => vec![format!("fail evals/golden error={error}")],
        }
    }

    fn mcp_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("list") | Some("servers") => {
                if self.mcp_servers.is_empty() {
                    return Ok("No MCP servers configured. Add servers to $VEGVISIR_HOME/mcp.json with HBSE secret refs for authenticated services.".to_string());
                }
                Ok(self
                    .mcp_servers
                    .iter()
                    .map(|server| {
                        format!(
                            "{:<18} {:?} enabled={} tools={} hbse_refs={}{}",
                            server.id,
                            server.transport,
                            server.enabled,
                            server.tools.len(),
                            server.hbse_secret_refs.len(),
                            server
                                .discovery_error
                                .as_ref()
                                .map(|error| format!(" discovery_error={error}"))
                                .unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            Some("show") | Some("view") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /mcp show <id>".to_string());
                };
                let Some(server) = self.mcp_servers.iter().find(|server| server.id == *id) else {
                    return Ok(format!("Unknown MCP server: {id}"));
                };
                Ok(format!(
                    "id={}\ndisplay_name={}\ntransport={:?}\nenabled={}\nurl={}\ncommand={}\nargs={}\nworking_dir={}\nhbse_secret_refs={}\nconsumer={}\npurpose={}\ntools={}\ndiscovery_error={}",
                    server.id,
                    if server.display_name.is_empty() {
                        "-"
                    } else {
                        &server.display_name
                    },
                    server.transport,
                    server.enabled,
                    server.url.as_deref().unwrap_or("-"),
                    server.command.as_deref().unwrap_or("-"),
                    list_or_dash(&server.args),
                    server.working_dir.as_deref().unwrap_or("-"),
                    list_or_dash(&server.hbse_secret_refs),
                    if server.consumer.is_empty() {
                        "-"
                    } else {
                        &server.consumer
                    },
                    if server.purpose.is_empty() {
                        "-"
                    } else {
                        &server.purpose
                    },
                    server
                        .tools
                        .iter()
                        .map(|tool| tool.name.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                    server.discovery_error.as_deref().unwrap_or("-")
                ))
            }
            Some("tools") => {
                let tools = self
                    .tool_registry
                    .schemas()
                    .into_iter()
                    .filter_map(|tool| {
                        tool.get("name")
                            .and_then(Value::as_str)
                            .filter(|name| name.starts_with("mcp::"))
                            .map(|name| name.to_string())
                    })
                    .collect::<Vec<_>>();
                Ok(if tools.is_empty() {
                    "No MCP tools are registered.".to_string()
                } else {
                    tools.join("\n")
                })
            }
            Some("status") | Some("verify") => Ok(self.verify_mcp_checks().join("\n")),
            Some("reload") => {
                self.mcp_servers = load_mcp_servers(&self.data_root)?;
                self.rebuild_tooling_for_cms()?;
                Ok(format!(
                    "Reloaded {} MCP server(s).",
                    self.mcp_servers.len()
                ))
            }
            Some("add-http") => {
                let Some(id) = args.get(1) else {
                    return Ok(
                        "Usage: /mcp add-http <id> <url> <secret_ref> [consumer] [purpose]"
                            .to_string(),
                    );
                };
                let Some(url) = args.get(2) else {
                    return Ok(
                        "Usage: /mcp add-http <id> <url> <secret_ref> [consumer] [purpose]"
                            .to_string(),
                    );
                };
                let Some(secret_ref) = args.get(3) else {
                    return Ok(
                        "Usage: /mcp add-http <id> <url> <secret_ref> [consumer] [purpose]"
                            .to_string(),
                    );
                };
                let id = crate::core::normalize_agent_id(id);
                if id.is_empty() {
                    return Ok(
                        "MCP server id must contain at least one letter or number.".to_string()
                    );
                }
                if !secret_ref.starts_with("secret://") {
                    return Ok("HTTP MCP credentials must be an HBSE secret ref such as secret://vegvisir/mcp/<server>/default.".to_string());
                }
                if contains_secret_like_value(url)
                    || contains_secret_like_value(secret_ref)
                        && !secret_ref.starts_with("secret://")
                {
                    return Ok(
                        "MCP server configuration must not contain plaintext secrets.".to_string(),
                    );
                }
                let consumer = args
                    .get(4)
                    .cloned()
                    .unwrap_or_else(|| format!("vegvisir.mcp.{id}"));
                let purpose = args
                    .get(5)
                    .cloned()
                    .unwrap_or_else(|| "mcp.tool.call".to_string());
                self.upsert_mcp_server(McpServerConfig {
                    id: id.clone(),
                    display_name: id.clone(),
                    transport: McpTransport::Http,
                    command: None,
                    args: Vec::new(),
                    working_dir: None,
                    url: Some(url.to_string()),
                    enabled: true,
                    hbse_secret_refs: vec![secret_ref.to_string()],
                    consumer,
                    purpose,
                    tools: Vec::new(),
                    metadata: BTreeMap::new(),
                    discovery_error: None,
                })?;
                Ok(format!(
                    "Configured HTTP MCP server {id} with HBSE secret ref {secret_ref}."
                ))
            }
            Some("add-http-service") | Some("add-service-http") => {
                let Some(id) = args.get(1) else {
                    return Ok(
                        "Usage: /mcp add-http-service <id> <url> <hbse-service-name>".to_string(),
                    );
                };
                let Some(url) = args.get(2) else {
                    return Ok(
                        "Usage: /mcp add-http-service <id> <url> <hbse-service-name>".to_string(),
                    );
                };
                let Some(service_name) = args.get(3) else {
                    return Ok(
                        "Usage: /mcp add-http-service <id> <url> <hbse-service-name>".to_string(),
                    );
                };
                if contains_secret_like_value(url) {
                    return Ok(
                        "MCP server configuration must not contain plaintext secrets.".to_string(),
                    );
                }
                let id = crate::core::normalize_agent_id(id);
                if id.is_empty() {
                    return Ok(
                        "MCP server id must contain at least one letter or number.".to_string()
                    );
                }
                let service_name = normalize_hbse_ref_segment(service_name, false);
                let Some(service) = self
                    .hbse_services
                    .iter()
                    .find(|service| service.name == service_name)
                else {
                    return Ok(format!(
                        "Unknown HBSE service ref: {service_name}. Use /hbse service add first."
                    ));
                };
                if !service.enabled {
                    return Ok(format!("HBSE service ref {service_name} is disabled."));
                }
                self.upsert_mcp_server(McpServerConfig {
                    id: id.clone(),
                    display_name: id.clone(),
                    transport: McpTransport::Http,
                    command: None,
                    args: Vec::new(),
                    working_dir: None,
                    url: Some(url.to_string()),
                    enabled: true,
                    hbse_secret_refs: vec![service.secret_ref.clone()],
                    consumer: service.consumer.clone(),
                    purpose: service.purpose.clone(),
                    tools: Vec::new(),
                    metadata: BTreeMap::from([(
                        "hbse_service_ref".to_string(),
                        Value::String(service.name.clone()),
                    )]),
                    discovery_error: None,
                })?;
                Ok(format!(
                    "Configured HTTP MCP server {id} from HBSE service ref {service_name}."
                ))
            }
            Some("add-stdio") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /mcp add-stdio <id> <command> [args...]".to_string());
                };
                let Some(command) = args.get(2) else {
                    return Ok("Usage: /mcp add-stdio <id> <command> [args...]".to_string());
                };
                let id = crate::core::normalize_agent_id(id);
                if id.is_empty() {
                    return Ok(
                        "MCP server id must contain at least one letter or number.".to_string()
                    );
                }
                let command_args = args.iter().skip(3).cloned().collect::<Vec<_>>();
                if contains_secret_like_value(command)
                    || command_args
                        .iter()
                        .any(|arg| contains_secret_like_value(arg))
                {
                    return Ok("MCP stdio command configuration must not contain plaintext secrets. Put service credentials in HBSE and reference them through tools or service policies.".to_string());
                }
                self.upsert_mcp_server(McpServerConfig {
                    id: id.clone(),
                    display_name: id.clone(),
                    transport: McpTransport::Stdio,
                    command: Some(command.to_string()),
                    args: command_args,
                    working_dir: None,
                    url: None,
                    enabled: true,
                    hbse_secret_refs: Vec::new(),
                    consumer: String::new(),
                    purpose: String::new(),
                    tools: Vec::new(),
                    metadata: BTreeMap::new(),
                    discovery_error: None,
                })?;
                Ok(format!("Configured stdio MCP server {id}."))
            }
            Some("add-tool") => {
                let Some(server_id) = args.get(1) else {
                    return Ok(
                        "Usage: /mcp add-tool <server-id> <tool-name> [description]".to_string()
                    );
                };
                let Some(tool_name) = args.get(2) else {
                    return Ok(
                        "Usage: /mcp add-tool <server-id> <tool-name> [description]".to_string()
                    );
                };
                let description = args
                    .get(3..)
                    .map(|items| items.join(" "))
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| format!("MCP tool {tool_name}"));
                let Some(server) = self
                    .mcp_servers
                    .iter_mut()
                    .find(|server| server.id == *server_id)
                else {
                    return Ok(format!("Unknown MCP server: {server_id}"));
                };
                if !server.tools.iter().any(|tool| tool.name == *tool_name) {
                    server.tools.push(McpToolConfig {
                        name: tool_name.to_string(),
                        description,
                        schema: json!({"properties": {}}),
                    });
                }
                self.save_mcp_config_and_reload()?;
                Ok(format!("Added MCP tool {tool_name} to server {server_id}."))
            }
            Some("remove-tool") | Some("delete-tool") => {
                let Some(server_id) = args.get(1) else {
                    return Ok("Usage: /mcp remove-tool <server-id> <tool-name>".to_string());
                };
                let Some(tool_name) = args.get(2) else {
                    return Ok("Usage: /mcp remove-tool <server-id> <tool-name>".to_string());
                };
                let Some(server) = self
                    .mcp_servers
                    .iter_mut()
                    .find(|server| server.id == *server_id)
                else {
                    return Ok(format!("Unknown MCP server: {server_id}"));
                };
                let before = server.tools.len();
                server.tools.retain(|tool| tool.name != *tool_name);
                if server.tools.len() == before {
                    return Ok(format!(
                        "Unknown MCP tool {tool_name} on server {server_id}."
                    ));
                }
                self.save_mcp_config_and_reload()?;
                Ok(format!(
                    "Removed MCP tool {tool_name} from server {server_id}."
                ))
            }
            Some("remove") | Some("delete") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /mcp remove <id>".to_string());
                };
                let before = self.mcp_servers.len();
                self.mcp_servers.retain(|server| server.id != *id);
                if self.mcp_servers.len() == before {
                    return Ok(format!("Unknown MCP server: {id}"));
                }
                self.save_mcp_config_and_reload()?;
                Ok(format!("Removed MCP server {id}."))
            }
            Some("enable") | Some("disable") => {
                let enable = args.first().map(String::as_str) == Some("enable");
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /mcp <enable|disable> <id>".to_string());
                };
                let Some(server) = self.mcp_servers.iter_mut().find(|server| server.id == *id)
                else {
                    return Ok(format!("Unknown MCP server: {id}"));
                };
                server.enabled = enable;
                self.save_mcp_config_and_reload()?;
                Ok(format!(
                    "{} MCP server {id}.",
                    if enable { "Enabled" } else { "Disabled" }
                ))
            }
            Some(other) => Ok(format!("Unknown /mcp command: {other}")),
        }
    }

    fn upsert_mcp_server(&mut self, server: McpServerConfig) -> anyhow::Result<()> {
        if let Some(existing) = self
            .mcp_servers
            .iter_mut()
            .find(|existing| existing.id == server.id)
        {
            *existing = server;
        } else {
            self.mcp_servers.push(server);
        }
        self.save_mcp_config_and_reload()
    }

    fn save_mcp_config_and_reload(&mut self) -> anyhow::Result<()> {
        McpConfigStore::new(self.data_root.join("mcp.json")).save(&self.mcp_servers)?;
        self.mcp_servers = load_mcp_servers(&self.data_root)?;
        self.rebuild_tooling_for_cms()?;
        Ok(())
    }

    fn hbse_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("status") => {
                let providers = self
                    .provider_registry
                    .list()
                    .into_iter()
                    .filter(|provider| provider.auth_type == "hbse")
                    .map(|provider| {
                        let socket = crate::provider::hbse_default_or_configured_socket(provider);
                        format!(
                            "{:<14} socket={} exists={}",
                            provider.name,
                            socket.display(),
                            socket.exists()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    "HBSE is the auth/secrets layer. Vegvisir only handles secret refs and broker policies; plaintext secrets must be entered into HBSE.\n{providers}\nregistered_services={}\nUsage: /hbse provider <openai|xai|openrouter|groq|mistral|deepseek|together|perplexity|anthropic>\nUsage: /hbse mcp <server> [url] [consumer] [purpose]\nUsage: /hbse service <name> [consumer] [purpose]\nUsage: /hbse service add <name> <secret_ref> [consumer] [purpose]\nUsage: /hbse services",
                    self.hbse_services.len()
                )
            }
            Some("provider") => {
                let Some(provider_id) = args.get(1) else {
                    return "Usage: /hbse provider <provider-id>".to_string();
                };
                hbse_model_provider_setup(provider_id)
            }
            Some("service") | Some("tool") => {
                if args.get(1).map(String::as_str) == Some("add") {
                    return self.hbse_service_add_command(args);
                }
                if matches!(args.get(1).map(String::as_str), Some("show" | "get")) {
                    return self.hbse_service_show_command(args);
                }
                if matches!(args.get(1).map(String::as_str), Some("enable" | "disable")) {
                    return self.hbse_service_toggle_command(args);
                }
                if matches!(args.get(1).map(String::as_str), Some("remove" | "delete")) {
                    return self.hbse_service_remove_command(args);
                }
                let Some(name) = args.get(1) else {
                    return "Usage: /hbse service <name> [consumer] [purpose] | /hbse service add|show|enable|disable|remove".to_string();
                };
                let consumer = args
                    .get(2)
                    .cloned()
                    .unwrap_or_else(|| format!("vegvisir.service.{name}"));
                let purpose = args
                    .get(3)
                    .cloned()
                    .unwrap_or_else(|| "service.access".to_string());
                hbse_service_setup(name, &consumer, &purpose)
            }
            Some("services") => self.hbse_services_command(),
            Some("mcp") => {
                let Some(name) = args.get(1) else {
                    return "Usage: /hbse mcp <server> [url] [consumer] [purpose]".to_string();
                };
                let maybe_url = args
                    .get(2)
                    .filter(|value| value.starts_with("http://") || value.starts_with("https://"));
                let consumer_index = if maybe_url.is_some() { 3 } else { 2 };
                let purpose_index = consumer_index + 1;
                let consumer = args
                    .get(consumer_index)
                    .cloned()
                    .unwrap_or_else(|| format!("vegvisir.mcp.{name}"));
                let purpose = args
                    .get(purpose_index)
                    .cloned()
                    .unwrap_or_else(|| "mcp.tool.call".to_string());
                hbse_mcp_setup(name, maybe_url.map(String::as_str), &consumer, &purpose)
            }
            Some(other) => format!("Unknown /hbse command: {other}"),
        }
    }

    fn hbse_services_command(&self) -> String {
        if self.hbse_services.is_empty() {
            return "No HBSE service refs registered. Use /hbse service add <name> <secret_ref> [consumer] [purpose].".to_string();
        }
        self.hbse_services
            .iter()
            .map(|service| {
                format!(
                    "{:<18} enabled={} secret_ref={} consumer={} purpose={}",
                    service.name,
                    service.enabled,
                    service.secret_ref,
                    service.consumer,
                    service.purpose
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn hbse_service_show_command(&self, args: &[String]) -> String {
        let Some(name) = args.get(2) else {
            return "Usage: /hbse service show <name>".to_string();
        };
        let name = normalize_hbse_ref_segment(name, false);
        let Some(service) = self
            .hbse_services
            .iter()
            .find(|service| service.name == name)
        else {
            return format!("Unknown HBSE service ref: {name}");
        };
        format!(
            "name={}\nenabled={}\nsecret_ref={}\nconsumer={}\npurpose={}\nmetadata_keys={}",
            service.name,
            service.enabled,
            service.secret_ref,
            service.consumer,
            service.purpose,
            service
                .metadata
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    fn hbse_service_toggle_command(&mut self, args: &[String]) -> String {
        let Some(action) = args.get(1).map(String::as_str) else {
            return "Usage: /hbse service <enable|disable> <name>".to_string();
        };
        let Some(name) = args.get(2) else {
            return "Usage: /hbse service <enable|disable> <name>".to_string();
        };
        let name = normalize_hbse_ref_segment(name, false);
        let Some(service) = self
            .hbse_services
            .iter_mut()
            .find(|service| service.name == name)
        else {
            return format!("Unknown HBSE service ref: {name}");
        };
        service.enabled = action == "enable";
        match self.save_hbse_services() {
            Ok(()) => format!(
                "{} HBSE service ref {name}.",
                if action == "enable" {
                    "Enabled"
                } else {
                    "Disabled"
                }
            ),
            Err(error) => format!("Failed to save HBSE service refs: {error}"),
        }
    }

    fn hbse_service_add_command(&mut self, args: &[String]) -> String {
        let Some(name) = args.get(2) else {
            return "Usage: /hbse service add <name> <secret_ref> [consumer] [purpose]".to_string();
        };
        let Some(secret_ref) = args.get(3) else {
            return "Usage: /hbse service add <name> <secret_ref> [consumer] [purpose]".to_string();
        };
        if !secret_ref.starts_with("secret://") {
            return "HBSE service refs must use secret:// references, not plaintext credentials."
                .to_string();
        }
        let name = normalize_hbse_ref_segment(name, false);
        if name.is_empty() {
            return "HBSE service name must contain at least one letter or number.".to_string();
        }
        let consumer = args
            .get(4)
            .cloned()
            .unwrap_or_else(|| format!("vegvisir.service.{name}"));
        let purpose = args
            .get(5)
            .cloned()
            .unwrap_or_else(|| "service.access".to_string());
        let service = HbseServiceRef {
            name: name.clone(),
            secret_ref: secret_ref.to_string(),
            consumer,
            purpose,
            enabled: true,
            metadata: BTreeMap::new(),
        };
        if let Some(existing) = self
            .hbse_services
            .iter_mut()
            .find(|existing| existing.name == service.name)
        {
            *existing = service;
        } else {
            self.hbse_services.push(service);
        }
        match self.save_hbse_services() {
            Ok(()) => format!("Registered HBSE service ref {name}."),
            Err(error) => format!("Failed to save HBSE service ref {name}: {error}"),
        }
    }

    fn hbse_service_remove_command(&mut self, args: &[String]) -> String {
        let Some(name) = args.get(2) else {
            return "Usage: /hbse service remove <name>".to_string();
        };
        let name = normalize_hbse_ref_segment(name, false);
        let before = self.hbse_services.len();
        self.hbse_services.retain(|service| service.name != name);
        if self.hbse_services.len() == before {
            return format!("Unknown HBSE service ref: {name}");
        }
        match self.save_hbse_services() {
            Ok(()) => format!("Removed HBSE service ref {name}."),
            Err(error) => format!("Failed to save HBSE service refs: {error}"),
        }
    }

    fn save_hbse_services(&self) -> anyhow::Result<()> {
        HbseServiceRefStore::new(self.data_root.join("hbse-services.json"))
            .save(&self.hbse_services)?;
        Ok(())
    }

    fn openai_sso_status(&self) -> String {
        crate::openai_sso::OpenAISsoAuthStore::new(Some(self.data_root.clone())).status()
    }

    fn openai_sso_login(&self) -> String {
        crate::openai_sso::login(Some(self.data_root.clone()), true, Duration::from_secs(300))
            .unwrap_or_else(|error| error.to_string())
    }

    fn save_global_model_defaults(&self) -> anyhow::Result<()> {
        self.save_config_defaults()
    }

    fn save_config_defaults(&self) -> anyhow::Result<()> {
        let mut data = self.config.load().unwrap_or_default();
        data.insert(
            "current_provider".to_string(),
            json!(self.session.current_provider),
        );
        data.insert(
            "current_model".to_string(),
            json!(self.session.current_model),
        );
        data.insert(
            "system_prompt".to_string(),
            json!(self.session.system_prompt),
        );
        self.config.save(&data)
    }
}

pub fn run_tui() -> anyhow::Result<()> {
    run_tui_with_dangerous_bypass(false)
}

pub fn run_tui_with_dangerous_bypass(
    dangerously_bypass_approvals_and_sandbox: bool,
) -> anyhow::Result<()> {
    let mut app = TuiApplication::new_with_dangerous_bypass(
        std::env::current_dir()?,
        dangerously_bypass_approvals_and_sandbox,
    )?;
    if !io::stdin().is_terminal() {
        print!("{}", app.render());
        return Ok(());
    }
    app.run()?;
    Ok(())
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> anyhow::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableMouseCapture
        )?;
        stdout.flush()?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            DisableMouseCapture,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
    }
}

fn join_or(args: &[String], default: &str) -> String {
    if args.is_empty() {
        default.to_string()
    } else {
        args.join(" ")
    }
}

fn list_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(", ")
    }
}

fn comma_items(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_limit_and_global(args: &[String], default_limit: usize) -> (usize, bool) {
    let mut limit = default_limit;
    let mut global = false;
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--global" | "--all" => global = true,
            "--project" | "--local" => global = false,
            "--limit" | "-n" => {
                if let Some(value) = iter.next() {
                    limit = value.parse::<usize>().unwrap_or(default_limit).clamp(1, 50);
                }
            }
            value if value.starts_with("--limit=") => {
                limit = value
                    .trim_start_matches("--limit=")
                    .parse::<usize>()
                    .unwrap_or(default_limit)
                    .clamp(1, 50);
            }
            _ => {}
        }
    }
    (limit, global)
}

fn parse_chatgpt_import_args(args: &[String]) -> anyhow::Result<(PathBuf, usize, usize)> {
    let mut path = None;
    let mut messages_per_memory = 40usize;
    let mut max_chars_per_memory = 0usize;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--messages-per-memory" => {
                let Some(value) = args.get(index + 1) else {
                    anyhow::bail!("Missing value for --messages-per-memory");
                };
                messages_per_memory = value
                    .parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("Invalid --messages-per-memory value: {value}"))?
                    .max(1);
                index += 2;
            }
            "--max-chars-per-memory" => {
                let Some(value) = args.get(index + 1) else {
                    anyhow::bail!("Missing value for --max-chars-per-memory");
                };
                max_chars_per_memory = value.parse::<usize>().map_err(|_| {
                    anyhow::anyhow!("Invalid --max-chars-per-memory value: {value}")
                })?;
                index += 2;
            }
            value if value.starts_with("--") => {
                anyhow::bail!("Unknown import-chatgpt option: {value}");
            }
            value => {
                if path.is_some() {
                    anyhow::bail!(
                        "Usage: /memory import-chatgpt <export-dir-or-conversations.json> [--messages-per-memory N] [--max-chars-per-memory N]"
                    );
                }
                path = Some(expand_workspace_path(value));
                index += 1;
            }
        }
    }
    let Some(path) = path else {
        anyhow::bail!(
            "Usage: /memory import-chatgpt <export-dir-or-conversations.json> [--messages-per-memory N] [--max-chars-per-memory N]"
        );
    };
    Ok((path, messages_per_memory, max_chars_per_memory))
}

fn parse_limit(args: &[String], default_limit: usize) -> usize {
    let mut limit = default_limit;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--limit" | "-n" => {
                if let Some(value) = iter.next() {
                    limit = value
                        .parse::<usize>()
                        .unwrap_or(default_limit)
                        .clamp(1, 200);
                }
            }
            value if value.starts_with("--limit=") => {
                limit = value
                    .trim_start_matches("--limit=")
                    .parse::<usize>()
                    .unwrap_or(default_limit)
                    .clamp(1, 200);
            }
            _ => {}
        }
    }
    limit
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

fn configured_user_id(defaults: &BTreeMap<String, Value>) -> String {
    if let Some(user_id) = defaults
        .get("current_user_id")
        .and_then(Value::as_str)
        .filter(|user_id| !user_id.trim().is_empty())
    {
        return user_id.to_string();
    }
    std::env::var("VEGVISIR_USER_ID")
        .ok()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| "local-user".to_string())
}

fn session_root_for_user(data_root: &Path, user_id: &str) -> PathBuf {
    if user_id == "local-user" {
        return data_root.join("sessions");
    }
    data_root
        .join("users")
        .join(user_storage_slug(user_id))
        .join("sessions")
}

fn workspace_index_path_for_user(data_root: &Path, user_id: &str) -> PathBuf {
    if user_id == "local-user" {
        return data_root.join("workspaces.json");
    }
    data_root
        .join("users")
        .join(user_storage_slug(user_id))
        .join("workspaces.json")
}

fn user_storage_slug(user_id: &str) -> String {
    let slug = crate::core::normalize_agent_id(user_id);
    if slug.is_empty() {
        "local-user".to_string()
    } else {
        slug
    }
}

fn validate_user_id(user_id: &str) -> anyhow::Result<()> {
    let valid = !user_id.trim().is_empty()
        && user_id.len() <= 128
        && user_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '@'))
        && !contains_secret_like_value(user_id);
    if !valid {
        anyhow::bail!(
            "User id must be 1-128 chars and contain only letters, numbers, '-', '_', '.', ':', or '@', with no secret-like material."
        );
    }
    Ok(())
}

fn contains_secret_like_value(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "api_key=",
        "apikey=",
        "access_token=",
        "auth_token=",
        "bearer ",
        "password=",
        "secret=",
        "token=",
        "sk-",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

fn self_model_invalid(models: &ModelRegistry, provider: &str, model_name: &str) -> bool {
    let Some(model) = models.get(model_name) else {
        return true;
    };
    !models.is_model_allowed_for_provider(model, provider)
}

fn model_known_but_invalid(models: &ModelRegistry, provider: &str, model_name: &str) -> bool {
    let Some(model) = models.get(model_name) else {
        return false;
    };
    !models.is_model_allowed_for_provider(model, provider)
}

fn set_openai_sso_auth_root(registry: &mut ProviderRegistry, data_root: &Path) {
    if let Some(provider) = registry.get_mut("openai-sso") {
        provider
            .metadata
            .entry("auth_root".to_string())
            .or_insert_with(|| Value::String(data_root.display().to_string()));
    }
}

fn canonical_workspace(path: &Path) -> anyhow::Result<PathBuf> {
    if !path.exists() {
        anyhow::bail!("Workspace path does not exist: {}", path.display());
    }
    if !path.is_dir() {
        anyhow::bail!("Workspace path is not a directory: {}", path.display());
    }
    Ok(path.canonicalize()?)
}

fn expand_workspace_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(raw));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(raw)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn workspace_title(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("workspace")
        .to_string()
}

pub fn workspace_project_id(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    format!(
        "workspace:{}:{}",
        workspace_title(&canonical),
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

#[derive(Clone)]
struct AgentTemplate {
    mode: String,
    display_name: String,
    description: String,
    system_prompt: String,
    enabled_tools: Vec<String>,
    enabled_skills: Vec<String>,
    usrl_contracts: Vec<String>,
    memory_policy: String,
}

fn agent_template(mode: &str) -> Option<AgentTemplate> {
    let normalized = crate::core::normalize_agent_id(mode);
    agent_templates()
        .into_iter()
        .find(|template| template.mode == normalized)
}

fn agent_templates() -> Vec<AgentTemplate> {
    vec![
        template(
            "planner",
            "Planner",
            "Decomposes goals into staged, verifiable plans.",
            "You are a planning specialist. Convert ambiguous goals into concrete phases, dependencies, risks, acceptance checks, and next actions. Do not edit files unless explicitly asked through an enabled tool path.",
            &[
                "list_files",
                "read_file",
                "cms_recall",
                "cms_recent",
                "eternium_prepare_context",
                "save_session",
            ],
        ),
        template(
            "researcher",
            "Researcher",
            "Finds, compares, and summarizes project evidence.",
            "You are a research specialist. Gather relevant local context, distinguish evidence from inference, cite files or memories when available, and produce concise findings with uncertainty called out.",
            &[
                "list_files",
                "read_file",
                "cms_recall",
                "cms_recent",
                "cms_remember",
                "eternium_prepare_context",
            ],
        ),
        template(
            "orchestrator",
            "Orchestrator",
            "Coordinates specialist agents and tracks execution state.",
            "You are an orchestration specialist. Break work into bounded tasks, delegate when useful, merge results, maintain task state, and keep execution aligned with the user's current objective.",
            &[
                "list_files",
                "read_file",
                "cms_recall",
                "cms_recent",
                "eternium_prepare_context",
                "spawn_subagent",
                "save_session",
                "audit_log",
            ],
        ),
        template(
            "engineer",
            "Engineer",
            "Implements scoped code changes with verification.",
            "You are an engineering specialist. Read the surrounding code before changing it, make minimal coherent edits, preserve existing behavior unless intentionally changed, and verify with focused tests.",
            &[
                "list_files",
                "read_file",
                "write_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_remember",
                "eternium_prepare_context",
                "audit_log",
            ],
        ),
        template(
            "coder",
            "Coder",
            "Focuses on implementation details and local patches.",
            "You are a coding specialist. Implement the requested behavior directly, keep patches small, follow local style, and report the exact verification performed.",
            &[
                "list_files",
                "read_file",
                "write_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_remember",
            ],
        ),
        template(
            "tester",
            "Tester",
            "Designs and runs verification for changed behavior.",
            "You are a testing specialist. Identify behavioral risk, add or run targeted tests, explain failures in terms of expected versus actual behavior, and avoid unrelated rewrites.",
            &[
                "list_files",
                "read_file",
                "write_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_remember",
                "audit_log",
            ],
        ),
        template(
            "agent-red",
            "Agent Red",
            "Security-oriented review and adversarial analysis.",
            "You are Agent Red, a security specialist. Focus on abuse cases, privilege boundaries, secret handling, injection paths, unsafe execution, and concrete mitigations. Treat secrets and credentials as out of scope for direct access.",
            &[
                "list_files",
                "read_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_remember",
                "eternium_prepare_context",
                "audit_log",
            ],
        ),
    ]
}

fn template(
    mode: &str,
    display_name: &str,
    description: &str,
    system_prompt: &str,
    enabled_tools: &[&str],
) -> AgentTemplate {
    AgentTemplate {
        mode: mode.to_string(),
        display_name: display_name.to_string(),
        description: description.to_string(),
        system_prompt: system_prompt.to_string(),
        enabled_tools: enabled_tools.iter().map(|tool| tool.to_string()).collect(),
        enabled_skills: Vec::new(),
        usrl_contracts: Vec::new(),
        memory_policy: "agent-scoped".to_string(),
    }
}

fn api_key_notice(provider: &ProviderConfig) -> String {
    let Some(env) = &provider.api_key_env else {
        return String::new();
    };
    if provider.auth_type != "api_key" {
        return String::new();
    }
    if !direct_provider_auth_allowed() {
        return format!(
            "{} direct API-key auth is disabled in production mode.\nConfigure the secret in HBSE with /hbse provider {}, then select the HBSE-routed provider when available, for example: /provider {}-hbse",
            provider.display_name.as_deref().unwrap_or(&provider.name),
            canonical_hbse_provider_id(&provider.name),
            canonical_hbse_provider_id(&provider.name)
        );
    }
    if get_env(env).is_some() {
        return format!(
            "{env} environment variable is set for legacy direct-provider use.\nProduction auth should use HBSE instead. Run: /hbse provider {}",
            canonical_hbse_provider_id(&provider.name)
        );
    }
    format!(
        "{} direct API-key auth is a legacy fallback and {env} is not set.\nProduction auth must be configured in HBSE so Vegvisir never sees the secret. Run: /hbse provider {}\nThen select the HBSE-routed provider when available, for example: /provider {}-hbse",
        provider.display_name.as_deref().unwrap_or(&provider.name),
        canonical_hbse_provider_id(&provider.name),
        canonical_hbse_provider_id(&provider.name)
    )
}

fn provider_auth_detail(provider: &ProviderConfig) -> String {
    match provider.auth_type.as_str() {
        "hbse" => provider
            .metadata
            .get("hbse_secret_ref")
            .and_then(Value::as_str)
            .map(|secret_ref| format!(" secret_ref={secret_ref}"))
            .unwrap_or_else(|| " secret_ref=missing".to_string()),
        "api_key" => provider
            .api_key_env
            .as_deref()
            .map(|env| {
                if direct_provider_auth_allowed() {
                    format!(
                        " legacy_env={env} hbse=/hbse provider {}",
                        canonical_hbse_provider_id(&provider.name)
                    )
                } else {
                    format!(
                        " legacy_env={env} blocked_by_production hbse=/hbse provider {}",
                        canonical_hbse_provider_id(&provider.name)
                    )
                }
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn canonical_hbse_provider_id(provider_name: &str) -> &str {
    provider_name.strip_suffix("-hbse").unwrap_or(provider_name)
}

fn close_provider_matches(registry: &ProviderRegistry, name: &str) -> Vec<String> {
    let needle = name.to_ascii_lowercase();
    registry
        .list()
        .into_iter()
        .filter(|provider| provider.name.to_ascii_lowercase().contains(&needle))
        .take(10)
        .map(|provider| provider.name.clone())
        .collect()
}

fn close_model_matches(registry: &ModelRegistry, provider: &str, name: &str) -> Vec<String> {
    let needle = name.to_ascii_lowercase();
    let mut matches = registry
        .by_provider(provider)
        .into_iter()
        .filter(|model| model.name.to_ascii_lowercase().contains(&needle))
        .map(|model| model.name.clone())
        .collect::<Vec<_>>();
    if matches.is_empty() {
        matches = registry
            .list()
            .into_iter()
            .filter(|model| model.name.to_ascii_lowercase().contains(&needle))
            .map(|model| model.name.clone())
            .collect();
    }
    matches.truncate(10);
    matches
}

fn hbse_model_provider_setup(provider_id: &str) -> String {
    let secret_ref = match provider_id {
        "openai" => "secret://vegvisir/providers/openai/default".to_string(),
        "xai" => "secret://vegvisir/providers/xai/default".to_string(),
        other => format!("secret://vegvisir/providers/{other}/default"),
    };
    let consumer = match provider_id {
        "openai" => "vegvisir.provider.openai-hbse".to_string(),
        "xai" => "vegvisir.provider.xai-hbse".to_string(),
        other => format!("vegvisir.provider.{other}-hbse"),
    };
    format!(
        "Vegvisir will not read or store the provider secret. Run this in a trusted terminal and paste the secret into HBSE stdin, then press Ctrl-D:\n\nhbse model-provider setup {provider_id} --stdin --secret-ref {secret_ref} --consumer {consumer} --purpose model.chat --model-discovery-purpose model.discovery\n\nAfter setup, select the HBSE-routed provider in Vegvisir, for example /provider {provider_id}-hbse when that provider exists in the catalog."
    )
}

fn hbse_service_setup(name: &str, consumer: &str, purpose: &str) -> String {
    let normalized = normalize_hbse_ref_segment(name, true);
    let secret_ref = format!("secret://vegvisir/services/{normalized}/default");
    let policy_id = format!("vegvisir-service-{normalized}");
    let policy = json!({
        "policy_id": policy_id,
        "secret_refs": [secret_ref],
        "allowed_consumers": [consumer],
        "denied_consumers": [],
        "allowed_purposes": [purpose],
        "denied_purposes": [],
        "allowed_delivery_modes": ["brokered_operation"],
        "allowed_http_hosts": [],
        "denied_http_hosts": [],
        "allowed_http_methods": [],
        "denied_http_methods": [],
        "allowed_http_path_prefixes": [],
        "denied_http_path_prefixes": [],
        "require_https_for_brokered_http": true,
        "max_http_request_body_bytes": null,
        "allowed_os_uids": [],
        "denied_os_uids": [],
        "allowed_executable_paths": [],
        "denied_executable_paths": [],
        "allowed_executable_sha256": [],
        "denied_executable_sha256": [],
        "exportable": false,
        "max_ticket_ttl_seconds": 60,
        "max_uses": 1,
        "minimum_provider_assurance": "A1",
        "require_mfa": false,
        "expires_at": null
    });
    format!(
        "Vegvisir will not read or store the service secret. Run these in a trusted terminal and paste the secret into HBSE stdin, then press Ctrl-D:\n\nhbse secret put {secret_ref} --stdin\n\nhbse policy put --stdin <<'JSON'\n{}\nJSON\n\nUse secret_ref={secret_ref}, consumer={consumer}, purpose={purpose} from Vegvisir tools or service adapters.",
        serde_json::to_string_pretty(&policy).unwrap_or_else(|_| policy.to_string())
    )
}

fn hbse_mcp_setup(name: &str, url: Option<&str>, consumer: &str, purpose: &str) -> String {
    let normalized = normalize_hbse_ref_segment(name, false);
    let secret_ref = format!("secret://vegvisir/mcp/{normalized}/default");
    let policy_id = format!("vegvisir-mcp-{normalized}");
    let host = url.and_then(http_host_from_url);
    let path_prefix = url
        .and_then(http_path_prefix_from_url)
        .map(|prefix| vec![prefix])
        .unwrap_or_default();
    let policy = json!({
        "policy_id": policy_id,
        "secret_refs": [secret_ref],
        "allowed_consumers": [consumer],
        "denied_consumers": [],
        "allowed_purposes": [purpose],
        "denied_purposes": [],
        "allowed_delivery_modes": ["brokered_http"],
        "allowed_http_hosts": host.map(|value| vec![value]).unwrap_or_default(),
        "denied_http_hosts": [],
        "allowed_http_methods": ["GET", "POST", "DELETE"],
        "denied_http_methods": [],
        "allowed_http_path_prefixes": path_prefix,
        "denied_http_path_prefixes": [],
        "require_https_for_brokered_http": url.map(|value| value.starts_with("https://")).unwrap_or(true),
        "max_http_request_body_bytes": 10485760,
        "allowed_os_uids": [],
        "denied_os_uids": [],
        "allowed_executable_paths": [],
        "denied_executable_paths": [],
        "allowed_executable_sha256": [],
        "denied_executable_sha256": [],
        "exportable": false,
        "max_ticket_ttl_seconds": 60,
        "max_uses": 1,
        "minimum_provider_assurance": "A1",
        "require_mfa": false,
        "expires_at": null
    });
    let url_hint = url
        .map(|value| format!(" for {value}"))
        .unwrap_or_else(|| " for the MCP server URL you configure in /mcp add-http".to_string());
    format!(
        "Vegvisir will not read or store the MCP service credential. Run these in a trusted terminal and paste the credential into HBSE stdin, then press Ctrl-D:\n\nhbse secret put {secret_ref} --stdin\n\nhbse policy put --stdin <<'JSON'\n{}\nJSON\n\nThen configure the MCP server in Vegvisir:\n/mcp add-http {normalized} <url> {secret_ref} {consumer} {purpose}\n\nThis grants only brokered HTTP access{url_hint}; Vegvisir stores the secret ref, consumer, and purpose, never the credential.",
        serde_json::to_string_pretty(&policy).unwrap_or_else(|_| policy.to_string())
    )
}

fn normalize_hbse_ref_segment(value: &str, allow_slash: bool) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric()
                || matches!(ch, '-' | '_' | '.')
                || (allow_slash && ch == '/')
            {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
}

fn http_host_from_url(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority)
        .split(':')
        .next()
        .unwrap_or(authority)
        .trim();
    (!host.is_empty()).then(|| host.to_string())
}

fn http_path_prefix_from_url(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let path = after_scheme
        .split_once('/')
        .map(|(_, path)| format!("/{path}"))
        .unwrap_or_else(|| "/".to_string());
    let path = path.split(['?', '#']).next().unwrap_or("/");
    Some(if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    })
}

pub(crate) fn terminal_frame(rendered: &str) -> String {
    rendered
        .split('\n')
        .map(|line| format!("{line}\x1b[K"))
        .collect::<Vec<_>>()
        .join("\r\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn terminal_frame_returns_carriage_on_each_rendered_line() {
        assert_eq!(
            super::terminal_frame("one\ntwo\nthree"),
            "one\x1b[K\r\ntwo\x1b[K\r\nthree\x1b[K"
        );
    }
}
