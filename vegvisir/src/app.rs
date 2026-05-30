use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::AtomicBool,
        mpsc::{Receiver, Sender},
    },
    thread::JoinHandle,
    time::Duration,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
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
    persona::{
        DEFAULT_PERSONA_ID, KA_PROMPT_HEADING, get_persona_with_root, render_persona_prompt_section,
    },
    policy::RuntimePolicy,
    provider::{
        ConversationRunner, ProviderRouter, ProviderRunEvent, configured_max_tool_rounds_label,
        direct_provider_auth_allowed, set_runtime_max_tool_rounds,
    },
    speech::{DEFAULT_PTT_SECONDS, PushToTalkKey},
    subagents::{SubAgentStatus, SubAgentTaskRecord},
    tools::{ToolExecutor, ToolRegistry, build_builtin_registry_with_cms_and_mode},
    types::ToolCall,
    ui::{
        input::{InputState, Suggestion},
        layout::LayoutRenderer,
    },
};

mod commands;
mod input;
mod lsl_runtime;
mod runtime;
mod shell;
mod tui_loop;
mod util;
mod workspace_state;

use lsl_runtime::{compiled_lsl_selected_from_trace, prepare_lsl_augmented_content};
pub use tui_loop::{run_tui, run_tui_with_dangerous_bypass};
pub use util::workspace_project_id;
use util::*;

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
    pending_speech_jobs: Vec<JoinHandle<anyhow::Result<String>>>,
    pub speech_ptt_key: Option<PushToTalkKey>,
    pub speech_ptt_seconds: u64,
    pending_stream: Option<Receiver<StreamEvent>>,
    pending_cancel: Option<Arc<AtomicBool>>,
    pending_steering: Option<Sender<String>>,
    pub pending_editor_action: Option<PendingEditorAction>,
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
    pub mouse_capture_enabled: bool,
    pub chat_area_x: u16,
    pub chat_area_y: u16,
    pub chat_area_width: u16,
    pub chat_area_height: u16,
    pub chat_render_scroll: usize,
    pub chat_rendered_lines: Vec<String>,
    pub drag_anchor: Option<(u16, u16)>,
    pub drag_current: Option<(u16, u16)>,
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
pub struct PendingEditorAction {
    pub kind: PendingEditorKind,
    pub id: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingEditorKind {
    KaProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffRenderer {
    Unified,
    Delta,
    Difftastic,
}

impl DiffRenderer {
    pub fn label(self) -> &'static str {
        match self {
            DiffRenderer::Unified => "unified",
            DiffRenderer::Delta => "delta",
            DiffRenderer::Difftastic => "difftastic",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffOverlay {
    pub title: String,
    pub diff: String,
    pub rendered_by: DiffRenderer,
    pub files_changed: usize,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub rendered_lines_cache: Option<DiffOverlayRenderCache>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffOverlayRenderCache {
    pub width: usize,
    pub lines: Vec<ratatui::text::Line<'static>>,
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
    diff_overlay_from_rendered(title, diff, DiffRenderer::Unified)
}

fn diff_overlay_from_rendered(title: &str, diff: &str, rendered_by: DiffRenderer) -> DiffOverlay {
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
        rendered_by,
        files_changed,
        added_lines,
        removed_lines,
        rendered_lines_cache: None,
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
        let mut approvals =
            crate::guardrails::ApprovalLedger::new_persisted(data_root.join("approvals.json"))
                .unwrap_or_default();
        if dangerously_bypass_approvals_and_sandbox {
            approvals.clear_pending();
        }
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
        session.active_persona_id = Some(DEFAULT_PERSONA_ID.to_string());
        if let Some(provider) = defaults.get("current_provider").and_then(Value::as_str) {
            session.current_provider = provider.to_string();
        }
        if let Some(model) = defaults.get("current_model").and_then(Value::as_str) {
            session.current_model = model.to_string();
        }
        if let Some(prompt) = defaults.get("system_prompt").and_then(Value::as_str) {
            session.system_prompt = prompt.to_string();
        }
        if let Some(persona) = defaults.get("active_persona_id").and_then(Value::as_str)
            && get_persona_with_root(&data_root, persona)?.is_some()
        {
            session.active_persona_id = Some(persona.to_string());
        }
        session.system_prompt = apply_persona_to_system_prompt(
            &session.system_prompt,
            session.active_persona_id.as_deref(),
            &data_root,
        );
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
            pending_speech_jobs: Vec::new(),
            speech_ptt_key: defaults
                .get("speech_ptt_key")
                .and_then(Value::as_str)
                .and_then(PushToTalkKey::parse)
                .or(Some(PushToTalkKey::F(8))),
            speech_ptt_seconds: defaults
                .get("speech_ptt_seconds")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_PTT_SECONDS)
                .clamp(1, 30),
            pending_stream: None,
            pending_cancel: None,
            pending_steering: None,
            pending_editor_action: None,
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
            mouse_capture_enabled: true,
            chat_area_x: 0,
            chat_area_y: 0,
            chat_area_width: 0,
            chat_area_height: 0,
            chat_render_scroll: 0,
            chat_rendered_lines: Vec::new(),
            drag_anchor: None,
            drag_current: None,
        };
        app.autoload_workspace_session()?;
        app.rebuild_tooling_for_cms()?;
        let provider = app.session.current_provider.clone();
        let _ = app.refresh_provider_models(&provider);
        Ok(app)
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
            cancel_token: None,
            steering_rx: None,
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
            cancel_token: None,
            steering_rx: None,
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
        self.tool_executor.guardrails.policy.require_human_approval = !bypass;
        self.tool_executor
            .guardrails
            .policy
            .bypass_approvals_and_sandbox = bypass;
        if bypass {
            self.tool_executor.guardrails.approvals.clear_pending();
        }
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
            json!(strip_persona_from_system_prompt(
                &self.session.system_prompt
            )),
        );
        if let Some(persona) = &self.session.active_persona_id {
            data.insert("active_persona_id".to_string(), json!(persona));
        }
        if let Some(key) = &self.speech_ptt_key {
            data.insert("speech_ptt_key".to_string(), json!(key.to_config_string()));
        } else {
            data.remove("speech_ptt_key");
        }
        data.insert(
            "speech_ptt_seconds".to_string(),
            json!(self.speech_ptt_seconds),
        );
        self.config.save(&data)
    }
}

#[cfg(test)]
pub(crate) fn terminal_frame(rendered: &str) -> String {
    rendered
        .split('\n')
        .map(|line| format!("{line}\x1b[K"))
        .collect::<Vec<_>>()
        .join("\r\n")
}

pub(crate) fn apply_persona_to_system_prompt(
    base_prompt: &str,
    persona_id: Option<&str>,
    data_root: &Path,
) -> String {
    let base = strip_persona_from_system_prompt(base_prompt);
    let Some(persona_id) = persona_id else {
        return base;
    };
    let Ok(Some(persona)) = get_persona_with_root(data_root, persona_id) else {
        return base;
    };
    let section = render_persona_prompt_section(&persona);
    if base.trim().is_empty() {
        section
    } else {
        format!("{}\n\n{}", base.trim_end(), section)
    }
}

pub(crate) fn strip_persona_from_system_prompt(prompt: &str) -> String {
    for heading in [
        KA_PROMPT_HEADING,
        "# Communication persona",
        "# Communication soul",
    ] {
        let marker = format!("\n\n{heading}\n");
        if let Some(index) = prompt.find(&marker) {
            return prompt[..index].trim_end().to_string();
        }
        let start_marker = format!("{heading}\n");
        if prompt.starts_with(&start_marker) {
            return String::new();
        }
    }
    prompt.to_string()
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
    fn persona_set_updates_system_prompt_and_defaults() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let response =
            app.persona_command(&["set".to_string(), "chaotic_competent".to_string()])?;
        assert!(response.contains("Ka set to chaotic_competent"));
        assert_eq!(
            app.session.active_persona_id.as_deref(),
            Some("chaotic_competent")
        );
        assert!(app.session.system_prompt.contains("# Communication ka"));
        assert!(
            app.session
                .system_prompt
                .contains("Active ka/persona: `chaotic_competent`")
        );
        assert!(
            app.session
                .system_prompt
                .contains("Ka/persona controls delivery style only")
        );

        let defaults = app.config.load()?;
        assert_eq!(
            defaults
                .get("active_persona_id")
                .and_then(serde_json::Value::as_str),
            Some("chaotic_competent")
        );
        assert!(
            !defaults
                .get("system_prompt")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .contains("# Communication ka")
        );
        Ok(())
    }

