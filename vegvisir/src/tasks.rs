use std::{
    collections::{BTreeMap, HashMap},
    fs::OpenOptions,
    io::{Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    command_sandbox::{CommandSandboxConfig, build_sandboxed_command},
    events::{TaskCompleted, TaskOutput, TaskStarted, TaskStatus, VegvisirEvent},
};

const DEFAULT_OUTPUT_RETENTION_BYTES: usize = 64 * 1024;
const DEFAULT_BACKGROUND_TIMEOUT_SECONDS: u64 = 30 * 60;
const DEFAULT_BACKGROUND_STALL_SECONDS: u64 = 10 * 60;
const DEFAULT_TASK_RECORD_MAX_COUNT: usize = 256;
const DEFAULT_TASK_EVENT_MAX_COUNT: usize = 512;
const TASK_OUTPUT_CHANNEL_CAPACITY: usize = 128;
const TASK_OUTPUT_CHUNK_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Shell,
    Test,
    Build,
    Watch,
    Agent,
    Workflow,
}

impl TaskKind {
    pub fn id_prefix(&self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Test => "test",
            Self::Build => "build",
            Self::Watch => "watch",
            Self::Agent => "agent",
            Self::Workflow => "workflow",
        }
    }

    pub fn event_kind(&self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Test => "test",
            Self::Build => "build",
            Self::Watch => "watch",
            Self::Agent => "agent",
            Self::Workflow => "workflow",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Queued,
    RunningForeground,
    RunningBackground,
    WaitingForInput,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

impl TaskState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::TimedOut
        )
    }

    pub fn is_running(&self) -> bool {
        matches!(
            self,
            Self::RunningForeground | Self::RunningBackground | Self::WaitingForInput
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub kind: TaskKind,
    pub state: TaskState,
    pub description: String,
    pub command: Option<String>,
    pub workspace: PathBuf,
    pub output_file: PathBuf,
    pub output_offset: u64,
    pub exit_code: Option<i32>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub owner_run_id: String,
    pub owner_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub retained_output: String,
}

impl TaskRecord {
    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskSpawnRequest {
    pub kind: TaskKind,
    pub description: String,
    pub command: Option<String>,
    pub workspace: PathBuf,
    pub output_file: Option<PathBuf>,
    pub owner_run_id: String,
    pub owner_agent_id: Option<String>,
}

impl TaskSpawnRequest {
    pub fn new(
        kind: TaskKind,
        description: impl Into<String>,
        workspace: impl Into<PathBuf>,
        owner_run_id: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            description: description.into(),
            command: None,
            workspace: workspace.into(),
            output_file: None,
            owner_run_id: owner_run_id.into(),
            owner_agent_id: None,
        }
    }

    pub fn command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    pub fn output_file(mut self, output_file: impl Into<PathBuf>) -> Self {
        self.output_file = Some(output_file.into());
        self
    }

    pub fn owner_agent_id(mut self, owner_agent_id: impl Into<String>) -> Self {
        self.owner_agent_id = Some(owner_agent_id.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskLifecycleEvent {
    Registered {
        task_id: String,
        kind: TaskKind,
        description: String,
    },
    Started {
        task_id: String,
        foreground: bool,
    },
    Backgrounded {
        task_id: String,
    },
    WaitingForInput {
        task_id: String,
    },
    Output {
        task_id: String,
        chunk: String,
        truncated: bool,
    },
    Completed {
        task_id: String,
        state: TaskState,
        exit_code: Option<i32>,
    },
}

impl TaskLifecycleEvent {
    pub fn task_id(&self) -> &str {
        match self {
            Self::Registered { task_id, .. }
            | Self::Started { task_id, .. }
            | Self::Backgrounded { task_id }
            | Self::WaitingForInput { task_id }
            | Self::Output { task_id, .. }
            | Self::Completed { task_id, .. } => task_id,
        }
    }

    pub fn to_vegvisir_event(&self, record: &TaskRecord) -> Option<VegvisirEvent> {
        match self {
            Self::Started { .. } => Some(VegvisirEvent::TaskStarted(TaskStarted {
                task_id: record.id.clone(),
                name: record.description.clone(),
                kind: record.kind.event_kind().to_string(),
            })),
            Self::Output {
                task_id,
                chunk,
                truncated,
            } if task_id == &record.id => Some(VegvisirEvent::TaskOutput(TaskOutput {
                task_id: record.id.clone(),
                chunk: chunk.clone(),
                truncated: *truncated,
            })),
            Self::Completed { state, .. } => terminal_state_to_event_status(state).map(|status| {
                VegvisirEvent::TaskCompleted(TaskCompleted {
                    task_id: record.id.clone(),
                    status,
                    summary: task_summary(record),
                })
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskTransitionError {
    UnknownTask(String),
    TerminalTask {
        id: String,
        state: TaskState,
    },
    InvalidTransition {
        id: String,
        from: TaskState,
        to: TaskState,
    },
}

impl std::fmt::Display for TaskTransitionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTask(id) => write!(formatter, "unknown task: {id}"),
            Self::TerminalTask { id, state } => {
                write!(formatter, "task {id} is already terminal: {state:?}")
            }
            Self::InvalidTransition { id, from, to } => {
                write!(
                    formatter,
                    "invalid task transition for {id}: {from:?} -> {to:?}"
                )
            }
        }
    }
}

impl std::error::Error for TaskTransitionError {}

#[derive(Clone, Debug)]
pub struct TaskManager {
    records: BTreeMap<String, TaskRecord>,
    events: Vec<TaskLifecycleEvent>,
    next_id: u64,
    output_retention_bytes: usize,
    max_records: usize,
    max_events: usize,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            records: BTreeMap::new(),
            events: Vec::new(),
            next_id: 1,
            output_retention_bytes: DEFAULT_OUTPUT_RETENTION_BYTES,
            max_records: DEFAULT_TASK_RECORD_MAX_COUNT,
            max_events: DEFAULT_TASK_EVENT_MAX_COUNT,
        }
    }

    pub fn with_output_retention_bytes(mut self, bytes: usize) -> Self {
        self.output_retention_bytes = bytes;
        self
    }

    pub fn with_max_records(mut self, max_records: usize) -> Self {
        self.max_records = max_records.max(1);
        self.prune_records();
        self
    }

    pub fn with_max_events(mut self, max_events: usize) -> Self {
        self.max_events = max_events.max(1);
        self.prune_events();
        self
    }

    pub fn register(&mut self, request: TaskSpawnRequest) -> String {
        let id = self.next_task_id(&request.kind);
        let output_file = request.output_file.unwrap_or_else(|| {
            request
                .workspace
                .join(".vegvisir")
                .join("tasks")
                .join(format!("{id}.log"))
        });
        let record = TaskRecord {
            id: id.clone(),
            kind: request.kind,
            state: TaskState::Queued,
            description: request.description,
            command: request.command,
            workspace: request.workspace,
            output_file,
            output_offset: 0,
            exit_code: None,
            started_at: None,
            finished_at: None,
            owner_run_id: request.owner_run_id,
            owner_agent_id: request.owner_agent_id,
            retained_output: String::new(),
        };
        self.push_event(TaskLifecycleEvent::Registered {
            task_id: id.clone(),
            kind: record.kind.clone(),
            description: record.description.clone(),
        });
        self.records.insert(id.clone(), record);
        self.prune_records();
        id
    }

    pub fn records(&self) -> Vec<&TaskRecord> {
        self.records.values().collect()
    }

    pub fn active_records(&self) -> Vec<&TaskRecord> {
        self.records
            .values()
            .filter(|record| !record.state.is_terminal())
            .collect()
    }

    pub fn record(&self, id: &str) -> Option<&TaskRecord> {
        self.records.get(id)
    }

    pub fn events(&self) -> &[TaskLifecycleEvent] {
        &self.events
    }

    pub fn drain_events(&mut self) -> Vec<TaskLifecycleEvent> {
        self.events.drain(..).collect()
    }

    pub fn start_foreground(&mut self, id: &str) -> Result<(), TaskTransitionError> {
        let was_never_started = self
            .record(id)
            .and_then(|record| record.started_at)
            .is_none();
        self.transition(id, TaskState::RunningForeground)?;
        if was_never_started {
            self.push_event(TaskLifecycleEvent::Started {
                task_id: id.to_string(),
                foreground: true,
            });
        }
        Ok(())
    }

    pub fn background(&mut self, id: &str) -> Result<(), TaskTransitionError> {
        let was_queued = self
            .record(id)
            .map(|record| record.state == TaskState::Queued)
            .unwrap_or(false);
        self.transition(id, TaskState::RunningBackground)?;
        if was_queued {
            self.push_event(TaskLifecycleEvent::Started {
                task_id: id.to_string(),
                foreground: false,
            });
        } else {
            self.push_event(TaskLifecycleEvent::Backgrounded {
                task_id: id.to_string(),
            });
        }
        Ok(())
    }

    pub fn mark_waiting_for_input(&mut self, id: &str) -> Result<(), TaskTransitionError> {
        self.transition(id, TaskState::WaitingForInput)?;
        self.push_event(TaskLifecycleEvent::WaitingForInput {
            task_id: id.to_string(),
        });
        Ok(())
    }

    pub fn append_output(&mut self, id: &str, chunk: &str) -> Result<(), TaskTransitionError> {
        let record = self
            .records
            .get_mut(id)
            .ok_or_else(|| TaskTransitionError::UnknownTask(id.to_string()))?;
        record.output_offset = record.output_offset.saturating_add(chunk.len() as u64);
        record.retained_output.push_str(chunk);
        let truncated = truncate_to_tail(&mut record.retained_output, self.output_retention_bytes);
        let event_chunk = crate::core::truncate_utf8_middle(
            chunk,
            DEFAULT_OUTPUT_RETENTION_BYTES,
            "task event output",
        );
        let event_chunk_truncated = event_chunk.len() < chunk.len();
        self.push_event(TaskLifecycleEvent::Output {
            task_id: id.to_string(),
            chunk: event_chunk,
            truncated: truncated || event_chunk_truncated,
        });
        Ok(())
    }

    pub fn complete(&mut self, id: &str, exit_code: i32) -> Result<(), TaskTransitionError> {
        let target = if exit_code == 0 {
            TaskState::Completed
        } else {
            TaskState::Failed
        };
        self.finish(id, target, Some(exit_code))
    }

    pub fn cancel(&mut self, id: &str) -> Result<(), TaskTransitionError> {
        self.finish(id, TaskState::Cancelled, None)
    }

    pub fn timeout(&mut self, id: &str) -> Result<(), TaskTransitionError> {
        self.finish(id, TaskState::TimedOut, None)
    }

    fn push_event(&mut self, event: TaskLifecycleEvent) {
        if self.max_events == 0 {
            return;
        }
        if self.events.len() >= self.max_events {
            let remove = self.events.len() - self.max_events + 1;
            self.events.drain(..remove);
        }
        self.events.push(event);
    }

    fn prune_events(&mut self) {
        if self.events.len() > self.max_events {
            let remove = self.events.len() - self.max_events;
            self.events.drain(..remove);
        }
    }

    fn prune_records(&mut self) {
        while self.records.len() > self.max_records {
            let Some(id) = self
                .records
                .iter()
                .find(|(_, record)| record.is_terminal())
                .map(|(id, _)| id.clone())
            else {
                // Never evict a running task merely to satisfy the history
                // bound; active records are needed for cancellation/status.
                break;
            };
            self.records.remove(&id);
        }
    }

    fn next_task_id(&mut self, kind: &TaskKind) -> String {
        let id = format!("{}-{:06}", kind.id_prefix(), self.next_id);
        self.next_id += 1;
        id
    }

    fn transition(&mut self, id: &str, to: TaskState) -> Result<(), TaskTransitionError> {
        let record = self
            .records
            .get_mut(id)
            .ok_or_else(|| TaskTransitionError::UnknownTask(id.to_string()))?;
        if record.state.is_terminal() {
            return Err(TaskTransitionError::TerminalTask {
                id: id.to_string(),
                state: record.state.clone(),
            });
        }
        let valid = matches!(
            (&record.state, &to),
            (TaskState::Queued, TaskState::RunningForeground)
                | (TaskState::Queued, TaskState::RunningBackground)
                | (TaskState::RunningForeground, TaskState::RunningBackground)
                | (TaskState::RunningForeground, TaskState::WaitingForInput)
                | (TaskState::RunningBackground, TaskState::WaitingForInput)
                | (TaskState::WaitingForInput, TaskState::RunningForeground)
                | (TaskState::WaitingForInput, TaskState::RunningBackground)
        );
        if !valid {
            return Err(TaskTransitionError::InvalidTransition {
                id: id.to_string(),
                from: record.state.clone(),
                to,
            });
        }
        if record.started_at.is_none() && to.is_running() {
            record.started_at = Some(Utc::now());
        }
        record.state = to;
        Ok(())
    }

    fn finish(
        &mut self,
        id: &str,
        state: TaskState,
        exit_code: Option<i32>,
    ) -> Result<(), TaskTransitionError> {
        debug_assert!(state.is_terminal());
        let record = self
            .records
            .get_mut(id)
            .ok_or_else(|| TaskTransitionError::UnknownTask(id.to_string()))?;
        if record.state.is_terminal() {
            return Err(TaskTransitionError::TerminalTask {
                id: id.to_string(),
                state: record.state.clone(),
            });
        }
        record.state = state.clone();
        record.exit_code = exit_code;
        if record.started_at.is_none() {
            record.started_at = Some(Utc::now());
        }
        record.finished_at = Some(Utc::now());
        self.push_event(TaskLifecycleEvent::Completed {
            task_id: id.to_string(),
            state,
            exit_code,
        });
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRunRequest {
    pub kind: TaskKind,
    pub description: String,
    pub command: Vec<String>,
    pub workspace: PathBuf,
    pub owner_run_id: String,
    pub owner_agent_id: Option<String>,
    pub timeout: Duration,
    pub stall_timeout: Option<Duration>,
}

impl TaskRunRequest {
    pub fn shell(
        command: Vec<String>,
        workspace: impl Into<PathBuf>,
        owner_run_id: impl Into<String>,
    ) -> Self {
        let description = command.join(" ");
        Self {
            kind: TaskKind::Shell,
            description,
            command,
            workspace: workspace.into(),
            owner_run_id: owner_run_id.into(),
            owner_agent_id: None,
            timeout: Duration::from_secs(DEFAULT_BACKGROUND_TIMEOUT_SECONDS),
            stall_timeout: Some(Duration::from_secs(DEFAULT_BACKGROUND_STALL_SECONDS)),
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn owner_agent_id(mut self, owner_agent_id: impl Into<String>) -> Self {
        self.owner_agent_id = Some(owner_agent_id.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn stall_timeout(mut self, stall_timeout: Option<Duration>) -> Self {
        self.stall_timeout = stall_timeout;
        self
    }
}

#[derive(Debug)]
pub enum TaskRunnerEvent {
    Output { task_id: String, chunk: String },
    Completed { task_id: String, exit_code: i32 },
    Cancelled { task_id: String },
    TimedOut { task_id: String },
    Failed { task_id: String, error: String },
}

#[derive(Debug)]
struct RunningTask {
    child: Child,
    output_rx: Receiver<String>,
    output_threads: Vec<JoinHandle<()>>,
    started_at: Instant,
    last_output_at: Instant,
    timeout: Duration,
    stall_timeout: Option<Duration>,
    cancel_requested: bool,
    timeout_requested: bool,
}

#[derive(Debug, Default)]
pub struct TaskRunner {
    running: HashMap<String, RunningTask>,
}

impl TaskRunner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.contains_key(id)
    }

    pub fn running_count(&self) -> usize {
        self.running.len()
    }

    pub fn spawn_background(
        &mut self,
        manager: &mut TaskManager,
        request: TaskRunRequest,
        sandbox_config: &CommandSandboxConfig,
    ) -> anyhow::Result<String> {
        if request.command.is_empty() {
            anyhow::bail!("Empty task command");
        }
        let command_display = request.command.join(" ");
        let task_id = manager.register(
            TaskSpawnRequest::new(
                request.kind,
                request.description,
                request.workspace.clone(),
                request.owner_run_id,
            )
            .command(command_display)
            .owner_agent_id_optional(request.owner_agent_id),
        );
        manager.background(&task_id)?;
        let record = manager
            .record(&task_id)
            .ok_or_else(|| anyhow::anyhow!("task disappeared after registration: {task_id}"))?;
        if let Some(parent) = record.output_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&record.output_file)?;

        let parts = request
            .command
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let sandboxed_command = match build_sandboxed_command(&parts, sandbox_config) {
            Ok(command) => command,
            Err(error) => {
                let message = format!(
                    "Task spawn failed before process start: {error}
"
                );
                let _ = append_output_file(&record.output_file, &message);
                let _ = manager.append_output(&task_id, &message);
                let _ = manager.complete(&task_id, 1);
                return Err(error);
            }
        };
        let mut command = Command::new(&sandboxed_command.program);
        command
            .args(&sandboxed_command.args)
            .current_dir(&sandboxed_command.current_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match spawn_command_in_own_process_group(&mut command) {
            Ok(child) => child,
            Err(error) => {
                let message = format!(
                    "Task process spawn failed: {error}
"
                );
                let _ = append_output_file(&record.output_file, &message);
                let _ = manager.append_output(&task_id, &message);
                let _ = manager.complete(&task_id, 1);
                return Err(error.into());
            }
        };
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (output_tx, output_rx) = mpsc::sync_channel(TASK_OUTPUT_CHANNEL_CAPACITY);
        let output_file = record.output_file.clone();
        let mut output_threads = Vec::new();
        if let Some(stdout) = stdout {
            output_threads.push(spawn_output_reader(
                stdout,
                output_tx.clone(),
                output_file.clone(),
            ));
        }
        if let Some(stderr) = stderr {
            output_threads.push(spawn_output_reader(stderr, output_tx, output_file));
        }
        let now = Instant::now();
        self.running.insert(
            task_id.clone(),
            RunningTask {
                child,
                output_rx,
                output_threads,
                started_at: now,
                last_output_at: now,
                timeout: request.timeout,
                stall_timeout: request.stall_timeout,
                cancel_requested: false,
                timeout_requested: false,
            },
        );
        Ok(task_id)
    }

    pub fn cancel(&mut self, id: &str) -> anyhow::Result<()> {
        let Some(task) = self.running.get_mut(id) else {
            anyhow::bail!("task is not running: {id}");
        };
        task.cancel_requested = true;
        terminate_child_process_group(&mut task.child);
        Ok(())
    }

    pub fn poll(&mut self) -> Vec<TaskRunnerEvent> {
        let mut events = Vec::new();
        let ids = self.running.keys().cloned().collect::<Vec<_>>();
        for task_id in ids {
            let Some(task) = self.running.get_mut(&task_id) else {
                continue;
            };
            loop {
                match task.output_rx.try_recv() {
                    Ok(chunk) => {
                        task.last_output_at = Instant::now();
                        events.push(TaskRunnerEvent::Output {
                            task_id: task_id.clone(),
                            chunk,
                        });
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }

            let now = Instant::now();
            let timed_out = now.duration_since(task.started_at) >= task.timeout;
            let stalled = task
                .stall_timeout
                .is_some_and(|timeout| now.duration_since(task.last_output_at) >= timeout);
            if (timed_out || stalled) && !task.timeout_requested {
                task.timeout_requested = true;
                terminate_child_process_group(&mut task.child);
            }

            match task.child.try_wait() {
                Ok(Some(status)) => {
                    let mut task = self.running.remove(&task_id).expect("running task exists");
                    drain_remaining_output(&mut task, &task_id, &mut events);
                    join_output_threads(task.output_threads);
                    if task.cancel_requested {
                        events.push(TaskRunnerEvent::Cancelled { task_id });
                    } else if task.timeout_requested {
                        events.push(TaskRunnerEvent::TimedOut { task_id });
                    } else {
                        events.push(TaskRunnerEvent::Completed {
                            task_id,
                            exit_code: status.code().unwrap_or(-1),
                        });
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    let mut task = self.running.remove(&task_id).expect("running task exists");
                    terminate_child_process_group(&mut task.child);
                    join_output_threads(task.output_threads);
                    events.push(TaskRunnerEvent::Failed {
                        task_id,
                        error: error.to_string(),
                    });
                }
            }
        }
        events
    }
}

trait TaskSpawnRequestExt {
    fn owner_agent_id_optional(self, owner_agent_id: Option<String>) -> Self;
}

impl TaskSpawnRequestExt for TaskSpawnRequest {
    fn owner_agent_id_optional(mut self, owner_agent_id: Option<String>) -> Self {
        self.owner_agent_id = owner_agent_id;
        self
    }
}

fn spawn_output_reader<R>(
    reader: R,
    output_tx: mpsc::SyncSender<String>,
    output_file: PathBuf,
) -> JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = [0_u8; TASK_OUTPUT_CHUNK_BYTES];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(bytes_read) => {
                    let chunk = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
                    let _ = append_output_file(&output_file, &chunk);
                    if output_tx.send(chunk).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let message = format!("[task output read error: {error}]\n");
                    let _ = append_output_file(&output_file, &message);
                    let _ = output_tx.send(message);
                    break;
                }
            }
        }
    })
}

fn append_output_file(path: &std::path::Path, chunk: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(chunk.as_bytes())?;
    Ok(())
}

fn drain_remaining_output(
    task: &mut RunningTask,
    task_id: &str,
    events: &mut Vec<TaskRunnerEvent>,
) {
    while let Ok(chunk) = task.output_rx.try_recv() {
        events.push(TaskRunnerEvent::Output {
            task_id: task_id.to_string(),
            chunk,
        });
    }
}

fn join_output_threads(threads: Vec<JoinHandle<()>>) {
    for thread in threads {
        let _ = thread.join();
    }
}

fn spawn_command_in_own_process_group(command: &mut Command) -> std::io::Result<Child> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    command.spawn()
}

fn terminate_child_process_group(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

fn truncate_to_tail(text: &mut String, max_bytes: usize) -> bool {
    if max_bytes == 0 {
        let truncated = !text.is_empty();
        text.clear();
        return truncated;
    }
    if text.len() <= max_bytes {
        return false;
    }
    let mut start = text.len() - max_bytes;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text.replace_range(..start, "");
    true
}

fn terminal_state_to_event_status(state: &TaskState) -> Option<TaskStatus> {
    match state {
        TaskState::Completed => Some(TaskStatus::Completed),
        TaskState::Failed | TaskState::TimedOut => Some(TaskStatus::Failed),
        TaskState::Cancelled => Some(TaskStatus::Cancelled),
        _ => None,
    }
}

fn task_summary(record: &TaskRecord) -> Option<String> {
    let exit = record
        .exit_code
        .map(|code| format!(" exit_code={code}"))
        .unwrap_or_default();
    Some(format!("{}: {:?}{exit}", record.description, record.state))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(kind: TaskKind) -> TaskSpawnRequest {
        TaskSpawnRequest::new(kind, "cargo test", "/workspace", "run-1").command("cargo test")
    }

    #[test]
    fn registers_tasks_with_stable_kind_prefixed_ids_and_output_paths() {
        let mut manager = TaskManager::new();
        let first = manager.register(request(TaskKind::Shell));
        let second = manager.register(request(TaskKind::Build));

        assert_eq!(first, "shell-000001");
        assert_eq!(second, "build-000002");
        let record = manager.record(&first).expect("registered task");
        assert_eq!(record.state, TaskState::Queued);
        assert_eq!(record.command.as_deref(), Some("cargo test"));
        assert_eq!(
            record.output_file,
            PathBuf::from("/workspace/.vegvisir/tasks/shell-000001.log")
        );
        assert!(matches!(
            manager.events().first(),
            Some(TaskLifecycleEvent::Registered { task_id, kind, .. })
                if task_id == "shell-000001" && kind == &TaskKind::Shell
        ));
    }

    #[test]
    fn tracks_foreground_background_waiting_and_completion_transitions() {
        let mut manager = TaskManager::new();
        let id = manager.register(request(TaskKind::Test));

        manager.start_foreground(&id).unwrap();
        assert_eq!(
            manager.record(&id).unwrap().state,
            TaskState::RunningForeground
        );
        assert!(manager.record(&id).unwrap().started_at.is_some());

        manager.background(&id).unwrap();
        assert_eq!(
            manager.record(&id).unwrap().state,
            TaskState::RunningBackground
        );

        manager.mark_waiting_for_input(&id).unwrap();
        assert_eq!(
            manager.record(&id).unwrap().state,
            TaskState::WaitingForInput
        );

        manager.start_foreground(&id).unwrap();
        manager.complete(&id, 0).unwrap();
        let record = manager.record(&id).unwrap();
        assert_eq!(record.state, TaskState::Completed);
        assert_eq!(record.exit_code, Some(0));
        assert!(record.finished_at.is_some());
        assert!(record.is_terminal());
        assert!(matches!(
            manager.events().last(),
            Some(TaskLifecycleEvent::Completed { state, .. }) if state == &TaskState::Completed
        ));
    }

    #[test]
    fn non_zero_exit_code_marks_task_failed() {
        let mut manager = TaskManager::new();
        let id = manager.register(request(TaskKind::Test));
        manager.start_foreground(&id).unwrap();
        manager.complete(&id, 101).unwrap();

        let record = manager.record(&id).unwrap();
        assert_eq!(record.state, TaskState::Failed);
        assert_eq!(record.exit_code, Some(101));
    }

    #[test]
    fn cancel_and_timeout_are_terminal() {
        let mut manager = TaskManager::new();
        let cancel_id = manager.register(request(TaskKind::Watch));
        let timeout_id = manager.register(request(TaskKind::Shell));

        manager.cancel(&cancel_id).unwrap();
        manager.timeout(&timeout_id).unwrap();

        assert_eq!(
            manager.record(&cancel_id).unwrap().state,
            TaskState::Cancelled
        );
        assert_eq!(
            manager.record(&timeout_id).unwrap().state,
            TaskState::TimedOut
        );
        assert!(manager.active_records().is_empty());
    }

    #[test]
    fn rejects_unknown_invalid_and_terminal_transitions() {
        let mut manager = TaskManager::new();
        let id = manager.register(request(TaskKind::Shell));

        assert!(matches!(
            manager.background("missing"),
            Err(TaskTransitionError::UnknownTask(missing)) if missing == "missing"
        ));
        assert!(matches!(
            manager.mark_waiting_for_input(&id),
            Err(TaskTransitionError::InvalidTransition { .. })
        ));

        manager.start_foreground(&id).unwrap();
        manager.complete(&id, 0).unwrap();
        assert!(matches!(
            manager.background(&id),
            Err(TaskTransitionError::TerminalTask { .. })
        ));
    }

    #[test]
    fn appends_output_with_monotonic_offset_and_bounded_tail_retention() {
        let mut manager = TaskManager::new().with_output_retention_bytes(10);
        let id = manager.register(request(TaskKind::Shell));

        manager.append_output(&id, "hello").unwrap();
        manager.append_output(&id, " world").unwrap();
        manager.append_output(&id, " 🚀").unwrap();

        let record = manager.record(&id).unwrap();
        assert_eq!(record.output_offset, "hello world 🚀".len() as u64);
        assert!(record.retained_output.len() <= 10);
        assert!(record.retained_output.ends_with("🚀"));
        assert!(record.retained_output.is_char_boundary(0));
        assert!(matches!(
            manager.events().last(),
            Some(TaskLifecycleEvent::Output {
                truncated: true,
                ..
            })
        ));
    }

    #[test]
    fn lifecycle_output_events_do_not_retain_unbounded_chunks() {
        let mut manager = TaskManager::new();
        let id = manager.register(request(TaskKind::Shell));
        let chunk = "x".repeat(DEFAULT_OUTPUT_RETENTION_BYTES * 2);

        manager.append_output(&id, &chunk).unwrap();

        match manager.events().last() {
            Some(TaskLifecycleEvent::Output {
                chunk: retained,
                truncated,
                ..
            }) => {
                assert!(retained.len() <= DEFAULT_OUTPUT_RETENTION_BYTES);
                assert!(*truncated);
            }
            other => panic!("expected output lifecycle event, got {other:?}"),
        }
    }

    #[test]
    fn custom_output_file_and_owner_agent_are_preserved() {
        let mut manager = TaskManager::new();
        let id = manager.register(
            TaskSpawnRequest::new(TaskKind::Agent, "review", "/workspace", "run-1")
                .output_file("/tmp/task.log")
                .owner_agent_id("agent-1"),
        );

        let record = manager.record(&id).unwrap();
        assert_eq!(record.output_file, PathBuf::from("/tmp/task.log"));
        assert_eq!(record.owner_agent_id.as_deref(), Some("agent-1"));
    }

    #[test]
    fn lifecycle_events_convert_to_runtime_events() {
        let mut manager = TaskManager::new();
        let id = manager.register(request(TaskKind::Build));
        manager.start_foreground(&id).unwrap();
        manager.append_output(&id, "building").unwrap();
        manager.complete(&id, 0).unwrap();

        let record = manager.record(&id).unwrap();
        let converted = manager
            .events()
            .iter()
            .filter_map(|event| event.to_vegvisir_event(record))
            .collect::<Vec<_>>();

        assert!(matches!(
            converted.first(),
            Some(VegvisirEvent::TaskStarted(TaskStarted { task_id, kind, .. }))
                if task_id == &id && kind == "build"
        ));
        assert!(matches!(
            converted.get(1),
            Some(VegvisirEvent::TaskOutput(TaskOutput { task_id, chunk, .. }))
                if task_id == &id && chunk == "building"
        ));
        assert!(matches!(
            converted.last(),
            Some(VegvisirEvent::TaskCompleted(TaskCompleted { task_id, status, .. }))
                if task_id == &id && status == &TaskStatus::Completed
        ));
    }

    #[test]
    fn drains_lifecycle_events() {
        let mut manager = TaskManager::new();
        manager.register(request(TaskKind::Shell));

        let drained = manager.drain_events();
        assert_eq!(drained.len(), 1);
        assert!(manager.events().is_empty());
    }

    #[test]
    fn task_runner_spawns_streams_and_completes_background_command() -> anyhow::Result<()> {
        let workspace = tempfile::tempdir()?;
        let mut manager = TaskManager::new();
        let mut runner = TaskRunner::new();
        let config = CommandSandboxConfig::path_only(workspace.path());
        let id = runner.spawn_background(
            &mut manager,
            TaskRunRequest::shell(
                vec![
                    "python3".to_string(),
                    "-c".to_string(),
                    "print('hello task')".to_string(),
                ],
                workspace.path(),
                "run-1",
            )
            .timeout(Duration::from_secs(10)),
            &config,
        )?;

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            for event in runner.poll() {
                match event {
                    TaskRunnerEvent::Output { task_id, chunk } => {
                        manager.append_output(&task_id, &chunk)?;
                    }
                    TaskRunnerEvent::Completed { task_id, exit_code } => {
                        manager.complete(&task_id, exit_code)?;
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
            if !runner.is_running(&id) {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        let record = manager.record(&id).unwrap();
        assert_eq!(record.state, TaskState::Completed);
        assert_eq!(record.exit_code, Some(0));
        assert!(record.retained_output.contains("hello task"));
        let persisted = std::fs::read_to_string(&record.output_file)?;
        assert!(persisted.contains("hello task"));
        Ok(())
    }

    #[test]
    fn task_runner_cancels_background_command() -> anyhow::Result<()> {
        let workspace = tempfile::tempdir()?;
        let mut manager = TaskManager::new();
        let mut runner = TaskRunner::new();
        let config = CommandSandboxConfig::path_only(workspace.path());
        let id = runner.spawn_background(
            &mut manager,
            TaskRunRequest::shell(
                vec![
                    "python3".to_string(),
                    "-c".to_string(),
                    "import time; time.sleep(30)".to_string(),
                ],
                workspace.path(),
                "run-1",
            )
            .timeout(Duration::from_secs(60)),
            &config,
        )?;

        runner.cancel(&id)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            for event in runner.poll() {
                if let TaskRunnerEvent::Cancelled { task_id } = event {
                    manager.cancel(&task_id)?;
                }
            }
            if !runner.is_running(&id) {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        assert_eq!(manager.record(&id).unwrap().state, TaskState::Cancelled);
        Ok(())
    }
}
