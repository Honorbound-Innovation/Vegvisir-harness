use crate::types::{Message, Role};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ContextManager {
    pub max_messages: usize,
    pub summary: String,
    pub messages: Vec<Message>,
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
        }
    }

    pub fn add(&mut self, message: Message) {
        self.messages.push(message);
        self.compact_if_needed();
    }

    pub fn visible_messages(&self) -> Vec<Message> {
        if self.summary.is_empty() {
            return self.messages.clone();
        }
        let mut visible = vec![Message::named(
            Role::System,
            format!("Prior context summary:\n{}", self.summary),
            "context_summary",
        )];
        visible.extend(self.messages.clone());
        visible
    }

    fn compact_if_needed(&mut self) {
        if self.messages.len() <= self.max_messages {
            return;
        }
        let keep = self.max_messages / 2;
        let stale: Vec<_> = self.messages.drain(..self.messages.len() - keep).collect();
        let mut lines = Vec::new();
        if !self.summary.is_empty() {
            lines.push(self.summary.clone());
        }
        for message in stale {
            let role = match message.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };
            lines.push(format!("{role}: {}", truncate_chars(&message.content, 500)));
        }
        self.summary = lines
            .into_iter()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
}

/// Default policy for reporting active-context pressure.
///
/// This is deliberately advisory: ECM still owns active context exposure and CMS is not mutated by
/// evaluating the policy. Callers can use the decision to warn, recommend compaction, or block a
/// future send path once that behavior is intentionally wired behind a feature flag.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
