use serde_json::{Map, Value};

use crate::types::{AgentDecision, Message};

pub trait Model: Send {
    fn decide(&mut self, messages: &[Message], tools: &[Value]) -> AgentDecision;
}

pub struct ScriptedModel {
    decisions: Vec<AgentDecision>,
    index: usize,
}

impl ScriptedModel {
    pub fn new(decisions: Vec<AgentDecision>) -> Self {
        Self {
            decisions,
            index: 0,
        }
    }

    pub fn from_json(values: Vec<Value>) -> anyhow::Result<Self> {
        let decisions = values
            .into_iter()
            .map(|mut value| {
                if let Value::Object(ref mut object) = value {
                    if let Some(final_value) = object.remove("final") {
                        object.insert("final_answer".to_string(), final_value);
                    }
                }
                serde_json::from_value(value).map_err(Into::into)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self::new(decisions))
    }
}

impl Default for ScriptedModel {
    fn default() -> Self {
        let mut args = Map::new();
        args.insert("path".to_string(), Value::String(".".to_string()));
        Self::new(vec![
            AgentDecision {
                thought: "I need workspace evidence.".to_string(),
                action: Some("list_files".to_string()),
                args,
                final_answer: None,
            },
            AgentDecision::final_decision(
                "I can now summarize the result.",
                "Vegvisir completed the scripted inspection.",
            ),
        ])
    }
}

impl Model for ScriptedModel {
    fn decide(&mut self, _messages: &[Message], _tools: &[Value]) -> AgentDecision {
        if self.index >= self.decisions.len() {
            return AgentDecision::final_decision(
                "No scripted decisions remain.",
                "Stopped because the scripted model has no more decisions.",
            );
        }
        let decision = self.decisions[self.index].clone();
        self.index += 1;
        decision
    }
}
