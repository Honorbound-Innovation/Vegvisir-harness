use crate::{
    state::RunState,
    types::{Message, Role},
};

#[derive(Clone, Debug, Default)]
pub struct PromptAssembler {
    pub system_fragments: Vec<String>,
}

impl PromptAssembler {
    pub fn assemble(&self, state: &RunState, context: Vec<Message>) -> Vec<Message> {
        let progress = state
            .progress
            .iter()
            .map(|item| format!("- {}: {}", item.status, item.description))
            .collect::<Vec<_>>()
            .join("\n");
        let mut fragments = vec![
            "You are Vegvisir, an agentic AI harness runtime.".to_string(),
            "Reason step by step, call tools when evidence is needed, and stop when the goal is satisfied.".to_string(),
            format!("Goal: {}", state.goal),
            format!("Run id: {}", state.run_id),
            if progress.is_empty() {
                "Progress: none recorded".to_string()
            } else {
                format!("Progress:\n{progress}")
            },
        ];
        fragments.extend(self.system_fragments.clone());
        let mut messages = vec![Message::new(Role::System, fragments.join("\n\n"))];
        messages.extend(context);
        messages
    }
}
