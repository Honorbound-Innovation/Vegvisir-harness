use std::{
    collections::HashMap,
    io::{self, BufRead, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use chrono::Utc;

use crate::{app::TuiApplication, types::ToolCall};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum BridgeRequestId {
    String(String),
    Integer(i64),
}

#[derive(Debug, Deserialize)]
struct BridgeRequest {
    id: Option<BridgeRequestId>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    client_info: Option<Value>,
    capabilities: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadStartParams {
    cwd: Option<PathBuf>,
    model_provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    ephemeral: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct StartParams {
    workspace: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnStartParams {
    thread_id: String,
    input: Vec<Value>,
    cwd: Option<PathBuf>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TurnParams {
    content: String,
}

#[derive(Debug, Deserialize)]
struct CommandParams {
    command: String,
}

#[derive(Debug, Deserialize)]
struct CommandsSuggestParams {
    prefix: String,
}

#[derive(Debug, Deserialize)]
struct CommandDescribeParams {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SessionLoadParams {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ProviderSelectParams {
    provider: String,
    #[serde(default)]
    global: bool,
}

#[derive(Debug, Deserialize)]
struct ModelSelectParams {
    model: String,
}

#[derive(Debug, Deserialize)]
struct AgentSelectParams {
    agent: String,
}

#[derive(Debug, Deserialize)]
struct EffortSetParams {
    effort: String,
}

#[derive(Debug, Deserialize)]
struct FastSetParams {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct ToolLimitSetParams {
    value: Value,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatInfoParams {
    #[serde(default = "default_openai_compat_host")]
    host: String,
    #[serde(default = "default_openai_compat_port")]
    port: u16,
}

#[derive(Debug, Deserialize)]
struct ControlRespondParams {
    response:
        crate::control_requests::ControlResponse<crate::control_requests::ApprovalControlDecision>,
}

#[derive(Debug, Deserialize)]
struct ApprovalIdParams {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ApprovalEditParams {
    id: String,
    args: serde_json::Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct DiffParams {
    staged: Option<bool>,
    stat: Option<bool>,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SystemPromptSetParams {
    prompt: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelsListParams {
    refresh: Option<bool>,
    #[serde(alias = "modelProvider")]
    provider: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CommandBackedParams {
    #[serde(default)]
    args: Vec<String>,
    raw: Option<String>,
    query: Option<String>,
    id: Option<String>,
    name: Option<String>,
    path: Option<String>,
    value: Option<String>,
    field: Option<String>,
    scope: Option<String>,
    target: Option<String>,
    command: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    #[serde(default)]
    global: bool,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct BridgeEvent {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<BridgeRequestId>,
    payload: Value,
}

#[derive(Default)]
struct BridgeState {
    initialized: bool,
    server_started_at: i64,
    last_heartbeat_at: Option<i64>,
    heartbeat_count: u64,
    threads: HashMap<String, ThreadRuntime>,
    pending_approval_turns: HashMap<String, PendingApprovalTurn>,
}

impl BridgeState {
    fn new() -> Self {
        Self {
            server_started_at: unix_now(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug)]
struct ThreadRuntime {
    created_at: i64,
    updated_at: i64,
    preview: String,
    ephemeral: bool,
}

#[derive(Clone, Debug)]
struct PendingApprovalTurn {
    content: String,
}

pub struct BridgeOptions {
    pub workspace: PathBuf,
    pub data_root: Option<PathBuf>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub dangerously_bypass_approvals_and_sandbox: bool,
}

pub fn run_app_server(options: BridgeOptions) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    run_app_server_with_io(stdin.lock(), &mut stdout, options)
}

pub fn run_app_server_with_io<R: BufRead, W: Write>(
    input: R,
    stdout: &mut W,
    options: BridgeOptions,
) -> anyhow::Result<()> {
    let mut app = start_app(
        options.workspace,
        options.data_root.clone(),
        options.provider,
        options.model,
        options.agent,
        options.dangerously_bypass_approvals_and_sandbox,
    )?;
    let mut state = BridgeState::new();

    emit_legacy(
        stdout,
        BridgeEvent {
            kind: "server.ready",
            id: None,
            payload: snapshot(&app),
        },
    )?;

    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: BridgeRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                emit_error(stdout, None, "invalid_request", error.to_string())?;
                continue;
            }
        };
        let request_id = request.id.clone();
        match handle_request(
            &mut app,
            &mut state,
            request,
            options.data_root.as_deref(),
            options.dangerously_bypass_approvals_and_sandbox,
            stdout,
        ) {
            Ok(BridgeControl::Continue) => {}
            Ok(BridgeControl::Shutdown) => break,
            Err(error) => emit_error(stdout, request_id, "request_failed", error.to_string())?,
        }
    }
    Ok(())
}

enum BridgeControl {
    Continue,
    Shutdown,
}

fn handle_request(
    app: &mut TuiApplication,
    state: &mut BridgeState,
    request: BridgeRequest,
    data_root: Option<&std::path::Path>,
    dangerously_bypass_approvals_and_sandbox: bool,
    stdout: &mut dyn Write,
) -> anyhow::Result<BridgeControl> {
    match request.method.as_str() {
        "initialize" if request.id.is_some() => {
            let params: InitializeParams =
                serde_json::from_value(request.params).unwrap_or(InitializeParams {
                    client_info: None,
                    capabilities: None,
                });
            let _client_info = params.client_info.as_ref();
            let _capabilities = params.capabilities.as_ref();
            state.initialized = true;
            emit_response(
                stdout,
                request.id.expect("checked above"),
                json!({
                    "userAgent": format!("vegvisir/{}", env!("CARGO_PKG_VERSION")),
                    "codexHome": default_data_root_path(),
                    "platformFamily": std::env::consts::FAMILY,
                    "platformOs": std::env::consts::OS,
                }),
            )?;
        }
        "initialized" => {
            state.initialized = true;
        }
        "thread/start" => {
            ensure_initialized(state)?;
            let params: ThreadStartParams = serde_json::from_value(request.params)?;
            let workspace = params.cwd.unwrap_or_else(|| app.cwd.clone());
            *app = start_app(
                workspace,
                data_root.map(PathBuf::from),
                params.model_provider,
                params.model,
                params.agent,
                dangerously_bypass_approvals_and_sandbox,
            )?;
            let now = unix_now();
            let runtime = ThreadRuntime {
                created_at: now,
                updated_at: now,
                preview: String::new(),
                ephemeral: params.ephemeral.unwrap_or(false),
            };
            state
                .threads
                .insert(app.session.session_id.clone(), runtime);
            let thread = codex_thread(app, state);
            if let Some(id) = request.id {
                emit_response(stdout, id, json!({ "thread": thread.clone() }))?;
            }
            emit_notification(stdout, "thread/started", json!({ "thread": thread }))?;
        }
        "turn/start" => {
            ensure_initialized(state)?;
            let params: TurnStartParams = serde_json::from_value(request.params)?;
            let requested_thread_id = params.thread_id.clone();
            let resolved_thread_id =
                if requested_thread_id.is_empty() || requested_thread_id == "current" {
                    app.session.session_id.clone()
                } else {
                    requested_thread_id
                };
            if resolved_thread_id != app.session.session_id {
                anyhow::bail!("unknown or unloaded thread: {}", params.thread_id);
            }
            if let Some(cwd) = params.cwd {
                *app = start_app(
                    cwd,
                    data_root.map(PathBuf::from),
                    None,
                    params.model,
                    None,
                    dangerously_bypass_approvals_and_sandbox,
                )?;
            } else if let Some(model) = params.model {
                apply_requested_model_or_default(app, &model)?;
            }
            let content = codex_input_text(&params.input);
            if let Some(thread) = state.threads.get_mut(&resolved_thread_id) {
                if thread.preview.is_empty() {
                    thread.preview = content.chars().take(160).collect();
                }
                thread.updated_at = unix_now();
            }
            let turn_id = new_id("turn");
            let user_item_id = new_id("item");
            let assistant_item_id = new_id("item");
            let started_at = unix_now();
            let started_turn = codex_turn(
                &turn_id,
                "inProgress",
                started_at,
                None,
                None,
                vec![codex_user_item(&user_item_id, &content)],
            );
            if let Some(id) = request.id {
                emit_response(stdout, id, json!({ "turn": started_turn.clone() }))?;
            }
            emit_notification(
                stdout,
                "turn/started",
                json!({ "threadId": resolved_thread_id, "turn": started_turn }),
            )?;
            let response_result = {
                let mut delta_emit_error = None::<anyhow::Error>;
                let thread_id = app.session.session_id.clone();
                let turn_id_for_delta = turn_id.clone();
                let assistant_item_id_for_delta = assistant_item_id.clone();
                let mut on_delta = |delta: &str| {
                    if delta.is_empty() || delta_emit_error.is_some() {
                        return;
                    }
                    if let Err(error) = emit_notification(
                        stdout,
                        "item/agentMessage/delta",
                        json!({
                            "threadId": thread_id,
                            "turnId": turn_id_for_delta,
                            "itemId": assistant_item_id_for_delta,
                            "delta": delta,
                        }),
                    ) {
                        delta_emit_error = Some(error);
                    }
                };
                let response = app.send_headless_streaming(&content, &mut on_delta);
                match (response, delta_emit_error) {
                    (Ok(response), None) => Ok(response),
                    (Ok(_), Some(error)) => Err(error),
                    (Err(error), _) => Err(error),
                }
            };
            let completed_at = unix_now();
            let duration_ms = (completed_at - started_at).max(0) * 1000;
            match response_result {
                Ok(response) => {
                    let turn = codex_turn(
                        &turn_id,
                        "completed",
                        started_at,
                        Some(completed_at),
                        Some(duration_ms),
                        vec![
                            codex_user_item(&user_item_id, &content),
                            codex_agent_item(&assistant_item_id, &response),
                        ],
                    );
                    emit_notification(
                        stdout,
                        "turn/completed",
                        json!({ "threadId": app.session.session_id, "turn": turn }),
                    )?;
                }
                Err(error) => {
                    let turn = codex_turn(
                        &turn_id,
                        "failed",
                        started_at,
                        Some(completed_at),
                        Some(duration_ms),
                        vec![codex_user_item(&user_item_id, &content)],
                    );
                    emit_notification(
                        stdout,
                        "turn/completed",
                        json!({ "threadId": app.session.session_id, "turn": turn }),
                    )?;
                    return Err(error);
                }
            }
        }
        "model/list" => {
            ensure_initialized(state)?;
            let params: ModelsListParams =
                serde_json::from_value(request.params).unwrap_or(ModelsListParams {
                    refresh: None,
                    provider: None,
                });
            let requested_provider = params
                .provider
                .clone()
                .unwrap_or_else(|| app.session.current_provider.clone());
            if params.refresh.unwrap_or(false)
                || provider_requires_dynamic_model_discovery(&requested_provider)
            {
                let _ = app.refresh_provider_models(&requested_provider);
            }
            let data: Vec<Value> = app
                .models
                .by_provider(&requested_provider)
                .into_iter()
                .map(|model| {
                    json!({
                        "id": model.name,
                        "name": model.display_name.as_deref().unwrap_or(&model.name),
                        "provider": requested_provider,
                        "modelProvider": model.provider,
                        "contextWindow": model.context_window,
                        "supported": model.enabled,
                    })
                })
                .collect();
            if let Some(id) = request.id {
                emit_response(stdout, id, json!({ "data": data, "nextCursor": null }))?;
            }
        }
        "initialize" | "session.status" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "session.status",
                    id: request.id,
                    payload: snapshot(app),
                },
            )?;
        }
        "session.start" | "workspace.switch" => {
            let params: StartParams = serde_json::from_value(request.params)?;
            let workspace = params.workspace.unwrap_or_else(|| app.cwd.clone());
            *app = start_app(
                workspace,
                data_root.map(PathBuf::from),
                params.provider,
                params.model,
                params.agent,
                dangerously_bypass_approvals_and_sandbox,
            )?;
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "session.started",
                    id: request.id,
                    payload: snapshot(app),
                },
            )?;
        }
        "session.messages" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "session.messages",
                    id: request.id,
                    payload: json!({
                        "session": snapshot(app),
                        "messages": app.session.messages,
                    }),
                },
            )?;
        }
        "session.list" => {
            let workspace = app.cwd.display().to_string();
            let sessions = app
                .sessions
                .list()?
                .into_iter()
                .filter(|session| session.cwd == workspace)
                .map(|session| {
                    json!({
                        "id": session.session_id,
                        "session_id": session.session_id,
                        "title": session.title,
                        "created_at": session.created_at,
                        "cwd": session.cwd,
                        "message_count": session.messages.len(),
                        "provider": session.current_provider,
                        "model": session.current_model,
                        "current": session.session_id == app.session.session_id,
                        "status": session.status,
                    })
                })
                .collect::<Vec<_>>();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "session.list",
                    id: request.id,
                    payload: json!({
                        "session": snapshot(app),
                        "workspace": workspace,
                        "sessions": sessions,
                    }),
                },
            )?;
        }
        "session.load" => {
            let params: SessionLoadParams = serde_json::from_value(request.params)?;
            let command = format!("/load {}", params.id);
            let output = app.execute_command(&command)?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "session.loaded",
                    id: request.id,
                    payload: json!({
                        "command": command,
                        "output": output,
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "session.exportMarkdown" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "session.exportMarkdown",
                    id: request.id,
                    payload: json!({
                        "session": snapshot(app),
                        "markdown": transcript_markdown(app),
                    }),
                },
            )?;
        }
        "turn.send" => {
            let params: TurnParams = serde_json::from_value(request.params)?;
            emit_legacy_turn(app, state, request.id, params.content, stdout)?;
        }
        "command.run" | "command.invoke" => {
            let params: CommandParams = serde_json::from_value(request.params)?;
            let output = app.execute_command(&params.command)?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "command.completed",
                    id: request.id,
                    payload: json!({
                        "command": params.command,
                        "output": output,
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "commands.list" => {
            let commands = app.commands.all().into_iter().collect::<Vec<_>>();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "commands.list",
                    id: request.id,
                    payload: json!({
                        "commands": commands,
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "commands.suggest" => {
            let params: CommandsSuggestParams = serde_json::from_value(request.params)?;
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "commands.suggest",
                    id: request.id,
                    payload: json!({
                        "prefix": params.prefix,
                        "suggestions": app.commands.suggest(&params.prefix),
                    }),
                },
            )?;
        }
        "commands.describe" => {
            let params: CommandDescribeParams = serde_json::from_value(request.params)?;
            let canonical = app.commands.canonical(&params.name);
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "commands.describe",
                    id: request.id,
                    payload: json!({
                        "name": params.name,
                        "canonical": canonical,
                        "command": app.commands.get(&canonical),
                    }),
                },
            )?;
        }
        "bridge.ping" | "ping" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "bridge.pong",
                    id: request.id,
                    payload: json!({
                        "now": unix_now(),
                        "session": snapshot(app),
                        "lease": bridge_lease_status(app, state),
                    }),
                },
            )?;
        }
        "bridge.heartbeat" | "session.heartbeat" => {
            state.last_heartbeat_at = Some(unix_now());
            state.heartbeat_count = state.heartbeat_count.saturating_add(1);
            let payload = bridge_lease_status(app, state);
            app.logger.emit(
                "bridge_heartbeat",
                json!({
                    "session": app.session.session_id,
                    "workspace": app.cwd.display().to_string(),
                    "heartbeat_count": state.heartbeat_count,
                    "last_heartbeat_at": state.last_heartbeat_at,
                }),
            );
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "bridge.heartbeat",
                    id: request.id,
                    payload,
                },
            )?;
        }
        "bridge.lease" | "session.lease" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "bridge.lease",
                    id: request.id,
                    payload: bridge_lease_status(app, state),
                },
            )?;
        }
        "bridge.capabilities" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "bridge.capabilities",
                    id: request.id,
                    payload: bridge_capabilities(app),
                },
            )?;
        }
        "provider.select" => {
            let params: ProviderSelectParams = serde_json::from_value(request.params)?;
            let command = if params.global {
                format!("/provider --global {}", params.provider)
            } else {
                format!("/provider {}", params.provider)
            };
            let output = app.execute_command(&command)?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "provider.selected",
                    id: request.id,
                    payload: json!({
                        "command": command,
                        "output": output,
                        "session": snapshot(app),
                        "current_provider": app.session.current_provider,
                    }),
                },
            )?;
        }
        "model.select" => {
            let params: ModelSelectParams = serde_json::from_value(request.params)?;
            let command = format!("/model {}", params.model);
            let output = app.execute_command(&command)?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "model.selected",
                    id: request.id,
                    payload: json!({
                        "command": command,
                        "output": output,
                        "session": snapshot(app),
                        "current_model": app.session.current_model,
                    }),
                },
            )?;
        }
        "agent.select" => {
            let params: AgentSelectParams = serde_json::from_value(request.params)?;
            let command = format!("/agent use {}", params.agent);
            let output = app.execute_command(&command)?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "agent.selected",
                    id: request.id,
                    payload: json!({
                        "command": command,
                        "output": output,
                        "session": snapshot(app),
                        "active_agent": app.session.active_agent_id,
                    }),
                },
            )?;
        }
        "effort.status" => {
            let output = app.execute_command("/effort")?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "effort.status",
                    id: request.id,
                    payload: json!({
                        "output": output,
                        "current_reasoning_level": app.session.current_reasoning_level,
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "effort.set" => {
            let params: EffortSetParams = serde_json::from_value(request.params)?;
            let command = format!("/effort {}", params.effort);
            let output = app.execute_command(&command)?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "effort.updated",
                    id: request.id,
                    payload: json!({
                        "command": command,
                        "output": output,
                        "current_reasoning_level": app.session.current_reasoning_level,
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "fast.status" => {
            let output = app.execute_command("/fast status")?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "fast.status",
                    id: request.id,
                    payload: json!({
                        "output": output,
                        "fast_mode": app.session.fast_mode,
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "fast.set" => {
            let params: FastSetParams = serde_json::from_value(request.params)?;
            let command = if params.enabled {
                "/fast on"
            } else {
                "/fast off"
            };
            let output = app.execute_command(command)?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "fast.updated",
                    id: request.id,
                    payload: json!({
                        "command": command,
                        "output": output,
                        "fast_mode": app.session.fast_mode,
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "toolLimit.status" => {
            let output = app.execute_command("/tool-limit")?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "toolLimit.status",
                    id: request.id,
                    payload: json!({
                        "output": output,
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "toolLimit.set" => {
            let params: ToolLimitSetParams = serde_json::from_value(request.params)?;
            let raw_value = match params.value {
                Value::Number(number) => number.to_string(),
                Value::String(value) => value,
                Value::Null => "default".to_string(),
                other => anyhow::bail!(
                    "toolLimit.set value must be a number, string, or null; got {other}"
                ),
            };
            let command = format!("/tool-limit {raw_value}");
            let output = app.execute_command(&command)?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "toolLimit.updated",
                    id: request.id,
                    payload: json!({
                        "command": command,
                        "output": output,
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "runtime.status" => {
            let status_output = app.execute_command("/status")?.unwrap_or_default();
            let tools_output = app.execute_command("/tools status")?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "runtime.status",
                    id: request.id,
                    payload: json!({
                        "session": snapshot(app),
                        "status_output": status_output,
                        "tools_output": tools_output,
                        "risky_tools_enabled": app.risky_tools_enabled,
                        "human_approval_required": app.tool_executor.guardrails.policy.require_human_approval,
                        "dangerously_bypass_approvals_and_sandbox": app.dangerously_bypass_approvals_and_sandbox,
                        "pending_approvals": pending_approvals(app),
                    }),
                },
            )?;
        }
        "openai.compat.info" => {
            let params: OpenAiCompatInfoParams =
                serde_json::from_value(request.params).unwrap_or(OpenAiCompatInfoParams {
                    host: default_openai_compat_host(),
                    port: default_openai_compat_port(),
                });
            let base_url = format!("http://{}:{}/v1", params.host, params.port);
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "openai.compat.info",
                    id: request.id,
                    payload: json!({
                        "base_url": base_url,
                        "endpoints": [
                            "/v1/models",
                            "/v1/chat/completions",
                            "/v1/responses"
                        ],
                        "launch_command": format!(
                            "vegvisir --provider {} --model {} open-ai-compat-server --host {} --port {} --workspace {}",
                            app.session.current_provider,
                            app.session.current_model,
                            params.host,
                            params.port,
                            app.cwd.display()
                        ),
                        "note": "OpenAI-compatible clients must point at this local Vegvisir bridge. Provider credentials remain behind Vegvisir/HBSE; clients should not receive plaintext secrets.",
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "workspace.status" => {
            let output = app.execute_command("/workspace")?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "workspace.status",
                    id: request.id,
                    payload: json!({
                        "workspace": app.cwd.display().to_string(),
                        "output": output,
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "tools.list" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "tools.list",
                    id: request.id,
                    payload: json!({
                        "tools": app.tool_registry.schemas(),
                        "risky_tools_enabled": app.risky_tools_enabled,
                        "human_approval_required": app.tool_executor.guardrails.policy.require_human_approval,
                        "dangerously_bypass_approvals_and_sandbox": app.dangerously_bypass_approvals_and_sandbox,
                    }),
                },
            )?;
        }
        "providers.list" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "providers.list",
                    id: request.id,
                    payload: json!({
                        "current_provider": app.session.current_provider,
                        "providers": app.provider_registry.list(),
                        "availability": app.provider_registry.availability(),
                    }),
                },
            )?;
        }
        "models.list" => {
            let params: ModelsListParams =
                serde_json::from_value(request.params).unwrap_or(ModelsListParams {
                    refresh: None,
                    provider: None,
                });
            let requested_provider = params
                .provider
                .clone()
                .unwrap_or_else(|| app.session.current_provider.clone());
            let should_refresh = params.refresh.unwrap_or(false)
                || provider_requires_dynamic_model_discovery(&requested_provider);
            let refresh_notes = if should_refresh {
                if params.provider.is_some()
                    || provider_requires_dynamic_model_discovery(&requested_provider)
                {
                    app.refresh_provider_models(&requested_provider)
                        .map(|note| vec![format!("{requested_provider}: {note}")])
                        .unwrap_or_default()
                } else {
                    app.refresh_all_provider_models()
                }
            } else {
                Vec::new()
            };
            let models = models_for_provider(app, &requested_provider);
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "models.list",
                    id: request.id,
                    payload: json!({
                        "current_model": app.session.current_model,
                        "current_provider": app.session.current_provider,
                        "requested_provider": requested_provider,
                        "models": models,
                        "all_models": app.models.list(),
                        "provider_models": provider_models(app),
                        "refresh_notes": refresh_notes,
                    }),
                },
            )?;
        }
        "hbse.onboarding.providers" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "hbse.onboarding.providers",
                    id: request.id,
                    payload: json!({
                        "providers": hbse_onboarding_providers(app),
                        "script": "scripts/hbse-provider-onboard.sh",
                        "note": "Secrets must be entered through deterministic HBSE onboarding, not through model chat.",
                    }),
                },
            )?;
        }
        "agents.list" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "agents.list",
                    id: request.id,
                    payload: json!({
                        "active_agent": app.session.active_agent_id,
                        "agents": app.agents.list()?,
                    }),
                },
            )?;
        }
        "approvals.list" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "approvals.list",
                    id: request.id,
                    payload: json!({
                        "approvals": pending_approvals(app),
                    }),
                },
            )?;
        }
        "control.respond" | "controlRequests.respond" => {
            let params: ControlRespondParams = serde_json::from_value(request.params)?;
            let applied = app.apply_approval_control_response(params.response);
            if applied.applied
                && matches!(
                    applied.decision,
                    crate::control_requests::ApprovalControlDecisionKind::Deny
                        | crate::control_requests::ApprovalControlDecisionKind::Cancel
                )
            {
                state.pending_approval_turns.remove(&applied.approval_id);
            }
            let audit = bridge_control_response_audit(app, &applied);
            app.logger.emit("bridge_control_response", audit.clone());
            let response_id = request.id.clone();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "control.responded",
                    id: request.id,
                    payload: json!({
                        "ok": applied.applied,
                        "request_id": applied.request_id,
                        "approval_id": applied.approval_id,
                        "decision": applied.decision,
                        "decision_source": applied.decision_source,
                        "message": applied.message,
                        "audit": audit,
                        "approvals": pending_approvals(app),
                        "session": snapshot(app),
                    }),
                },
            )?;
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "control.respond.audit",
                    id: response_id,
                    payload: bridge_control_response_audit(app, &applied),
                },
            )?;
        }
        "approvals.approveOnce" => {
            let params: ApprovalIdParams = serde_json::from_value(request.params)?;
            let applied = app.apply_approval_control_decision(
                &params.id,
                "bridge",
                crate::control_requests::ApprovalControlDecisionKind::AllowOnce,
            );
            emit_approval_mutation(stdout, request.id, applied.applied, app)?;
        }
        "approvals.approveOnceAndExecute" => {
            let params: ApprovalIdParams = serde_json::from_value(request.params)?;
            let applied = app.apply_approval_control_decision(
                &params.id,
                "bridge",
                crate::control_requests::ApprovalControlDecisionKind::AllowOnce,
            );
            continue_or_execute_approved_request(app, state, request.id, applied.approval, stdout)?;
        }
        "approvals.approveSession" => {
            let params: ApprovalIdParams = serde_json::from_value(request.params)?;
            let applied = app.apply_approval_control_decision(
                &params.id,
                "bridge",
                crate::control_requests::ApprovalControlDecisionKind::AllowForSession,
            );
            emit_approval_mutation(stdout, request.id, applied.applied, app)?;
        }
        "approvals.approveSessionAndExecute" => {
            let params: ApprovalIdParams = serde_json::from_value(request.params)?;
            let applied = app.apply_approval_control_decision(
                &params.id,
                "bridge",
                crate::control_requests::ApprovalControlDecisionKind::AllowForSession,
            );
            continue_or_execute_approved_request(app, state, request.id, applied.approval, stdout)?;
        }
        "approvals.deny" => {
            let params: ApprovalIdParams = serde_json::from_value(request.params)?;
            let applied = app.apply_approval_control_decision(
                &params.id,
                "bridge",
                crate::control_requests::ApprovalControlDecisionKind::Deny,
            );
            if applied.applied {
                state.pending_approval_turns.remove(&params.id);
            }
            emit_approval_mutation(stdout, request.id, applied.applied, app)?;
        }
        "approvals.edit" => {
            let params: ApprovalEditParams = serde_json::from_value(request.params)?;
            let edited = app
                .tool_executor
                .guardrails
                .approvals
                .edit(&params.id, params.args);
            if let Some(edited) = edited.as_ref()
                && edited.id != params.id
            {
                app.cancel_approval_control_request(
                    &params.id,
                    "approval edited by bridge; superseded by new approval id",
                );
                if let Some(turn) = state.pending_approval_turns.remove(&params.id) {
                    state.pending_approval_turns.insert(edited.id.clone(), turn);
                }
            }
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "approvals.updated",
                    id: request.id,
                    payload: json!({
                        "ok": edited.is_some(),
                        "edited": edited,
                        "approvals": pending_approvals(app),
                    }),
                },
            )?;
        }
        "diff.current" => {
            let params: DiffParams = serde_json::from_value(request.params)?;
            let mut command = String::from("/diff");
            if params.staged.unwrap_or(false) {
                command.push_str(" --staged");
            }
            if params.stat.unwrap_or(false) {
                command.push_str(" --stat");
            }
            if let Some(path) = params.path.filter(|path| !path.trim().is_empty()) {
                command.push(' ');
                command.push_str(&path);
            }
            let output = app.execute_command(&command)?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "diff.current",
                    id: request.id,
                    payload: json!({
                        "command": command,
                        "diff": output,
                    }),
                },
            )?;
        }
        "memory.status" => {
            let output = app.execute_command("/memory status")?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "memory.status",
                    id: request.id,
                    payload: json!({
                        "output": output,
                        "cms": {
                            "user_id": app.cms.config.user_id,
                            "project_id": app.cms.config.project_id,
                            "context_mode": format!("{:?}", app.cms.config.context_mode),
                        },
                    }),
                },
            )?;
        }
        "system.prompt" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "system.prompt",
                    id: request.id,
                    payload: json!({
                        "prompt": app.session.system_prompt,
                    }),
                },
            )?;
        }
        "system.prompt.set" => {
            let params: SystemPromptSetParams = serde_json::from_value(request.params)?;
            app.session.system_prompt = params.prompt;
            app.autosave_session();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "system.prompt",
                    id: request.id,
                    payload: json!({
                        "prompt": app.session.system_prompt,
                    }),
                },
            )?;
        }
        method if command_backed_bridge_spec(method).is_some() => {
            let spec = command_backed_bridge_spec(method).expect("checked above");
            let command = bridge_command_from_params(spec, request.params)?;
            let output = app.execute_command(&command)?.unwrap_or_default();
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: spec.event_kind,
                    id: request.id,
                    payload: json!({
                        "method": method,
                        "command": command,
                        "output": output,
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "shutdown" => {
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "server.shutdown",
                    id: request.id,
                    payload: json!({ "ok": true }),
                },
            )?;
            return Ok(BridgeControl::Shutdown);
        }
        other => {
            emit_error(
                stdout,
                request.id,
                "unknown_method",
                format!("Unknown bridge method: {other}"),
            )?;
        }
    }
    Ok(BridgeControl::Continue)
}

