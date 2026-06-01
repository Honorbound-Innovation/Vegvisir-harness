use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use crate::{
    model::Model,
    observability::EventLogger,
    orchestrator::{AgentHarness, AgentResult, AgentTask},
    parallelism::ParallelismConfig,
    tools::ToolRegistry,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubAgentStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubAgentWorkBudget {
    #[serde(default)]
    pub max_steps: Option<u64>,
    #[serde(default)]
    pub max_tool_calls: Option<u64>,
    #[serde(default)]
    pub max_read_bytes: Option<u64>,
    #[serde(default)]
    pub max_output_bytes: Option<u64>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

impl SubAgentWorkBudget {
    pub fn is_empty(&self) -> bool {
        self.max_steps.is_none()
            && self.max_tool_calls.is_none()
            && self.max_read_bytes.is_none()
            && self.max_output_bytes.is_none()
            && self.allowed_tools.is_empty()
            && self.notes.trim().is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubAgentTaskRecord {
    pub id: String,
    pub name: String,
    pub workspace: PathBuf,
    pub goal: String,
    #[serde(default)]
    pub file_scope: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "SubAgentWorkBudget::is_empty")]
    pub work_budget: SubAgentWorkBudget,
    pub status: SubAgentStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub checkpoint: Option<PathBuf>,
    pub final_answer: Option<String>,
    pub error: Option<String>,
}

impl SubAgentTaskRecord {
    fn new(name: impl Into<String>, task: &AgentTask) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            workspace: task.workspace.clone(),
            goal: task.goal.clone(),
            file_scope: Vec::new(),
            work_budget: SubAgentWorkBudget::default(),
            status: SubAgentStatus::Queued,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            checkpoint: None,
            final_answer: None,
            error: None,
        }
    }
}

pub struct SubAgentSupervisor<M: Model> {
    pub model: M,
    pub tools: ToolRegistry,
    pub max_children: usize,
    pub children: BTreeMap<String, AgentResult>,
    pub board: BTreeMap<String, SubAgentTaskRecord>,
    pub board_path: Option<PathBuf>,
    pub logger: EventLogger,
}

impl<M: Model> SubAgentSupervisor<M> {
    pub fn new(model: M, tools: ToolRegistry) -> Self {
        Self {
            model,
            tools,
            max_children: ParallelismConfig::detect().constrained_workers(4),
            children: BTreeMap::new(),
            board: BTreeMap::new(),
            board_path: None,
            logger: EventLogger::default(),
        }
    }

    pub fn with_board_path(
        model: M,
        tools: ToolRegistry,
        board_path: impl Into<PathBuf>,
    ) -> anyhow::Result<Self> {
        let mut supervisor = Self::new(model, tools);
        supervisor.board_path = Some(board_path.into());
        supervisor.load_board()?;
        Ok(supervisor)
    }

    pub fn task_records(&self) -> Vec<&SubAgentTaskRecord> {
        self.board.values().collect()
    }

    pub fn task_record(&self, id_or_name: &str) -> Option<&SubAgentTaskRecord> {
        self.board.get(id_or_name).or_else(|| {
            self.board
                .values()
                .find(|record| record.name == id_or_name || record.id == id_or_name)
        })
    }

    pub fn cancel_child(&mut self, id_or_name: &str) -> anyhow::Result<bool> {
        let Some(key) = self.board_key(id_or_name) else {
            return Ok(false);
        };
        let Some(record) = self.board.get_mut(&key) else {
            return Ok(false);
        };
        if matches!(
            record.status,
            SubAgentStatus::Completed | SubAgentStatus::Failed | SubAgentStatus::Cancelled
        ) {
            return Ok(false);
        }
        record.status = SubAgentStatus::Cancelled;
        record.finished_at = Some(Utc::now());
        let event_record = record.clone();
        self.emit_task_event("subagent.cancelled", &event_record);
        self.save_board()?;
        Ok(true)
    }

