use serde_json::{Map, json};

use crate::{
    tools::ToolExecutor,
    types::{Observation, ToolCall},
};

pub struct VerificationRunner {
    pub executor: ToolExecutor,
}

impl VerificationRunner {
    pub fn run_pytest(&mut self, target: &str) -> Observation {
        let mut args = Map::new();
        args.insert("command".to_string(), json!(["pytest", target]));
        args.insert("timeout".to_string(), json!(120));
        self.executor.execute(ToolCall {
            name: "run_command".to_string(),
            args,
        })
    }
}
