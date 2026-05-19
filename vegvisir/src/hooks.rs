use crate::{
    state::RunState,
    types::{AgentDecision, Message, Observation, ToolCall},
};

pub trait Hook: Send {
    fn before_model(&mut self, _state: &RunState, messages: Vec<Message>) -> Vec<Message> {
        messages
    }
    fn after_model(&mut self, _state: &RunState, decision: AgentDecision) -> AgentDecision {
        decision
    }
    fn before_tool(&mut self, _state: &RunState, call: ToolCall) -> ToolCall {
        call
    }
    fn after_tool(
        &mut self,
        _state: &RunState,
        _call: &ToolCall,
        observation: Observation,
    ) -> Observation {
        observation
    }
}

#[derive(Default)]
pub struct HookManager {
    hooks: Vec<Box<dyn Hook>>,
}

impl HookManager {
    pub fn new(hooks: Vec<Box<dyn Hook>>) -> Self {
        Self { hooks }
    }

    pub fn before_model(&mut self, state: &RunState, mut messages: Vec<Message>) -> Vec<Message> {
        for hook in &mut self.hooks {
            messages = hook.before_model(state, messages);
        }
        messages
    }

    pub fn after_model(&mut self, state: &RunState, mut decision: AgentDecision) -> AgentDecision {
        for hook in &mut self.hooks {
            decision = hook.after_model(state, decision);
        }
        decision
    }

    pub fn before_tool(&mut self, state: &RunState, mut call: ToolCall) -> ToolCall {
        for hook in &mut self.hooks {
            call = hook.before_tool(state, call);
        }
        call
    }

    pub fn after_tool(
        &mut self,
        state: &RunState,
        call: &ToolCall,
        mut observation: Observation,
    ) -> Observation {
        for hook in &mut self.hooks {
            observation = hook.after_tool(state, call, observation);
        }
        observation
    }
}
