#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    env,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
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

fn bridge_status_locked(process: &mut Option<BridgeProcess>) -> BridgeStatus {
    if let Some(active) = process.as_mut() {
        match active.child.try_wait() {
            Ok(Some(_status)) => {
                *process = None;
            }
            Ok(None) => {
                return BridgeStatus {
                    running: true,
                    pid: process.as_ref().map(|process| process.child.id()),
                };
            }
            Err(_error) => {
                *process = None;
            }
        }
    }

    BridgeStatus { running: false, pid: None }
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn path_candidates(binary: &str) -> Vec<PathBuf> {
    let requested = PathBuf::from(binary);
    if requested.components().count() > 1 || requested.is_absolute() {
        return vec![requested];
    }

    let mut candidates = Vec::new();

    if let Some(paths) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&paths).map(|path| path.join(binary)));
    }

    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(home.join(".local/bin").join(binary));
        candidates.push(home.join("bin").join(binary));
    }

    candidates.push(PathBuf::from("/usr/local/bin").join(binary));
    candidates.push(PathBuf::from("/usr/bin").join(binary));
    candidates.push(PathBuf::from("/bin").join(binary));

    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            candidates.push(parent.join(binary));
            candidates.push(parent.join("resources").join(binary));
            candidates.push(parent.join("bin").join(binary));
        }
    }

    candidates
}

fn resolve_vegvisir_binary(requested: Option<String>) -> Result<PathBuf, String> {
    let binary = requested
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vegvisir".to_string());

    let candidates = path_candidates(binary.trim());
    for candidate in &candidates {
        if is_executable_file(candidate) {
            return Ok(candidate.to_path_buf());
        }
    }

    let searched = candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n  - ");

    Err(format!(
        "could not find Vegvisir binary '{binary}'. Install Vegvisir or set Settings → Vegvisir binary to an absolute path. Searched:\n  - {searched}"
    ))
}

#[tauri::command]
fn bridge_status(state: State<'_, BridgeState>) -> Result<BridgeStatus, String> {
    let mut guard = state.process.lock().map_err(|error| error.to_string())?;
    Ok(bridge_status_locked(&mut guard))
}

#[tauri::command]
fn bridge_start(request: StartBridgeRequest, state: State<'_, BridgeState>) -> Result<BridgeStatus, String> {
    let mut guard = state.process.lock().map_err(|error| error.to_string())?;
    let status = bridge_status_locked(&mut guard);
    if status.running {
        return Ok(status);
    }

    let binary = resolve_vegvisir_binary(request.vegvisir_binary)?;

    let workspace = request
        .workspace
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .or_else(|| env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."))
        });

    let mut command = Command::new(&binary);
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

    let mut child = command.spawn().map_err(|error| {
        format!(
            "failed to spawn Vegvisir bridge using '{}': {error}",
            binary.display()
        )
    })?;
    let stdin = child.stdin.take().ok_or_else(|| "failed to open app-server stdin".to_string())?;
    let stdout = child.stdout.take().ok_or_else(|| "failed to open app-server stdout".to_string())?;
    let stderr = child.stderr.take().ok_or_else(|| "failed to open app-server stderr".to_string())?;
    let (sender, receiver) = mpsc::channel::<String>();

    let start_event = json!({
        "type": "desktop.bridge.spawned",
        "id": null,
        "payload": {
            "binary": binary.display().to_string(),
            "workspace": workspace.display().to_string()
        }
    });
    let _ = sender.send(start_event.to_string());

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
    let mut guard = state.process.lock().map_err(|error| error.to_string())?;
    let Some(process) = guard.as_mut() else {
        return Ok(Vec::new());
    };

    let mut events = Vec::new();
    while let Ok(event) = process.events.try_recv() {
        events.push(event);
        if events.len() >= 250 {
            break;
        }
    }

    match process.child.try_wait() {
        Ok(Some(status)) => {
            events.push(json!({
                "type": "desktop.bridge.exited",
                "id": null,
                "payload": { "status": status.to_string() }
            }).to_string());
            *guard = None;
        }
        Ok(None) => {}
        Err(error) => {
            events.push(json!({
                "type": "desktop.bridge.error",
                "id": null,
                "payload": { "message": error.to_string() }
            }).to_string());
            *guard = None;
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
