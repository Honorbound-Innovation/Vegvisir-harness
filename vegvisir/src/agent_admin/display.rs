use std::path::{Path, PathBuf};

use anyhow::Context;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::core::AgentProfile;

use super::{
    AgentComparison, AgentTemplate, MetricsReport, ValidationReport, compact_json, dash_if_empty,
    list_or_dash, percent_or_dash,
};

pub fn print_saved(profile: &AgentProfile, path: &Path, json_output: bool) -> anyhow::Result<()> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "id": profile.id,
                "path": path,
                "profile": profile,
            }))?
        );
    } else {
        println!(
            "Saved agent {} ({}, mode={}) at {}",
            profile.id,
            profile.display_name,
            profile.mode,
            path.display()
        );
    }
    Ok(())
}

pub fn tui_help_text() -> &'static str {
    "Keyboard:
  Esc         quit, or close help when help is open
  Ctrl+C      quit
  F1 / ?      toggle help / show all TUI commands
  N           create a new draft agent: id[, template[, display name]]
  F2 / A      open conventional action menu
  F / Ctrl+F  search agents by id, name, mode, profile text, permissions, and metadata
  E           edit primary scope metadata for selected agent
  Y           edit memory policy for selected agent
  B           edit budget max steps for selected agent
  P           edit provider for selected agent ('-' or 'clear' inherits)
  O           edit model for selected agent ('-' or 'clear' inherits)
  U           edit comma-separated tool allow-list for selected agent
  S           edit comma-separated enabled skills for selected agent
  D           edit comma-separated allowed MCP servers for selected agent
  L           edit comma-separated bound USRL contracts for selected agent
  T           edit comma-separated tags for selected agent
  ↑/↓         move selection
  Home/End    jump to start/end
  PageUp/Down jump by 5
  Enter / V   validate selected agent
  M           show metrics for selected agent
  H           show history count for selected agent
  R           refresh

When help is open:
  F1 / ?      close help
  Esc         close help
  Ctrl+C      quit

Action menu:
  Create new agent opens the same one-line create form as N.
  ↑/↓         move action selection
  Enter       apply selected action
  Esc         cancel

Create mode:
  type text   id[, template[, display name]]; template may be planner, researcher, orchestrator, engineer, coder, tester, or agent-red
  Enter       create a draft agent; duplicate/invalid ids stay in the TUI and show an error
  Esc         cancel

Scope/memory/budget/provider/model/permission/tag edit modes:
  type text   edit the selected field
  Enter       save the field; invalid entries stay in the TUI and show an error
  Esc         cancel

Search mode:
  type text   live-filter the agent list
  Enter       apply and exit
  Esc         cancel and exit

Clone, delete, import, export, prompt replacement, bulk set operations,
and registry-wide operations still use explicit vegvisir-agent-admin CLI
subcommands outside the TUI. There is no ':' command
entry in this TUI."
}

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn metadata_string_list(profile: &AgentProfile, key: &str) -> Vec<String> {
    profile
        .metadata
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn metadata_list_or_dash(profile: &AgentProfile, key: &str) -> String {
    let values = metadata_string_list(profile, key);
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(", ")
    }
}

pub fn budget_value<'a>(profile: &'a AgentProfile, key: &str) -> Option<&'a Value> {
    profile
        .metadata
        .get("default_work_budget")
        .and_then(Value::as_object)
        .and_then(|budget| budget.get(key))
}

pub fn budget_u64(profile: &AgentProfile, key: &str) -> Option<u64> {
    budget_value(profile, key).and_then(Value::as_u64)
}

pub fn budget_string<'a>(profile: &'a AgentProfile, key: &str) -> Option<&'a str> {
    budget_value(profile, key).and_then(Value::as_str)
}

