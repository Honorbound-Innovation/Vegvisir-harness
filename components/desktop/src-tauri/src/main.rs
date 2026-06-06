#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    env, fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::{mpsc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BridgeStopResult {
    was_running: bool,
    graceful: bool,
    killed: bool,
    status: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileExplorerEntry {
    name: String,
    path: String,
    is_dir: bool,
    is_file: bool,
    is_symlink: bool,
    size: Option<u64>,
    modified_ms: Option<u128>,
    git_repo: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileExplorerListing {
    path: String,
    parent: Option<String>,
    home: Option<String>,
    entries: Vec<FileExplorerEntry>,
    truncated: bool,
    total_entries: usize,
    limit: usize,
}

fn default_browser_path() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

const FILE_EXPLORER_ENTRY_LIMIT: usize = 800;

fn modified_ms(modified: SystemTime) -> Option<u128> {
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

#[tauri::command]
fn fs_list_directory(path: Option<String>) -> Result<FileExplorerListing, String> {
    let requested = path
        .filter(|value| !value.trim().is_empty())
        .map(|value| PathBuf::from(value.trim()))
        .unwrap_or_else(default_browser_path);
    let directory = fs::canonicalize(&requested).map_err(|error| {
        format!(
            "failed to resolve directory '{}': {error}",
            requested.display()
        )
    })?;

    if !directory.is_dir() {
        return Err(format!("'{}' is not a directory", directory.display()));
    }

    let mut entries = Vec::new();
    let mut total_entries = 0usize;
    let mut truncated = false;
    for item in fs::read_dir(&directory).map_err(|error| {
        format!(
            "failed to read directory '{}': {error}",
            directory.display()
        )
    })? {
        let item = item.map_err(|error| error.to_string())?;
        total_entries += 1;
        if entries.len() >= FILE_EXPLORER_ENTRY_LIMIT {
            truncated = true;
            break;
        }
        let path = item.path();
        let file_type = item.file_type().ok();
        let is_dir = file_type.as_ref().is_some_and(|kind| kind.is_dir());
        let is_file = file_type.as_ref().is_some_and(|kind| kind.is_file());
        let is_symlink = file_type.as_ref().is_some_and(|kind| kind.is_symlink());
        let metadata = if is_file { item.metadata().ok() } else { None };
        let name = item.file_name().to_string_lossy().to_string();
        let git_repo = false;
        entries.push(FileExplorerEntry {
            name,
            path: path.display().to_string(),
            is_dir,
            is_file,
            is_symlink,
            size: metadata.as_ref().map(|metadata| metadata.len()),
            modified_ms: metadata
                .and_then(|metadata| metadata.modified().ok())
                .and_then(modified_ms),
            git_repo,
        });
    }

    entries.sort_by_cached_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));

    Ok(FileExplorerListing {
        parent: directory.parent().map(|path| path.display().to_string()),
        home: env::var_os("HOME").map(|path| PathBuf::from(path).display().to_string()),
        path: directory.display().to_string(),
        entries,
        truncated,
        total_entries,
        limit: FILE_EXPLORER_ENTRY_LIMIT,
    })
}

struct BridgeProcess {
    child: Child,
    stdin: ChildStdin,
    events: mpsc::Receiver<String>,
}

const DEFAULT_GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_millis(2_000);

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

    BridgeStatus {
        running: false,
        pid: None,
    }
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

fn bundled_binary_candidates(binary: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(resource_dir) = env::var("VEGVISIR_DESKTOP_RESOURCE_DIR") {
        let resource_dir = PathBuf::from(resource_dir);
        candidates.push(resource_dir.join(binary));
        candidates.push(resource_dir.join("bin").join(binary));
    }
    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            candidates.push(parent.join("resources").join(binary));
            candidates.push(parent.join("resources").join("bin").join(binary));
            candidates.push(parent.join("../Resources").join(binary));
            candidates.push(parent.join("../Resources").join("bin").join(binary));
        }
    }
    candidates
}

