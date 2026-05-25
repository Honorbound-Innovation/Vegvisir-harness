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
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use serde_json::{Value, json};

use crate::{
    attachments::{attachment_for, extract_attachments},
    command_registry::CommandRegistry,
    core::{
        AgentProfileStore, ChatMessage, ConfigStore, HbseServiceRef, HbseServiceRefStore,
        McpConfigStore, McpServerConfig, McpToolConfig, McpTransport, ModelRegistry,
        ProviderConfig, ProviderRegistry, SessionManager, SessionState, SessionStore,
        default_system_prompt, default_tool_definitions, load_skill_definitions,
    },
    environment::get_env,
    guardrails::{
        ApprovalRequest, GuardrailEngine, PermissionPolicy, command_name_from_args,
        default_allowed_commands, normalize_command_name,
    },
    lsl::{LslSkillTrace, append_skill_trace, update_skill_metrics_for_load},
    mcp::{load_mcp_servers, register_mcp_tools},
    memory::{VegvisirCms, VegvisirCmsConfig, default_vegvisir_data_root},
    model_discovery::discover_provider_models,
    observability::EventLogger,
    policy::RuntimePolicy,
    provider::{
        ConversationRunner, ProviderRouter, ProviderRunEvent, configured_max_tool_rounds,
        direct_provider_auth_allowed, max_tool_rounds_hard_limit, set_runtime_max_tool_rounds,
    },
    subagents::{SubAgentStatus, SubAgentTaskRecord},
    tools::{ToolExecutor, ToolRegistry, build_builtin_registry_with_cms_and_mode},
    types::ToolCall,
    ui::{
        input::{InputState, Suggestion},
        layout::LayoutRenderer,
    },
};

mod commands;
mod lsl_runtime;

use lsl_runtime::{compiled_lsl_selected_from_trace, prepare_lsl_augmented_content};

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
    pub command_palette_open: bool,
    pub help_overlay_open: bool,
    pub diff_overlay: Option<DiffOverlay>,
    pub diff_scroll_offset: usize,
    pub info_overlay: Option<InfoOverlay>,
    pub info_scroll_offset: usize,
    pub approval_selected_index: usize,
    pub search_open: bool,
    pub search_query: String,
    pub search_match_index: usize,
}

enum StreamEvent {
    Delta(String),
    Activity(String),
    ToolStart {
        name: String,
        args: String,
    },
    ToolEnd {
        name: String,
        ok: bool,
        summary: String,
        detail: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffOverlay {
    pub title: String,
    pub diff: String,
    pub files_changed: usize,
    pub added_lines: usize,
    pub removed_lines: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfoOverlay {
    pub title: String,
    pub body: String,
}

fn command_matches_palette_query(name: &str, description: &str, raw: &str) -> bool {
    let query = raw.trim().trim_start_matches('/').to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }
    let name = name.to_ascii_lowercase();
    let description = description.to_ascii_lowercase();
    name.trim_start_matches('/').starts_with(&query)
        || name.contains(&query)
        || description.contains(&query)
}

fn should_refresh_suggestions_before_key(key: &KeyEvent) -> bool {
    !matches!(
        key.code,
        KeyCode::Enter | KeyCode::Up | KeyCode::Down | KeyCode::Tab
    )
}

fn diff_overlay_from_patch(title: &str, diff: &str) -> DiffOverlay {
    let files_changed = diff
        .lines()
        .filter(|line| line.starts_with("diff --git "))
        .count();
    let added_lines = diff
        .lines()
        .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
        .count();
    let removed_lines = diff
        .lines()
        .filter(|line| line.starts_with('-') && !line.starts_with("---"))
        .count();
    DiffOverlay {
        title: title.to_string(),
        diff: diff.to_string(),
        files_changed,
        added_lines,
        removed_lines,
    }
}

fn apply_scroll_delta(current: usize, delta: isize) -> usize {
    if delta >= 0 {
        current.saturating_add(delta as usize)
    } else {
        current.saturating_sub(delta.unsigned_abs())
    }
}

fn should_show_info_overlay(command: &str, response: &str) -> bool {
    if response.trim().is_empty() || response.lines().count() < 3 {
        return false;
    }
    matches!(
        command,
        "/tools"
            | "/skills"
            | "/context"
            | "/models"
            | "/providers"
            | "/sessions"
            | "/projects"
            | "/approvals"
            | "/system"
            | "/system-prompt"
            | "/trace"
            | "/config"
            | "/mcp"
            | "/hbse"
    )
}

#[derive(Clone, Debug)]
pub(crate) struct LslRuntimeConfig {
    mode: String,
    token_budget: usize,
    max_primary_subskills: usize,
    max_total_subskills: usize,
    max_dependency_depth: usize,
    allow_extended: bool,
    semantic_router: bool,
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
                    require_human_approval: !dangerously_bypass_approvals_and_sandbox,
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
            command_palette_open: false,
            help_overlay_open: false,
            diff_overlay: None,
            diff_scroll_offset: 0,
            info_overlay: None,
            info_scroll_offset: 0,
            approval_selected_index: 0,
            search_open: false,
            search_query: String::new(),
            search_match_index: 0,
        };
        app.rebuild_tooling_for_cms()?;
        let provider = app.session.current_provider.clone();
        let _ = app.refresh_provider_models(&provider);
        Ok(app)
    }

    pub fn render(&mut self) -> String {
        let suggestions = self.build_suggestions();
        self.input.update_suggestions(suggestions);
        let pending_approvals = self.pending_approval_requests();
        self.renderer.render_startup(
            &self.session,
            &self.commands,
            &self.input,
            &self.input.suggestions,
            self.input.selected_suggestion,
            self.chat_scroll_offset,
            &pending_approvals,
        )
    }

    fn pending_approval_requests(&self) -> Vec<ApprovalRequest> {
        self.tool_executor
            .guardrails
            .approvals
            .pending()
            .into_values()
            .collect()
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
            .filter(|command| {
                command_matches_palette_query(&command.name, &command.description, raw)
            })
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
                self.clear_requested = true;
                self.redraw_requested = true;
                "Full redraw requested.".to_string()
            }
            "/cancel" => self.cancel_pending_response(),
            "/history" => self.history(),
            "/diff" => self.diff_command(&args)?,
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
            "/tool-limit" => self.tool_limit_command(&args),
            "/approvals" => self.approvals_command(&args),
            "/skills" => self.skills_command(&args)?,
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
            "/work" => self.work_command(&args),
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
        self.update_command_overlay(&command, &response);
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

