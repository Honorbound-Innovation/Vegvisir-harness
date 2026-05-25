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
