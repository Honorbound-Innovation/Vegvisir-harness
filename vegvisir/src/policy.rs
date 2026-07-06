use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::observability::EventLogger;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuntimePolicy {
    pub active_agent_id: Option<String>,
    pub active_agent_mode: Option<String>,
    pub allowed_tools: Vec<String>,
    pub usrl_contracts: Vec<String>,
    #[serde(default)]
    pub usrl_rules: Vec<String>,
    #[serde(default)]
    pub usrl_constraints: Vec<String>,
    #[serde(default)]
    pub usrl_stages: Vec<String>,
    #[serde(default)]
    pub usrl_triggers: Vec<String>,
    pub strict_usrl: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeGateRequest {
    pub operation: String,
    pub target: String,
    pub args_summary: Value,
    #[serde(default)]
    pub tool_metadata: Option<RuntimeToolMetadata>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuntimeToolMetadata {
    pub risky: bool,
    #[serde(default)]
    pub safety_labels: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeGateDecision {
    pub allowed: bool,
    pub reason: String,
    pub contract_id: Option<String>,
    pub audit_metadata: Value,
}

impl RuntimePolicy {
    pub fn gate(&self, request: RuntimeGateRequest) -> RuntimeGateDecision {
        let contract_id = self.usrl_contracts.first().cloned();
        if !self.allowed_tools.is_empty() && !tool_allowed(&self.allowed_tools, &request.target) {
            return RuntimeGateDecision {
                allowed: false,
                reason: format!("tool is not enabled for active agent: {}", request.target),
                contract_id,
                audit_metadata: json!({
                    "agent_id": self.active_agent_id,
                    "agent_mode": self.active_agent_mode,
                    "operation": request.operation,
                    "target": request.target,
                    "args_summary": request.args_summary,
                    "tool_metadata": request.tool_metadata,
                    "strict_usrl": self.strict_usrl,
                    "allowed_tools": self.allowed_tools,
                }),
            };
        }
        let risky = request
            .tool_metadata
            .as_ref()
            .map(|metadata| metadata.risky)
            .unwrap_or_else(|| legacy_risky_operation(&request.operation));
        if self.strict_usrl && risky && contract_id.is_none() {
            return RuntimeGateDecision {
                allowed: false,
                reason: "risky operation requires an active USRL contract".to_string(),
                contract_id,
                audit_metadata: self.audit_metadata(
                    &request,
                    json!({
                        "decision": "missing_contract",
                    }),
                ),
            };
        }
        if risky
            && contract_id.is_some()
            && let Some(denial) = self.evaluate_usrl_constraints(&request)
        {
            return RuntimeGateDecision {
                allowed: false,
                reason: denial,
                contract_id,
                audit_metadata: self.audit_metadata(
                    &request,
                    json!({
                        "decision": "contract_denied",
                        "usrl_rules": self.usrl_rules,
                        "usrl_constraints": self.usrl_constraints,
                        "usrl_stages": self.usrl_stages,
                        "usrl_triggers": self.usrl_triggers,
                    }),
                ),
            };
        }
        let allowed = !self.strict_usrl || !risky || contract_id.is_some();
        RuntimeGateDecision {
            allowed,
            reason: if allowed {
                if let Some(contract_id) = &contract_id {
                    format!("allowed by USRL contract {contract_id}; constraints evaluated")
                } else {
                    "allowed by default runtime policy".to_string()
                }
            } else {
                "risky operation requires an active USRL contract".to_string()
            },
            contract_id,
            audit_metadata: self.audit_metadata(
                &request,
                json!({
                    "decision": "allowed",
                    "usrl_rules": self.usrl_rules,
                    "usrl_constraints": self.usrl_constraints,
                    "usrl_stages": self.usrl_stages,
                    "usrl_triggers": self.usrl_triggers,
                }),
            ),
        }
    }

    fn audit_metadata(&self, request: &RuntimeGateRequest, extra: Value) -> Value {
        json!({
            "agent_id": self.active_agent_id,
            "agent_mode": self.active_agent_mode,
            "operation": request.operation,
            "target": request.target,
            "args_summary": request.args_summary,
            "tool_metadata": request.tool_metadata,
            "strict_usrl": self.strict_usrl,
            "usrl_contracts": self.usrl_contracts,
            "extra": extra,
        })
    }

    fn evaluate_usrl_constraints(&self, request: &RuntimeGateRequest) -> Option<String> {
        let lowered = self
            .usrl_constraints
            .iter()
            .chain(self.usrl_rules.iter())
            .chain(self.usrl_stages.iter())
            .chain(self.usrl_triggers.iter())
            .map(|item| item.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let has = |needles: &[&str]| {
            lowered
                .iter()
                .any(|item| needles.iter().any(|needle| item.contains(needle)))
        };
        if has(&[
            "no_secret",
            "no secrets",
            "secret_output",
            "no_secret_output",
        ]) && value_contains_secret(&request.args_summary)
        {
            return Some("USRL contract denied operation: secret-like argument violates no-secret constraint".to_string());
        }
        if has(&["require_stage", "stage_required", "staged_execution"]) {
            let Some(stage) = string_field(&request.args_summary, &["usrl_stage", "stage"]) else {
                return Some(
                    "USRL contract denied operation: stage evidence is required".to_string(),
                );
            };
            if !self.usrl_stages.is_empty()
                && !self
                    .usrl_stages
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(stage))
            {
                return Some(format!(
                    "USRL contract denied operation: stage `{stage}` is not in contract stages"
                ));
            }
        }
        if has(&["require_evidence", "evidence_required", "required_evidence"])
            && string_field(
                &request.args_summary,
                &["evidence", "justification", "reason", "verification"],
            )
            .is_none()
        {
            return Some(
                "USRL contract denied operation: evidence or justification is required".to_string(),
            );
        }
        if matches!(
            request.operation.as_str(),
            "run_command" | "run_privileged_command"
        ) && has(&["no_command", "no_shell", "no_exec"])
        {
            return Some(
                "USRL contract denied operation: command execution is forbidden".to_string(),
            );
        }
        if request.operation == "write_file" && has(&["read_only", "no_write", "no_modify"]) {
            return Some("USRL contract denied operation: writes are forbidden".to_string());
        }
        if request.operation.starts_with("mcp::") && has(&["no_external", "no_network", "no_mcp"]) {
            return Some(
                "USRL contract denied operation: external tool access is forbidden".to_string(),
            );
        }
        None
    }

    pub fn authorize_tool(
        &self,
        tool_name: &str,
        args: &Map<String, Value>,
        logger: &EventLogger,
    ) -> Result<RuntimeGateDecision, String> {
        self.authorize_tool_with_metadata(
            tool_name,
            args,
            RuntimeToolMetadata {
                risky: legacy_risky_operation(tool_name),
                safety_labels: Vec::new(),
            },
            logger,
        )
    }

    pub fn authorize_tool_with_metadata(
        &self,
        tool_name: &str,
        args: &Map<String, Value>,
        metadata: RuntimeToolMetadata,
        logger: &EventLogger,
    ) -> Result<RuntimeGateDecision, String> {
        let decision = self.gate(RuntimeGateRequest {
            operation: tool_name.to_string(),
            target: tool_name.to_string(),
            args_summary: summarize_args(args),
            tool_metadata: Some(metadata),
        });
        logger.emit("runtime_gate", json!(decision));
        if decision.allowed {
            Ok(decision)
        } else {
            Err(decision.reason.clone())
        }
    }
}

fn legacy_risky_operation(operation: &str) -> bool {
    matches!(
        operation,
        "write_file"
            | "run_command"
            | "run_privileged_command"
            | "mcp_tool_call"
            | "spawn_subagent"
            | "cms_writeback"
    )
}

fn string_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    let object = value.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
}

fn value_contains_secret(value: &Value) -> bool {
    match value {
        Value::String(text) => {
            let text = text.to_ascii_lowercase();
            [
                "api_key=",
                "apikey=",
                "access_token=",
                "auth_token=",
                "bearer ",
                "password=",
                "secret=",
                "token=",
                "sk-",
            ]
            .iter()
            .any(|needle| text.contains(needle))
        }
        Value::Array(items) => items.iter().any(value_contains_secret),
        Value::Object(map) => map.values().any(value_contains_secret),
        _ => false,
    }
}

fn tool_allowed(allowed_tools: &[String], target: &str) -> bool {
    allowed_tools.iter().any(|allowed| {
        allowed == "*" || allowed == target || target.starts_with(&format!("{allowed}::"))
    })
}

fn summarize_args(args: &Map<String, Value>) -> Value {
    let mut summary = Map::new();
    for (key, value) in args {
        let summarized = match value {
            Value::String(text) if text.len() > 160 => {
                Value::String(format!("{}...", text.chars().take(160).collect::<String>()))
            }
            Value::Array(items) if items.len() > 8 => {
                Value::String(format!("array(len={})", items.len()))
            }
            Value::Object(map) if map.len() > 8 => {
                Value::String(format!("object(len={})", map.len()))
            }
            other => other.clone(),
        };
        summary.insert(key.clone(), summarized);
    }
    Value::Object(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::EventLogger;

    #[test]
    fn strict_usrl_uses_tool_metadata_for_unknown_risky_tool_names() {
        let policy = RuntimePolicy {
            strict_usrl: true,
            ..RuntimePolicy::default()
        };
        let args = Map::new();
        let logger = EventLogger::new(None);

        let err = policy
            .authorize_tool_with_metadata(
                "plugin_publish_release",
                &args,
                RuntimeToolMetadata {
                    risky: true,
                    safety_labels: vec!["external_publish".to_string()],
                },
                &logger,
            )
            .unwrap_err();

        assert_eq!(err, "risky operation requires an active USRL contract");
        let events = logger.events();
        let gate = events
            .iter()
            .find(|event| event.name == "runtime_gate")
            .expect("runtime gate event");
        assert_eq!(gate.payload["allowed"], json!(false));
        assert_eq!(
            gate.payload["audit_metadata"]["tool_metadata"]["risky"],
            json!(true)
        );
    }

    #[test]
    fn strict_usrl_allows_unknown_safe_tool_without_contract() {
        let policy = RuntimePolicy {
            strict_usrl: true,
            ..RuntimePolicy::default()
        };
        let args = Map::new();
        let logger = EventLogger::new(None);

        let decision = policy
            .authorize_tool_with_metadata(
                "plugin_read_status",
                &args,
                RuntimeToolMetadata {
                    risky: false,
                    safety_labels: vec!["read_only".to_string()],
                },
                &logger,
            )
            .expect("safe plugin tool should pass strict USRL without a contract");

        assert!(decision.allowed);
        assert_eq!(
            decision.audit_metadata["tool_metadata"]["risky"],
            json!(false)
        );
    }

    #[test]
    fn legacy_authorize_tool_wrapper_preserves_existing_risky_names() {
        let policy = RuntimePolicy {
            strict_usrl: true,
            ..RuntimePolicy::default()
        };
        let logger = EventLogger::new(None);
        let err = policy
            .authorize_tool("run_command", &Map::new(), &logger)
            .unwrap_err();

        assert_eq!(err, "risky operation requires an active USRL contract");
    }
}