fn emit_legacy_turn(
    app: &mut TuiApplication,
    state: &mut BridgeState,
    id: Option<BridgeRequestId>,
    content: String,
    stdout: &mut dyn Write,
) -> anyhow::Result<()> {
    emit_legacy(
        stdout,
        BridgeEvent {
            kind: "turn.started",
            id: id.clone(),
            payload: json!({
                "session_id": app.session.session_id,
                "workspace": app.cwd.display().to_string(),
            }),
        },
    )?;
    let response_result = {
        let mut delta_emit_error = None::<anyhow::Error>;
        let mut on_delta = |delta: &str| {
            if delta.is_empty() || delta_emit_error.is_some() {
                return;
            }
            if let Err(error) = emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "content.delta",
                    id: id.clone(),
                    payload: json!({
                        "role": "assistant",
                        "text": delta,
                    }),
                },
            ) {
                delta_emit_error = Some(error);
            }
        };
        let response = app.send_headless_streaming(&content, &mut on_delta);
        match (response, delta_emit_error) {
            (Ok(response), None) => Ok(response),
            (Ok(_), Some(error)) => Err(error),
            (Err(error), _) => Err(error),
        }
    };
    match response_result {
        Ok(response) => emit_legacy(
            stdout,
            BridgeEvent {
                kind: "turn.completed",
                id,
                payload: json!({
                    "answer": response,
                    "session": snapshot(app),
                }),
            },
        )?,
        Err(error) => emit_legacy_turn_failure(app, state, id, &content, error, stdout)?,
    }
    Ok(())
}

