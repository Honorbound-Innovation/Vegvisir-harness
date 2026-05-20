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
    pub provider: Option<String>,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub dangerously_bypass_approvals_and_sandbox: bool,
}

pub fn run_app_server(options: BridgeOptions) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut app = start_app(
        options.workspace,
        options.provider,
        options.model,
        options.agent,
        options.dangerously_bypass_approvals_and_sandbox,
    )?;

    emit(
        &mut stdout,
        BridgeEvent {
            kind: "server.ready",
            id: None,
            payload: snapshot(&app),
        },
    )?;

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: BridgeRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                emit_error(&mut stdout, None, "invalid_request", error.to_string())?;
                continue;
            }
        };
        let request_id = request.id.clone();
        match handle_request(
            &mut app,
            request,
            options.dangerously_bypass_approvals_and_sandbox,
            &mut stdout,
        ) {
            Ok(BridgeControl::Continue) => {}
            Ok(BridgeControl::Shutdown) => break,
            Err(error) => emit_error(&mut stdout, request_id, "request_failed", error.to_string())?,
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
        "session.start" => {
            let params: StartParams = serde_json::from_value(request.params)?;
            let workspace = params.workspace.unwrap_or_else(|| app.cwd.clone());
            *app = start_app(
                workspace,
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
            let response = app.send_headless(&params.content)?;
            emit(
                stdout,
                BridgeEvent {
                    kind: "content.delta",
                    id: request.id.clone(),
                    payload: json!({
                        "role": "assistant",
                        "text": response,
                    }),
                },
            )?;
            emit(
                stdout,
                BridgeEvent {
                    kind: "turn.completed",
                    id: request.id,
                    payload: snapshot(app),
                },
            )?;
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

fn start_app(
    workspace: PathBuf,
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    dangerously_bypass_approvals_and_sandbox: bool,
) -> anyhow::Result<TuiApplication> {
    let mut app = TuiApplication::new_with_dangerous_bypass(
        workspace,
        dangerously_bypass_approvals_and_sandbox,
    )?;
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
