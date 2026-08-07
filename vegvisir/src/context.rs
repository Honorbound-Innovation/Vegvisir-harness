use std::collections::BTreeMap;

use crate::types::{Message, Role};

const MAX_CONTEXT_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_CONTEXT_SUMMARY_BYTES: usize = 256 * 1024;
const MAX_COMPACTED_SUMMARIES: usize = 32;
const MAX_PENDING_COMPACTIONS: usize = 8;
const MAX_COMPACTED_VALUE_BYTES: usize = 64 * 1024;
const CONTEXT_TRUNCATION_MARKER: &str = "\n[context content truncated by Vegvisir memory bound]\n";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ContextManager {
    pub max_messages: usize,
    /// Legacy/plain text active summary. New compactions rebuild this from structured summaries, but
    /// old checkpoints that only have this field remain readable.
    pub summary: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub compacted_summaries: Vec<ContextCompactionSummary>,
    #[serde(default, skip)]
    pending_compactions: Vec<ContextCompactionSummary>,
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new(24)
    }
}

impl ContextManager {
    pub fn new(max_messages: usize) -> Self {
        Self {
            max_messages,
            summary: String::new(),
            messages: Vec::new(),
            compacted_summaries: Vec::new(),
            pending_compactions: Vec::new(),
        }
    }

    pub fn add(&mut self, message: Message) {
        self.messages.push(bound_message(message));
        self.compact_if_needed();
        self.enforce_limits();
    }

    /// Repair limits after loading an older checkpoint and keep active context
    /// proportional to the current run rather than the amount of output a tool
    /// happened to produce.
    pub fn enforce_limits(&mut self) {
        for message in &mut self.messages {
            bound_message_in_place(message);
        }
        while self.messages.len() > self.max_messages.max(1) {
            self.compact_if_needed();
        }
        if self.compacted_summaries.len() > MAX_COMPACTED_SUMMARIES {
            let remove = self.compacted_summaries.len() - MAX_COMPACTED_SUMMARIES;
            self.compacted_summaries.drain(..remove);
        }
        if self.pending_compactions.len() > MAX_PENDING_COMPACTIONS {
            let remove = self.pending_compactions.len() - MAX_PENDING_COMPACTIONS;
            self.pending_compactions.drain(..remove);
        }
        self.rebuild_summary_text();
    }

    pub fn visible_messages(&self) -> Vec<Message> {
        let summary = bounded_text(
            &self.visible_summary(),
            MAX_CONTEXT_MESSAGE_BYTES.saturating_sub(64),
        );
        if summary.trim().is_empty() {
            return self.messages.clone();
        }
        let mut visible = vec![Message::named(
            Role::System,
            format!("Prior context summary:\n{summary}"),
            "context_summary",
        )];
        visible.extend(self.messages.clone());
        visible
    }

    pub fn take_pending_compactions(&mut self) -> Vec<ContextCompactionSummary> {
        std::mem::take(&mut self.pending_compactions)
    }

    pub fn mark_compaction_persisted(&mut self, sequence: usize, memory_id: impl Into<String>) {
        let memory_id = memory_id.into();
        for summary in &mut self.compacted_summaries {
            if summary.sequence == sequence {
                summary.cms_memory_id = Some(memory_id.clone());
            }
        }
        self.rebuild_summary_text();
    }

    fn visible_summary(&self) -> String {
        if self.compacted_summaries.is_empty() {
            return bounded_text(&self.summary, MAX_CONTEXT_SUMMARY_BYTES);
        }
        let mut summaries = Vec::new();
        let mut used = 0usize;
        for summary in self.compacted_summaries.iter().rev() {
            let rendered = summary.render();
            if rendered.trim().is_empty() {
                continue;
            }
            let separator = usize::from(!summaries.is_empty()) * 2;
            let remaining = MAX_CONTEXT_SUMMARY_BYTES.saturating_sub(used + separator);
            if remaining == 0 {
                break;
            }
            let rendered = bounded_text(&rendered, remaining);
            used = used.saturating_add(separator + rendered.len());
            summaries.push(rendered);
            if used >= MAX_CONTEXT_SUMMARY_BYTES {
                break;
            }
        }
        summaries.reverse();
        summaries.join("\n\n")
    }