fn emit_legacy_turn_failure(
    app: &mut TuiApplication,
    state: &mut BridgeState,
    id: Option<BridgeRequestId>,
    content: &str,
    error: anyhow::Error,
    stdout: &mut dyn Write,
) -> anyhow::Result<()> {
    let message = error.to_string();
    let pending = pending_approvals(app);
    if !pending.is_empty() {
        if app
            .session
            .messages
            .last()
            .map(|message| message.role == "user" && message.content == content)
            .unwrap_or(false)
        {
            app.session.messages.pop();
        }
        remember_pending_approval_turns(
            state,
            &pending,
            content,
            approval_id_from_message(&message),
        );
        app.session.status = "ready".to_string();
        app.session.activity.clear();
        app.autosave_session();
        emit_legacy(
            stdout,
            BridgeEvent {
                kind: "approval.required",
                id: id.clone(),
                payload: json!({
                    "approvals": pending,
                    "session": snapshot(app),
                }),
            },
        )?;
    }
    emit_legacy(
        stdout,
        BridgeEvent {
            kind: "turn.failed",
            id,
            payload: json!({
                "error": message.clone(),
                "message": message,
                "session": snapshot(app),
            }),
        },
    )
}

fn remember_pending_approval_turns(
    state: &mut BridgeState,
    approvals: &[Value],
    content: &str,
    requested_approval_id: Option<String>,
) {
    for approval in approvals {
        let Some(id) = approval.get("id").and_then(Value::as_str) else {
            continue;
        };
        if requested_approval_id
            .as_deref()
            .is_some_and(|requested| requested != id)
        {
            continue;
        }
        state.pending_approval_turns.insert(
            id.to_string(),
            PendingApprovalTurn {
                content: content.to_string(),
            },
        );
    }
}

