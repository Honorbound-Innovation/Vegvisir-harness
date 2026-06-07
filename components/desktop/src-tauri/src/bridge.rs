use std::{
    env,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartBridgeRequest {
    workspace: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    vegvisir_binary: Option<String>,
    dangerous_bypass: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatus {
    running: bool,
    pid: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStopResult {
    was_running: bool,
    graceful: bool,
    killed: bool,
    status: Option<String>,
}

struct BridgeProcess {
    child: Child,
    stdin: ChildStdin,
    events: mpsc::Receiver<String>,
}

const DEFAULT_GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_millis(2_000);
const BRIDGE_LOG_FILE_NAME: &str = "bridge.log";
const BRIDGE_LOG_MAX_LINE_CHARS: usize = 32_000;

#[derive(Clone, Default)]
pub struct BridgeState {
    process: Arc<Mutex<Option<BridgeProcess>>>,
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

fn bridge_log_path() -> PathBuf {
    let base = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(|| env::temp_dir());
    base.join("vegvisir-desktop").join(BRIDGE_LOG_FILE_NAME)
}

fn append_bridge_log(line: impl AsRef<str>) {
    let path = bridge_log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut line = line.as_ref().replace('\n', "\\n");
    if line.chars().count() > BRIDGE_LOG_MAX_LINE_CHARS {
        let omitted = line
            .chars()
            .count()
            .saturating_sub(BRIDGE_LOG_MAX_LINE_CHARS);
        line = format!(
            "{} … truncated {omitted} chars",
            line.chars()
                .take(BRIDGE_LOG_MAX_LINE_CHARS)
                .collect::<String>()
        );
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let timestamp = chrono_like_timestamp();
        let _ = writeln!(file, "[{timestamp}] {line}");
    }
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("{}.{:03}", duration.as_secs(), duration.subsec_millis()),
        Err(_) => "unknown-time".to_string(),
    }
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn is_path_like_binary(binary: &str) -> bool {
    let requested = PathBuf::from(binary);
    requested.components().count() > 1 || requested.is_absolute()
}

fn binary_name_variants(binary: &str) -> Vec<String> {
    let trimmed = binary.trim();
    if trimmed.is_empty() || is_path_like_binary(trimmed) {
        return vec![trimmed.to_string()];
    }

    let mut names = Vec::from([trimmed.to_string()]);
    if trimmed == "vegvisir" {
        names.push("vegvisir-rust".to_string());
    } else if trimmed == "vegvisir-rust" {
        names.push("vegvisir".to_string());
    }

    let suffix = env::consts::EXE_SUFFIX;
    if !suffix.is_empty() {
        let suffixed = names
            .iter()
            .filter(|name| !name.ends_with(suffix))
            .map(|name| format!("{name}{suffix}"))
            .collect::<Vec<_>>();
        names.extend(suffixed);
    }

    names.dedup();
    names
}

fn push_named_candidates(candidates: &mut Vec<PathBuf>, base: impl AsRef<Path>, names: &[String]) {
    let base = base.as_ref();
    for name in names {
        candidates.push(base.join(name));
    }
}

fn push_workspace_target_candidates(
    candidates: &mut Vec<PathBuf>,
    start: impl AsRef<Path>,
    names: &[String],
) {
    for ancestor in start.as_ref().ancestors() {
        let has_workspace_manifest = ancestor.join("Cargo.toml").is_file();
        let has_vegvisir_crate = ancestor.join("vegvisir").join("Cargo.toml").is_file();
        if has_workspace_manifest || has_vegvisir_crate {
            push_named_candidates(candidates, ancestor.join("target").join("debug"), names);
            push_named_candidates(candidates, ancestor.join("target").join("release"), names);
        }
    }
}

fn development_binary_candidates(names: &[String]) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current_dir) = env::current_dir() {
        push_workspace_target_candidates(&mut candidates, current_dir, names);
    }
    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            push_workspace_target_candidates(&mut candidates, parent, names);
        }
    }
    if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
        push_workspace_target_candidates(&mut candidates, PathBuf::from(manifest_dir), names);
    }
    candidates
}

