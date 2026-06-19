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
            Some("run" | "spawn" | "start") => self.tasks_run_command(&args[1..]),
            Some("cancel" | "kill" | "stop") => self.tasks_cancel_command(&args[1..]),
            Some("events") => self.task_events_text(),
            Some("json") => self.tasks_json(),
            Some("help") => tasks_usage().to_string(),
            Some(other) => format!("Unknown /tasks command: {other}\n{}", tasks_usage()),
        }
    }

    fn tasks_run_command(&mut self, args: &[String]) -> String {
        let parsed = match parse_tasks_run_args(args) {
            Ok(parsed) => parsed,
            Err(error) => return error,
        };
        match self.spawn_background_shell_task(
            parsed.command,
            parsed.timeout_seconds,
            parsed.stall_timeout_seconds,
        ) {
            Ok(task_id) => format!(
                "Spawned background shell task {task_id}. Use /tasks show {task_id} or /tasks cancel {task_id}."
            ),
            Err(error) => format!("Failed to spawn background task: {error}"),
        }
    }

    fn tasks_cancel_command(&mut self, args: &[String]) -> String {
        let Some(id) = args.first().map(String::as_str) else {
            return "Usage: /tasks cancel <task-id>".to_string();
        };
        match self.cancel_background_task(id) {
            Ok(()) => format!("Cancellation requested for task {id}."),
            Err(error) => format!("Failed to cancel task {id}: {error}"),
        }
    }

    fn tasks_text(&self) -> String {
        let records = self.task_manager.records();
        if records.is_empty() {
            return format!("No task records for this session yet.\n\n{}", tasks_usage());
        }
        let mut lines = vec![format!(
            "Tasks for session {} ({} total, {} active, {} runner processes)",
            self.session.session_id,
            records.len(),
            self.task_manager.active_records().len(),
            self.task_runner.running_count(),
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
            "Use /tasks show <task-id> for details, /tasks run <cmd> [args...] for background shell tasks, or /tasks --json for machine-readable output."
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
            "Task {}\n  kind: {:?}\n  state: {:?}\n  description: {}\n  command: {}\n  workspace: {}\n  output_file: {}\n  output_offset: {}\n  exit_code: {}\n  started_at: {}\n  finished_at: {}\n  owner_run_id: {}\n  owner_agent_id: {}\n  runner_active: {}\n\nRetained output:\n```text\n{}\n```",
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
            self.task_runner.is_running(id),
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
            "runner_process_count": self.task_runner.running_count(),
            "pending_lifecycle_event_count": self.task_manager.events().len(),
        }))
        .unwrap_or_else(|error| format!("Failed to serialize tasks: {error}"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ParsedTasksRunArgs {
    command: Vec<String>,
    timeout_seconds: u64,
    stall_timeout_seconds: Option<u64>,
}

fn parse_tasks_run_args(args: &[String]) -> Result<ParsedTasksRunArgs, String> {
    let mut timeout_seconds = 30 * 60;
    let mut stall_timeout_seconds = Some(10 * 60);
    let mut command = Vec::new();
    let mut parsing_options = true;

    for arg in args {
        if parsing_options && arg == "--" {
            parsing_options = false;
            continue;
        }
        if parsing_options && let Some(raw) = arg.strip_prefix("--timeout=") {
            timeout_seconds = raw
                .parse::<u64>()
                .map_err(|_| format!("Invalid --timeout value: {raw}"))?
                .clamp(1, 86_400);
            continue;
        }
        if parsing_options && let Some(raw) = arg.strip_prefix("--stall-timeout=") {
            stall_timeout_seconds = Some(
                raw.parse::<u64>()
                    .map_err(|_| format!("Invalid --stall-timeout value: {raw}"))?
                    .max(1),
            );
            continue;
        }
        if parsing_options && arg == "--no-stall-timeout" {
            stall_timeout_seconds = None;
            continue;
        }
        command.push(arg.clone());
    }

    if command.is_empty() {
        return Err(format!(
            "Usage: /tasks run [--timeout=<seconds>] [--stall-timeout=<seconds>|--no-stall-timeout] -- <command> [args...]\n{}",
            tasks_usage()
        ));
    }
    Ok(ParsedTasksRunArgs {
        command,
        timeout_seconds,
        stall_timeout_seconds,
    })
}

fn tasks_usage() -> &'static str {
    "Usage: /tasks [list|show <task-id>|run [--timeout=<seconds>] [--stall-timeout=<seconds>|--no-stall-timeout] -- <cmd> [args...]|cancel <task-id>|events|--json]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tasks_run_options_and_command_separator() {
        let parsed = parse_tasks_run_args(&[
            "--timeout=5".to_string(),
            "--stall-timeout=2".to_string(),
            "--".to_string(),
            "python3".to_string(),
            "-c".to_string(),
            "print('hi')".to_string(),
        ])
        .unwrap();

        assert_eq!(parsed.timeout_seconds, 5);
        assert_eq!(parsed.stall_timeout_seconds, Some(2));
        assert_eq!(parsed.command, vec!["python3", "-c", "print('hi')"]);
    }

    #[test]
    fn parses_tasks_run_no_stall_timeout() {
        let parsed = parse_tasks_run_args(&[
            "--no-stall-timeout".to_string(),
            "python3".to_string(),
            "-V".to_string(),
        ])
        .unwrap();

        assert_eq!(parsed.stall_timeout_seconds, None);
        assert_eq!(parsed.command, vec!["python3", "-V"]);
    }
}