fn approval_id_from_message(message: &str) -> Option<String> {
    message
        .split("approval_id=")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .map(|id| {
            id.trim_matches(|ch: char| ch == ',' || ch == ';' || ch == '.')
                .to_string()
        })
        .filter(|id| !id.is_empty())
}

fn continue_or_execute_approved_request(
    app: &mut TuiApplication,
    state: &mut BridgeState,
    id: Option<BridgeRequestId>,
    approved: Option<crate::guardrails::ApprovalRequest>,
    stdout: &mut dyn Write,
) -> anyhow::Result<()> {
    let continuation = approved
        .as_ref()
        .and_then(|approval| state.pending_approval_turns.remove(&approval.id));
    if let Some(turn) = continuation {
        emit_legacy(
            stdout,
            BridgeEvent {
                kind: "approval.executed",
                id: id.clone(),
                payload: json!({
                    "ok": true,
                    "approval": approved,
                    "observation": null,
                    "continued": true,
                    "message": "Approval applied; resuming model turn so the approved tool result is visible to the model.",
                    "approvals": pending_approvals(app),
                    "session": snapshot(app),
                }),
            },
        )?;
        return emit_legacy_turn(app, state, id, turn.content, stdout);
    }

    let observation = approved.as_ref().map(|approval| {
        app.tool_executor.execute(ToolCall {
            name: approval.tool_name.clone(),
            args: approval.args.clone(),
        })
    });
    emit_legacy(
        stdout,
        BridgeEvent {
            kind: "approval.executed",
            id,
            payload: json!({
                "ok": approved.is_some(),
                "approval": approved,
                "observation": observation,
                "continued": false,
                "approvals": pending_approvals(app),
                "session": snapshot(app),
            }),
        },
    )
}

#[derive(Clone, Copy)]
struct CommandBackedBridgeSpec {
    method: &'static str,
    event_kind: &'static str,
    command: &'static str,
    default_subcommand: Option<&'static str>,
}