fn path_candidates(binary: &str) -> Vec<PathBuf> {
    let requested = PathBuf::from(binary);
    if is_path_like_binary(binary) {
        return vec![requested];
    }

    let names = binary_name_variants(binary);
    let mut candidates = Vec::new();

    if let Some(paths) = env::var_os("PATH") {
        for path in env::split_paths(&paths) {
            push_named_candidates(&mut candidates, path, &names);
        }
    }

    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        push_named_candidates(&mut candidates, home.join(".local/bin"), &names);
        push_named_candidates(&mut candidates, home.join("bin"), &names);
    }

    push_named_candidates(&mut candidates, "/usr/local/bin", &names);
    push_named_candidates(&mut candidates, "/usr/bin", &names);
    push_named_candidates(&mut candidates, "/bin", &names);
    candidates.extend(development_binary_candidates(&names));

    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            push_named_candidates(&mut candidates, parent, &names);
            push_named_candidates(&mut candidates, parent.join("resources"), &names);
            push_named_candidates(&mut candidates, parent.join("bin"), &names);
        }
    }

    candidates
}

fn bundled_binary_candidates(binary: &str) -> Vec<PathBuf> {
    let names = binary_name_variants(binary);
    let mut candidates = Vec::new();
    if let Ok(resource_dir) = env::var("VEGVISIR_DESKTOP_RESOURCE_DIR") {
        let resource_dir = PathBuf::from(resource_dir);
        push_named_candidates(&mut candidates, &resource_dir, &names);
        push_named_candidates(&mut candidates, resource_dir.join("bin"), &names);
    }
    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            push_named_candidates(&mut candidates, parent.join("resources"), &names);
            push_named_candidates(
                &mut candidates,
                parent.join("resources").join("bin"),
                &names,
            );
            push_named_candidates(&mut candidates, parent.join("../Resources"), &names);
            push_named_candidates(
                &mut candidates,
                parent.join("../Resources").join("bin"),
                &names,
            );
        }
    }
    candidates
}

fn resolve_vegvisir_binary(requested: Option<String>) -> Result<PathBuf, String> {
    let binary = requested
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("VEGVISIR_BINARY")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "vegvisir".to_string());

    let mut candidates = bundled_binary_candidates(binary.trim());
    candidates.extend(path_candidates(binary.trim()));
    candidates.dedup();
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
        "could not find Vegvisir binary '{binary}'. Install Vegvisir, set Settings → Vegvisir binary to an absolute path, or set VEGVISIR_BINARY. In source checkouts, desktop also searches target/debug/vegvisir-rust. Searched:\n  - {searched}"
    ))
}

