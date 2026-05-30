use std::{collections::BTreeMap, fs, path::PathBuf, process::Command};

use chrono::Utc;

use crate::core::ChatMessage;

use super::super::TuiApplication;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SummaryMode {
    Standard,
    Handoff,
}

#[derive(Debug)]
struct SummaryOptions {
    mode: SummaryMode,
    save: bool,
    memory: bool,
    global_memory: bool,
    file: Option<PathBuf>,
    since_last: bool,
    since_start: bool,
}

impl Default for SummaryOptions {
    fn default() -> Self {
        Self {
            mode: SummaryMode::Standard,
            save: false,
            memory: false,
            global_memory: false,
            file: None,
            since_last: false,
            since_start: true,
        }
    }
}

impl TuiApplication {
    pub(crate) fn summary_command(
        &mut self,
        args: &[String],
        force_handoff: bool,
    ) -> anyhow::Result<String> {
        let options = self.parse_summary_options(args, force_handoff)?;
        let summary = self.build_session_summary(&options);
        let mut notes = Vec::new();

        if options.save || options.file.is_some() {
            let path = if let Some(path) = &options.file {
                path.clone()
            } else {
                self.default_summary_path(options.mode)
            };
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, &summary)?;
            notes.push(format!("Saved summary to {}", path.display()));
        }

        if options.memory {
            let title = match options.mode {
                SummaryMode::Standard => format!("Session summary {}", self.session.session_id),
                SummaryMode::Handoff => format!("Agent handoff {}", self.session.session_id),
            };
            let result = if options.global_memory {
                self.cms
                    .remember_global("session_summary", title, &summary)?
            } else {
                self.cms.remember("session_summary", title, &summary)?
            };
            notes.push(format!(
                "Remembered {}summary memory {}",
                if options.global_memory { "global " } else { "" },
                result.memory_id.0
            ));
        }

        if notes.is_empty() {
            Ok(summary)
        } else {
            Ok(format!("{}\n\n---\n{}", summary, notes.join("\n")))
        }
    }

    fn parse_summary_options(
        &self,
        args: &[String],
        force_handoff: bool,
    ) -> anyhow::Result<SummaryOptions> {
        let mut options = SummaryOptions::default();
        if force_handoff {
            options.mode = SummaryMode::Handoff;
        }
        let mut idx = 0usize;
        while idx < args.len() {
            match args[idx].as_str() {
                "--handoff" | "handoff" => options.mode = SummaryMode::Handoff,
                "--standard" | "standard" => options.mode = SummaryMode::Standard,
                "--save" | "save" => options.save = true,
                "--memory" | "memory" => options.memory = true,
                "--global" | "--user" => {
                    options.memory = true;
                    options.global_memory = true;
                }
                "--since-last" | "since-last" => {
                    options.since_last = true;
                    options.since_start = false;
                }
                "--since-start" | "since-start" => {
                    options.since_start = true;
                    options.since_last = false;
                }
                "--file" | "file" => {
                    let Some(path) = args.get(idx + 1) else {
                        anyhow::bail!("Usage: /summary --file <path>");
                    };
                    options.file = Some(self.resolve_workspace_path(path));
                    idx += 1;
                }
                other if other.starts_with("--file=") => {
                    let path = other.trim_start_matches("--file=");
                    options.file = Some(self.resolve_workspace_path(path));
                }
                "--help" | "help" => anyhow::bail!(
                    "Usage: /summary [--handoff] [--save] [--file <path>] [--memory] [--global] [--since-start|--since-last]\nAliases: /handoff, /session-summary"
                ),
                other => anyhow::bail!("Unknown /summary option: {other}"),
            }
            idx += 1;
        }
        Ok(options)
    }

    fn default_summary_path(&self, mode: SummaryMode) -> PathBuf {
        let dir = self.cwd.join(".vegvisir").join("session-summaries");
        let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
        let suffix = match mode {
            SummaryMode::Standard => "summary",
            SummaryMode::Handoff => "handoff",
        };
        dir.join(format!(
            "{}-{}-{}.md",
            timestamp, self.session.session_id, suffix
        ))
    }

    fn build_session_summary(&self, options: &SummaryOptions) -> String {
        let messages = selected_messages(&self.session.messages, options.since_last);
        let mut out = String::new();
        let title = match options.mode {
            SummaryMode::Standard => "Session Summary",
            SummaryMode::Handoff => "Agent Handoff Summary",
        };
        push_line(&mut out, &format!("# {title}"));
        push_line(&mut out, "");
        push_line(&mut out, "## Scope");
        push_kv(&mut out, "Session", &self.session.session_id);
        push_kv(&mut out, "Title", &self.session.title);
        push_kv(&mut out, "Workspace", &self.cwd.display().to_string());
        push_kv(&mut out, "Provider", &self.session.current_provider);
        push_kv(&mut out, "Model", &self.session.current_model);
        if let Some(agent) = &self.session.active_agent_name {
            push_kv(&mut out, "Active agent", agent);
        }
        if let Some(ka) = &self.session.active_persona_id {
            push_kv(&mut out, "Active ka", ka);
        }
        push_kv(&mut out, "Generated", &Utc::now().to_rfc3339());
        push_kv(
            &mut out,
            "Message window",
            if options.since_last {
                "since last saved/generated summary marker"
            } else {
                "since session start"
            },
        );
        push_line(&mut out, "");

        append_counts(&mut out, messages);
        append_conversation_digest(&mut out, messages, options.mode);
        append_tool_activity(&mut out, messages);
        append_decisions_and_failures(&mut out, messages);
        append_git_state(&mut out, &self.cwd);
        append_changed_files(&mut out, &self.cwd);
        append_tests_and_commands(&mut out, messages);
        append_open_items(&mut out, messages, options.mode);

        if options.mode == SummaryMode::Handoff {
            append_handoff_tail(&mut out);
        } else {
            append_standard_tail(&mut out);
        }
        out
    }
}