    fn compact_if_needed(&mut self) {
        if self.messages.len() <= self.max_messages.max(1) {
            return;
        }
        let keep = (self.max_messages.max(1) / 2).max(1);
        let stale: Vec<_> = self.messages.drain(..self.messages.len() - keep).collect();
        let sequence = self.compacted_summaries.len() + 1;
        let summary = ContextCompactionSummary::from_messages(sequence, &stale);
        self.pending_compactions.push(summary.clone());
        self.compacted_summaries.push(summary);
        self.rebuild_summary_text();
    }

    fn rebuild_summary_text(&mut self) {
        // `summary` is retained for checkpoints written by older versions. Do
        // not also keep a full rendered copy once structured summaries exist.
        if self.compacted_summaries.is_empty() {
            self.summary = bounded_text(&self.summary, MAX_CONTEXT_SUMMARY_BYTES);
        } else {
            self.summary.clear();
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ContextCompactionSummary {
    pub sequence: usize,
    pub message_count: usize,
    pub role_counts: BTreeMap<String, usize>,
    pub decisions: Vec<String>,
    pub files_touched: Vec<String>,
    pub commands_run: Vec<String>,
    pub failures: Vec<String>,
    pub open_questions: Vec<String>,
    pub verification: Vec<String>,
    pub follow_up: Vec<String>,
    pub digest: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cms_memory_id: Option<String>,
}

impl ContextCompactionSummary {
    pub fn from_messages(sequence: usize, messages: &[Message]) -> Self {
        let mut role_counts = BTreeMap::new();
        let mut decisions = Vec::new();
        let mut files_touched = Vec::new();
        let mut commands_run = Vec::new();
        let mut failures = Vec::new();
        let mut open_questions = Vec::new();
        let mut verification = Vec::new();
        let mut follow_up = Vec::new();
        let mut digest = Vec::new();

        for message in messages {
            let role = role_label(&message.role).to_string();
            *role_counts.entry(role.clone()).or_insert(0) += 1;
            let clean = compact_whitespace(&message.content);
            if clean.is_empty() {
                continue;
            }

            push_unique_limited(
                &mut digest,
                format!("{role}: {}", truncate_chars(&clean, 260)),
                20,
            );
            collect_path_like_tokens(&clean, &mut files_touched);
            collect_command_lines(&clean, message.name.as_deref(), &mut commands_run);

            let lower = clean.to_ascii_lowercase();
            if matches!(message.role, Role::Assistant)
                && contains_any(
                    &lower,
                    &[
                        "decided",
                        "decision",
                        "implemented",
                        "changed",
                        "fixed",
                        "chose",
                    ],
                )
            {
                push_unique_limited(&mut decisions, truncate_chars(&clean, 220), 10);
            }
            if contains_any(
                &lower,
                &[
                    "error", "failed", "failure", "panic", "timeout", "denied", "nonzero",
                    "blocked",
                ],
            ) {
                push_unique_limited(&mut failures, truncate_chars(&clean, 220), 10);
            }
            if contains_any(
                &lower,
                &[
                    "cargo test",
                    "cargo check",
                    "pytest",
                    "npm test",
                    "verification",
                    "passed",
                    "failing test",
                    "test failure",
                ],
            ) {
                push_unique_limited(&mut verification, truncate_chars(&clean, 220), 10);
            }
            if contains_any(
                &lower,
                &["todo", "follow-up", "follow up", "next step", "remaining"],
            ) {
                push_unique_limited(&mut follow_up, truncate_chars(&clean, 220), 10);
            }
            if clean.contains('?') || contains_any(&lower, &["open question", "unclear", "unknown"])
            {
                push_unique_limited(&mut open_questions, truncate_chars(&clean, 220), 10);
            }
        }

        Self {
            sequence,
            message_count: messages.len(),
            role_counts,
            decisions,
            files_touched,
            commands_run,
            failures,
            open_questions,
            verification,
            follow_up,
            digest,
            cms_memory_id: None,
        }
    }

    pub fn render(&self) -> String {
        let mut out = Vec::new();
        out.push(format!("# Compacted context summary {}", self.sequence));
        out.push(format!("Messages compacted: {}", self.message_count));
        if !self.role_counts.is_empty() {
            out.push(format!(
                "Role counts: {}",
                self.role_counts
                    .iter()
                    .map(|(role, count)| format!("{role}={count}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if let Some(memory_id) = &self.cms_memory_id {
            out.push(format!("CMS memory: {memory_id}"));
        }
        push_section(&mut out, "Decisions / Outcomes", &self.decisions);
        push_section(&mut out, "Files Touched", &self.files_touched);
        push_section(&mut out, "Commands Run", &self.commands_run);
        push_section(&mut out, "Failures / Blockers", &self.failures);
        push_section(&mut out, "Open Questions", &self.open_questions);
        push_section(&mut out, "Verification", &self.verification);
        push_section(&mut out, "Follow-Up", &self.follow_up);
        push_section(&mut out, "Message Digest", &self.digest);
        out.join("\n")
    }
}

/// Default policy for evaluating active-context pressure.
///
/// ECM still owns active context exposure and CMS is not mutated by evaluating this policy.
/// Provider send paths can use the decision as an enforceable gate: warn below soft limits,
/// compact/repair near budget, and block sends that remain over the blocking threshold.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ContextBudgetPolicy {
    pub warning_percent: f64,
    pub compaction_percent: f64,
    pub block_percent: f64,
}

impl Default for ContextBudgetPolicy {
    fn default() -> Self {
        Self {
            warning_percent: 60.0,
            compaction_percent: 80.0,
            block_percent: 95.0,
        }
    }
}

impl ContextBudgetPolicy {
    pub fn normalized(mut self) -> Self {
        self.warning_percent = normalize_percent(self.warning_percent, 60.0);
        self.compaction_percent = normalize_percent(self.compaction_percent, 80.0);
        self.block_percent = normalize_percent(self.block_percent, 95.0);
        if self.compaction_percent < self.warning_percent {
            self.compaction_percent = self.warning_percent;
        }
        if self.block_percent < self.compaction_percent {
            self.block_percent = self.compaction_percent;
        }
        self
    }

    pub fn evaluate(&self, used_tokens: usize, max_tokens: usize) -> ContextBudgetDecision {
        let policy = self.clone().normalized();
        if max_tokens == 0 {
            return ContextBudgetDecision {
                action: ContextBudgetAction::Warn,
                percentage: 0.0,
                remaining_tokens: None,
                overflow_tokens: 0,
                warnings: vec!["session context limit is unknown".to_string()],
            };
        }

        let percentage = (used_tokens as f64 / max_tokens as f64) * 100.0;
        let remaining_tokens = Some(max_tokens.saturating_sub(used_tokens));
        let overflow_tokens = used_tokens.saturating_sub(max_tokens);
        let (action, warning) = if used_tokens > max_tokens || percentage >= policy.block_percent {
            (
                ContextBudgetAction::Block,
                if overflow_tokens > 0 {
                    format!(
                        "context exceeds the model limit by {overflow_tokens} token(s); block or compact before sending"
                    )
                } else {
                    "context usage is at or above the blocking threshold".to_string()
                },
            )
        } else if percentage >= policy.compaction_percent {
            (
                ContextBudgetAction::CompactRecommended,
                "context usage is above the compaction threshold".to_string(),
            )
        } else if percentage >= policy.warning_percent {
            (
                ContextBudgetAction::Warn,
                "context usage is above the warning threshold".to_string(),
            )
        } else {
            (ContextBudgetAction::Ok, String::new())
        };

        let warnings = if warning.is_empty() {
            Vec::new()
        } else {
            vec![warning]
        };
        ContextBudgetDecision {
            action,
            percentage,
            remaining_tokens,
            overflow_tokens,
            warnings,
        }
    }
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextBudgetAction {
    Ok,
    Warn,
    CompactRecommended,
    Block,
}

impl ContextBudgetAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::CompactRecommended => "compact_recommended",
            Self::Block => "block",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ContextBudgetDecision {
    pub action: ContextBudgetAction,
    pub percentage: f64,
    pub remaining_tokens: Option<usize>,
    pub overflow_tokens: usize,
    pub warnings: Vec<String>,
}

fn push_section(out: &mut Vec<String>, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push(format!("## {title}"));
    for item in items {
        out.push(format!("- {item}"));
    }
}

fn collect_path_like_tokens(content: &str, out: &mut Vec<String>) {
    for token in content.split_whitespace() {
        let candidate = token.trim_matches(|ch: char| {
            matches!(
                ch,
                '`' | '\'' | '"' | ',' | ';' | ':' | ')' | '(' | '[' | ']' | '{' | '}'
            )
        });
        if !looks_like_path(candidate) {
            continue;
        }
        push_unique_limited(out, candidate.to_string(), 16);
    }
}

fn collect_command_lines(content: &str, tool_name: Option<&str>, out: &mut Vec<String>) {
    if let Some(name) = tool_name
        && matches!(name, "run_command" | "run_tests" | "run_privileged_command")
    {
        push_unique_limited(
            out,
            format!("tool:{name} {}", truncate_chars(content, 180)),
            12,
        );
    }
    for line in content.lines() {
        let trimmed = line.trim().trim_start_matches('$').trim();
        if starts_with_command(trimmed) {
            push_unique_limited(out, truncate_chars(trimmed, 180), 12);
        }
    }
}

fn looks_like_path(candidate: &str) -> bool {
    if candidate.len() < 3 || candidate.contains("//") || candidate.contains('@') {
        return false;
    }
    let has_separator = candidate.contains('/');
    let has_known_extension = [
        ".rs", ".toml", ".md", ".json", ".yaml", ".yml", ".ts", ".js", ".py", ".java", ".cpp",
        ".c", ".h", ".cs", ".sh",
    ]
    .iter()
    .any(|suffix| candidate.ends_with(suffix));
    has_separator || has_known_extension
}

fn starts_with_command(line: &str) -> bool {
    [
        "cargo ", "git ", "npm ", "pnpm ", "yarn ", "python ", "python3 ", "pytest ", "rg ",
        "sed ", "grep ", "bash ", "sh ", "./",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn push_unique_limited(out: &mut Vec<String>, value: String, limit: usize) {
    let value = bounded_text(&compact_whitespace(&value), 2_048);
    if value.is_empty() || out.iter().any(|existing| existing == &value) {
        return;
    }
    if out.len() < limit {
        out.push(value);
    }
}

fn compact_whitespace(value: &str) -> String {
    let mut compact = String::new();
    let mut first = true;
    let mut truncated = false;
    for word in value.split_whitespace() {
        let separator = usize::from(!first);
        if compact
            .len()
            .saturating_add(separator)
            .saturating_add(word.len())
            > MAX_COMPACTED_VALUE_BYTES
        {
            truncated = true;
            break;
        }
        if !first {
            compact.push(' ');
        }
        compact.push_str(word);
        first = false;
    }
    if truncated {
        compact = bounded_text(&compact, MAX_COMPACTED_VALUE_BYTES.saturating_sub(32));
        compact.push_str(" …[truncated]");
    }
    compact
}

fn role_label(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn normalize_percent(value: f64, default: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        default
    }
}

fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn bound_message(mut message: Message) -> Message {
    bound_message_in_place(&mut message);
    message
}

fn bound_message_in_place(message: &mut Message) {
    message.content = bounded_text(&message.content, MAX_CONTEXT_MESSAGE_BYTES);
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    if max_bytes <= CONTEXT_TRUNCATION_MARKER.len() {
        return utf8_prefix(value, max_bytes);
    }
    let content_bytes = max_bytes - CONTEXT_TRUNCATION_MARKER.len();
    let head_bytes = content_bytes / 2;
    let tail_bytes = content_bytes.saturating_sub(head_bytes);
    let head_end = safe_char_boundary_at_or_before(value, head_bytes);
    let tail_start = safe_char_boundary_at_or_after(value, value.len().saturating_sub(tail_bytes));
    format!(
        "{}{}{}",
        &value[..head_end],
        CONTEXT_TRUNCATION_MARKER,
        &value[tail_start..]
    )
}

fn utf8_prefix(value: &str, max_bytes: usize) -> String {
    let mut prefix = String::new();
    for ch in value.chars() {
        if prefix.len().saturating_add(ch.len_utf8()) > max_bytes {
            break;
        }
        prefix.push(ch);
    }
    prefix
}

fn safe_char_boundary_at_or_before(value: &str, index: usize) -> usize {
    let mut index = index.min(value.len());
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn safe_char_boundary_at_or_after(value: &str, index: usize) -> usize {
    let mut index = index.min(value.len());
    while index < value.len() && !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compaction_creates_structured_summary_and_pending_cms_record() {
        let mut context = ContextManager::new(4);
        context.add(Message::new(
            Role::User,
            "Please inspect vegvisir/src/context.rs and run cargo test -p vegvisir-rust context",
        ));
        context.add(Message::new(
            Role::Assistant,
            "Implemented structured compaction and decided to keep newest context.",
        ));
        context.add(Message::named(
            Role::Tool,
            "cargo test -p vegvisir-rust context failed with error E0425",
            "run_command",
        ));
        context.add(Message::new(Role::User, "What remains as follow-up?"));
        context.add(Message::new(
            Role::Assistant,
            "Next step is persisting the summary to CMS.",
        ));

        assert_eq!(context.compacted_summaries.len(), 1);
        let summary = &context.compacted_summaries[0];
        assert_eq!(summary.message_count, 3);
        assert!(
            summary
                .files_touched
                .iter()
                .any(|path| path == "vegvisir/src/context.rs")
        );
        assert!(
            summary
                .commands_run
                .iter()
                .any(|command| command.contains("cargo test"))
        );
        assert!(
            summary
                .failures
                .iter()
                .any(|failure| failure.contains("error E0425"))
        );
        assert!(summary.render().contains("## Message Digest"));

        let pending = context.take_pending_compactions();
        assert_eq!(pending.len(), 1);
        context.mark_compaction_persisted(pending[0].sequence, "mem_context_summary_1");
        assert!(
            context.visible_messages()[0]
                .content
                .contains("mem_context_summary_1")
        );
    }

    #[test]
    fn budget_policy_selects_warning_compact_and_block_actions() {
        let policy = ContextBudgetPolicy::default();
        assert_eq!(policy.evaluate(100, 1000).action, ContextBudgetAction::Ok);
        assert_eq!(policy.evaluate(650, 1000).action, ContextBudgetAction::Warn);
        assert_eq!(
            policy.evaluate(850, 1000).action,
            ContextBudgetAction::CompactRecommended
        );
        assert_eq!(
            policy.evaluate(950, 1000).action,
            ContextBudgetAction::Block
        );
        let overflow = policy.evaluate(1200, 1000);
        assert_eq!(overflow.action, ContextBudgetAction::Block);
        assert_eq!(overflow.remaining_tokens, Some(0));
        assert_eq!(overflow.overflow_tokens, 200);
    }

    #[test]
    fn budget_policy_normalizes_threshold_order() {
        let policy = ContextBudgetPolicy {
            warning_percent: 90.0,
            compaction_percent: 50.0,
            block_percent: f64::NAN,
        }
        .normalized();

        assert_eq!(policy.warning_percent, 90.0);
        assert_eq!(policy.compaction_percent, 90.0);
        assert_eq!(policy.block_percent, 95.0);
    }

    #[test]
    fn context_bounds_large_messages_and_summary_history() {
        let mut context = ContextManager::new(2);
        for index in 0..80 {
            context.add(Message::new(
                Role::Tool,
                format!("step-{index} {}", "output ".repeat(100_000)),
            ));
        }

        assert!(
            context
                .messages
                .iter()
                .all(|message| message.content.len() <= MAX_CONTEXT_MESSAGE_BYTES)
        );
        assert!(context.compacted_summaries.len() <= MAX_COMPACTED_SUMMARIES);
        assert!(context.visible_messages().iter().all(|message| {
            message.content.len() <= MAX_CONTEXT_MESSAGE_BYTES.max(MAX_CONTEXT_SUMMARY_BYTES)
        }));
    }

    #[test]
    fn compact_whitespace_does_not_materialize_unbounded_input() {
        let compacted = compact_whitespace(&"word ".repeat(MAX_COMPACTED_VALUE_BYTES * 2));
        assert!(compacted.len() <= MAX_COMPACTED_VALUE_BYTES + 32);
        assert!(compacted.contains("truncated"));
    }
}