    #[test]
    fn ka_edit_queues_tui_safe_editor_action() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let response = app.persona_command(&["edit".to_string(), "bug_goblin".to_string()])?;
        assert!(response.contains("Opening editor for ka `bug_goblin`"));
        let action = app
            .pending_editor_action
            .as_ref()
            .expect("ka edit queues external editor action");
        assert_eq!(action.kind, crate::app::PendingEditorKind::KaProfile);
        assert_eq!(action.id, "bug_goblin");
        assert!(action.path.ends_with("ka/bug_goblin.json"));
        assert!(action.path.exists());
        Ok(())
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
    fn completed_turn_does_not_replace_full_response_with_partial_live_trace() -> anyhow::Result<()>
    {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: "make a change".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        app.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "**Thinking trace**\n\nworking\n\n**Answer**\n\nDone.\n\nSummary:".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        let mut completed = app.session.clone();
        completed.messages.pop();
        completed.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "**Thinking trace**\n\nworking\n\n**Answer**\n\nDone.\n\nSummary:\n- final line visible".to_string(),
            attachments: Vec::new(),
                    created_at: chrono::Utc::now(),
});

        app.merge_live_reasoning_trace(&mut completed);

        assert!(
            completed
                .messages
                .last()
                .unwrap()
                .content
                .contains("final line visible")
        );
        Ok(())
    }

    #[test]
    fn activity_pulse_animates_each_streaming_tick() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.session.status = "streaming".to_string();
        app.redraw_requested = false;

        app.pulse_activity();
        assert!(app.redraw_requested);
        app.redraw_requested = false;
        app.pulse_activity();
        assert!(app.redraw_requested);
        Ok(())
    }

    #[test]
    fn dangerous_bypass_startup_clears_persisted_pending_approvals() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let cwd = temp.path().join("workspace");
        std::fs::create_dir_all(&cwd)?;
        let data_root = temp.path().join("data");
        std::fs::create_dir_all(&data_root)?;

        let mut args = serde_json::Map::new();
        args.insert(
            "path".to_string(),
            serde_json::Value::String("x".to_string()),
        );
        let request = crate::guardrails::ApprovalRequest {
            id: "stale-approval".to_string(),
            tool_name: "write_file".to_string(),
            args,
            reason: "stale approval from a prior non-yolo session".to_string(),
            risk_label: "risky".to_string(),
        };
        let mut ledger =
            crate::guardrails::ApprovalLedger::new_persisted(data_root.join("approvals.json"))?;
        ledger.enqueue(request);
        assert_eq!(ledger.pending_len(), 1);

        let app = TuiApplication::with_data_root_and_dangerous_bypass(&cwd, &data_root, true)?;

        assert!(app.dangerously_bypass_approvals_and_sandbox);
        assert!(
            app.tool_executor
                .guardrails
                .policy
                .bypass_approvals_and_sandbox
        );
        assert!(!app.tool_executor.guardrails.policy.require_human_approval);
        assert_eq!(app.tool_executor.guardrails.approvals.pending_len(), 0);
        Ok(())
    }

    #[test]
    fn failed_worker_final_drain_preserves_tool_failure_context() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        let (tx, rx) = mpsc::channel();
        app.pending_stream = Some(rx);
        app.session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: String::new(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        tx.send(StreamEvent::ToolStart {
            name: "run_command".to_string(),
            args: r#"{"command":["false"]}"#.to_string(),
        })?;
        tx.send(StreamEvent::ToolEnd {
            name: "run_command".to_string(),
            ok: false,
            summary: "ToolError: command exited with status 1".to_string(),
            detail: Some("stderr: simulated failure".to_string()),
        })?;
        drop(tx);

        // This mirrors poll_pending_send's worker-error path: drain final stream
        // events before clearing pending_stream and add an assistant-facing
        // recovery summary. The previous behavior cleared pending_stream first,
        // losing the ToolEnd detail and making the turn look silently truncated.
        app.poll_stream_events();
        app.pending_stream = None;
        app.pop_empty_assistant_placeholder();
        app.push_turn_failure_summary("simulated provider abort".to_string());

        let transcript = app
            .session
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join(
                "
---
",
            );
        assert!(transcript.contains("Running tool: run_command"));
        assert!(transcript.contains("Tool failed: run_command"));
        assert!(transcript.contains("stderr: simulated failure"));
        assert!(
            transcript.contains("Turn failed before the model produced a normal final summary")
        );
        assert!(transcript.contains("Recent tool/progress events:"));
        assert!(transcript.contains("Failure:"));
        assert!(transcript.contains("simulated provider abort"));
        assert!(app.session.messages.iter().any(|message| {
            message.role == "system"
                && message
                    .content
                    .contains("Turn failed before the model produced a normal final summary")
        }));
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
    fn chat_drag_selection_extracts_rendered_text() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        app.chat_area_x = 2;
        app.chat_area_y = 3;
        app.chat_area_height = 4;
        app.chat_render_scroll = 10;
        app.chat_rendered_lines = vec![String::new(); 12];
        app.chat_rendered_lines
            .push("  hello selectable world".to_string());

        let selected = app.extract_chat_drag_selection((10, 5), (20, 5));

        assert_eq!(selected, "selectable");
        Ok(())
    }

    #[test]
    fn f12_toggles_mouse_capture_mode() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        assert!(app.mouse_capture_enabled);

        app.handle_key_event(KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE));
        assert!(!app.mouse_capture_enabled);
        assert!(
            app.session
                .messages
                .last()
                .unwrap()
                .content
                .contains("Mouse capture OFF")
        );

        app.handle_key_event(KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE));
        assert!(app.mouse_capture_enabled);
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
