use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};

use crate::{
    core::{ChatMessage, ModelRegistry, ProviderRegistry},
    subagents::{
        SubAgentFileChange, SubAgentObservedEvent, SubAgentObservedEventKind, SubAgentTaskRecord,
    },
};

use super::workspace_state::user_storage_slug;

pub(crate) fn join_or(args: &[String], default: &str) -> String {
    if args.is_empty() {
        default.to_string()
    } else {
        args.join(" ")
    }
}

pub(crate) const CONTEXT_DECISION_MARKERS: &[&str] = &[
    "decided",
    "decision",
    "agreed",
    "preference",
    "prefer",
    "constraint",
    "boundary",
    "must",
    "should",
    "should not",
    "only",
    "remains responsible",
];

pub(crate) const CONTEXT_OPEN_ISSUE_MARKERS: &[&str] = &[
    "todo",
    "next",
    "follow-up",
    "follow up",
    "not fixed",
    "bug",
    "issue",
    "failed",
    "failure",
    "error",
    "unresolved",
    "need to",
    "needs",
    "want",
];

pub(crate) fn append_bullets(lines: &mut Vec<String>, items: Vec<String>, empty: &str) {
    if items.is_empty() {
        lines.push(format!("- {empty}"));
    } else {
        lines.extend(items.into_iter().map(|item| format!("- {item}")));
    }
}

pub(crate) fn summarize_recent_actions(messages: &[ChatMessage], limit: usize) -> Vec<String> {
    let mut actions = Vec::new();
    for message in messages.iter().rev() {
        let lower = message.content.to_ascii_lowercase();
        let action_like = message.role == "tool"
            || lower.contains("verified")
            || lower.contains("ran ")
            || lower.contains("changed")
            || lower.contains("implemented")
            || lower.contains("fixed")
            || lower.contains("pushed")
            || lower.contains("committed")
            || lower.contains("git status")
            || lower.contains("test")
            || lower.contains("cargo ");
        if action_like {
            actions.push(format!(
                "{}: {}",
                message.role,
                compact_context_line(&message.content, 500)
            ));
        }
        if actions.len() >= limit {
            break;
        }
    }
    actions.reverse();
    dedupe_preserve_order(actions)
}

pub(crate) fn summarize_context_signals(
    messages: &[ChatMessage],
    limit: usize,
    markers: &[&str],
) -> Vec<String> {
    let mut signals = Vec::new();
    for message in messages.iter().rev() {
        let lower = message.content.to_ascii_lowercase();
        if markers.iter().any(|marker| lower.contains(marker)) {
            signals.push(format!(
                "{}: {}",
                message.role,
                compact_context_line(&message.content, 500)
            ));
        }
        if signals.len() >= limit {
            break;
        }
    }
    signals.reverse();
    dedupe_preserve_order(signals)
}

fn dedupe_preserve_order(items: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        if !out.iter().any(|existing| existing == &item) {
            out.push(item);
        }
    }
    out
}

pub(crate) fn compact_context_line(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        compact
    } else {
        format!("{}…", compact.chars().take(max_chars).collect::<String>())
    }
}

pub(crate) fn git_status_summary(cwd: &Path) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .arg("status")
        .arg("--short")
        .output();
    let Ok(output) = output else {
        return "unavailable (git status failed to start)".to_string();
    };
    if !output.status.success() {
        return "unavailable (not a git repository or git status failed)".to_string();
    }
    let status = String::from_utf8_lossy(&output.stdout);
    let changed = status
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    if changed == 0 {
        "clean".to_string()
    } else {
        let sample = status
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(8)
            .collect::<Vec<_>>()
            .join("; ");
        if changed > 8 {
            format!("{changed} changed path(s): {sample}; …")
        } else {
            format!("{changed} changed path(s): {sample}")
        }
    }
}

pub(crate) fn parse_config_value(raw: &str) -> Value {
    if let Ok(value) = raw.parse::<u64>() {
        return json!(value);
    }
    match raw {
        "true" => json!(true),
        "false" => json!(false),
        other => json!(other),
    }
}

pub(crate) fn list_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(", ")
    }
}

pub(crate) fn comma_items(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn parse_limit_and_global(args: &[String], default_limit: usize) -> (usize, bool) {
    let mut limit = default_limit;
    let mut global = false;
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--global" | "--all" => global = true,
            "--project" | "--local" => global = false,
            "--limit" | "-n" => {
                if let Some(value) = iter.next() {
                    limit = value.parse::<usize>().unwrap_or(default_limit).clamp(1, 50);
                }
            }
            value if value.starts_with("--limit=") => {
                limit = value
                    .trim_start_matches("--limit=")
                    .parse::<usize>()
                    .unwrap_or(default_limit)
                    .clamp(1, 50);
            }
            _ => {}
        }
    }
    (limit, global)
}

