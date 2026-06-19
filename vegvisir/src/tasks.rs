use std::{collections::BTreeMap, path::PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::events::{TaskCompleted, TaskOutput, TaskStarted, TaskStatus, VegvisirEvent};

const DEFAULT_OUTPUT_RETENTION_BYTES: usize = 64 * 1024;

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
        }
    }

    pub fn with_output_retention_bytes(mut self, bytes: usize) -> Self {
        self.output_retention_bytes = bytes;
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
        self.events.push(TaskLifecycleEvent::Registered {
            task_id: id.clone(),
            kind: record.kind.clone(),
            description: record.description.clone(),
        });
        self.records.insert(id.clone(), record);
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
            self.events.push(TaskLifecycleEvent::Started {
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
            self.events.push(TaskLifecycleEvent::Started {
                task_id: id.to_string(),
                foreground: false,
            });
        } else {
            self.events.push(TaskLifecycleEvent::Backgrounded {
                task_id: id.to_string(),
            });
        }
        Ok(())
    }

    pub fn mark_waiting_for_input(&mut self, id: &str) -> Result<(), TaskTransitionError> {
        self.transition(id, TaskState::WaitingForInput)?;
        self.events.push(TaskLifecycleEvent::WaitingForInput {
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
        self.events.push(TaskLifecycleEvent::Output {
            task_id: id.to_string(),
            chunk: chunk.to_string(),
            truncated,
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
        self.events.push(TaskLifecycleEvent::Completed {
            task_id: id.to_string(),
            state,
            exit_code,
        });
        Ok(())
    }
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
}