fn selected_messages(messages: &[ChatMessage], since_last: bool) -> &[ChatMessage] {
    if !since_last {
        return messages;
    }
    let marker = messages.iter().rposition(|message| {
        message.role == "system" && message.content.contains("Session summary saved")
    });
    marker.map(|idx| &messages[idx + 1..]).unwrap_or(messages)
}

fn append_counts(out: &mut String, messages: &[ChatMessage]) {
    let mut by_role: BTreeMap<&str, usize> = BTreeMap::new();
    for message in messages {
        *by_role.entry(message.role.as_str()).or_insert(0) += 1;
    }
    push_line(out, "## Message Counts");
    push_line(out, &format!("- total: {}", messages.len()));
    for (role, count) in by_role {
        push_line(out, &format!("- {role}: {count}"));
    }
    push_line(out, "");
}

fn append_conversation_digest(out: &mut String, messages: &[ChatMessage], mode: SummaryMode) {
    push_line(
        out,
        if mode == SummaryMode::Handoff {
            "## Current Objective / Conversation Digest"
        } else {
            "## Conversation Digest"
        },
    );
    let user_messages = messages
        .iter()
        .filter(|m| m.role == "user")
        .collect::<Vec<_>>();
    let assistant_messages = messages
        .iter()
        .filter(|m| m.role == "assistant")
        .collect::<Vec<_>>();
    push_line(out, "### Recent user requests");
    append_recent_bullets(out, &user_messages, 8);
    push_line(out, "");
    push_line(out, "### Recent assistant outcomes");
    append_recent_bullets(out, &assistant_messages, 8);
    push_line(out, "");
}

fn append_tool_activity(out: &mut String, messages: &[ChatMessage]) {
    push_line(out, "## Tool / Note / Error Activity");
    let system = messages
        .iter()
        .filter(|m| m.role == "system")
        .collect::<Vec<_>>();
    if system.is_empty() {
        push_line(
            out,
            "- No tool/note/error activity captured in selected window.",
        );
    } else {
        append_recent_bullets(out, &system, 12);
    }
    push_line(out, "");
}

fn append_decisions_and_failures(out: &mut String, messages: &[ChatMessage]) {
    push_line(out, "## Decisions, Fixes, and Failures Mentioned");
    let needles = [
        "fixed",
        "implemented",
        "changed",
        "patched",
        "committed",
        "pushed",
        "failed",
        "error",
        "blocked",
        "todo",
        "next",
        "remaining",
        "verified",
        "passed",
    ];
    let mut emitted = 0usize;
    for message in messages.iter().rev() {
        let lower = message.content.to_ascii_lowercase();
        if needles.iter().any(|needle| lower.contains(needle)) {
            push_line(
                out,
                &format!("- {}: {}", message.role, one_line(&message.content, 220)),
            );
            emitted += 1;
            if emitted >= 14 {
                break;
            }
        }
    }
    if emitted == 0 {
        push_line(
            out,
            "- No obvious decision/failure markers found in selected window.",
        );
    }
    push_line(out, "");
}

fn append_git_state(out: &mut String, cwd: &std::path::Path) {
    push_line(out, "## Git State");
    match run_git(cwd, &["status", "--short"]) {
        Ok(status) if status.trim().is_empty() => push_line(out, "- Working tree appears clean."),
        Ok(status) => {
            push_line(out, "```text");
            push_line(out, status.trim_end());
            push_line(out, "```");
        }
        Err(error) => push_line(out, &format!("- Git status unavailable: {error}")),
    }
    if let Ok(branch) = run_git(cwd, &["branch", "--show-current"]) {
        if !branch.trim().is_empty() {
            push_line(out, &format!("- branch: `{}`", branch.trim()));
        }
    }
    if let Ok(head) = run_git(cwd, &["rev-parse", "--short", "HEAD"]) {
        if !head.trim().is_empty() {
            push_line(out, &format!("- head: `{}`", head.trim()));
        }
    }
    push_line(out, "");
}