    pub fn run_child(
        &mut self,
        name: impl Into<String>,
        task: AgentTask,
    ) -> anyhow::Result<&AgentResult> {
        if self.children.len() >= self.max_children {
            anyhow::bail!("Maximum child agents reached");
        }
        let name = name.into();
        let mut record = SubAgentTaskRecord::new(name.clone(), &task);
        let record_key = record.id.clone();
        self.emit_task_event("subagent.queued", &record);
        self.board.insert(record_key.clone(), record.clone());
        self.save_board()?;
        record.status = SubAgentStatus::Running;
        record.started_at = Some(Utc::now());
        self.emit_task_event("subagent.started", &record);
        self.board.insert(record_key.clone(), record.clone());
        self.save_board()?;

        let workspace: PathBuf = task.workspace.clone();
        let mut harness = AgentHarness::with_options(
            &mut self.model,
            workspace,
            Some(self.tools.clone()),
            false,
            false,
            None,
        )?;
        let result = match harness.run(task) {
            Ok(result) => result,
            Err(error) => {
                let mut event_record = None;
                if let Some(record) = self.board.get_mut(&record_key) {
                    record.status = SubAgentStatus::Failed;
                    record.finished_at = Some(Utc::now());
                    record.error = Some(error.to_string());
                    event_record = Some(record.clone());
                }
                if let Some(record) = event_record {
                    self.emit_task_event("subagent.failed", &record);
                }
                self.save_board()?;
                return Err(error);
            }
        };
        let mut event_record = None;
        if let Some(record) = self.board.get_mut(&record_key) {
            record.status = SubAgentStatus::Completed;
            record.finished_at = Some(Utc::now());
            record.checkpoint = result.checkpoint.clone();
            record.final_answer = result.final_answer.clone();
            event_record = Some(record.clone());
        }
        if let Some(record) = event_record {
            self.emit_task_event("subagent.completed", &record);
        }
        self.save_board()?;
        self.children.insert(name.clone(), result);
        Ok(self.children.get(&name).expect("inserted child result"))
    }

    pub fn run_parallel(
        &mut self,
        tasks: BTreeMap<String, AgentTask>,
    ) -> anyhow::Result<BTreeMap<String, AgentResult>> {
        if self.children.len() + tasks.len() > self.max_children {
            anyhow::bail!("Maximum child agents reached");
        }
        let mut results = BTreeMap::new();
        for (name, task) in tasks {
            let result = self.run_child(name.clone(), task)?.clone();
            results.insert(name, result);
        }
        Ok(results)
    }

    pub fn run_parallel_with_models<N: Model + 'static>(
        &mut self,
        jobs: BTreeMap<String, (N, AgentTask)>,
    ) -> anyhow::Result<BTreeMap<String, AgentResult>> {
        if self.children.len() + jobs.len() > self.max_children {
            anyhow::bail!("Maximum child agents reached");
        }
        let shared_board = Arc::new(Mutex::new(self.board.clone()));
        let board_path = self.board_path.clone();
        let logger = self.logger.clone();
        let mut handles = Vec::new();
        for (name, (mut model, task)) in jobs {
            let tools = self.tools.clone();
            let record = SubAgentTaskRecord::new(name.clone(), &task);
            upsert_record(
                &shared_board,
                &board_path,
                &logger,
                "subagent.queued",
                record.clone(),
            )?;
            let handle_board = Arc::clone(&shared_board);
            let handle_board_path = board_path.clone();
            let handle_logger = logger.clone();
            handles.push(thread::spawn(move || {
                run_child_worker(
                    name,
                    &mut model,
                    task,
                    tools,
                    record,
                    handle_board,
                    handle_board_path,
                    handle_logger,
                )
            }));
        }

        let mut results = BTreeMap::new();
        for handle in handles {
            let (name, result) = handle
                .join()
                .map_err(|_| anyhow::anyhow!("subagent worker panicked"))??;
            self.children.insert(name.clone(), result.clone());
            results.insert(name, result);
        }
        self.board = shared_board
            .lock()
            .map(|board| board.clone())
            .unwrap_or_default();
        self.save_board()?;
        Ok(results)
    }

    fn load_board(&mut self) -> anyhow::Result<()> {
        let Some(path) = &self.board_path else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }
        let records: Vec<SubAgentTaskRecord> = serde_json::from_str(&fs::read_to_string(path)?)?;
        self.board = records
            .into_iter()
            .map(|record| (record.id.clone(), record))
            .collect();
        Ok(())
    }

    fn save_board(&self) -> anyhow::Result<()> {
        let Some(path) = &self.board_path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let records: Vec<_> = self.board.values().cloned().collect();
        fs::write(path, serde_json::to_string_pretty(&records)?)?;
        Ok(())
    }

    fn board_key(&self, id_or_name: &str) -> Option<String> {
        self.board
            .get(id_or_name)
            .map(|record| record.id.clone())
            .or_else(|| {
                self.board
                    .values()
                    .find(|record| record.name == id_or_name || record.id == id_or_name)
                    .map(|record| record.id.clone())
            })
    }

    fn emit_task_event(&self, name: &str, record: &SubAgentTaskRecord) {
        self.logger.emit(
            name,
            json!({
                "id": record.id,
                "name": record.name,
                "status": record.status,
                "workspace": record.workspace,
                "goal": record.goal,
                "file_scope": record.file_scope,
            }),
        );
    }
}