pub(crate) fn parse_limit(args: &[String], default_limit: usize) -> usize {
    let mut limit = default_limit;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--limit" | "-n" => {
                if let Some(value) = iter.next() {
                    limit = value
                        .parse::<usize>()
                        .unwrap_or(default_limit)
                        .clamp(1, 200);
                }
            }
            value if value.starts_with("--limit=") => {
                limit = value
                    .trim_start_matches("--limit=")
                    .parse::<usize>()
                    .unwrap_or(default_limit)
                    .clamp(1, 200);
            }
            _ => {}
        }
    }
    limit
}

pub(crate) fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

pub(crate) fn configured_user_id(defaults: &BTreeMap<String, Value>) -> String {
    if let Some(user_id) = defaults
        .get("current_user_id")
        .and_then(Value::as_str)
        .filter(|user_id| !user_id.trim().is_empty())
    {
        return user_id.to_string();
    }
    std::env::var("VEGVISIR_USER_ID")
        .ok()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| "local-user".to_string())
}

pub(crate) fn session_root_for_user(data_root: &Path, user_id: &str) -> PathBuf {
    if user_id == "local-user" {
        return data_root.join("sessions");
    }
    data_root
        .join("users")
        .join(user_storage_slug(user_id))
        .join("sessions")
}

pub(crate) fn validate_user_id(user_id: &str) -> anyhow::Result<()> {
    let valid = !user_id.trim().is_empty()
        && user_id.len() <= 128
        && user_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '@'))
        && !contains_secret_like_value(user_id);
    if !valid {
        anyhow::bail!(
            "User id must be 1-128 chars and contain only letters, numbers, '-', '_', '.', ':', or '@', with no secret-like material."
        );
    }
    Ok(())
}

pub(crate) fn contains_secret_like_value(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "api_key=",
        "apikey=",
        "access_token=",
        "auth_token=",
        "bearer ",
        "password=",
        "secret=",
        "token=",
        "sk-",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

pub(crate) fn self_model_invalid(models: &ModelRegistry, provider: &str, model_name: &str) -> bool {
    let Some(model) = models.get(model_name) else {
        return true;
    };
    !models.is_model_allowed_for_provider(model, provider)
}

pub(crate) fn model_known_invalid_or_missing(
    models: &ModelRegistry,
    provider: &str,
    model_name: &str,
) -> bool {
    let Some(model) = models.get(model_name) else {
        return true;
    };
    !models.is_model_allowed_for_provider(model, provider)
}

pub(crate) fn set_openai_sso_auth_root(registry: &mut ProviderRegistry, data_root: &Path) {
    if let Some(provider) = registry.get_mut("openai-sso") {
        provider
            .metadata
            .entry("auth_root".to_string())
            .or_insert_with(|| Value::String(data_root.display().to_string()));
    }
}

pub(crate) fn canonical_workspace(path: &Path) -> anyhow::Result<PathBuf> {
    if !path.exists() {
        anyhow::bail!("Workspace path does not exist: {}", path.display());
    }
    if !path.is_dir() {
        anyhow::bail!("Workspace path is not a directory: {}", path.display());
    }
    Ok(path.canonicalize()?)
}

pub(crate) fn expand_workspace_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(raw));
    }
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(raw)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn workspace_title(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("workspace")
        .to_string()
}

pub fn workspace_project_id(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    format!(
        "workspace:{}:{}",
        workspace_title(&canonical),
        short_stable_hash(&canonical.display().to_string())
    )
}

