use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::app::TuiApplication;

#[derive(Debug, Deserialize)]
struct BridgeRequest {
    id: Option<String>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct StartParams {
    workspace: Option<PathBuf>,
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
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

#[derive(Debug, Serialize)]
struct BridgeEvent {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    payload: Value,
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

    emit(
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
    request: BridgeRequest,
    data_root: Option<&std::path::Path>,
    dangerously_bypass_approvals_and_sandbox: bool,
    stdout: &mut dyn Write,
) -> anyhow::Result<BridgeControl> {
    match request.method.as_str() {
        "initialize" | "session.status" => {
            emit(
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
            emit(
                stdout,
                BridgeEvent {
                    kind: "session.started",
                    id: request.id,
                    payload: snapshot(app),
                },
            )?;
        }
        "session.messages" => {
            emit(
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
            emit(
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
            emit(
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
                    if let Err(error) = emit(
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
                Ok(response) => emit(
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
                        emit(
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
                    emit(
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
            emit(
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
            emit(
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
            emit(
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
            emit(
                stdout,
                BridgeEvent {
                    kind: "models.list",
                    id: request.id,
                    payload: json!({
                        "current_model": app.session.current_model,
                        "models": app.models.list(),
                    }),
                },
            )?;
        }
        "agents.list" => {
            emit(
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
            emit(
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
            emit(
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
            emit(
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
            emit(
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
            emit(
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
            emit(
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
            emit(
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
    id: Option<String>,
    ok: bool,
    app: &TuiApplication,
) -> anyhow::Result<()> {
    emit(
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

fn emit(stdout: &mut dyn Write, event: BridgeEvent) -> anyhow::Result<()> {
    writeln!(stdout, "{}", serde_json::to_string(&event)?)?;
    stdout.flush()?;
    Ok(())
}

fn emit_error(
    stdout: &mut dyn Write,
    id: Option<String>,
    code: &'static str,
    message: String,
) -> anyhow::Result<()> {
    emit(
        stdout,
        BridgeEvent {
            kind: "error",
            id,
            payload: json!({
                "code": code,
                "message": message,
            }),
        },
    )
}
