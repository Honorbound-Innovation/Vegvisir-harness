use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            name: None,
            metadata: Map::new(),
        }
    }

    pub fn named(role: Role, content: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            name: Some(name.into()),
            metadata: Map::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentDecision {
    pub thought: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default)]
    pub args: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_answer: Option<String>,
}

impl AgentDecision {
    pub fn final_decision(thought: impl Into<String>, final_answer: impl Into<String>) -> Self {
        Self {
            thought: thought.into(),
            action: None,
            args: Map::new(),
            final_answer: Some(final_answer.into()),
        }
    }

    pub fn is_final(&self) -> bool {
        self.final_answer.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub ok: bool,
    pub content: String,
    #[serde(default)]
    pub data: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Observation {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            ok: true,
            content: content.into(),
            data: Map::new(),
            error: None,
        }
    }

    pub fn err(content: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            ok: false,
            content: content.into(),
            data: Map::new(),
            error: Some(error.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    #[serde(default)]
    pub args: Map<String, Value>,
}
