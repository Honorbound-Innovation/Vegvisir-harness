use super::super::*;

impl TuiApplication {
    pub(crate) fn tasks_command(&mut self, args: &[String]) -> String {
        if wants_json(args) {
            return self.tasks_json();
        }
        match args.first().map(String::as_str) {
            None => self.tasks_text(),
            Some("list" | "status") => self.tasks_text(),
            Some("show") if args.len() <= 1 => self.tasks_text(),
            Some("show" | "view" | "inspect") => {
                let Some(id) = args.get(1) else {
                    return "Usage: /tasks show <task-id>".to_string();
                };
                self.task_show_text(id)
            }
            Some("events") => self.task_events_text(),
            Some("json") => self.tasks_json(),
            Some("help") => tasks_usage().to_string(),
            Some(other) => format!("Unknown /tasks command: {other}\n{}", tasks_usage()),
        }
    }

    fn tasks_text(&self) -> String {
        let records = self.task_manager.records();
        if records.is_empty() {
            return format!("No task records for this session yet.\n\n{}", tasks_usage());
        }
        let mut lines = vec![format!(
            "Tasks for session {} ({} total, {} active)",
            self.session.session_id,
            records.len(),
            self.task_manager.active_records().len()
        )];
        for record in records {
            lines.push(format!(
                "  {}  {:?}  {:?}  {}{}",
                record.id,
                record.kind,
                record.state,
                record.description,
                record
                    .exit_code
                    .map(|code| format!("  exit_code={code}"))
                    .unwrap_or_default()
            ));
        }
        lines.push(
            "Use /tasks show <task-id> for details or /tasks --json for machine-readable output."
                .to_string(),
        );
        lines.join("\n")
    }

    fn task_show_text(&self, id: &str) -> String {
        let Some(record) = self.task_manager.record(id) else {
            return format!("Unknown task: {id}");
        };
        let retained_output = if record.retained_output.trim().is_empty() {
            "(no retained output)".to_string()
        } else {
            record.retained_output.clone()
        };
        format!(
            "Task {}\n  kind: {:?}\n  state: {:?}\n  description: {}\n  command: {}\n  workspace: {}\n  output_file: {}\n  output_offset: {}\n  exit_code: {}\n  started_at: {}\n  finished_at: {}\n  owner_run_id: {}\n  owner_agent_id: {}\n\nRetained output:\n```text\n{}\n```",
            record.id,
            record.kind,
            record.state,
            record.description,
            record.command.as_deref().unwrap_or("none"),
            record.workspace.display(),
            record.output_file.display(),
            record.output_offset,
            record
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "none".to_string()),
            record
                .started_at
                .map(|ts| ts.to_rfc3339())
                .unwrap_or_else(|| "none".to_string()),
            record
                .finished_at
                .map(|ts| ts.to_rfc3339())
                .unwrap_or_else(|| "none".to_string()),
            record.owner_run_id,
            record.owner_agent_id.as_deref().unwrap_or("none"),
            retained_output.trim_end()
        )
    }

    fn task_events_text(&self) -> String {
        let events = self.task_manager.events();
        if events.is_empty() {
            return "No pending task lifecycle events.".to_string();
        }
        let mut lines = vec![format!("Pending task lifecycle events: {}", events.len())];
        for event in events {
            lines.push(format!("  {event:?}"));
        }
        lines.join("\n")
    }

    fn tasks_json(&self) -> String {
        serde_json::to_string_pretty(&json!({
            "session_id": self.session.session_id,
            "tasks": self.task_manager.records(),
            "active_task_count": self.task_manager.active_records().len(),
            "pending_lifecycle_event_count": self.task_manager.events().len(),
        }))
        .unwrap_or_else(|error| format!("Failed to serialize tasks: {error}"))
    }
}

fn tasks_usage() -> &'static str {
    "Usage: /tasks [list|show <task-id>|events|--json]"
}