fn short_stable_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub(crate) fn format_subagent_record_markdown(record: &SubAgentTaskRecord) -> String {
    let scope = if record.file_scope.is_empty() {
        "<unspecified>".to_string()
    } else {
        record
            .file_scope
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut out = format!(
        "## Subagent trace: {name}\n\n- id: `{id}`\n- status: `{status:?}`\n- workspace: `{workspace}`\n- file_scope: {scope}\n- created: {created}\n- started: {started}\n- finished: {finished}\n- provider/model: {provider}/{model}\n\n**Goal**\n\n{goal}",
        name = record.name,
        id = record.id,
        status = record.status,
        workspace = record.workspace.display(),
        created = record.created_at.to_rfc3339(),
        started = record
            .started_at
            .map(|ts| ts.to_rfc3339())
            .unwrap_or_else(|| "none".to_string()),
        finished = record
            .finished_at
            .map(|ts| ts.to_rfc3339())
            .unwrap_or_else(|| "none".to_string()),
        provider = record.provider.as_deref().unwrap_or("-"),
        model = record.model.as_deref().unwrap_or("-"),
        goal = record.goal.trim(),
    );

    if !record.work_budget.is_empty() {
        out.push_str(&markdown_details(
            "work budget",
            &format!(
                "```json\n{}\n```",
                serde_json::to_string_pretty(&record.work_budget).unwrap_or_default()
            ),
        ));
    }
    if !record.observability.launch_argv.is_empty()
        || !record.observability.launch_env_keys.is_empty()
    {
        let mut body = String::new();
        if !record.observability.launch_argv.is_empty() {
            body.push_str("Launch argv:\n\n```text\n");
            body.push_str(&record.observability.launch_argv.join(" "));
            body.push_str("\n```\n");
        }
        if !record.observability.launch_env_keys.is_empty() {
            body.push_str("\nLaunch env keys:\n\n");
            body.push_str(&record.observability.launch_env_keys.join(", "));
            body.push('\n');
        }
        out.push_str(&markdown_details("launch details", &body));
    }
    if !record.observability.events.is_empty() {
        out.push_str(&markdown_details(
            &format!("observed events ({})", record.observability.events.len()),
            &format_subagent_events_body(&record.observability.events),
        ));
    }
    if !record.observability.file_changes.is_empty() {
        out.push_str(&markdown_details(
            &format!("file changes ({})", record.observability.file_changes.len()),
            &format_subagent_file_changes_body(&record.observability.file_changes),
        ));
    }
    if !record.observability.notes.is_empty() {
        out.push_str(&markdown_details(
            &format!("observability notes ({})", record.observability.notes.len()),
            &record
                .observability
                .notes
                .iter()
                .map(|note| format!("- {}", note.trim()))
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    if let Some(error) = record
        .error
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        out.push_str(&markdown_details(
            "error",
            &format!("```text\n{}\n```", error.trim()),
        ));
    }
    if let Some(final_answer) = record
        .final_answer
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        out.push_str(&markdown_details(
            "captured output / final report",
            &format!("```text\n{}\n```", final_answer.trim()),
        ));
    }
    out
}

pub(crate) fn format_subagent_events_markdown(record: &SubAgentTaskRecord) -> String {
    if record.observability.events.is_empty() {
        return format!("No observed events captured for subagent {}.", record.id);
    }
    format!(
        "## Subagent events: {}\n{}",
        record.name,
        markdown_details(
            &format!("observed events ({})", record.observability.events.len()),
            &format_subagent_events_body(&record.observability.events),
        )
    )
}

pub(crate) fn format_subagent_diffs_markdown(record: &SubAgentTaskRecord) -> String {
    let changes = &record.observability.file_changes;
    if changes.is_empty() {
        return format!("No file-change diffs captured for subagent {}.", record.id);
    }
    format!(
        "## Subagent diffs: {}\n{}",
        record.name,
        markdown_details(
            &format!("file changes ({})", changes.len()),
            &format_subagent_file_changes_body(changes),
        )
    )
}

fn format_subagent_events_body(events: &[SubAgentObservedEvent]) -> String {
    events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let mut line = format!(
                "{}. **{}**",
                index + 1,
                subagent_event_kind_label(&event.kind)
            );
            if let Some(name) = event.name.as_deref().filter(|value| !value.is_empty()) {
                line.push_str(&format!(" `{name}`"));
            }
            if let Some(ok) = event.ok {
                line.push_str(&format!(" — ok={ok}"));
            }
            if let Some(summary) = event
                .summary
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                line.push_str(&format!(" — {}", summary.trim()));
            }
            if let Some(args) = event
                .args
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                line.push_str("\n\n   Args:\n\n   ```json\n");
                line.push_str(&indent_lines(args.trim(), "   "));
                line.push_str("\n   ```");
            }
            if let Some(detail) = event
                .detail
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                line.push_str("\n\n   Detail:\n\n   ```text\n");
                line.push_str(&indent_lines(detail.trim(), "   "));
                line.push_str("\n   ```");
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn format_subagent_file_changes_body(changes: &[SubAgentFileChange]) -> String {
    changes
        .iter()
        .map(|change| {
            let mut item = format!(
                "### {:?}: `{}`\n\n- before_bytes: {:?}\n- after_bytes: {:?}",
                change.change,
                change.path.display(),
                change.before_bytes,
                change.after_bytes
            );
            if let Some(diff) = change
                .diff
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                item.push_str("\n\n```diff\n");
                item.push_str(diff.trim());
                item.push_str("\n```");
            } else {
                item.push_str("\n\n_No diff text captured._");
            }
            item
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn markdown_details(summary: &str, body: &str) -> String {
    format!(
        "\n\n<details>\n<summary>{}</summary>\n\n{}\n\n</details>",
        escape_html_text(summary),
        body.trim()
    )
}

fn escape_html_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn subagent_event_kind_label(kind: &SubAgentObservedEventKind) -> &'static str {
    match kind {
        SubAgentObservedEventKind::Activity => "activity",
        SubAgentObservedEventKind::ToolStart => "tool start",
        SubAgentObservedEventKind::ToolEnd => "tool end",
    }
}

fn indent_lines(value: &str, prefix: &str) -> String {
    value
        .lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn estimated_message_line_count(message: &ChatMessage) -> usize {
    let content_lines = message
        .content
        .lines()
        .map(|line| (line.chars().count() / 80).saturating_add(1))
        .sum::<usize>()
        .max(1);
    content_lines + 2
}

pub(crate) fn is_live_tool_message(content: &str) -> bool {
    content.starts_with("Running tool: ")
        || content.starts_with("Tool finished: ")
        || content.starts_with("Tool failed: ")
}

pub(crate) fn is_turn_failure_summary(content: &str) -> bool {
    content.starts_with("Turn failed before the model produced a normal final summary.")
}