fn append_changed_files(out: &mut String, cwd: &std::path::Path) {
    push_line(out, "## Changed Files / Diff Stat");
    match run_git(cwd, &["diff", "--stat"]) {
        Ok(stat) if !stat.trim().is_empty() => {
            push_line(out, "```text");
            push_line(out, stat.trim_end());
            push_line(out, "```");
        }
        _ => push_line(out, "- No unstaged diff stat detected."),
    }
    match run_git(cwd, &["diff", "--cached", "--stat"]) {
        Ok(stat) if !stat.trim().is_empty() => {
            push_line(out, "### Staged");
            push_line(out, "```text");
            push_line(out, stat.trim_end());
            push_line(out, "```");
        }
        _ => {}
    }
    push_line(out, "");
}

fn append_tests_and_commands(out: &mut String, messages: &[ChatMessage]) {
    push_line(out, "## Commands / Verification Observed");
    let mut emitted = 0usize;
    for message in messages.iter().rev() {
        let lower = message.content.to_ascii_lowercase();
        if lower.contains("cargo test")
            || lower.contains("cargo check")
            || lower.contains("npm test")
            || lower.contains("npm run check")
            || lower.contains("pytest")
            || lower.contains("mvn")
            || lower.contains("gradle")
            || lower.contains("tool finished")
        {
            push_line(out, &format!("- {}", one_line(&message.content, 220)));
            emitted += 1;
            if emitted >= 12 {
                break;
            }
        }
    }
    if emitted == 0 {
        push_line(
            out,
            "- No explicit verification commands detected in selected window.",
        );
    }
    push_line(out, "");
}

fn append_open_items(out: &mut String, messages: &[ChatMessage], mode: SummaryMode) {
    push_line(
        out,
        if mode == SummaryMode::Handoff {
            "## Known Open Items / Next Exact Steps"
        } else {
            "## Open Items / Next Steps"
        },
    );
    let needles = [
        "next",
        "remaining",
        "todo",
        "not yet",
        "later",
        "blocked",
        "future",
        "uncommitted",
        "need to",
        "should",
    ];
    let mut emitted = 0usize;
    for message in messages.iter().rev() {
        let lower = message.content.to_ascii_lowercase();
        if needles.iter().any(|needle| lower.contains(needle)) {
            push_line(
                out,
                &format!("- {}: {}", message.role, one_line(&message.content, 240)),
            );
            emitted += 1;
            if emitted >= 10 {
                break;
            }
        }
    }
    if emitted == 0 {
        push_line(
            out,
            "- No explicit open items detected. Review recent conversation before assuming complete closure.",
        );
    }
    push_line(out, "");
}

fn append_handoff_tail(out: &mut String) {
    push_line(out, "## Handoff Notes for Next Agent");
    push_line(
        out,
        "- Treat this summary as a compact orientation, not a substitute for inspecting current files/tests.",
    );
    push_line(
        out,
        "- Preserve unrelated user work; check `git status` before editing.",
    );
    push_line(
        out,
        "- Re-run focused verification before committing or making broad claims.",
    );
    push_line(out, "");
}

fn append_standard_tail(out: &mut String) {
    push_line(out, "## Summary Notes");
    push_line(
        out,
        "- This is a deterministic Vegvisir session summary scaffold generated from current session state, tool/system messages, and local git state.",
    );
    push_line(
        out,
        "- Use `--memory` to store it in CMS or `--file <path>` / `--save` to persist it as a workspace artifact.",
    );
    push_line(out, "");
}

fn append_recent_bullets(out: &mut String, messages: &[&ChatMessage], limit: usize) {
    if messages.is_empty() {
        push_line(out, "- none");
        return;
    }
    let start = messages.len().saturating_sub(limit);
    for message in &messages[start..] {
        push_line(
            out,
            &format!(
                "- {} `{}`: {}",
                message.created_at.format("%H:%M:%S"),
                message.role,
                one_line(&message.content, 220)
            ),
        );
    }
}

fn push_kv(out: &mut String, key: &str, value: &str) {
    push_line(out, &format!("- {key}: {value}"));
}

fn push_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

fn one_line(value: &str, max_chars: usize) -> String {
    let compact = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('`', "'");
    truncate_chars(&compact, max_chars)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut iter = value.chars();
    let mut out = String::new();
    for _ in 0..max_chars {
        let Some(ch) = iter.next() else {
            return out;
        };
        out.push(ch);
    }
    if iter.next().is_some() {
        out.push('…');
    }
    out
}

fn run_git(cwd: &std::path::Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    if !output.status.success() {
        anyhow::bail!(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
