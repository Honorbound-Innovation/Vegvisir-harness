use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const EVENT_SCHEMA_VERSION: u32 = 1;
const REDACTION: &str = "[REDACTED]";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub v: u32,
    pub run_id: String,
    pub seq: u64,
    pub ts: DateTime<Utc>,
    #[serde(flatten)]
    pub event: VegvisirEvent,
}

impl EventEnvelope {
    pub fn new(run_id: impl Into<String>, seq: u64, event: VegvisirEvent) -> Self {
        Self::at(run_id, seq, Utc::now(), event)
    }

    pub fn at(
        run_id: impl Into<String>,
        seq: u64,
        ts: DateTime<Utc>,
        event: VegvisirEvent,
    ) -> Self {
        Self {
            v: EVENT_SCHEMA_VERSION,
            run_id: run_id.into(),
            seq,
            ts,
            event,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VegvisirEvent {
    RunStarted(RunStarted),
    UserMessage(UserMessage),
    AssistantDelta(AssistantDelta),
    AssistantMessageCompleted(AssistantMessageCompleted),
    ToolRequested(ToolRequested),
    ToolStarted(ToolStarted),
    ToolOutput(ToolOutput),
    ToolCompleted(ToolCompleted),
    ToolFailed(ToolFailed),
    ApprovalRequested(ApprovalRequested),
    ApprovalResolved(ApprovalResolved),
    ControlRequestCreated(ControlRequestCreated),
    ControlRequestResolved(ControlRequestResolved),
    ControlRequestCancelled(ControlRequestCancelled),
    MemoryRead(MemoryRead),
    MemoryWritten(MemoryWritten),
    ContextPrepared(ContextPrepared),
    ContextCompacted(ContextCompacted),
    TaskStarted(TaskStarted),
    TaskOutput(TaskOutput),
    TaskCompleted(TaskCompleted),
    SubagentStarted(SubagentStarted),
    SubagentCompleted(SubagentCompleted),
    RunCompleted(RunCompleted),
    RunFailed(RunFailed),
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStarted {
    pub session_id: String,
    pub workspace: String,
    pub provider: String,
    pub model: String,
    pub agent: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMessage {
    pub message_id: String,
    pub content_preview: String,
    #[serde(default)]
    pub attachment_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantDelta {
    pub message_id: String,
    pub delta: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantMessageCompleted {
    pub message_id: String,
    pub output_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRequested {
    pub tool_call_id: String,
    pub tool_name: String,
    pub args_summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolStarted {
    pub tool_call_id: String,
    pub tool_name: String,
    pub approval_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolOutput {
    pub tool_call_id: String,
    pub chunk: String,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCompleted {
    pub tool_call_id: String,
    pub tool_name: String,
    pub ok: bool,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolFailed {
    pub tool_call_id: String,
    pub tool_name: String,
    pub error: String,
    #[serde(default)]
    pub recoverable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequested {
    pub approval_id: String,
    pub category: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalResolved {
    pub approval_id: String,
    pub decision: ApprovalDecision,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Allow,
    AllowForSession,
    Deny,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlRequestCreated {
    pub request_id: String,
    pub subtype: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlRequestResolved {
    pub request_id: String,
    pub subtype: String,
    pub decision_source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlRequestCancelled {
    pub request_id: String,
    pub subtype: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRead {
    pub system: String,
    pub query: Option<String>,
    pub result_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryWritten {
    pub system: String,
    pub memory_id: Option<String>,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPrepared {
    pub system: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub source_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextCompacted {
    pub strategy: String,
    pub before_tokens: Option<u64>,
    pub after_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskStarted {
    pub task_id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOutput {
    pub task_id: String,
    pub chunk: String,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCompleted {
    pub task_id: String,
    pub status: TaskStatus,
    pub summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentStarted {
    pub task_id: String,
    pub name: String,
    pub agent: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentCompleted {
    pub task_id: String,
    pub status: TaskStatus,
    pub summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCompleted {
    pub status: RunCompletionStatus,
    pub summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunCompletionStatus {
    Ok,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFailed {
    pub error: String,
    #[serde(default)]
    pub recoverable: bool,
}

pub trait EventSink: Send + Sync {
    fn emit(&self, envelope: &EventEnvelope) -> anyhow::Result<()>;
}

#[derive(Clone, Debug, Default)]
pub struct MemoryEventSink {
    events: Arc<Mutex<Vec<EventEnvelope>>>,
}

impl MemoryEventSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<EventEnvelope> {
        self.events
            .lock()
            .expect("event sink mutex poisoned")
            .clone()
    }
}

impl EventSink for MemoryEventSink {
    fn emit(&self, envelope: &EventEnvelope) -> anyhow::Result<()> {
        self.events
            .lock()
            .expect("event sink mutex poisoned")
            .push(envelope.clone());
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _envelope: &EventEnvelope) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
pub struct CompositeEventSink {
    sinks: Vec<Box<dyn EventSink>>,
}

impl CompositeEventSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push<S: EventSink + 'static>(&mut self, sink: S) {
        self.sinks.push(Box::new(sink));
    }

    pub fn len(&self) -> usize {
        self.sinks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sinks.is_empty()
    }
}

impl EventSink for CompositeEventSink {
    fn emit(&self, envelope: &EventEnvelope) -> anyhow::Result<()> {
        for sink in &self.sinks {
            sink.emit(envelope)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct JsonlFileEventSink {
    path: PathBuf,
}

impl JsonlFileEventSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl EventSink for JsonlFileEventSink {
    fn emit(&self, envelope: &EventEnvelope) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create event sink directory {}", parent.display())
            })?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("failed to open event sink {}", self.path.display()))?;
        writeln!(file, "{}", to_jsonl_record(envelope)?)?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct EventEmitter<S> {
    run_id: String,
    next_seq: u64,
    sink: S,
}

impl<S: EventSink> EventEmitter<S> {
    pub fn new(run_id: impl Into<String>, sink: S) -> Self {
        Self {
            run_id: run_id.into(),
            next_seq: 1,
            sink,
        }
    }

    pub fn emit(&mut self, event: VegvisirEvent) -> anyhow::Result<EventEnvelope> {
        let envelope = EventEnvelope::new(self.run_id.clone(), self.next_seq, event);
        self.next_seq += 1;
        self.sink.emit(&envelope)?;
        Ok(envelope)
    }

    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }
}

pub fn to_jsonl_record(envelope: &EventEnvelope) -> anyhow::Result<String> {
    let value = serde_json::to_value(envelope)?;
    let redacted = redact_event_value(value);
    Ok(serde_json::to_string(&redacted)?)
}

pub fn parse_jsonl_record(line: &str) -> anyhow::Result<EventEnvelope> {
    serde_json::from_str(line).map_err(Into::into)
}

fn redact_event_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let redacted = if secret_like_key(&key) {
                        Value::String(REDACTION.to_string())
                    } else {
                        redact_event_value(value)
                    };
                    (key, redacted)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(redact_event_value).collect()),
        Value::String(text) => Value::String(redact_secret_like_text(&text)),
        other => other,
    }
}

fn secret_like_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "authorization",
        "api_key",
        "apikey",
        "access_token",
        "refresh_token",
        "password",
        "secret",
        "private_key",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn redact_secret_like_text(text: &str) -> String {
    let mut redacted = String::with_capacity(text.len());
    let mut token = String::new();

    for ch in text.chars() {
        if ch.is_whitespace() {
            push_redacted_token(&mut redacted, &token);
            token.clear();
            redacted.push(ch);
        } else {
            token.push(ch);
        }
    }
    push_redacted_token(&mut redacted, &token);
    redacted
}

fn push_redacted_token(output: &mut String, token: &str) {
    if token.is_empty() {
        return;
    }
    if looks_like_secret(token) {
        output.push_str(REDACTION);
    } else {
        output.push_str(token);
    }
}

fn looks_like_secret(text: &str) -> bool {
    let trimmed = text.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | ';' | ':' | '"' | '\'' | ')' | '(' | '[' | ']' | '{' | '}'
        )
    });
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("sk-")
        || lower.starts_with("github_pat_")
        || lower.starts_with("ghp_")
        || lower.starts_with("gho_")
        || lower.starts_with("ghu_")
        || lower.starts_with("ghs_")
        || lower.starts_with("ghr_")
        || lower.starts_with("xoxb-")
        || lower.starts_with("xoxp-")
        || lower.starts_with("xoxa-")
        || lower.starts_with("bearer") && trimmed.len() > 24
        || looks_like_jwt(trimmed)
        || looks_like_high_entropy_token(trimmed)
}

fn looks_like_jwt(text: &str) -> bool {
    let parts = text.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts[0].starts_with("eyJ")
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '='))
        })
}

fn looks_like_high_entropy_token(text: &str) -> bool {
    if text.len() < 40 || text.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return false;
    }
    let allowed = text
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '+' | '/' | '='));
    let has_letter = text.chars().any(|ch| ch.is_ascii_alphabetic());
    let has_digit = text.chars().any(|ch| ch.is_ascii_digit());
    let has_token_marker = text
        .chars()
        .any(|ch| matches!(ch, '-' | '_' | '+' | '/' | '='))
        || (text.chars().any(|ch| ch.is_ascii_lowercase())
            && text.chars().any(|ch| ch.is_ascii_uppercase()));
    allowed && has_letter && has_digit && has_token_marker
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixed_ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()
    }

    #[test]
    fn event_envelope_jsonl_is_single_line_and_round_trips() -> anyhow::Result<()> {
        let envelope = EventEnvelope::at(
            "run-1",
            7,
            fixed_ts(),
            VegvisirEvent::ToolOutput(ToolOutput {
                tool_call_id: "tool-1".to_string(),
                chunk: "line one\nline two\u{2028}line three".to_string(),
                truncated: false,
            }),
        );

        let line = to_jsonl_record(&envelope)?;
        assert_eq!(line.lines().count(), 1);
        assert!(line.contains("\\n"));
        assert!(line.contains("tool_output"));

        let parsed = parse_jsonl_record(&line)?;
        assert_eq!(parsed, envelope);
        Ok(())
    }

    #[test]
    fn run_started_jsonl_matches_golden_shape() -> anyhow::Result<()> {
        let envelope = EventEnvelope::at(
            "run-abc",
            1,
            fixed_ts(),
            VegvisirEvent::RunStarted(RunStarted {
                session_id: "session-1".to_string(),
                workspace: "/workspace".to_string(),
                provider: "demo".to_string(),
                model: "demo-local".to_string(),
                agent: Some("default".to_string()),
            }),
        );

        assert_eq!(
            to_jsonl_record(&envelope)?,
            r#"{"agent":"default","model":"demo-local","provider":"demo","run_id":"run-abc","seq":1,"session_id":"session-1","ts":"2026-01-02T03:04:05Z","type":"run_started","v":1,"workspace":"/workspace"}"#
        );
        Ok(())
    }

    #[test]
    fn core_event_family_jsonl_golden_shapes_are_stable() -> anyhow::Result<()> {
        let cases = vec![
            (
                EventEnvelope::at(
                    "run-abc",
                    2,
                    fixed_ts(),
                    VegvisirEvent::ApprovalRequested(ApprovalRequested {
                        approval_id: "approval-1".to_string(),
                        category: "risky_tool".to_string(),
                        reason: "run_command requires approval".to_string(),
                    }),
                ),
                r#"{"approval_id":"approval-1","category":"risky_tool","reason":"run_command requires approval","run_id":"run-abc","seq":2,"ts":"2026-01-02T03:04:05Z","type":"approval_requested","v":1}"#,
            ),
            (
                EventEnvelope::at(
                    "run-abc",
                    3,
                    fixed_ts(),
                    VegvisirEvent::ApprovalResolved(ApprovalResolved {
                        approval_id: "approval-1".to_string(),
                        decision: ApprovalDecision::AllowForSession,
                    }),
                ),
                r#"{"approval_id":"approval-1","decision":"allow_for_session","run_id":"run-abc","seq":3,"ts":"2026-01-02T03:04:05Z","type":"approval_resolved","v":1}"#,
            ),
            (
                EventEnvelope::at(
                    "run-abc",
                    4,
                    fixed_ts(),
                    VegvisirEvent::ControlRequestCreated(ControlRequestCreated {
                        request_id: "ctrl-approval-1".to_string(),
                        subtype: "approval".to_string(),
                        expires_at: Some(fixed_ts()),
                    }),
                ),
                r#"{"expires_at":"2026-01-02T03:04:05Z","request_id":"ctrl-approval-1","run_id":"run-abc","seq":4,"subtype":"approval","ts":"2026-01-02T03:04:05Z","type":"control_request_created","v":1}"#,
            ),
            (
                EventEnvelope::at(
                    "run-abc",
                    5,
                    fixed_ts(),
                    VegvisirEvent::ControlRequestResolved(ControlRequestResolved {
                        request_id: "ctrl-approval-1".to_string(),
                        subtype: "approval".to_string(),
                        decision_source: "local_ui".to_string(),
                    }),
                ),
                r#"{"decision_source":"local_ui","request_id":"ctrl-approval-1","run_id":"run-abc","seq":5,"subtype":"approval","ts":"2026-01-02T03:04:05Z","type":"control_request_resolved","v":1}"#,
            ),
            (
                EventEnvelope::at(
                    "run-abc",
                    6,
                    fixed_ts(),
                    VegvisirEvent::ControlRequestCancelled(ControlRequestCancelled {
                        request_id: "ctrl-approval-2".to_string(),
                        subtype: "approval".to_string(),
                        reason: "run aborted".to_string(),
                    }),
                ),
                r#"{"reason":"run aborted","request_id":"ctrl-approval-2","run_id":"run-abc","seq":6,"subtype":"approval","ts":"2026-01-02T03:04:05Z","type":"control_request_cancelled","v":1}"#,
            ),
            (
                EventEnvelope::at(
                    "run-abc",
                    7,
                    fixed_ts(),
                    VegvisirEvent::MemoryRead(MemoryRead {
                        system: "cms-v2".to_string(),
                        query: Some("registry plan".to_string()),
                        result_count: 2,
                    }),
                ),
                r#"{"query":"registry plan","result_count":2,"run_id":"run-abc","seq":7,"system":"cms-v2","ts":"2026-01-02T03:04:05Z","type":"memory_read","v":1}"#,
            ),
            (
                EventEnvelope::at(
                    "run-abc",
                    8,
                    fixed_ts(),
                    VegvisirEvent::MemoryWritten(MemoryWritten {
                        system: "cms-v2".to_string(),
                        memory_id: Some("mem_123".to_string()),
                        title: "Decision".to_string(),
                    }),
                ),
                r#"{"memory_id":"mem_123","run_id":"run-abc","seq":8,"system":"cms-v2","title":"Decision","ts":"2026-01-02T03:04:05Z","type":"memory_written","v":1}"#,
            ),
            (
                EventEnvelope::at(
                    "run-abc",
                    9,
                    fixed_ts(),
                    VegvisirEvent::ContextPrepared(ContextPrepared {
                        system: "ecm".to_string(),
                        input_tokens: Some(100),
                        output_tokens: Some(50),
                        source_count: 3,
                    }),
                ),
                r#"{"input_tokens":100,"output_tokens":50,"run_id":"run-abc","seq":9,"source_count":3,"system":"ecm","ts":"2026-01-02T03:04:05Z","type":"context_prepared","v":1}"#,
            ),
            (
                EventEnvelope::at(
                    "run-abc",
                    10,
                    fixed_ts(),
                    VegvisirEvent::ContextCompacted(ContextCompacted {
                        strategy: "manual".to_string(),
                        before_tokens: Some(1000),
                        after_tokens: Some(300),
                    }),
                ),
                r#"{"after_tokens":300,"before_tokens":1000,"run_id":"run-abc","seq":10,"strategy":"manual","ts":"2026-01-02T03:04:05Z","type":"context_compacted","v":1}"#,
            ),
            (
                EventEnvelope::at(
                    "run-abc",
                    11,
                    fixed_ts(),
                    VegvisirEvent::RunCompleted(RunCompleted {
                        status: RunCompletionStatus::Ok,
                        summary: Some("done".to_string()),
                    }),
                ),
                r#"{"run_id":"run-abc","seq":11,"status":"ok","summary":"done","ts":"2026-01-02T03:04:05Z","type":"run_completed","v":1}"#,
            ),
            (
                EventEnvelope::at(
                    "run-abc",
                    12,
                    fixed_ts(),
                    VegvisirEvent::RunFailed(RunFailed {
                        error: "provider timed out".to_string(),
                        recoverable: true,
                    }),
                ),
                r#"{"error":"provider timed out","recoverable":true,"run_id":"run-abc","seq":12,"ts":"2026-01-02T03:04:05Z","type":"run_failed","v":1}"#,
            ),
        ];

        for (envelope, expected) in cases {
            assert_eq!(to_jsonl_record(&envelope)?, expected);
            assert_eq!(parse_jsonl_record(expected)?, envelope);
        }
        Ok(())
    }

    #[test]
    fn unknown_event_types_parse_without_breaking_archived_logs() -> anyhow::Result<()> {
        let parsed = parse_jsonl_record(
            r#"{"v":1,"run_id":"run-1","seq":99,"ts":"2026-01-02T03:04:05Z","type":"future_event"}"#,
        )?;
        assert_eq!(parsed.run_id, "run-1");
        assert_eq!(parsed.seq, 99);
        assert_eq!(parsed.event, VegvisirEvent::Unknown);
        Ok(())
    }

    #[test]
    fn event_emitter_assigns_monotonic_sequences() -> anyhow::Result<()> {
        let sink = MemoryEventSink::new();
        let mut emitter = EventEmitter::new("run-1", sink.clone());

        emitter.emit(VegvisirEvent::RunStarted(RunStarted {
            session_id: "session-1".to_string(),
            workspace: "/tmp/work".to_string(),
            provider: "demo".to_string(),
            model: "demo-local".to_string(),
            agent: None,
        }))?;
        emitter.emit(VegvisirEvent::RunCompleted(RunCompleted {
            status: RunCompletionStatus::Ok,
            summary: Some("done".to_string()),
        }))?;

        let events = sink.events();
        assert_eq!(
            events.iter().map(|event| event.seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(emitter.next_seq(), 3);
        Ok(())
    }

    #[test]
    fn composite_sink_fans_out_to_multiple_sinks() -> anyhow::Result<()> {
        let first = MemoryEventSink::new();
        let second = MemoryEventSink::new();
        let mut composite = CompositeEventSink::new();
        composite.push(first.clone());
        composite.push(second.clone());
        assert_eq!(composite.len(), 2);

        let mut emitter = EventEmitter::new("run-1", composite);
        emitter.emit(VegvisirEvent::RunFailed(RunFailed {
            error: "boom".to_string(),
            recoverable: true,
        }))?;

        assert_eq!(first.events().len(), 1);
        assert_eq!(second.events().len(), 1);
        assert_eq!(first.events()[0].seq, second.events()[0].seq);
        Ok(())
    }

    #[test]
    fn jsonl_file_sink_appends_records() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("events").join("run.jsonl");
        let sink = JsonlFileEventSink::new(&path);
        let mut emitter = EventEmitter::new("run-1", sink);

        emitter.emit(VegvisirEvent::TaskStarted(TaskStarted {
            task_id: "task-1".to_string(),
            name: "cargo test".to_string(),
            kind: "shell".to_string(),
        }))?;
        emitter.emit(VegvisirEvent::TaskCompleted(TaskCompleted {
            task_id: "task-1".to_string(),
            status: TaskStatus::Completed,
            summary: Some("passed".to_string()),
        }))?;

        let text = fs::read_to_string(path)?;
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("task_started"));
        assert!(text.contains("task_completed"));
        Ok(())
    }

    #[test]
    fn jsonl_serializer_redacts_secret_like_values_without_hiding_commit_shas() -> anyhow::Result<()>
    {
        let commit_sha = "0123456789abcdef0123456789abcdef01234567";
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.sflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let opaque = "aBcdEF1234567890_aBcdEF1234567890_aBcdEF1234567890";
        let envelope = EventEnvelope::at(
            "run-1",
            1,
            fixed_ts(),
            VegvisirEvent::ToolRequested(ToolRequested {
                tool_call_id: "tool-1".to_string(),
                tool_name: "run_command".to_string(),
                args_summary: Some(format!(
                    "commit {commit_sha} curl -H authorization: bearer sk-123456789012345678901234567890 jwt {jwt} opaque {opaque}"
                )),
            }),
        );

        let line = to_jsonl_record(&envelope)?;
        assert!(line.contains(commit_sha));
        assert!(line.contains(REDACTION));
        assert!(!line.contains("sk-123456"));
        assert!(!line.contains("eyJhbGci"));
        assert!(!line.contains("aBcdEF123456"));
        Ok(())
    }
}