const COMMAND_BACKED_BRIDGE_SPECS: &[CommandBackedBridgeSpec] = &[
    CommandBackedBridgeSpec {
        method: "sessions.list",
        event_kind: "sessions.list",
        command: "/sessions",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.new",
        event_kind: "session.new",
        command: "/new",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.save",
        event_kind: "session.save",
        command: "/save",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.load",
        event_kind: "session.load",
        command: "/load",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.reset",
        event_kind: "session.reset",
        command: "/reset",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.retry",
        event_kind: "session.retry",
        command: "/retry",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.undo",
        event_kind: "session.undo",
        command: "/undo",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.title",
        event_kind: "session.title",
        command: "/title",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.branch",
        event_kind: "session.branch",
        command: "/branch",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.fork",
        event_kind: "session.fork",
        command: "/fork",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.history",
        event_kind: "session.history",
        command: "/history",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.compress",
        event_kind: "session.compress",
        command: "/compress",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.summary",
        event_kind: "session.summary",
        command: "/summary",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "session.handoff",
        event_kind: "session.handoff",
        command: "/handoff",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "projects.list",
        event_kind: "projects.list",
        command: "/projects",
        default_subcommand: Some("list"),
    },
    CommandBackedBridgeSpec {
        method: "projects.use",
        event_kind: "projects.use",
        command: "/projects",
        default_subcommand: Some("use"),
    },
    CommandBackedBridgeSpec {
        method: "projects.name",
        event_kind: "projects.name",
        command: "/projects",
        default_subcommand: Some("name"),
    },
    CommandBackedBridgeSpec {
        method: "projects.forget",
        event_kind: "projects.forget",
        command: "/projects",
        default_subcommand: Some("forget"),
    },
    CommandBackedBridgeSpec {
        method: "runtime.cancel",
        event_kind: "runtime.cancel",
        command: "/cancel",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "runtime.turnRepair",
        event_kind: "runtime.turnRepair",
        command: "/turn-repair",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "runtime.recover",
        event_kind: "runtime.recover",
        command: "/recover",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "runtime.auto",
        event_kind: "runtime.auto",
        command: "/auto",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "runtime.autonomy",
        event_kind: "runtime.autonomy",
        command: "/autonomy",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "tools.status",
        event_kind: "tools.status",
        command: "/tools",
        default_subcommand: Some("status"),
    },
    CommandBackedBridgeSpec {
        method: "tools.explain",
        event_kind: "tools.explain",
        command: "/tools",
        default_subcommand: Some("explain"),
    },
    CommandBackedBridgeSpec {
        method: "tools.allowRisky",
        event_kind: "tools.allowRisky",
        command: "/tools",
        default_subcommand: Some("allow-risky"),
    },
    CommandBackedBridgeSpec {
        method: "tools.denyRisky",
        event_kind: "tools.denyRisky",
        command: "/tools",
        default_subcommand: Some("deny-risky"),
    },
    CommandBackedBridgeSpec {
        method: "tools.requireApproval",
        event_kind: "tools.requireApproval",
        command: "/tools",
        default_subcommand: Some("require-approval"),
    },
    CommandBackedBridgeSpec {
        method: "tools.noApproval",
        event_kind: "tools.noApproval",
        command: "/tools",
        default_subcommand: Some("no-approval"),
    },
    CommandBackedBridgeSpec {
        method: "memory.recent",
        event_kind: "memory.recent",
        command: "/memory",
        default_subcommand: Some("recent"),
    },
    CommandBackedBridgeSpec {
        method: "memory.recall",
        event_kind: "memory.recall",
        command: "/recall",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "memory.remember",
        event_kind: "memory.remember",
        command: "/remember",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "memory.context",
        event_kind: "memory.context",
        command: "/context",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "memory.modelRequest",
        event_kind: "memory.modelRequest",
        command: "/model-request",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "memory.usedThisTurn",
        event_kind: "memory.usedThisTurn",
        command: "/memory",
        default_subcommand: Some("used-this-turn"),
    },
    CommandBackedBridgeSpec {
        method: "memory.writesThisSession",
        event_kind: "memory.writesThisSession",
        command: "/memory",
        default_subcommand: Some("writes-this-session"),
    },
    CommandBackedBridgeSpec {
        method: "memory.why",
        event_kind: "memory.why",
        command: "/memory",
        default_subcommand: Some("why"),
    },
    CommandBackedBridgeSpec {
        method: "memory.diff",
        event_kind: "memory.diff",
        command: "/memory",
        default_subcommand: Some("diff"),
    },
    CommandBackedBridgeSpec {
        method: "memory.quarantine",
        event_kind: "memory.quarantine",
        command: "/memory",
        default_subcommand: Some("quarantine"),
    },
    CommandBackedBridgeSpec {
        method: "memory.forget",
        event_kind: "memory.forget",
        command: "/memory",
        default_subcommand: Some("forget"),
    },
    CommandBackedBridgeSpec {
        method: "memory.export",
        event_kind: "memory.export",
        command: "/memory",
        default_subcommand: Some("export"),
    },
    CommandBackedBridgeSpec {
        method: "memory.importChatGpt",
        event_kind: "memory.importChatGpt",
        command: "/memory",
        default_subcommand: Some("import-chatgpt"),
    },
    CommandBackedBridgeSpec {
        method: "memory.searchChatGpt",
        event_kind: "memory.searchChatGpt",
        command: "/memory",
        default_subcommand: Some("search-chatgpt"),
    },
    CommandBackedBridgeSpec {
        method: "skills.status",
        event_kind: "skills.status",
        command: "/skills",
        default_subcommand: Some("status"),
    },
    CommandBackedBridgeSpec {
        method: "skills.compile",
        event_kind: "skills.compile",
        command: "/skills",
        default_subcommand: Some("compile"),
    },
    CommandBackedBridgeSpec {
        method: "skills.route",
        event_kind: "skills.route",
        command: "/skills",
        default_subcommand: Some("route"),
    },
    CommandBackedBridgeSpec {
        method: "skills.load",
        event_kind: "skills.load",
        command: "/skills",
        default_subcommand: Some("load"),
    },
    CommandBackedBridgeSpec {
        method: "skills.eval",
        event_kind: "skills.eval",
        command: "/skills",
        default_subcommand: Some("eval"),
    },
    CommandBackedBridgeSpec {
        method: "skills.forge",
        event_kind: "skills.forge",
        command: "/skills",
        default_subcommand: Some("forge"),
    },
    CommandBackedBridgeSpec {
        method: "skills.patch",
        event_kind: "skills.patch",
        command: "/skills",
        default_subcommand: Some("patch"),
    },
    CommandBackedBridgeSpec {
        method: "skills.curate",
        event_kind: "skills.curate",
        command: "/skills",
        default_subcommand: Some("curate"),
    },
    CommandBackedBridgeSpec {
        method: "skills.detect",
        event_kind: "skills.detect",
        command: "/skills",
        default_subcommand: Some("detect"),
    },
    CommandBackedBridgeSpec {
        method: "skills.trace",
        event_kind: "skills.trace",
        command: "/skills",
        default_subcommand: Some("trace"),
    },
    CommandBackedBridgeSpec {
        method: "skills.promote",
        event_kind: "skills.promote",
        command: "/skills",
        default_subcommand: Some("promote"),
    },
    CommandBackedBridgeSpec {
        method: "skills.archive",
        event_kind: "skills.archive",
        command: "/skills",
        default_subcommand: Some("archive"),
    },
    CommandBackedBridgeSpec {
        method: "skills.config",
        event_kind: "skills.config",
        command: "/skills",
        default_subcommand: Some("config"),
    },
    CommandBackedBridgeSpec {
        method: "skills.explain",
        event_kind: "skills.explain",
        command: "/skills",
        default_subcommand: Some("explain"),
    },
    CommandBackedBridgeSpec {
        method: "skills.invoke",
        event_kind: "skills.invoke",
        command: "/skills",
        default_subcommand: Some("invoke"),
    },
    CommandBackedBridgeSpec {
        method: "agents.templates",
        event_kind: "agents.templates",
        command: "/agent",
        default_subcommand: Some("templates"),
    },
    CommandBackedBridgeSpec {
        method: "agents.create",
        event_kind: "agents.create",
        command: "/agent",
        default_subcommand: Some("create"),
    },
    CommandBackedBridgeSpec {
        method: "agents.design",
        event_kind: "agents.design",
        command: "/agent",
        default_subcommand: Some("design"),
    },
    CommandBackedBridgeSpec {
        method: "agents.show",
        event_kind: "agents.show",
        command: "/agent",
        default_subcommand: Some("show"),
    },
    CommandBackedBridgeSpec {
        method: "agents.delete",
        event_kind: "agents.delete",
        command: "/agent",
        default_subcommand: Some("delete"),
    },
    CommandBackedBridgeSpec {
        method: "agents.clear",
        event_kind: "agents.clear",
        command: "/agent",
        default_subcommand: Some("clear"),
    },
    CommandBackedBridgeSpec {
        method: "subagents.list",
        event_kind: "subagents.list",
        command: "/subagents",
        default_subcommand: Some("list"),
    },
    CommandBackedBridgeSpec {
        method: "subagents.show",
        event_kind: "subagents.show",
        command: "/subagents",
        default_subcommand: Some("show"),
    },
    CommandBackedBridgeSpec {
        method: "subagents.cancel",
        event_kind: "subagents.cancel",
        command: "/subagents",
        default_subcommand: Some("cancel"),
    },
    CommandBackedBridgeSpec {
        method: "subagents.timeline",
        event_kind: "subagents.timeline",
        command: "/subagents",
        default_subcommand: Some("timeline"),
    },
    CommandBackedBridgeSpec {
        method: "subagents.events",
        event_kind: "subagents.events",
        command: "/subagents",
        default_subcommand: Some("events"),
    },
    CommandBackedBridgeSpec {
        method: "subagents.artifacts",
        event_kind: "subagents.artifacts",
        command: "/subagents",
        default_subcommand: Some("artifacts"),
    },
    CommandBackedBridgeSpec {
        method: "subagents.ownership",
        event_kind: "subagents.ownership",
        command: "/subagents",
        default_subcommand: Some("ownership"),
    },
    CommandBackedBridgeSpec {
        method: "subagents.policy",
        event_kind: "subagents.policy",
        command: "/subagents",
        default_subcommand: Some("policy"),
    },
    CommandBackedBridgeSpec {
        method: "subagents.max",
        event_kind: "subagents.max",
        command: "/subagents",
        default_subcommand: Some("max"),
    },
    CommandBackedBridgeSpec {
        method: "subagents.config",
        event_kind: "subagents.config",
        command: "/subagents",
        default_subcommand: Some("config"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.list",
        event_kind: "mcp.list",
        command: "/mcp",
        default_subcommand: Some("list"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.status",
        event_kind: "mcp.status",
        command: "/mcp",
        default_subcommand: Some("status"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.authMap",
        event_kind: "mcp.authMap",
        command: "/mcp",
        default_subcommand: Some("auth-map"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.show",
        event_kind: "mcp.show",
        command: "/mcp",
        default_subcommand: Some("show"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.tools",
        event_kind: "mcp.tools",
        command: "/mcp",
        default_subcommand: Some("tools"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.reload",
        event_kind: "mcp.reload",
        command: "/mcp",
        default_subcommand: Some("reload"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.addHttp",
        event_kind: "mcp.addHttp",
        command: "/mcp",
        default_subcommand: Some("add-http"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.addHttpService",
        event_kind: "mcp.addHttpService",
        command: "/mcp",
        default_subcommand: Some("add-http-service"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.addStdio",
        event_kind: "mcp.addStdio",
        command: "/mcp",
        default_subcommand: Some("add-stdio"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.addTool",
        event_kind: "mcp.addTool",
        command: "/mcp",
        default_subcommand: Some("add-tool"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.removeTool",
        event_kind: "mcp.removeTool",
        command: "/mcp",
        default_subcommand: Some("remove-tool"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.remove",
        event_kind: "mcp.remove",
        command: "/mcp",
        default_subcommand: Some("remove"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.enable",
        event_kind: "mcp.enable",
        command: "/mcp",
        default_subcommand: Some("enable"),
    },
    CommandBackedBridgeSpec {
        method: "mcp.disable",
        event_kind: "mcp.disable",
        command: "/mcp",
        default_subcommand: Some("disable"),
    },
    CommandBackedBridgeSpec {
        method: "hbse.status",
        event_kind: "hbse.status",
        command: "/hbse",
        default_subcommand: Some("status"),
    },
    CommandBackedBridgeSpec {
        method: "hbse.usageThisSession",
        event_kind: "hbse.usageThisSession",
        command: "/hbse",
        default_subcommand: Some("usage-this-session"),
    },
    CommandBackedBridgeSpec {
        method: "hbse.usageThisRun",
        event_kind: "hbse.usageThisRun",
        command: "/hbse",
        default_subcommand: Some("usage-this-run"),
    },
    CommandBackedBridgeSpec {
        method: "hbse.provider",
        event_kind: "hbse.provider",
        command: "/hbse",
        default_subcommand: Some("provider"),
    },
    CommandBackedBridgeSpec {
        method: "hbse.mcp",
        event_kind: "hbse.mcp",
        command: "/hbse",
        default_subcommand: Some("mcp"),
    },
    CommandBackedBridgeSpec {
        method: "hbse.services",
        event_kind: "hbse.services",
        command: "/hbse",
        default_subcommand: Some("services"),
    },
    CommandBackedBridgeSpec {
        method: "hbse.service",
        event_kind: "hbse.service",
        command: "/hbse",
        default_subcommand: Some("service"),
    },
    CommandBackedBridgeSpec {
        method: "verify.run",
        event_kind: "verify.run",
        command: "/verify",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "eval.run",
        event_kind: "eval.run",
        command: "/eval",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "trace.list",
        event_kind: "trace.list",
        command: "/trace",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "work.list",
        event_kind: "work.list",
        command: "/work",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "runs.list",
        event_kind: "runs.list",
        command: "/runs",
        default_subcommand: Some("list"),
    },
    CommandBackedBridgeSpec {
        method: "runs.show",
        event_kind: "runs.show",
        command: "/runs",
        default_subcommand: Some("show"),
    },
    CommandBackedBridgeSpec {
        method: "runs.open",
        event_kind: "runs.open",
        command: "/runs",
        default_subcommand: Some("open"),
    },
    CommandBackedBridgeSpec {
        method: "runs.export",
        event_kind: "runs.export",
        command: "/runs",
        default_subcommand: Some("export"),
    },
    CommandBackedBridgeSpec {
        method: "runs.diff",
        event_kind: "runs.diff",
        command: "/runs",
        default_subcommand: Some("diff"),
    },
    CommandBackedBridgeSpec {
        method: "runs.replayPlan",
        event_kind: "runs.replayPlan",
        command: "/runs",
        default_subcommand: Some("replay-plan"),
    },
    CommandBackedBridgeSpec {
        method: "config.status",
        event_kind: "config.status",
        command: "/config",
        default_subcommand: Some("status"),
    },
    CommandBackedBridgeSpec {
        method: "config.user",
        event_kind: "config.user",
        command: "/config",
        default_subcommand: Some("user"),
    },
    CommandBackedBridgeSpec {
        method: "config.path",
        event_kind: "config.path",
        command: "/config",
        default_subcommand: Some("path"),
    },
    CommandBackedBridgeSpec {
        method: "profile.show",
        event_kind: "profile.show",
        command: "/profile",
        default_subcommand: Some("show"),
    },
    CommandBackedBridgeSpec {
        method: "profile.path",
        event_kind: "profile.path",
        command: "/profile",
        default_subcommand: Some("path"),
    },
    CommandBackedBridgeSpec {
        method: "profile.init",
        event_kind: "profile.init",
        command: "/profile",
        default_subcommand: Some("init"),
    },
    CommandBackedBridgeSpec {
        method: "profile.set",
        event_kind: "profile.set",
        command: "/profile",
        default_subcommand: Some("set"),
    },
    CommandBackedBridgeSpec {
        method: "profile.add",
        event_kind: "profile.add",
        command: "/profile",
        default_subcommand: Some("add"),
    },
    CommandBackedBridgeSpec {
        method: "profile.remove",
        event_kind: "profile.remove",
        command: "/profile",
        default_subcommand: Some("remove"),
    },
    CommandBackedBridgeSpec {
        method: "profile.clear",
        event_kind: "profile.clear",
        command: "/profile",
        default_subcommand: Some("clear"),
    },
    CommandBackedBridgeSpec {
        method: "persona.list",
        event_kind: "persona.list",
        command: "/ka",
        default_subcommand: Some("list"),
    },
    CommandBackedBridgeSpec {
        method: "persona.show",
        event_kind: "persona.show",
        command: "/ka",
        default_subcommand: Some("show"),
    },
    CommandBackedBridgeSpec {
        method: "persona.set",
        event_kind: "persona.set",
        command: "/ka",
        default_subcommand: Some("set"),
    },
    CommandBackedBridgeSpec {
        method: "persona.create",
        event_kind: "persona.create",
        command: "/ka",
        default_subcommand: Some("create"),
    },
    CommandBackedBridgeSpec {
        method: "persona.import",
        event_kind: "persona.import",
        command: "/ka",
        default_subcommand: Some("import"),
    },
    CommandBackedBridgeSpec {
        method: "persona.edit",
        event_kind: "persona.edit",
        command: "/ka",
        default_subcommand: Some("edit"),
    },
    CommandBackedBridgeSpec {
        method: "persona.clear",
        event_kind: "persona.clear",
        command: "/ka",
        default_subcommand: Some("clear"),
    },
    CommandBackedBridgeSpec {
        method: "persona.default",
        event_kind: "persona.default",
        command: "/ka",
        default_subcommand: Some("default"),
    },
    CommandBackedBridgeSpec {
        method: "speech.status",
        event_kind: "speech.status",
        command: "/speech",
        default_subcommand: Some("status"),
    },
    CommandBackedBridgeSpec {
        method: "speech.transcribe",
        event_kind: "speech.transcribe",
        command: "/speech",
        default_subcommand: Some("transcribe"),
    },
    CommandBackedBridgeSpec {
        method: "speech.ptt",
        event_kind: "speech.ptt",
        command: "/speech",
        default_subcommand: Some("ptt"),
    },
    CommandBackedBridgeSpec {
        method: "speech.pttKey",
        event_kind: "speech.pttKey",
        command: "/speech",
        default_subcommand: Some("ptt-key"),
    },
    CommandBackedBridgeSpec {
        method: "speech.pttSeconds",
        event_kind: "speech.pttSeconds",
        command: "/speech",
        default_subcommand: Some("ptt-seconds"),
    },
    CommandBackedBridgeSpec {
        method: "tts.speak",
        event_kind: "tts.speak",
        command: "/tts",
        default_subcommand: None,
    },
    CommandBackedBridgeSpec {
        method: "auth.status",
        event_kind: "auth.status",
        command: "/auth",
        default_subcommand: None,
    },
];

fn command_backed_bridge_spec(method: &str) -> Option<CommandBackedBridgeSpec> {
    COMMAND_BACKED_BRIDGE_SPECS
        .iter()
        .copied()
        .find(|spec| spec.method == method)
}

fn bridge_capabilities(app: &TuiApplication) -> Value {
    let command_backed_methods = COMMAND_BACKED_BRIDGE_SPECS
        .iter()
        .map(|spec| {
            json!({
                "method": spec.method,
                "event": spec.event_kind,
                "command": spec.command,
                "default_subcommand": spec.default_subcommand,
            })
        })
        .collect::<Vec<_>>();
    let security_posture = bridge_security_posture(app);
    json!({
        "session": snapshot(app),
        "security_posture": security_posture,
        "securityPosture": security_posture,
        "native_methods": [
            "initialize",
            "initialized",
            "thread/start",
            "turn/start",
            "model/list",
            "session.status",
            "session.start",
            "workspace.switch",
            "session.messages",
            "session.list",
            "session.load",
            "session.exportMarkdown",
            "turn.send",
            "command.run",
            "command.invoke",
            "commands.list",
            "commands.suggest",
            "commands.describe",
            "bridge.ping",
            "ping",
            "bridge.heartbeat",
            "session.heartbeat",
            "bridge.lease",
            "session.lease",
            "bridge.capabilities",
            "provider.select",
            "model.select",
            "agent.select",
            "effort.status",
            "effort.set",
            "fast.status",
            "fast.set",
            "toolLimit.status",
            "toolLimit.set",
            "runtime.status",
            "openai.compat.info",
            "workspace.status",
            "tools.list",
            "providers.list",
            "models.list",
            "hbse.onboarding.providers",
            "agents.list",
            "approvals.list",
            "control.respond",
            "controlRequests.respond",
            "approvals.approveOnce",
            "approvals.approveOnceAndExecute",
            "approvals.approveSession",
            "approvals.approveSessionAndExecute",
            "approvals.deny",
            "approvals.edit",
            "diff.current",
            "memory.status",
            "system.prompt",
            "system.prompt.set",
            "shutdown"
        ],
        "lease": bridge_lease_capabilities(),
        "sessionLease": bridge_lease_capabilities(),
        "command_backed_methods": command_backed_methods,
        "commands": app.commands.all().into_iter().collect::<Vec<_>>(),
        "note": "command-backed methods execute the same Vegvisir slash-command handlers as the TUI and return structured envelope metadata plus command text output; command.invoke remains the universal escape hatch.",
    })
}

fn bridge_lease_capabilities() -> Value {
    json!({
        "mode": "process_scoped_stdio",
        "authority": "bridge process lifetime",
        "timeout_enforced": false,
        "recommended_heartbeat_interval_ms": 30_000,
        "methods": {
            "ping": ["bridge.ping", "ping"],
            "heartbeat": ["bridge.heartbeat", "session.heartbeat"],
            "status": ["bridge.lease", "session.lease"]
        },
        "note": "The initial lease is process-scoped: if the stdio bridge process exits, the lease ends. Heartbeats are observable/auditable but do not yet enforce timeout-based revocation."
    })
}

fn bridge_lease_status(app: &TuiApplication, state: &BridgeState) -> Value {
    json!({
        "mode": "process_scoped_stdio",
        "lease_id": format!("bridge:{}", app.session.session_id),
        "session_id": app.session.session_id,
        "initialized": state.initialized,
        "server_started_at": state.server_started_at,
        "now": unix_now(),
        "last_heartbeat_at": state.last_heartbeat_at,
        "heartbeat_count": state.heartbeat_count,
        "timeout_enforced": false,
        "recommended_heartbeat_interval_ms": 30_000,
    })
}

fn bridge_security_posture(app: &TuiApplication) -> Value {
    let tools = app.tool_registry.list();
    let remote_safe_tools = tools.iter().filter(|tool| !tool.risky).count();
    json!({
        "transport": {
            "mode": "stdio",
            "network_listener": false,
            "bind_address": null,
            "local_only": true,
            "note": "The app-server bridge is stdio IPC only; it does not bind a TCP listener. Network exposure, if any, must come from an explicit parent process wrapper outside Vegvisir."
        },
        "serving": {
            "activation": "explicit app-server command",
            "feature_flag_required": false,
            "dangerously_bypass_approvals_and_sandbox": app.dangerously_bypass_approvals_and_sandbox,
            "dangerous_bypass_startup_only": true
        },
        "session_lease": bridge_lease_capabilities(),
        "registry_remote_safe_filtering": {
            "status": "metadata_reported_not_enforced",
            "policy": "Bridge calls still route through the same TUI command handlers, GuardrailEngine, RuntimePolicy, approval ledger, and sandbox as local calls.",
            "total_tools": tools.len(),
            "remote_safe_tool_count": remote_safe_tools,
            "risky_tool_count": tools.len().saturating_sub(remote_safe_tools)
        },
        "approval_control": {
            "control_respond_audited": true,
            "authority": "ApprovalLedger + GuardrailEngine",
            "external_response_grants_permission_directly": false
        }
    })
}

fn bridge_control_response_audit(
    app: &TuiApplication,
    applied: &crate::app::ApprovalControlApplication,
) -> Value {
    json!({
        "method": "control.respond",
        "request_id": applied.request_id,
        "approval_id": applied.approval_id,
        "decision": applied.decision,
        "decision_source": applied.decision_source,
        "applied": applied.applied,
        "message": applied.message,
        "dangerously_bypass_approvals_and_sandbox": app.dangerously_bypass_approvals_and_sandbox,
        "pending_approvals": app.tool_executor.guardrails.approvals.pending_len(),
        "policy_authority": "ApprovalLedger + GuardrailEngine",
        "secret_payload_included": false
    })
}

fn bridge_command_from_params(
    spec: CommandBackedBridgeSpec,
    params: Value,
) -> anyhow::Result<String> {
    let params: CommandBackedParams = if params.is_null() {
        CommandBackedParams::default()
    } else {
        serde_json::from_value(params)?
    };
    if let Some(raw) = params.raw.filter(|raw| !raw.trim().is_empty()) {
        let raw = raw.trim();
        if raw.starts_with(spec.command) {
            return Ok(raw.to_string());
        }
        return Ok(format!("{} {raw}", spec.command));
    }

    let mut args = Vec::new();
    if let Some(subcommand) = spec.default_subcommand {
        args.push(subcommand.to_string());
    }
    if params.global {
        args.push("--global".to_string());
    }
    if let Some(limit) = params.limit {
        args.push("--limit".to_string());
        args.push(limit.to_string());
    }
    for value in [
        params.scope,
        params.target,
        params.id,
        params.name,
        params.field,
        params.value,
        params.path,
        params.provider,
        params.model,
        params.agent,
        params.command,
        params.query,
    ]
    .into_iter()
    .flatten()
    {
        if !value.trim().is_empty() {
            args.push(value);
        }
    }
    args.extend(params.args.into_iter().filter(|arg| !arg.trim().is_empty()));

    Ok(join_command_args(spec.command, &args))
}

fn join_command_args(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        format!("{command} {}", args.join(" "))
    }
}

fn emit_approval_mutation(
    stdout: &mut dyn Write,
    id: Option<BridgeRequestId>,
    ok: bool,
    app: &TuiApplication,
) -> anyhow::Result<()> {
    emit_legacy(
        stdout,
        BridgeEvent {
            kind: "approvals.updated",
            id,
            payload: json!({
                "ok": ok,
                "approvals": pending_approvals(app),
            }),
        },
    )
}

fn ensure_initialized(state: &BridgeState) -> anyhow::Result<()> {
    if state.initialized {
        Ok(())
    } else {
        anyhow::bail!("Not initialized")
    }
}

fn default_data_root_path() -> String {
    crate::memory::default_vegvisir_data_root()
        .display()
        .to_string()
}

fn default_openai_compat_host() -> String {
    "127.0.0.1".to_string()
}

fn default_openai_compat_port() -> u16 {
    11435
}

fn unix_now() -> i64 {
    Utc::now().timestamp()
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
}

fn codex_input_text(input: &[Value]) -> String {
    input
        .iter()
        .filter_map(|item| match item.get("type").and_then(Value::as_str) {
            Some("text") => item.get("text").and_then(Value::as_str).map(str::to_string),
            Some("mention") | Some("skill") => item
                .get("path")
                .and_then(Value::as_str)
                .map(|path| format!("@{path}")),
            Some("image") => item
                .get("url")
                .and_then(Value::as_str)
                .map(|url| format!("[image: {url}]")),
            Some("localImage") => item
                .get("path")
                .and_then(Value::as_str)
                .map(|path| format!("[image: {path}]")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn codex_thread(app: &TuiApplication, state: &BridgeState) -> Value {
    let thread_id = &app.session.session_id;
    let runtime = state.threads.get(thread_id);
    let now = unix_now();
    json!({
        "id": thread_id,
        "sessionId": thread_id,
        "forkedFromId": null,
        "preview": runtime.map(|thread| thread.preview.as_str()).unwrap_or(""),
        "ephemeral": runtime.map(|thread| thread.ephemeral).unwrap_or(true),
        "modelProvider": app.session.current_provider,
        "createdAt": runtime.map(|thread| thread.created_at).unwrap_or(now),
        "updatedAt": runtime.map(|thread| thread.updated_at).unwrap_or(now),
        "status": { "type": "idle" },
        "path": null,
        "cwd": app.cwd.display().to_string(),
        "cliVersion": env!("CARGO_PKG_VERSION"),
        "source": "appServer",
        "threadSource": null,
        "agentNickname": null,
        "agentRole": app.session.active_agent_name,
        "gitInfo": null,
        "name": app.session.title,
        "turns": [],
    })
}

fn codex_turn(
    id: &str,
    status: &'static str,
    started_at: i64,
    completed_at: Option<i64>,
    duration_ms: Option<i64>,
    items: Vec<Value>,
) -> Value {
    json!({
        "id": id,
        "items": items,
        "itemsView": "full",
        "status": status,
        "error": null,
        "startedAt": started_at,
        "completedAt": completed_at,
        "durationMs": duration_ms,
    })
}

fn codex_user_item(id: &str, text: &str) -> Value {
    json!({
        "type": "userMessage",
        "id": id,
        "content": [{
            "type": "text",
            "text": text,
            "text_elements": [],
        }],
    })
}

fn codex_agent_item(id: &str, text: &str) -> Value {
    json!({
        "type": "agentMessage",
        "id": id,
        "text": text,
        "phase": null,
        "memoryCitation": null,
    })
}

fn start_app(
    workspace: PathBuf,
    data_root: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    dangerously_bypass_approvals_and_sandbox: bool,
) -> anyhow::Result<TuiApplication> {
    let mut app = if let Some(data_root) = data_root {
        TuiApplication::with_data_root_and_dangerous_bypass(
            workspace,
            data_root,
            dangerously_bypass_approvals_and_sandbox,
        )?
    } else {
        TuiApplication::new_with_dangerous_bypass(
            workspace,
            dangerously_bypass_approvals_and_sandbox,
        )?
    };
    if let Some(provider) = provider {
        apply_command(&mut app, &format!("/provider {provider}"))?;
    }
    if let Some(model) = model {
        apply_requested_model_or_default(&mut app, &model)?;
    }
    if let Some(agent) = agent {
        apply_command(&mut app, &format!("/agent use {agent}"))?;
    }
    Ok(app)
}

fn apply_requested_model_or_default(app: &mut TuiApplication, model: &str) -> anyhow::Result<()> {
    let requested = model.trim();
    if requested.is_empty() {
        return Ok(());
    }
    let valid_for_provider = app.models.get(requested).is_some_and(|model_info| {
        app.models
            .is_model_allowed_for_provider(model_info, &app.session.current_provider)
    });
    if valid_for_provider {
        return apply_command(app, &format!("/model {requested}"));
    }
    if let Some(default) = app
        .models
        .default_for_provider(&app.session.current_provider)
    {
        let default_name = default.name.clone();
        return apply_command(app, &format!("/model {default_name}"));
    }
    app.session.current_model.clear();
    Ok(())
}

fn provider_requires_dynamic_model_discovery(provider: &str) -> bool {
    matches!(provider, "xai" | "xai-hbse")
}

fn apply_command(app: &mut TuiApplication, command: &str) -> anyhow::Result<()> {
    let output = app.execute_command(command)?.unwrap_or_default();
    if output.starts_with("Unknown ")
        || output.contains(" is not available")
        || output.contains("Unknown provider")
        || output.contains("Unknown model")
        || output.contains("Unknown agent")
    {
        anyhow::bail!("{output}");
    }
    Ok(())
}

fn snapshot(app: &TuiApplication) -> Value {
    json!({
        "workspace": app.cwd.display().to_string(),
        "session_id": app.session.session_id,
        "provider": app.session.current_provider,
        "model": app.session.current_model,
        "agent": app.session.active_agent_id,
        "status": app.session.status,
        "messages": app.session.messages.len(),
        "tokens_used": app.session.tokens_used,
        "last_latency_ms": app.session.last_latency_ms,
        "dangerously_bypass_approvals_and_sandbox": app.dangerously_bypass_approvals_and_sandbox,
        "tools_enabled": app.tool_registry.list().len(),
        "pending_approvals": app.tool_executor.guardrails.approvals.pending_len(),
        "parallelism": {
            "available_parallelism": app.parallelism.available_parallelism,
            "reserved_cores": app.parallelism.reserved_cores,
            "max_workers": app.parallelism.max_workers,
            "source": app.parallelism.source_label(),
        },
    })
}

fn pending_approvals(app: &TuiApplication) -> Vec<Value> {
    app.tool_executor
        .guardrails
        .approvals
        .pending()
        .into_values()
        .map(|request| json!(request))
        .collect()
}

fn models_for_provider(app: &TuiApplication, provider: &str) -> Vec<Value> {
    app.models
        .by_provider(provider)
        .into_iter()
        .map(|model| {
            json!({
                "id": model.name,
                "name": model.name,
                "display_name": model.display_name.as_deref().unwrap_or(&model.name),
                "displayName": model.display_name.as_deref().unwrap_or(&model.name),
                "provider": provider,
                "modelProvider": model.provider,
                "context_window": model.context_window,
                "contextWindow": model.context_window,
                "supports_streaming": model.supports_streaming,
                "supportsStreaming": model.supports_streaming,
                "supported": model.enabled,
                "source": model.metadata.get("source").and_then(Value::as_str).unwrap_or("catalog"),
            })
        })
        .collect()
}

fn provider_models(app: &TuiApplication) -> Vec<Value> {
    let mut out = Vec::new();
    for provider in app.provider_registry.list() {
        if !provider.enabled {
            continue;
        }
        for model in app.models.by_provider(&provider.name) {
            out.push(json!({
                "id": format!("{}/{}", provider.name, model.name),
                "provider": provider.name,
                "model": model.name,
                "display_name": model.display_name.as_deref().unwrap_or(&model.name),
                "context_window": model.context_window,
                "supports_streaming": model.supports_streaming,
                "source": model.metadata.get("source").and_then(Value::as_str).unwrap_or("catalog"),
            }));
        }
    }
    out
}

fn hbse_onboarding_providers(app: &TuiApplication) -> Vec<Value> {
    let mut providers: Vec<Value> = app
        .provider_registry
        .list()
        .into_iter()
        .filter(|provider| provider.enabled && provider.auth_type == "hbse")
        .map(|provider| {
            let setup_provider = provider
                .name
                .strip_suffix("-hbse")
                .unwrap_or(&provider.name)
                .to_string();
            json!({
                "provider": setup_provider,
                "hbse_provider": provider.name,
                "display_name": provider.display_name.as_deref().unwrap_or(&provider.name),
                "kind": provider.kind,
                "base_url": provider.base_url,
                "secret_ref": provider.metadata.get("hbse_secret_ref").and_then(Value::as_str).unwrap_or(""),
                "consumer": provider.metadata.get("hbse_consumer").and_then(Value::as_str).unwrap_or(""),
                "chat_purpose": provider.metadata.get("hbse_purpose").and_then(Value::as_str).unwrap_or("model.chat"),
                "discovery_purpose": provider.metadata.get("hbse_model_discovery_purpose").and_then(Value::as_str).unwrap_or("model.discovery"),
                "credential_header": provider.metadata.get("credential_header").and_then(Value::as_str).unwrap_or("Authorization"),
                "credential_prefix": provider.metadata.get("credential_prefix").and_then(Value::as_str).unwrap_or("Bearer "),
            })
        })
        .collect();
    providers.push(json!({
        "provider": "openai-sso",
        "hbse_provider": "openai-sso-hbse",
        "display_name": "OpenAI SSO token bundle via HBSE",
        "kind": "openai_sso_hbse",
        "base_url": "https://chatgpt.com/backend-api/codex",
        "secret_ref": "secret://vegvisir/providers/openai-sso/tokens",
        "consumer": "vegvisir.provider.openai-sso-hbse",
        "chat_purpose": "model.chat",
        "discovery_purpose": "model.discovery",
        "credential_json_field": "tokens.access_token",
        "credential_json_headers": { "ChatGPT-Account-ID": "tokens.account_id" },
    }));
    providers
}

fn transcript_markdown(app: &TuiApplication) -> String {
    let mut out = String::new();
    out.push_str("# Vegvisir Session Transcript\n\n");
    out.push_str(&format!("- Session: `{}`\n", app.session.session_id));
    out.push_str(&format!("- Workspace: `{}`\n", app.cwd.display()));
    out.push_str(&format!("- Provider: `{}`\n", app.session.current_provider));
    out.push_str(&format!("- Model: `{}`\n\n", app.session.current_model));
    for message in &app.session.messages {
        out.push_str(&format!("## {}\n\n", message.role));
        out.push_str(message.content.trim());
        out.push_str("\n\n");
    }
    out
}

fn emit_legacy(stdout: &mut dyn Write, event: BridgeEvent) -> anyhow::Result<()> {
    writeln!(stdout, "{}", serde_json::to_string(&event)?)?;
    stdout.flush()?;
    Ok(())
}

fn emit_response(stdout: &mut dyn Write, id: BridgeRequestId, result: Value) -> anyhow::Result<()> {
    writeln!(
        stdout,
        "{}",
        serde_json::to_string(&json!({ "id": id, "result": result }))?
    )?;
    stdout.flush()?;
    Ok(())
}

fn emit_notification(
    stdout: &mut dyn Write,
    method: &'static str,
    params: Value,
) -> anyhow::Result<()> {
    writeln!(
        stdout,
        "{}",
        serde_json::to_string(&json!({ "method": method, "params": params }))?
    )?;
    stdout.flush()?;
    Ok(())
}

fn emit_error(
    stdout: &mut dyn Write,
    id: Option<BridgeRequestId>,
    code: &'static str,
    message: String,
) -> anyhow::Result<()> {
    if let Some(id) = id {
        writeln!(
            stdout,
            "{}",
            serde_json::to_string(&json!({
                "id": id,
                "error": {
                    "code": -32000,
                    "message": message,
                    "data": { "code": code },
                }
            }))?
        )?;
        stdout.flush()?;
        Ok(())
    } else {
        emit_legacy(
            stdout,
            BridgeEvent {
                kind: "error",
                id: None,
                payload: json!({
                    "code": code,
                    "message": message,
                }),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn test_app() -> anyhow::Result<(tempfile::TempDir, TuiApplication)> {
        let tmp = tempdir()?;
        let app = TuiApplication::with_data_root(tmp.path(), tmp.path().join("home"))?;
        Ok((tmp, app))
    }

    fn execute_sh(app: &mut TuiApplication, text: &str) -> crate::types::Observation {
        app.tool_executor.execute(crate::types::ToolCall {
            name: "run_command".to_string(),
            args: json!({"command": ["sh", "-c", format!("printf {text}")]})
                .as_object()
                .unwrap()
                .clone(),
        })
    }

    #[test]
    fn bridge_capabilities_report_stdio_local_security_posture() -> anyhow::Result<()> {
        let (_tmp, app) = test_app()?;
        let capabilities = bridge_capabilities(&app);

        assert_eq!(
            capabilities["security_posture"]["transport"]["mode"],
            "stdio"
        );
        assert_eq!(
            capabilities["security_posture"]["transport"]["network_listener"],
            false
        );
        assert_eq!(
            capabilities["security_posture"]["transport"]["local_only"],
            true
        );
        assert_eq!(
            capabilities["security_posture"]["approval_control"]["external_response_grants_permission_directly"],
            false
        );
        assert_eq!(
            capabilities["security_posture"]["registry_remote_safe_filtering"]["status"],
            "metadata_reported_not_enforced"
        );
        assert!(
            capabilities["native_methods"]
                .as_array()
                .unwrap()
                .iter()
                .any(|method| method == "control.respond")
        );
        assert!(
            capabilities["native_methods"]
                .as_array()
                .unwrap()
                .iter()
                .any(|method| method == "bridge.heartbeat")
        );
        assert_eq!(capabilities["lease"]["mode"], "process_scoped_stdio");
        assert_eq!(capabilities["lease"]["timeout_enforced"], false);
        Ok(())
    }

    #[test]
    fn bridge_ping_and_heartbeat_report_process_scoped_lease() -> anyhow::Result<()> {
        let (_tmp, mut app) = test_app()?;
        let mut state = BridgeState {
            initialized: true,
            ..BridgeState::new()
        };
        let mut output = Vec::new();

        handle_request(
            &mut app,
            &mut state,
            BridgeRequest {
                id: Some(BridgeRequestId::String("ping".to_string())),
                method: "bridge.ping".to_string(),
                params: json!({}),
            },
            None,
            false,
            &mut output,
        )?;
        handle_request(
            &mut app,
            &mut state,
            BridgeRequest {
                id: Some(BridgeRequestId::String("heartbeat".to_string())),
                method: "bridge.heartbeat".to_string(),
                params: json!({}),
            },
            None,
            false,
            &mut output,
        )?;
        handle_request(
            &mut app,
            &mut state,
            BridgeRequest {
                id: Some(BridgeRequestId::String("lease".to_string())),
                method: "bridge.lease".to_string(),
                params: json!({}),
            },
            None,
            false,
            &mut output,
        )?;

        let events = String::from_utf8(output)?
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>()?;
        let pong = events
            .iter()
            .find(|event| event["type"] == "bridge.pong")
            .expect("bridge.pong event");
        assert_eq!(pong["payload"]["lease"]["mode"], "process_scoped_stdio");
        assert_eq!(pong["payload"]["lease"]["timeout_enforced"], false);

        let heartbeat = events
            .iter()
            .find(|event| event["type"] == "bridge.heartbeat")
            .expect("bridge.heartbeat event");
        assert_eq!(heartbeat["payload"]["heartbeat_count"], 1);
        assert!(heartbeat["payload"]["last_heartbeat_at"].is_number());

        let lease = events
            .iter()
            .find(|event| event["type"] == "bridge.lease")
            .expect("bridge.lease event");
        assert_eq!(lease["payload"]["heartbeat_count"], 1);
        assert_eq!(
            lease["payload"]["lease_id"],
            format!("bridge:{}", app.session.session_id)
        );
        Ok(())
    }

    #[test]
    fn bridge_control_response_without_pending_approval_cannot_bypass_policy() -> anyhow::Result<()>
    {
        let (_tmp, mut app) = test_app()?;
        app.execute_command("/tools allow-risky")?;
        let mut state = BridgeState {
            initialized: true,
            ..BridgeState::new()
        };
        let mut output = Vec::new();

        let request = BridgeRequest {
            id: Some(BridgeRequestId::String("ctrl".to_string())),
            method: "control.respond".to_string(),
            params: json!({
                "response": {
                    "request_id": "ctrl_apr_missing",
                    "decision_source": "bridge-test",
                    "payload": { "decision": "allow_for_session" }
                }
            }),
        };
        handle_request(&mut app, &mut state, request, None, false, &mut output)?;

        let events = String::from_utf8(output)?
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>()?;
        let responded = events
            .iter()
            .find(|event| event["type"] == "control.responded")
            .expect("control.responded event");
        assert_eq!(responded["payload"]["ok"], false);
        assert_eq!(responded["payload"]["audit"]["applied"], false);
        assert!(
            events
                .iter()
                .any(|event| event["type"] == "control.respond.audit")
        );

        let blocked = execute_sh(&mut app, "blocked");
        assert!(!blocked.ok, "{blocked:?}");
        assert!(blocked.content.contains("approval_id="));
        assert!(
            !app.tool_executor
                .guardrails
                .policy
                .allowed_commands
                .contains("sh")
        );
        Ok(())
    }

    #[test]
    fn bridge_control_response_applies_only_existing_ledger_approval() -> anyhow::Result<()> {
        let (_tmp, mut app) = test_app()?;
        app.execute_command("/tools allow-risky")?;

        let blocked = execute_sh(&mut app, "approved");
        assert!(!blocked.ok, "{blocked:?}");
        let approval_id = app
            .tool_executor
            .guardrails
            .approvals
            .pending_ids()
            .first()
            .cloned()
            .expect("pending command approval");

        let mut state = BridgeState {
            initialized: true,
            ..BridgeState::new()
        };
        let mut output = Vec::new();
        let request = BridgeRequest {
            id: Some(BridgeRequestId::String("ctrl".to_string())),
            method: "control.respond".to_string(),
            params: json!({
                "response": {
                    "request_id": format!("ctrl_{approval_id}"),
                    "decision_source": "bridge-test",
                    "payload": { "decision": "allow_once" }
                }
            }),
        };
        handle_request(&mut app, &mut state, request, None, false, &mut output)?;

        let events = String::from_utf8(output)?
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>()?;
        let responded = events
            .iter()
            .find(|event| event["type"] == "control.responded")
            .expect("control.responded event");
        assert_eq!(responded["payload"]["ok"], true);
        assert_eq!(
            responded["payload"]["audit"]["policy_authority"],
            "ApprovalLedger + GuardrailEngine"
        );

        let approved = execute_sh(&mut app, "approved");
        assert!(approved.ok, "{}", approved.content);
        assert_eq!(approved.content, "approved");
        assert!(
            !app.tool_executor
                .guardrails
                .policy
                .allowed_commands
                .contains("sh")
        );
        Ok(())
    }
}