    fn update_command_overlay(&mut self, command: &str, response: &str) {
        if should_show_info_overlay(command, response) {
            self.info_scroll_offset = 0;
            self.info_overlay = Some(InfoOverlay {
                title: command.trim_start_matches('/').replace('-', " "),
                body: response.to_string(),
            });
        }
    }

    fn work_command(&mut self, args: &[String]) -> String {
        let limit = parse_limit(args, 40);
        let body = self.work_activity_report(limit);
        self.info_scroll_offset = 0;
        self.info_overlay = Some(InfoOverlay {
            title: "work activity".to_string(),
            body: body.clone(),
        });
        body
    }

    fn work_activity_report(&self, limit: usize) -> String {
        let mut events = self.logger.events();
        if events.len() > limit {
            events = events.split_off(events.len() - limit);
        }
        let mut lines = vec![
            format!("Work activity for session {}", self.session.session_id),
            format!("workspace: {}", self.cwd.display()),
            format!("status: {}", self.session.status),
            String::new(),
        ];
        if self.pending_send.is_some() {
            lines.push("running: model response in progress".to_string());
        }
        if !self.session.activity.trim().is_empty() {
            lines.push(format!("activity: {}", self.session.activity));
        }
        let pending = self.tool_executor.guardrails.approvals.pending();
        if !pending.is_empty() {
            lines.push(String::new());
            lines.push("Pending approvals".to_string());
            for approval in pending.values() {
                lines.push(format!(
                    "? {} {} approval_id={}",
                    approval.risk_label, approval.tool_name, approval.id
                ));
            }
        }
        lines.push(String::new());
        lines.push("Recent events".to_string());
        if events.is_empty() {
            lines.push("No trace events recorded yet.".to_string());
        } else {
            for event in events {
                lines.push(format!(
                    "{} {} {}",
                    event.timestamp.format("%H:%M:%S"),
                    event.name,
                    compact_json(&event.payload)
                ));
            }
        }
        lines.join("\n")
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

    fn diff_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let staged = args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--staged" | "--cached" | "staged" | "cached"));
        let stat = args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--stat" | "stat"));
        let paths = args
            .iter()
            .filter(|arg| {
                !matches!(
                    arg.as_str(),
                    "--staged" | "--cached" | "staged" | "cached" | "--stat" | "stat"
                )
            })
            .collect::<Vec<_>>();
        let mut command = std::process::Command::new("git");
        command
            .arg("-C")
            .arg(&self.cwd)
            .arg("--no-pager")
            .arg("diff")
            .arg("--no-ext-diff")
            .arg("--color=never");
        if staged {
            command.arg("--cached");
        }
        if stat {
            command.arg("--stat");
        }
        if !paths.is_empty() {
            command.arg("--");
            for path in paths {
                command.arg(path);
            }
        }
        let output = command.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            anyhow::bail!(
                "git diff failed{}",
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(": {stderr}")
                }
            );
        }
        let diff = String::from_utf8_lossy(&output.stdout).to_string();
        if diff.trim().is_empty() {
            return Ok(if staged {
                "No staged changes.".to_string()
            } else {
                "No workspace changes.".to_string()
            });
        }
        if stat {
            return Ok(format!("Git diff stat\n\n```text\n{diff}\n```"));
        }
        let overlay = diff_overlay_from_patch("Git diff", &diff);
        self.diff_scroll_offset = 0;
        self.diff_overlay = Some(overlay);
        Ok(format!("Git diff\n\n```diff\n{diff}\n```"))
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
            Some("skills") | Some("lsl") => self.skills_config_command(&args[1..]),
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
            event_sink: None,
        };
        let (model_content, skill_trace) = self.prepare_lsl_for_content(content)?;
        let envelope = self.cms.prepare_cached_prompt(
            &model_content,
            self.session.current_provider.clone(),
            self.session.current_model.clone(),
        )?;
        let response = runner.send_with_envelope(&mut self.session, &model_content, envelope)?;
        if let Some(trace) = skill_trace {
            let _ = append_skill_trace(&self.skill_trace_path(), trace);
        }
        let _ = self.cms.complete_turn(content, &response);
        self.autosave_session();
        Ok(response)
    }

    pub fn send_headless(&mut self, content: &str) -> anyhow::Result<String> {
        self.send_headless_streaming(content, &mut |_| {})
    }

    pub fn send_headless_streaming(
        &mut self,
        content: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
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
            event_sink: None,
        };
        let (model_content, skill_trace) = self.prepare_lsl_for_content(content)?;
        let envelope = self.cms.prepare_cached_prompt(
            &model_content,
            self.session.current_provider.clone(),
            self.session.current_model.clone(),
        )?;
        let response = runner.send_with_envelope_streaming(
            &mut self.session,
            &model_content,
            envelope,
            on_delta,
        )?;
        if let Some(trace) = skill_trace {
            let _ = append_skill_trace(&self.skill_trace_path(), trace);
        }
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

        if attachments.is_empty()
            && let Some(response) = self.try_handle_natural_agent_template_request(&content)
        {
            self.session.messages.push(ChatMessage {
                role: "user".to_string(),
                content,
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            });
            self.push_system_message(response);
            self.autosave_session();
            self.chat_scroll_offset = 0;
            self.redraw_requested = true;
            return;
        }

        self.start_background_send(content, attachments);
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        if should_refresh_suggestions_before_key(&key) {
            let suggestions = self.build_suggestions();
            self.input.update_suggestions(suggestions);
        }
        if self.handle_search_key(key) {
            self.redraw_requested = true;
            return;
        }
        if self.handle_pending_approval_key(key) {
            self.redraw_requested = true;
            return;
        }
        if self.help_overlay_open {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => self.help_overlay_open = false,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.handle_ctrl_c();
                }
                _ => {}
            }
            self.redraw_requested = true;
            return;
        }
        if self.diff_overlay.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.diff_overlay = None;
                    self.diff_scroll_offset = 0;
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.handle_ctrl_c();
                }
                KeyCode::PageUp => {
                    self.diff_scroll_offset = self
                        .diff_scroll_offset
                        .saturating_add(self.chat_page_size());
                }
                KeyCode::PageDown => {
                    self.diff_scroll_offset = self
                        .diff_scroll_offset
                        .saturating_sub(self.chat_page_size());
                }
                KeyCode::Home => self.diff_scroll_offset = usize::MAX / 2,
                KeyCode::End => self.diff_scroll_offset = 0,
                _ => {}
            }
            self.redraw_requested = true;
            return;
        }
        if self.info_overlay.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.info_overlay = None;
                    self.info_scroll_offset = 0;
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.handle_ctrl_c();
                }
                KeyCode::PageUp => {
                    self.info_scroll_offset = self
                        .info_scroll_offset
                        .saturating_add(self.chat_page_size());
                }
                KeyCode::PageDown => {
                    self.info_scroll_offset = self
                        .info_scroll_offset
                        .saturating_sub(self.chat_page_size());
                }
                KeyCode::Home => self.info_scroll_offset = usize::MAX / 2,
                KeyCode::End => self.info_scroll_offset = 0,
                _ => {}
            }
            self.redraw_requested = true;
            return;
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.handle_ctrl_c();
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.open_command_palette();
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.open_search();
            }
            KeyCode::Char('?') if key.modifiers.is_empty() && self.input.buffer.is_empty() => {
                self.help_overlay_open = true;
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.input.append_text("\n", false);
            }
            KeyCode::Enter => {
                if self.command_palette_open {
                    self.accept_palette_selection_for_execution();
                    self.command_palette_open = false;
                    self.handle_submit();
                } else if self.should_execute_selected_slash_suggestion() {
                    self.accept_palette_selection_for_execution();
                    self.handle_submit();
                } else {
                    self.handle_submit();
                }
            }
            KeyCode::Tab => {
                self.input.accept_suggestion();
            }
            KeyCode::Esc => {
                if self.command_palette_open || self.input.buffer == "/" {
                    self.input.clear();
                }
                self.command_palette_open = false;
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
                if self.command_palette_open {
                    self.input
                        .move_selection_by_page(-(self.command_palette_page_size() as isize));
                } else {
                    self.chat_scroll_offset = self
                        .chat_scroll_offset
                        .saturating_add(self.chat_page_size());
                }
            }
            KeyCode::PageDown => {
                if self.command_palette_open {
                    self.input
                        .move_selection_by_page(self.command_palette_page_size() as isize);
                } else {
                    self.chat_scroll_offset = self
                        .chat_scroll_offset
                        .saturating_sub(self.chat_page_size());
                }
            }
            KeyCode::Home => {
                if self.command_palette_open {
                    self.input.selected_suggestion = 0;
                } else if self.input.buffer.is_empty() {
                    self.chat_scroll_offset = usize::MAX / 2;
                } else {
                    self.input.move_cursor_home();
                }
            }
            KeyCode::End => {
                if self.command_palette_open {
                    self.input.selected_suggestion = self.input.suggestions.len().saturating_sub(1);
                } else if self.input.buffer.is_empty() {
                    self.chat_scroll_offset = 0;
                } else {
                    self.input.move_cursor_end();
                }
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if ch == '/' && self.input.buffer.is_empty() && key.modifiers.is_empty() {
                    self.open_command_palette();
                    self.chat_scroll_offset = 0;
                    self.redraw_requested = true;
                    return;
                }
                self.input.append_text(&ch.to_string(), false);
                self.chat_scroll_offset = 0;
            }
            _ => {}
        }
        let suggestions = self.build_suggestions();
        self.input.update_suggestions(suggestions);
        self.redraw_requested = true;
    }

    fn open_command_palette(&mut self) {
        self.input.set_buffer("/");
        self.input.selected_suggestion = 0;
        self.command_palette_open = true;
        let suggestions = self.build_suggestions();
        self.input.update_suggestions(suggestions);
    }

    fn accept_palette_selection_for_execution(&mut self) {
        let replacement = self
            .input
            .suggestions
            .get(self.input.selected_suggestion)
            .map(|suggestion| {
                suggestion
                    .replacement
                    .as_deref()
                    .unwrap_or(&suggestion.value)
                    .to_string()
            });
        if let Some(replacement) = replacement {
            self.input.set_buffer(replacement);
        }
        self.input.suggestions.clear();
        self.input.selected_suggestion = 0;
    }

    fn should_execute_selected_slash_suggestion(&self) -> bool {
        let raw = self.input.buffer.trim();
        if !raw.starts_with('/')
            || raw.contains(char::is_whitespace)
            || self.input.suggestions.is_empty()
        {
            return false;
        }
        let Some((command, _)) = self.commands.parse_with_aliases(raw) else {
            return true;
        };
        self.commands.get(&command).is_none()
    }

    fn open_search(&mut self) {
        self.search_open = true;
        self.command_palette_open = false;
        self.input.update_suggestions(Vec::new());
        self.search_match_index = self
            .search_match_index
            .min(self.search_matches().len().saturating_sub(1));
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> bool {
        if !self.search_open {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.search_open = false;
                true
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.handle_ctrl_c();
                true
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.search_match_index = 0;
                self.jump_to_search_match(0);
                true
            }
            KeyCode::Enter | KeyCode::Down => {
                self.jump_to_search_match(1);
                true
            }
            KeyCode::Up => {
                self.jump_to_search_match(-1);
                true
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.search_query.push(ch);
                self.search_match_index = 0;
                self.jump_to_search_match(0);
                true
            }
            _ => true,
        }
    }

    pub fn search_matches(&self) -> Vec<usize> {
        let query = self.search_query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        self.session
            .messages
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                let role_matches = message.role.to_ascii_lowercase().contains(&query);
                let content_matches = message.content.to_ascii_lowercase().contains(&query);
                (role_matches || content_matches).then_some(index)
            })
            .collect()
    }

    fn jump_to_search_match(&mut self, delta: isize) {
        let matches = self.search_matches();
        if matches.is_empty() {
            self.search_match_index = 0;
            return;
        }
        let len = matches.len() as isize;
        self.search_match_index =
            (self.search_match_index as isize + delta).rem_euclid(len) as usize;
        let message_index = matches[self.search_match_index];
        self.chat_scroll_offset = self.estimated_chat_scroll_offset_for_message(message_index);
    }

    fn estimated_chat_scroll_offset_for_message(&self, message_index: usize) -> usize {
        self.session
            .messages
            .iter()
            .skip(message_index + 1)
            .map(estimated_message_line_count)
            .sum()
    }

    fn handle_pending_approval_key(&mut self, key: KeyEvent) -> bool {
        if !key.modifiers.is_empty() {
            return false;
        }
        let pending_ids = self.tool_executor.guardrails.approvals.pending_ids();
        if pending_ids.is_empty() {
            return false;
        }
        self.approval_selected_index = self
            .approval_selected_index
            .min(pending_ids.len().saturating_sub(1));
        let id = pending_ids[self.approval_selected_index].clone();
        let message = match key.code {
            KeyCode::Up => {
                self.approval_selected_index = self.approval_selected_index.saturating_sub(1);
                return true;
            }
            KeyCode::Down => {
                self.approval_selected_index =
                    (self.approval_selected_index + 1).min(pending_ids.len().saturating_sub(1));
                return true;
            }
            KeyCode::Esc => {
                self.approval_selected_index = 0;
                return true;
            }
            KeyCode::Char('1') | KeyCode::Enter | KeyCode::Char('a') | KeyCode::Char('A') => {
                match self
                    .tool_executor
                    .guardrails
                    .approvals
                    .approve_once_request(&id)
                {
                    Some(request) => self.execute_approved_request("Approved once", request),
                    None => format!("Unknown pending approval: {id}"),
                }
            }
            KeyCode::Char('2') | KeyCode::Char('s') | KeyCode::Char('S') => {
                match self
                    .tool_executor
                    .guardrails
                    .approvals
                    .approve_for_session(&id)
                {
                    Some(request) => self.execute_approved_request(
                        "Approved matching call for this running session",
                        request,
                    ),
                    None => format!("Unknown pending approval: {id}"),
                }
            }
            KeyCode::Char('3') | KeyCode::Char('d') | KeyCode::Char('D') => {
                if self.tool_executor.guardrails.approvals.deny(&id) {
                    format!("Denied approval {id}.")
                } else {
                    format!("Unknown pending approval: {id}")
                }
            }
            _ => return false,
        };
        let remaining = self.tool_executor.guardrails.approvals.pending_len();
        if remaining == 0 {
            self.approval_selected_index = 0;
        } else {
            self.approval_selected_index = self.approval_selected_index.min(remaining - 1);
        }
        self.session.status = "ready".to_string();
        self.session.activity.clear();
        self.push_system_message(message);
        self.autosave_session();
        self.chat_scroll_offset = 0;
        true
    }

    pub fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        let delta = match mouse.kind {
            MouseEventKind::ScrollUp => 3isize,
            MouseEventKind::ScrollDown => -3isize,
            _ => return,
        };
        let pending_approvals = self.tool_executor.guardrails.approvals.pending_len();
        if pending_approvals > 0 {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.approval_selected_index = self.approval_selected_index.saturating_sub(1);
                }
                MouseEventKind::ScrollDown => {
                    self.approval_selected_index =
                        (self.approval_selected_index + 1).min(pending_approvals - 1);
                }
                _ => return,
            }
            self.redraw_requested = true;
            return;
        }
        if self.command_palette_open
            && self.input.buffer.starts_with('/')
            && self.input.buffer.chars().count() > 1
        {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.input.move_selection(-1);
                }
                MouseEventKind::ScrollDown => {
                    self.input.move_selection(1);
                }
                _ => return,
            }
            self.redraw_requested = true;
            return;
        }
        if self.diff_overlay.is_some() {
            self.diff_scroll_offset = apply_scroll_delta(self.diff_scroll_offset, delta);
            self.redraw_requested = true;
            return;
        }
        if self.info_overlay.is_some() {
            self.info_scroll_offset = apply_scroll_delta(self.info_scroll_offset, delta);
            self.redraw_requested = true;
            return;
        }
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
        let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
        let mut terminal = ratatui::Terminal::new(backend)?;
        terminal.clear()?;
        terminal.draw(|frame| crate::tui2::draw(frame, self))?;
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
                terminal.clear()?;
                self.chat_scroll_offset = 0;
                self.clear_requested = false;
                self.redraw_requested = true;
            }
            if self.redraw_requested
                || self.pending_send.is_some()
                || !self.pending_background_jobs.is_empty()
            {
                self.redraw_requested = false;
                terminal.draw(|frame| crate::tui2::draw(frame, self))?;
            }
        }
        terminal.show_cursor()?;
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

    fn push_system_message(&mut self, content: impl Into<String>) {
        self.session.messages.push(ChatMessage {
            role: "system".to_string(),
            content: content.into(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
    }

    pub(crate) fn autosave_session(&self) {
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
        let cwd = self.cwd.clone();
        let data_root = self.data_root.clone();
        let lsl_config = self.lsl_runtime_config();
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
                event_sink: Some(Arc::new({
                    let stream_tx = stream_tx.clone();
                    move |event| {
                        let event = match event {
                            ProviderRunEvent::Activity(activity) => StreamEvent::Activity(activity),
                            ProviderRunEvent::ToolStart { name, args } => {
                                StreamEvent::ToolStart { name, args }
                            }
                            ProviderRunEvent::ToolEnd {
                                name,
                                ok,
                                summary,
                                detail,
                            } => StreamEvent::ToolEnd {
                                name,
                                ok,
                                summary,
                                detail,
                            },
                        };
                        let _ = stream_tx.send(event);
                    }
                })),
            };
            let (model_content, skill_trace) = prepare_lsl_augmented_content(
                &cwd,
                &data_root,
                &display_content,
                &worker_session,
                &lsl_config,
            )?;
            let envelope = cms.prepare_cached_prompt(
                &model_content,
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
                &model_content,
                envelope,
                &mut on_delta,
            )?;
            if worker_cancel_token.load(Ordering::SeqCst) {
                anyhow::bail!("Cancelled");
            }
            if skill_trace
                .as_ref()
                .is_some_and(|trace| trace.event == "auto_load")
            {
                let _ = update_skill_metrics_for_load(
                    &cwd.join("skills"),
                    &compiled_lsl_selected_from_trace(
                        &cwd,
                        &data_root,
                        &display_content,
                        &lsl_config,
                    ),
                    Some(true),
                );
            }
            if let Some(trace) = skill_trace {
                let _ = append_skill_trace(
                    &cwd.join(".vegvisir")
                        .join("compiled")
                        .join("skill_traces.json"),
                    trace,
                );
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
            Ok(Ok(mut session)) => {
                self.merge_live_tool_messages(&mut session);
                self.merge_live_reasoning_trace(&mut session);
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

    fn handle_ctrl_c(&mut self) {
        if self.pending_send.is_some() {
            let _ = self.cancel_pending_response();
        } else {
            self.running = false;
        }
    }

    fn poll_stream_events(&mut self) {
        let mut events = Vec::new();
        if let Some(receiver) = &self.pending_stream {
            while let Ok(event) = receiver.try_recv() {
                events.push(event);
            }
        }
        if events.is_empty() {
            return;
        }
        for event in events {
            match event {
                StreamEvent::Delta(delta) => {
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
                    self.session.messages[assistant_index]
                        .content
                        .push_str(&delta);
                }
                StreamEvent::Activity(activity) => {
                    self.session.activity = activity;
                }
                StreamEvent::ToolStart { name, args } => {
                    self.session.activity = format!("using tool {name}");
                    self.push_live_tool_message(format!("Running tool: {name} {args}"));
                }
                StreamEvent::ToolEnd {
                    name,
                    ok,
                    summary,
                    detail,
                } => {
                    self.session.activity = format!("finished tool {name}");
                    let status = if ok { "finished" } else { "failed" };
                    let mut content = format!("Tool {status}: {name} - {summary}");
                    if let Some(detail) = detail.filter(|detail| !detail.trim().is_empty()) {
                        content.push_str("\n\n");
                        content.push_str(&detail);
                    }
                    self.push_live_tool_message(content);
                }
            }
        }
        self.redraw_requested = true;
    }

    fn push_live_tool_message(&mut self, content: String) {
        if self
            .session
            .messages
            .last()
            .map(|message| message.role == "system" && message.content == content)
            .unwrap_or(false)
        {
            return;
        }
        self.session.messages.push(ChatMessage {
            role: "system".to_string(),
            content,
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
    }

    fn merge_live_tool_messages(&self, completed: &mut SessionState) {
        let live_messages = self
            .session
            .messages
            .iter()
            .filter(|message| message.role == "system" && is_live_tool_message(&message.content))
            .filter(|message| {
                !completed.messages.iter().any(|existing| {
                    existing.role == message.role && existing.content == message.content
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        if live_messages.is_empty() {
            return;
        }
        let insert_at = completed
            .messages
            .iter()
            .rposition(|message| message.role == "assistant")
            .unwrap_or(completed.messages.len());
        completed
            .messages
            .splice(insert_at..insert_at, live_messages);
    }

    fn merge_live_reasoning_trace(&self, completed: &mut SessionState) {
        let Some(live_content) = self
            .session
            .messages
            .iter()
            .rposition(|message| message.role == "user")
            .and_then(|last_user_index| {
                self.session.messages[last_user_index + 1..]
                    .iter()
                    .find(|message| {
                        message.role == "assistant"
                            && message.content.contains("**Thinking trace**")
                    })
            })
            .map(|message| message.content.clone())
        else {
            return;
        };
        if let Some(completed_message) = completed
            .messages
            .iter_mut()
            .rev()
            .find(|message| message.role == "assistant")
        {
            completed_message.content = live_content;
        }
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

    fn command_palette_page_size(&self) -> usize {
        self.renderer
            .viewport
            .map(|(_, lines)| usize::from(lines.min(12)))
            .or_else(|| {
                crossterm::terminal::size()
                    .ok()
                    .map(|(_, lines)| usize::from(lines.min(12)))
            })
            .unwrap_or(12)
            .max(4)
    }

    fn pulse_activity(&mut self) {
        if self.session.status != "streaming" {
            return;
        }
        self.session.activity_tick = self.session.activity_tick.saturating_add(1);
        if self.session.activity_tick % 8 == 0 {
            self.redraw_requested = true;
        }
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
        let original_count = self.session.messages.len();
        if original_count == 0 {
            return "No conversation history to compress.".to_string();
        }

        let capsule = self.context_capsule(&topic, original_count);
        let keep_recent = 6.min(original_count);
        let recent_messages = self
            .session
            .messages
            .iter()
            .skip(original_count.saturating_sub(keep_recent))
            .cloned()
            .collect::<Vec<_>>();

        self.session.messages = vec![ChatMessage {
            role: "system".to_string(),
            content: capsule.clone(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        }];
        self.session.messages.extend(recent_messages);

        capsule
    }

    fn context_capsule(&self, topic: &str, original_count: usize) -> String {
        let recent_window = self
            .session
            .messages
            .iter()
            .rev()
            .take(18)
            .collect::<Vec<_>>();
        let current_objective = recent_window
            .iter()
            .find(|message| message.role == "user")
            .map(|message| compact_context_line(&message.content, 700))
            .unwrap_or_else(|| "Not stated in recent context.".to_string());
        let git_status = git_status_summary(&self.cwd);
        let recent_actions = summarize_recent_actions(&self.session.messages, 10);
        let decisions =
            summarize_context_signals(&self.session.messages, 8, CONTEXT_DECISION_MARKERS);
        let open_issues =
            summarize_context_signals(&self.session.messages, 8, CONTEXT_OPEN_ISSUE_MARKERS);
        let retained = 6.min(original_count);

        let mut lines = vec![
            format!("Context Capsule: {topic}"),
            "Generated by /compress. This is a structured handoff, not a full transcript."
                .to_string(),
            String::new(),
            "Current Objective:".to_string(),
            format!("- {current_objective}"),
            String::new(),
            "Session State:".to_string(),
            format!("- Session: {}", self.session.session_id),
            format!("- Workspace: {}", self.cwd.display()),
            format!(
                "- Provider/model: {}/{}",
                self.session.current_provider, self.session.current_model
            ),
            format!("- Original message count: {original_count}"),
            format!("- Recent messages retained verbatim after this capsule: {retained}"),
            format!("- Git status: {git_status}"),
            String::new(),
            "Recent Actions / Evidence:".to_string(),
        ];
        append_bullets(&mut lines, recent_actions, "No recent actions detected.");
        lines.push(String::new());
        lines.push("Decisions / Constraints / Preferences:".to_string());
        append_bullets(
            &mut lines,
            decisions,
            "No explicit recent decisions detected.",
        );
        lines.push(String::new());
        lines.push("Open Issues / Follow-ups:".to_string());
        append_bullets(
            &mut lines,
            open_issues,
            "No explicit unresolved issue detected.",
        );
        lines.push(String::new());
        lines.push("Continuity Instructions:".to_string());
        lines.push("- Treat this capsule plus the retained recent messages as the active conversation state.".to_string());
        lines.push("- Do not assume details omitted by compression; inspect files, git state, traces, or CMS memory before making project-specific claims.".to_string());
        lines.push("- Preserve user work and keep future changes scoped and verified.".to_string());
        lines.join("\n")
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
        if matches!(
            args.first().map(String::as_str),
            Some("max-rounds" | "tool-rounds" | "tool-limit" | "limit")
        ) {
            return self.tool_limit_command(&args[1..]);
        }
        if matches!(
            args.first().map(String::as_str),
            Some("commands" | "command" | "allowed-commands" | "allow-command")
        ) {
            return self.tool_commands_command(&args[1..]);
        }
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
                    self.tool_executor.guardrails.policy.require_human_approval = false;
                    return "Risky tools enabled for this running session.".to_string();
                }
                "deny-risky" | "disable-risky" => {
                    self.risky_tools_enabled = false;
                    self.tool_executor.guardrails.policy.allow_risky_tools = false;
                    return "Risky tools disabled for this running session.".to_string();
                }
                "status" => {
                    let mut commands = self
                        .tool_executor
                        .guardrails
                        .policy
                        .allowed_commands
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>();
                    commands.sort();
                    return format!(
                        "Risky tools: {}\nHuman approval: {}\nDangerous bypass: {}\nPending approvals: {}\nAllowed shell commands: {}",
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
                        self.tool_executor.guardrails.approvals.pending_len(),
                        commands.join(", ")
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
            "{inventory}\nRisky tools: {}\nHuman approval: {}\nDangerous bypass: {}\nPending approvals: {}\nAllowed shell commands: use /tools commands",
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

    fn tool_commands_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("list") | Some("show") | Some("status") => {
                let mut commands = self
                    .tool_executor
                    .guardrails
                    .policy
                    .allowed_commands
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>();
                commands.sort();
                format!(
                    "Allowed shell commands:\n{}\nUsage: /tools commands add <cmd...> | remove <cmd...> | reset",
                    commands.join(", ")
                )
            }
            Some("add") | Some("allow") => {
                if args.len() < 2 {
                    return "Usage: /tools commands add <cmd...>".to_string();
                }
                let mut added = Vec::new();
                let mut rejected = Vec::new();
                for command in args.iter().skip(1) {
                    match normalize_command_name(command) {
                        Some(command) => {
                            if self
                                .tool_executor
                                .guardrails
                                .policy
                                .allowed_commands
                                .insert(command.clone())
                            {
                                added.push(command);
                            }
                        }
                        None => rejected.push(command.clone()),
                    }
                }
                tool_command_update_message("Allowed", "added", added, rejected)
            }
            Some("remove") | Some("revoke") | Some("deny") => {
                if args.len() < 2 {
                    return "Usage: /tools commands remove <cmd...>".to_string();
                }
                let mut removed = Vec::new();
                let mut rejected = Vec::new();
                for command in args.iter().skip(1) {
                    match normalize_command_name(command) {
                        Some(command) => {
                            if self
                                .tool_executor
                                .guardrails
                                .policy
                                .allowed_commands
                                .remove(&command)
                            {
                                removed.push(command);
                            }
                        }
                        None => rejected.push(command.clone()),
                    }
                }
                tool_command_update_message("Removed", "removed", removed, rejected)
            }
            Some("reset") | Some("default") => {
                self.tool_executor.guardrails.policy.allowed_commands = default_allowed_commands();
                "Allowed shell commands reset to Vegvisir defaults.".to_string()
            }
            Some(_) => {
                "Usage: /tools commands [list|add <cmd...>|remove <cmd...>|reset]".to_string()
            }
        }
    }

    fn tool_limit_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("show") | Some("status") => format!(
                "Max tool-call rounds per turn: {}\nHard limit: {}\nUsage: /tool-limit <rounds>|default",
                configured_max_tool_rounds(),
                max_tool_rounds_hard_limit()
            ),
            Some("default") | Some("reset") | Some("clear") => {
                let effective = set_runtime_max_tool_rounds(None);
                format!("Max tool-call rounds reset to default/environment value: {effective}.")
            }
            Some(raw) => match raw.parse::<usize>() {
                Ok(0) => "Tool-call round limit must be at least 1.".to_string(),
                Ok(limit) => {
                    let effective = set_runtime_max_tool_rounds(Some(limit));
                    let clamped = if effective != limit {
                        format!(" Requested value was clamped to the hard limit {effective}.")
                    } else {
                        String::new()
                    };
                    format!(
                        "Max tool-call rounds per turn set to {effective} for this running session.{clamped}"
                    )
                }
                Err(_) => "Usage: /tool-limit <rounds>|default".to_string(),
            },
        }
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
                        "approve_for_session": format!("/approvals session {}", request.id),
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
                match self
                    .tool_executor
                    .guardrails
                    .approvals
                    .approve_once_request(id)
                {
                    Some(request) => self.execute_approved_request("Approved once", request),
                    None => format!("Unknown pending approval: {id}"),
                }
            }
            Some("session")
            | Some("approve-session")
            | Some("allow-session")
            | Some("approve-pattern")
            | Some("allow-pattern") => {
                let Some(id) = args.get(1) else {
                    return "Usage: /approvals session <id>".to_string();
                };
                match self
                    .tool_executor
                    .guardrails
                    .approvals
                    .approve_for_session(id)
                {
                    Some(request) => {
                        let mut prefix =
                            "Approved matching call for this running session".to_string();
                        if request.risk_label == "command-allow"
                            && let Some(command) = command_name_from_args(&request.args)
                        {
                            self.tool_executor
                                .guardrails
                                .policy
                                .allowed_commands
                                .insert(command.clone());
                            prefix = format!(
                                "Approved matching call and allowed shell command `{command}` for this running session"
                            );
                        }
                        self.execute_approved_request(&prefix, request)
                    }
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

    fn execute_approved_request(
        &mut self,
        approval_message: &str,
        request: ApprovalRequest,
    ) -> String {
        let tool_name = request.tool_name.clone();
        let observation = self.tool_executor.execute(ToolCall {
            name: request.tool_name,
            args: request.args,
        });
        let status = if observation.ok { "ok" } else { "failed" };
        let body = if observation.content.trim().is_empty() {
            "(no output)".to_string()
        } else {
            observation.content
        };
        format!(
            "{approval_message}: {tool_name}\nTool execution {status}.\n{body}\n\nIf this was part of a model task, send `continue` so the agent can inspect the result and proceed."
        )
    }

    fn prepare_lsl_for_content(
        &mut self,
        content: &str,
    ) -> anyhow::Result<(String, Option<LslSkillTrace>)> {
        let cfg = self.lsl_runtime_config();
        let (model_content, trace) = prepare_lsl_augmented_content(
            &self.cwd,
            &self.data_root,
            content,
            &self.session,
            &cfg,
        )?;
        Ok((model_content, trace))
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
                    "HBSE is the auth/secrets layer. Vegvisir only handles secret refs and broker policies; plaintext secrets must be entered into HBSE.\n{providers}\nregistered_services={}\nUsage: /hbse onboard [provider|all]\nUsage: /hbse provider <openai|xai|openrouter|groq|mistral|deepseek|together|perplexity|anthropic|google>\nUsage: /hbse mcp <server> [url] [consumer] [purpose]\nUsage: /hbse service <name> [consumer] [purpose]\nUsage: /hbse service add <name> <secret_ref> [consumer] [purpose]\nUsage: /hbse services",
                    self.hbse_services.len()
                )
            }
            Some("onboard") | Some("setup") => {
                let provider = args.get(1).map(String::as_str).unwrap_or("all");
                hbse_onboarding_script_setup(provider)
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

fn tool_command_update_message(
    action: &str,
    empty_action: &str,
    changed: Vec<String>,
    rejected: Vec<String>,
) -> String {
    let mut lines = Vec::new();
    if !changed.is_empty() {
        lines.push(format!(
            "{action} shell command{} for this running session: {}",
            if changed.len() == 1 { "" } else { "s" },
            changed.join(", ")
        ));
    }
    if !rejected.is_empty() {
        lines.push(format!(
            "Rejected invalid command name{}: {}",
            if rejected.len() == 1 { "" } else { "s" },
            rejected.join(", ")
        ));
    }
    if lines.is_empty() {
        format!("No shell commands were {empty_action}.")
    } else {
        lines.join("\n")
    }
}

const CONTEXT_DECISION_MARKERS: &[&str] = &[
    "decided",
    "decision",
    "agreed",
    "preference",
    "prefer",
    "constraint",
    "boundary",
    "must",
    "should",
    "should not",
    "only",
    "remains responsible",
];

const CONTEXT_OPEN_ISSUE_MARKERS: &[&str] = &[
    "todo",
    "next",
    "follow-up",
    "follow up",
    "not fixed",
    "bug",
    "issue",
    "failed",
    "failure",
    "error",
    "unresolved",
    "need to",
    "needs",
    "want",
];

fn append_bullets(lines: &mut Vec<String>, items: Vec<String>, empty: &str) {
    if items.is_empty() {
        lines.push(format!("- {empty}"));
    } else {
        lines.extend(items.into_iter().map(|item| format!("- {item}")));
    }
}

fn summarize_recent_actions(messages: &[ChatMessage], limit: usize) -> Vec<String> {
    let mut actions = Vec::new();
    for message in messages.iter().rev() {
        let lower = message.content.to_ascii_lowercase();
        let action_like = message.role == "tool"
            || lower.contains("verified")
            || lower.contains("ran ")
            || lower.contains("changed")
            || lower.contains("implemented")
            || lower.contains("fixed")
            || lower.contains("pushed")
            || lower.contains("committed")
            || lower.contains("git status")
            || lower.contains("test")
            || lower.contains("cargo ");
        if action_like {
            actions.push(format!(
                "{}: {}",
                message.role,
                compact_context_line(&message.content, 500)
            ));
        }
        if actions.len() >= limit {
            break;
        }
    }
    actions.reverse();
    dedupe_preserve_order(actions)
}

fn summarize_context_signals(
    messages: &[ChatMessage],
    limit: usize,
    markers: &[&str],
) -> Vec<String> {
    let mut signals = Vec::new();
    for message in messages.iter().rev() {
        let lower = message.content.to_ascii_lowercase();
        if markers.iter().any(|marker| lower.contains(marker)) {
            signals.push(format!(
                "{}: {}",
                message.role,
                compact_context_line(&message.content, 500)
            ));
        }
        if signals.len() >= limit {
            break;
        }
    }
    signals.reverse();
    dedupe_preserve_order(signals)
}

fn dedupe_preserve_order(items: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        if !out.iter().any(|existing| existing == &item) {
            out.push(item);
        }
    }
    out
}

fn compact_context_line(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        compact
    } else {
        format!("{}…", compact.chars().take(max_chars).collect::<String>())
    }
}

fn git_status_summary(cwd: &Path) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .arg("status")
        .arg("--short")
        .output();
    let Ok(output) = output else {
        return "unavailable (git status failed to start)".to_string();
    };
    if !output.status.success() {
        return "unavailable (not a git repository or git status failed)".to_string();
    }
    let status = String::from_utf8_lossy(&output.stdout);
    let changed = status
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    if changed == 0 {
        "clean".to_string()
    } else {
        let sample = status
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(8)
            .collect::<Vec<_>>()
            .join("; ");
        if changed > 8 {
            format!("{changed} changed path(s): {sample}; …")
        } else {
            format!("{changed} changed path(s): {sample}")
        }
    }
}

fn parse_config_value(raw: &str) -> Value {
    if let Ok(value) = raw.parse::<u64>() {
        return json!(value);
    }
    match raw {
        "true" => json!(true),
        "false" => json!(false),
        other => json!(other),
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
        "Vegvisir will not read or store the provider secret. Prefer the deterministic onboarding helper when available:\n\nscripts/hbse-provider-onboard.sh {provider_id}\n\nManual equivalent:\n\nhbse model-provider setup {provider_id} --stdin --secret-ref {secret_ref} --consumer {consumer} --purpose model.chat --model-discovery-purpose model.discovery\n\nAfter setup, select the HBSE-routed provider in Vegvisir, for example /provider {provider_id}-hbse when that provider exists in the catalog."
    )
}

fn hbse_onboarding_script_setup(provider: &str) -> String {
    format!(
        "Use the deterministic HBSE onboarding helper from the Vegvisir repo root:\n\nscripts/hbse-provider-onboard.sh {provider}\n\nIt prompts for provider secrets outside model chat, writes them into HBSE, installs chat/discovery broker policies, and verifies model.discovery policy access. T3 can call the same onboarding metadata through the hbse.onboarding.providers bridge method."
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

fn estimated_message_line_count(message: &ChatMessage) -> usize {
    let content_lines = message
        .content
        .lines()
        .map(|line| (line.chars().count() / 80).saturating_add(1))
        .sum::<usize>()
        .max(1);
    content_lines + 2
}

fn is_live_tool_message(content: &str) -> bool {
    content.starts_with("Running tool: ")
        || content.starts_with("Tool finished: ")
        || content.starts_with("Tool failed: ")
}

#[cfg(test)]
pub(crate) fn terminal_frame(rendered: &str) -> String {
    rendered
        .split('\n')
        .map(|line| format!("{line}\x1b[K"))
        .collect::<Vec<_>>()
        .join("\r\n")
}

#[cfg(test)]
mod tests {
    use super::{StreamEvent, TuiApplication};
    use crate::core::ChatMessage;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
    use std::sync::mpsc;

    #[test]
    fn terminal_frame_returns_carriage_on_each_rendered_line() {
        assert_eq!(
            super::terminal_frame("one\ntwo\nthree"),
            "one\x1b[K\r\ntwo\x1b[K\r\nthree\x1b[K"
        );
    }

    #[test]
    fn mouse_wheel_scrolls_chat_when_command_palette_is_open() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.open_command_palette();
        app.chat_scroll_offset = 0;
        app.input.selected_suggestion = 3;

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(app.chat_scroll_offset, 3);
        assert_eq!(app.input.selected_suggestion, 3);
        assert!(app.command_palette_open);
        assert!(app.redraw_requested);
        Ok(())
    }

    #[test]
    fn stream_deltas_do_not_force_follow_when_user_scrolled_up() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "existing".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        let (tx, rx) = mpsc::channel();
        app.pending_stream = Some(rx);
        app.chat_scroll_offset = 7;

        tx.send(StreamEvent::Delta(" delta".to_string()))?;
        app.poll_stream_events();

        assert_eq!(
            app.session.messages.last().unwrap().content,
            "existing delta"
        );
        assert_eq!(app.chat_scroll_offset, 7);
        assert!(app.redraw_requested);
        Ok(())
    }

    #[test]
    fn tool_events_update_live_chat_and_activity() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let (tx, rx) = mpsc::channel();
        app.pending_stream = Some(rx);

        tx.send(StreamEvent::Activity(
            "thinking through tool use".to_string(),
        ))?;
        tx.send(StreamEvent::ToolStart {
            name: "write_file".to_string(),
            args: r#"{"path":"src/lib.rs"}"#.to_string(),
        })?;
        tx.send(StreamEvent::ToolEnd {
            name: "write_file".to_string(),
            ok: true,
            summary: "ok: Wrote src/lib.rs".to_string(),
            detail: None,
        })?;
        app.poll_stream_events();

        assert_eq!(app.session.activity, "finished tool write_file");
        let transcript = app
            .session
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(transcript.contains("Running tool: write_file"));
        assert!(transcript.contains("Tool finished: write_file"));
        assert!(app.redraw_requested);
        Ok(())
    }

    #[test]
    fn completed_turn_preserves_live_tool_messages() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.push_live_tool_message("Running tool: read_file {\"path\":\"src/lib.rs\"}".to_string());
        app.push_live_tool_message("Tool finished: read_file - ok: read 20 bytes".to_string());

        let mut completed = app.session.clone();
        completed
            .messages
            .retain(|message| message.role != "system");
        completed.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "done".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        app.merge_live_tool_messages(&mut completed);

        let transcript = completed
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(transcript.contains("Running tool: read_file"));
        assert!(transcript.contains("Tool finished: read_file"));
        assert!(completed.messages.last().unwrap().content.contains("done"));
        Ok(())
    }

    #[test]
    fn completed_turn_does_not_reuse_previous_reasoning_trace() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: "tell me a story".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        app.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "**Thinking trace**\n\nold story response".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        app.session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: "do you like Goblins?".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        app.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: String::new(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        let mut completed = app.session.clone();
        completed.messages.pop();
        completed.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "Yes, in fantasy stories.".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        app.merge_live_reasoning_trace(&mut completed);

        assert_eq!(
            completed.messages.last().unwrap().content,
            "Yes, in fantasy stories."
        );
        Ok(())
    }

    #[test]
    fn completed_turn_keeps_current_reasoning_trace() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: "do you like Goblins?".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        app.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "**Thinking trace**\n\ncurrent trace\n\n**Answer**\n\nYes.".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        let mut completed = app.session.clone();
        completed.messages.pop();
        completed.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "Yes.".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        app.merge_live_reasoning_trace(&mut completed);

        assert_eq!(
            completed.messages.last().unwrap().content,
            "**Thinking trace**\n\ncurrent trace\n\n**Answer**\n\nYes."
        );
        Ok(())
    }

    #[test]
    fn activity_pulse_throttles_idle_streaming_redraws() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.session.status = "streaming".to_string();
        app.redraw_requested = false;

        app.pulse_activity();
        assert!(!app.redraw_requested);
        for _ in 0..7 {
            app.pulse_activity();
        }
        assert!(app.redraw_requested);
        Ok(())
    }

    #[test]
    fn chat_search_opens_filters_and_jumps_matches() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: "first parser note".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        app.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "second auth note".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        app.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "third parser result".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        app.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        assert!(app.search_open);
        for ch in "parser".chars() {
            app.handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(app.search_query, "parser");
        assert_eq!(app.search_matches(), vec![0, 2]);
        assert_eq!(app.search_match_index, 0);
        let first_offset = app.chat_scroll_offset;

        app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.search_match_index, 1);
        assert!(app.chat_scroll_offset < first_offset);
        app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.search_open);
        Ok(())
    }

    #[test]
    fn ctrl_c_cancels_in_flight_response_before_quitting() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let session = app.session.clone();
        app.pending_send = Some(std::thread::spawn(move || Ok(session)));
        app.session.status = "streaming".to_string();
        app.running = true;

        app.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

        assert!(app.running);
        assert!(app.pending_send.is_none());
        assert_eq!(app.session.status, "ready");
        assert!(
            app.session
                .messages
                .last()
                .is_some_and(|message| message.content.contains("Cancelled in-flight"))
        );

        app.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(!app.running);
        Ok(())
    }
}
