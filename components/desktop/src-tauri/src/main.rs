#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{mpsc, Mutex},
    thread,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartBridgeRequest {
    workspace: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    vegvisir_binary: Option<String>,
    dangerous_bypass: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeStatus {
    running: bool,
    pid: Option<u32>,
}

struct BridgeProcess {
    child: Child,
    stdin: ChildStdin,
    events: mpsc::Receiver<String>,
}

#[derive(Default)]
struct BridgeState {
    process: Mutex<Option<BridgeProcess>>,
}

#[tauri::command]
fn bridge_status(state: State<'_, BridgeState>) -> Result<BridgeStatus, String> {
    let guard = state.process.lock().map_err(|error| error.to_string())?;
    Ok(BridgeStatus {
        running: guard.is_some(),
        pid: guard.as_ref().map(|process| process.child.id()),
    })
}

#[tauri::command]
fn bridge_start(request: StartBridgeRequest, state: State<'_, BridgeState>) -> Result<BridgeStatus, String> {
    let mut guard = state.process.lock().map_err(|error| error.to_string())?;
    if guard.is_some() {
        return Ok(BridgeStatus {
            running: true,
            pid: guard.as_ref().map(|process| process.child.id()),
        });
    }

    let binary = request
        .vegvisir_binary
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vegvisir".to_string());

    let workspace = request
        .workspace
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let mut command = Command::new(binary);
    if let Some(provider) = request.provider.filter(|value| !value.trim().is_empty()) {
        command.args(["--provider", provider.as_str()]);
    }
    if let Some(model) = request.model.filter(|value| !value.trim().is_empty()) {
        command.args(["--model", model.as_str()]);
    }
    if let Some(agent) = request.agent.filter(|value| !value.trim().is_empty()) {
        command.args(["--agent", agent.as_str()]);
    }
    if request.dangerous_bypass.unwrap_or(false) {
        command.arg("--dangerously-bypass-approvals-and-sandbox");
    }
    command
        .args(["app-server", "--workspace"])
        .arg(&workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|error| format!("failed to spawn vegvisir app-server: {error}"))?;
    let stdin = child.stdin.take().ok_or_else(|| "failed to open app-server stdin".to_string())?;
    let stdout = child.stdout.take().ok_or_else(|| "failed to open app-server stdout".to_string())?;
    let stderr = child.stderr.take().ok_or_else(|| "failed to open app-server stderr".to_string())?;
    let (sender, receiver) = mpsc::channel::<String>();

    let out_sender = sender.clone();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            let _ = out_sender.send(line);
        }
    });

    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            let event = json!({
                "type": "bridge.stderr",
                "id": null,
                "payload": { "line": line }
            });
            let _ = sender.send(event.to_string());
        }
    });

    let pid = child.id();
    *guard = Some(BridgeProcess { child, stdin, events: receiver });
    Ok(BridgeStatus { running: true, pid: Some(pid) })
}

#[tauri::command]
fn bridge_send(request: Value, state: State<'_, BridgeState>) -> Result<(), String> {
    let mut guard = state.process.lock().map_err(|error| error.to_string())?;
    let process = guard.as_mut().ok_or_else(|| "bridge is not running".to_string())?;
    let mut line = serde_json::to_string(&request).map_err(|error| error.to_string())?;
    line.push('\n');
    process.stdin.write_all(line.as_bytes()).map_err(|error| error.to_string())?;
    process.stdin.flush().map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn bridge_poll(state: State<'_, BridgeState>) -> Result<Vec<String>, String> {
    let guard = state.process.lock().map_err(|error| error.to_string())?;
    let Some(process) = guard.as_ref() else {
        return Ok(Vec::new());
    };

    let mut events = Vec::new();
    while let Ok(event) = process.events.try_recv() {
        events.push(event);
        if events.len() >= 250 {
            break;
        }
    }
    Ok(events)
}

#[tauri::command]
fn bridge_stop(state: State<'_, BridgeState>) -> Result<(), String> {
    let mut guard = state.process.lock().map_err(|error| error.to_string())?;
    if let Some(mut process) = guard.take() {
        let _ = process.stdin.write_all(b"{\"id\":\"desktop-shutdown\",\"method\":\"shutdown\",\"params\":{}}\n");
        let _ = process.stdin.flush();
        let _ = process.child.kill();
        let _ = process.child.wait();
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(BridgeState::default())
        .invoke_handler(tauri::generate_handler![
            bridge_status,
            bridge_start,
            bridge_send,
            bridge_poll,
            bridge_stop,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Vegvisir Desktop");
}