fn run_child_worker<N: Model>(
    name: String,
    model: &mut N,
    task: AgentTask,
    tools: ToolRegistry,
    mut record: SubAgentTaskRecord,
    shared_board: Arc<Mutex<BTreeMap<String, SubAgentTaskRecord>>>,
    board_path: Option<PathBuf>,
    logger: EventLogger,
) -> anyhow::Result<(String, AgentResult)> {
    record.status = SubAgentStatus::Running;
    record.started_at = Some(Utc::now());
    upsert_record(
        &shared_board,
        &board_path,
        &logger,
        "subagent.started",
        record.clone(),
    )?;
    let workspace = task.workspace.clone();
    let mut harness =
        AgentHarness::with_options(model, workspace, Some(tools), false, false, None)?;
    match harness.run(task) {
        Ok(result) => {
            record.status = SubAgentStatus::Completed;
            record.finished_at = Some(Utc::now());
            record.checkpoint = result.checkpoint.clone();
            record.final_answer = result.final_answer.clone();
            upsert_record(
                &shared_board,
                &board_path,
                &logger,
                "subagent.completed",
                record,
            )?;
            Ok((name, result))
        }
        Err(error) => {
            record.status = SubAgentStatus::Failed;
            record.finished_at = Some(Utc::now());
            record.error = Some(error.to_string());
            let _ = upsert_record(
                &shared_board,
                &board_path,
                &logger,
                "subagent.failed",
                record,
            );
            Err(error)
        }
    }
}

fn upsert_record(
    board: &Arc<Mutex<BTreeMap<String, SubAgentTaskRecord>>>,
    board_path: &Option<PathBuf>,
    logger: &EventLogger,
    event: &str,
    record: SubAgentTaskRecord,
) -> anyhow::Result<()> {
    if let Ok(mut board) = board.lock() {
        board.insert(record.id.clone(), record.clone());
        save_board_records(board_path, board.values().cloned().collect())?;
    }
    logger.emit(
        event,
        json!({
            "id": record.id,
            "name": record.name,
            "status": record.status,
            "workspace": record.workspace,
            "goal": record.goal,
        }),
    );
    Ok(())
}

fn save_board_records(
    board_path: &Option<PathBuf>,
    records: Vec<SubAgentTaskRecord>,
) -> anyhow::Result<()> {
    let Some(path) = board_path else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(&records)?)?;
    Ok(())
}

impl<T: Model + ?Sized> Model for &mut T {
    fn decide(
        &mut self,
        messages: &[crate::types::Message],
        tools: &[serde_json::Value],
    ) -> crate::types::AgentDecision {
        (**self).decide(messages, tools)
    }
}
