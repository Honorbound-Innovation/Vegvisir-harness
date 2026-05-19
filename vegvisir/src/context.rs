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

fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}