pub fn budget_string_list(profile: &AgentProfile, key: &str) -> Vec<String> {
    budget_value(profile, key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn budget_summary(profile: &AgentProfile) -> String {
    let Some(budget) = profile
        .metadata
        .get("default_work_budget")
        .and_then(Value::as_object)
    else {
        return "-".to_string();
    };
    if budget.is_empty() {
        return "-".to_string();
    }
    let mut parts = Vec::new();
    for (key, label) in [
        ("max_steps", "steps"),
        ("max_tool_calls", "calls"),
        ("max_read_bytes", "read"),
        ("max_output_bytes", "output"),
    ] {
        if let Some(value) = budget.get(key).and_then(Value::as_u64) {
            parts.push(format!("{label}={value}"));
        }
    }
    let tools = budget_string_list(profile, "allowed_tools");
    if !tools.is_empty() {
        parts.push(format!("tools={}", tools.join(",")));
    }
    if let Some(notes) = budget.get("notes").and_then(Value::as_str)
        && !notes.trim().is_empty()
    {
        parts.push(format!("notes={}", notes.trim()));
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join(" ")
    }
}

pub fn profile_tags(profile: &AgentProfile) -> Vec<String> {
    profile
        .metadata
        .get("tags")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn filtered_profiles(profiles: &[AgentProfile], filter: &str) -> Vec<AgentProfile> {
    let needle = filter.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return profiles.to_vec();
    }
    profiles
        .iter()
        .filter(|profile| profile_matches_filter(profile, &needle))
        .cloned()
        .collect()
}

pub fn profile_matches_filter(profile: &AgentProfile, needle: &str) -> bool {
    let mut haystack = String::new();
    haystack.push_str(&profile.id);
    haystack.push(' ');
    haystack.push_str(&profile.display_name);
    haystack.push(' ');
    haystack.push_str(&profile.mode);
    haystack.push(' ');
    haystack.push_str(&profile.description);
    haystack.push(' ');
    haystack.push_str(profile.current_provider.as_deref().unwrap_or(""));
    haystack.push(' ');
    haystack.push_str(profile.current_model.as_deref().unwrap_or(""));
    haystack.push(' ');
    haystack.push_str(&profile.memory_policy);
    haystack.push(' ');
    haystack.push_str(&profile.system_prompt);
    haystack.push(' ');
    haystack.push_str(&profile.enabled_tools.join(" "));
    haystack.push(' ');
    haystack.push_str(&profile.enabled_skills.join(" "));
    haystack.push(' ');
    haystack.push_str(&profile.enabled_mcp_servers.join(" "));
    haystack.push(' ');
    haystack.push_str(&profile.usrl_contracts.join(" "));
    haystack.push(' ');
    for (key, value) in &profile.metadata {
        haystack.push_str(key);
        haystack.push(' ');
        haystack.push_str(&compact_json(value));
        haystack.push(' ');
    }
    haystack.to_ascii_lowercase().contains(needle)
}
pub fn print_validation_report(report: &ValidationReport) {
    println!("\nValidation {}: {}", report.id, report.status);
    if report.errors.is_empty() && report.warnings.is_empty() && report.recommendations.is_empty() {
        println!("  ok");
        return;
    }
    for issue in &report.errors {
        println!("  ERROR {}: {}", issue.field, issue.message);
    }
    for issue in &report.warnings {
        println!("  WARN  {}: {}", issue.field, issue.message);
    }
    for issue in &report.recommendations {
        println!("  REC   {}: {}", issue.field, issue.message);
    }
}

pub fn print_metrics_report(report: &MetricsReport) {
    println!("# Metrics: {}", report.id);
    println!("path: {}", report.path.display());
    println!("tasks_completed: {}", report.metrics.tasks_completed);
    println!("tasks_failed: {}", report.metrics.tasks_failed);
    println!("tasks_cancelled: {}", report.metrics.tasks_cancelled);
    println!(
        "task_success_rate: {}",
        percent_or_dash(report.task_success_rate)
    );
    println!(
        "verification_success_rate: {}",
        percent_or_dash(report.verification_success_rate)
    );
    println!("scope_violations: {}", report.metrics.scope_violations);
    println!("follow_up_fixes: {}", report.metrics.follow_up_fixes);
    println!("retries: {}", report.metrics.retries);
    if !report.metrics.capability_scores.is_empty() {
        println!("capability_scores:");
        for (name, score) in &report.metrics.capability_scores {
            println!("  {name}: {score:.2}");
        }
    }
    for warning in &report.warnings {
        println!("warning: {warning}");
    }
}

pub fn print_comparison(comparison: &AgentComparison) {
    println!(
        "# Compare {} -> {}",
        comparison.left_id, comparison.right_id
    );
    if comparison.differences.is_empty() {
        println!("No differences in compared fields.");
        return;
    }
    for diff in &comparison.differences {
        println!("\n## {}", diff.field);
        println!("left: {}", compact_json(&diff.left));
        println!("right: {}", compact_json(&diff.right));
    }
}

pub fn prompt_digest(prompt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn print_profile(profile: &AgentProfile) {
    println!("# Agent: {}", profile.display_name);
    println!("id: {}", profile.id);
    println!("mode: {}", profile.mode);
    println!("description: {}", dash_if_empty(&profile.description));
    println!("cms_user_id: {}", profile.cms_user_id);
    println!("cms_project_id: {}", profile.cms_project_id);
    println!("memory_scope: {}", profile.memory_scope);
    println!(
        "provider: {}",
        profile.current_provider.as_deref().unwrap_or("-")
    );
    println!("model: {}", profile.current_model.as_deref().unwrap_or("-"));
    println!("tools: {}", list_or_dash(&profile.enabled_tools));
    println!("skills: {}", list_or_dash(&profile.enabled_skills));
    println!(
        "mcp_servers: {}",
        list_or_dash(&profile.enabled_mcp_servers)
    );
    println!("usrl_contracts: {}", list_or_dash(&profile.usrl_contracts));
    println!("memory_policy: {}", profile.memory_policy);
    if !profile.metadata.is_empty() {
        println!(
            "metadata: {}",
            serde_json::to_string(&profile.metadata).unwrap_or_else(|_| "{}".to_string())
        );
    }
    println!(
        "\n## System prompt\n\n```text\n{}\n```",
        profile.system_prompt
    );
}

pub fn print_template(template: &AgentTemplate) {
    println!("# Template: {}", template.display_name);
    println!("mode: {}", template.mode);
    println!("description: {}", template.description);
    println!("tools: {}", list_or_dash(&template.enabled_tools));
    println!("skills: {}", list_or_dash(&template.enabled_skills));
    println!("usrl_contracts: {}", list_or_dash(&template.usrl_contracts));
    println!("memory_policy: {}", template.memory_policy);
    println!(
        "\n## System prompt\n\n```text\n{}\n```",
        template.system_prompt
    );
}

pub fn read_prompt(
    prompt: Option<String>,
    prompt_file: Option<PathBuf>,
) -> anyhow::Result<Option<String>> {
    if let Some(path) = prompt_file {
        return Ok(Some(
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?,
        ));
    }
    Ok(prompt)
}

pub fn touch_metadata(profile: &mut AgentProfile, action: &str) {
    profile.metadata.insert(
        "managed_by".to_string(),
        Value::String("vegvisir-agent-admin".to_string()),
    );
    profile.metadata.insert(
        "last_admin_action".to_string(),
        Value::String(action.to_string()),
    );
}
