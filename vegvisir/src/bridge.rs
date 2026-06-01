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
    threads: HashMap<String, ThreadRuntime>,
}

#[derive(Clone, Debug)]
struct ThreadRuntime {
    created_at: i64,
    updated_at: i64,
    preview: String,
    ephemeral: bool,
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
    let mut state = BridgeState::default();

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
                apply_command(app, &format!("/model {model}"))?;
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
            if params.refresh.unwrap_or(false) {
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
            emit_legacy(
                stdout,
                BridgeEvent {
                    kind: "turn.started",
                    id: request.id.clone(),
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
                            id: request.id.clone(),
                            payload: json!({
                                "role": "assistant",
                                "text": delta,
                            }),
                        },
                    ) {
                        delta_emit_error = Some(error);
                    }
                };
                let response = app.send_headless_streaming(&params.content, &mut on_delta);
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
                        id: request.id,
                        payload: json!({
                            "answer": response,
                            "session": snapshot(app),
                        }),
                    },
                )?,
                Err(error) => {
                    let pending = pending_approvals(app);
                    if !pending.is_empty() {
                        if app
                            .session
                            .messages
                            .last()
                            .map(|message| {
                                message.role == "user" && message.content == params.content
                            })
                            .unwrap_or(false)
                        {
                            app.session.messages.pop();
                        }
                        emit_legacy(
                            stdout,
                            BridgeEvent {
                                kind: "approval.required",
                                id: request.id.clone(),
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
                            id: request.id,
                            payload: json!({
                                "error": error.to_string(),
                                "session": snapshot(app),
                            }),
                        },
                    )?;
                }
            }
        }
        "command.run" => {
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
            let refresh_notes = if params.refresh.unwrap_or(false) {
                if params.provider.is_some() {
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
        "approvals.approveOnce" => {
            let params: ApprovalIdParams = serde_json::from_value(request.params)?;
            let ok = app
                .tool_executor
                .guardrails
                .approvals
                .approve_once(&params.id);
            emit_approval_mutation(stdout, request.id, ok, app)?;
        }
        "approvals.approveOnceAndExecute" => {
            let params: ApprovalIdParams = serde_json::from_value(request.params)?;
            let approved = app
                .tool_executor
                .guardrails
                .approvals
                .approve_once_request(&params.id);
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
                    id: request.id,
                    payload: json!({
                        "ok": approved.is_some(),
                        "approval": approved,
                        "observation": observation,
                        "approvals": pending_approvals(app),
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "approvals.approveSession" => {
            let params: ApprovalIdParams = serde_json::from_value(request.params)?;
            let ok = app
                .tool_executor
                .guardrails
                .approvals
                .approve_for_session(&params.id)
                .is_some();
            emit_approval_mutation(stdout, request.id, ok, app)?;
        }
        "approvals.approveSessionAndExecute" => {
            let params: ApprovalIdParams = serde_json::from_value(request.params)?;
            let approved = app
                .tool_executor
                .guardrails
                .approvals
                .approve_for_session(&params.id);
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
                    id: request.id,
                    payload: json!({
                        "ok": approved.is_some(),
                        "approval": approved,
                        "observation": observation,
                        "approvals": pending_approvals(app),
                        "session": snapshot(app),
                    }),
                },
            )?;
        }
        "approvals.deny" => {
            let params: ApprovalIdParams = serde_json::from_value(request.params)?;
            let ok = app.tool_executor.guardrails.approvals.deny(&params.id);
            emit_approval_mutation(stdout, request.id, ok, app)?;
        }
        "approvals.edit" => {
            let params: ApprovalEditParams = serde_json::from_value(request.params)?;
            let edited = app
                .tool_executor
                .guardrails
                .approvals
                .edit(&params.id, params.args);
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
        apply_command(&mut app, &format!("/model {model}"))?;
    }
    if let Some(agent) = agent {
        apply_command(&mut app, &format!("/agent use {agent}"))?;
    }
    Ok(app)
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