#[tauri::command]
pub async fn bridge_status(state: State<'_, BridgeState>) -> Result<BridgeStatus, String> {
    let process = state.process.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = process.lock().map_err(|error| error.to_string())?;
        Ok(bridge_status_locked(&mut guard))
    })
    .await
    .map_err(|error| error.to_string())?
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
        let message = format!(
            "failed to spawn Vegvisir bridge using '{}': {error}",
            binary.display()
        );
        append_bridge_log(format!(
            "desktop.bridge.spawn_failed workspace={} {message}",
            workspace.display()
        ));
        message
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
    append_bridge_log(format!(
        "desktop.bridge.spawned pid={} binary={} workspace={} log={}",
        child.id(),
        binary.display(),
        workspace.display(),
        bridge_log_path().display()
    ));
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
            append_bridge_log(format!("bridge.stderr {line}"));
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
pub async fn bridge_start(
    request: StartBridgeRequest,
    state: State<'_, BridgeState>,
) -> Result<BridgeStatus, String> {
    let process = state.process.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = process.lock().map_err(|error| error.to_string())?;
        let status = bridge_status_locked(&mut guard);
        if status.running {
            return Ok(status);
        }

        let (process, status) = spawn_bridge_process(request)?;
        *guard = Some(process);
        Ok(status)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn bridge_send(request: Value, state: State<'_, BridgeState>) -> Result<(), String> {
    let process = state.process.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let request_id = request.get("id").cloned().unwrap_or(Value::Null);
        let request_method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let mut guard = process.lock().map_err(|error| error.to_string())?;
        let process = guard.as_mut().ok_or_else(|| {
            let message = "bridge is not running".to_string();
            append_bridge_log(format!(
                "desktop.bridge_send.failed method={request_method} id={request_id} error={message}"
            ));
            message
        })?;
        let mut line = serde_json::to_string(&request).map_err(|error| error.to_string())?;
        line.push('\n');
        if let Err(error) = process.stdin.write_all(line.as_bytes()) {
            let message = error.to_string();
            append_bridge_log(format!(
                "desktop.bridge_send.failed method={request_method} id={request_id} error={message}"
            ));
            return Err(message);
        }
        if let Err(error) = process.stdin.flush() {
            let message = error.to_string();
            append_bridge_log(format!(
                "desktop.bridge_send.failed method={request_method} id={request_id} error={message}"
            ));
            return Err(message);
        }
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn bridge_poll(state: State<'_, BridgeState>) -> Result<Vec<String>, String> {
    let process = state.process.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = process.lock().map_err(|error| error.to_string())?;
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
                append_bridge_log(format!("desktop.bridge.exited status={status}"));
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
                append_bridge_log(format!("desktop.bridge.error {error}"));
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
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn bridge_stop(state: State<'_, BridgeState>) -> Result<BridgeStopResult, String> {
    let process = state.process.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = process.lock().map_err(|error| error.to_string())?;
        let Some(process) = guard.take() else {
            return Ok(BridgeStopResult {
                was_running: false,
                graceful: true,
                killed: false,
                status: None,
            });
        };
        let result = stop_process(process, DEFAULT_GRACEFUL_STOP_TIMEOUT);
        append_bridge_log(format!(
            "desktop.bridge.stopped was_running={} graceful={} killed={} status={}",
            result.was_running,
            result.graceful,
            result.killed,
            result.status.as_deref().unwrap_or("none")
        ));
        Ok(result)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn bridge_restart(
    request: StartBridgeRequest,
    state: State<'_, BridgeState>,
) -> Result<BridgeStatus, String> {
    let process = state.process.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = process.lock().map_err(|error| error.to_string())?;
        if let Some(process) = guard.take() {
            let _ = stop_process(process, DEFAULT_GRACEFUL_STOP_TIMEOUT);
        }
        let (process, status) = spawn_bridge_process(request)?;
        *guard = Some(process);
        Ok(status)
    })
    .await
    .map_err(|error| error.to_string())?
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vegvisir_binary_variants_include_cargo_target_name() {
        let variants = binary_name_variants("vegvisir");
        assert!(variants.iter().any(|name| name == "vegvisir"));
        assert!(variants.iter().any(|name| name == "vegvisir-rust"));
    }

    #[test]
    fn path_like_binary_is_not_rewritten_to_aliases() {
        let variants = binary_name_variants("./target/debug/vegvisir-rust");
        assert_eq!(variants, vec!["./target/debug/vegvisir-rust".to_string()]);
    }

    #[test]
    fn development_candidates_include_workspace_target_binary() {
        let dev_binary = format!("vegvisir-rust{}", env::consts::EXE_SUFFIX);
        let expected_suffix = Path::new("target").join("debug").join(dev_binary);
        let candidates = development_binary_candidates(&binary_name_variants("vegvisir"));
        assert!(
            candidates.iter().any(|path| path.ends_with(&expected_suffix)),
            "expected a candidate ending with {}, got {candidates:?}",
            expected_suffix.display()
        );
    }
}