fn resolve_vegvisir_binary(requested: Option<String>) -> Result<PathBuf, String> {
    let binary = requested
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vegvisir".to_string());

    let mut candidates = bundled_binary_candidates(binary.trim());
    candidates.extend(path_candidates(binary.trim()));
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

fn workspace_from_request(request: &StartBridgeRequest) -> PathBuf {
    request
        .workspace
        .clone()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .or_else(|| env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

fn spawn_bridge_process(
    request: StartBridgeRequest,
) -> Result<(BridgeProcess, BridgeStatus), String> {
    let binary = resolve_vegvisir_binary(request.vegvisir_binary.clone())?;
    let workspace = workspace_from_request(&request);

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
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "failed to open app-server stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "failed to open app-server stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "failed to open app-server stderr".to_string())?;
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
    Ok((
        BridgeProcess {
            child,
            stdin,
            events: receiver,
        },
        BridgeStatus {
            running: true,
            pid: Some(pid),
        },
    ))
}

fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<Option<ExitStatus>, String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) if started.elapsed() >= timeout => return Ok(None),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn stop_process(mut process: BridgeProcess, timeout: Duration) -> BridgeStopResult {
    let _ = process
        .stdin
        .write_all(b"{\"id\":\"desktop-shutdown\",\"method\":\"shutdown\",\"params\":{}}\n");
    let _ = process.stdin.flush();

    match wait_for_child_exit(&mut process.child, timeout) {
        Ok(Some(status)) => BridgeStopResult {
            was_running: true,
            graceful: true,
            killed: false,
            status: Some(status.to_string()),
        },
        Ok(None) => {
            let _ = process.child.kill();
            let status = process.child.wait().ok().map(|status| status.to_string());
            BridgeStopResult {
                was_running: true,
                graceful: false,
                killed: true,
                status,
            }
        }
        Err(error) => {
            let _ = process.child.kill();
            let status = process.child.wait().ok().map(|status| status.to_string());
            BridgeStopResult {
                was_running: true,
                graceful: false,
                killed: true,
                status: status.or(Some(error)),
            }
        }
    }
}

#[tauri::command]
fn bridge_start(
    request: StartBridgeRequest,
    state: State<'_, BridgeState>,
) -> Result<BridgeStatus, String> {
    let mut guard = state.process.lock().map_err(|error| error.to_string())?;
    let status = bridge_status_locked(&mut guard);
    if status.running {
        return Ok(status);
    }

    let (process, status) = spawn_bridge_process(request)?;
    *guard = Some(process);
    Ok(status)
}

#[tauri::command]
fn bridge_send(request: Value, state: State<'_, BridgeState>) -> Result<(), String> {
    let mut guard = state.process.lock().map_err(|error| error.to_string())?;
    let process = guard
        .as_mut()
        .ok_or_else(|| "bridge is not running".to_string())?;
    let mut line = serde_json::to_string(&request).map_err(|error| error.to_string())?;
    line.push('\n');
    process
        .stdin
        .write_all(line.as_bytes())
        .map_err(|error| error.to_string())?;
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
            events.push(
                json!({
                    "type": "desktop.bridge.exited",
                    "id": null,
                    "payload": { "status": status.to_string() }
                })
                .to_string(),
            );
            *guard = None;
        }
        Ok(None) => {}
        Err(error) => {
            events.push(
                json!({
                    "type": "desktop.bridge.error",
                    "id": null,
                    "payload": { "message": error.to_string() }
                })
                .to_string(),
            );
            *guard = None;
        }
    }

    Ok(events)
}

#[tauri::command]
fn bridge_stop(state: State<'_, BridgeState>) -> Result<BridgeStopResult, String> {
    let mut guard = state.process.lock().map_err(|error| error.to_string())?;
    let Some(process) = guard.take() else {
        return Ok(BridgeStopResult {
            was_running: false,
            graceful: true,
            killed: false,
            status: None,
        });
    };
    Ok(stop_process(process, DEFAULT_GRACEFUL_STOP_TIMEOUT))
}

#[tauri::command]
fn bridge_restart(
    request: StartBridgeRequest,
    state: State<'_, BridgeState>,
) -> Result<BridgeStatus, String> {
    let mut guard = state.process.lock().map_err(|error| error.to_string())?;
    if let Some(process) = guard.take() {
        let _ = stop_process(process, DEFAULT_GRACEFUL_STOP_TIMEOUT);
    }
    let (process, status) = spawn_bridge_process(request)?;
    *guard = Some(process);
    Ok(status)
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
            bridge_restart,
            fs_list_directory,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Vegvisir Desktop");
}
