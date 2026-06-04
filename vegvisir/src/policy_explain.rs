use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    guardrails::{ApprovalRequest, PermissionPolicy, command_name_from_args},
    tools::Tool,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyExplanation {
    pub tool: String,
    pub risk: String,
    pub gates: Vec<PolicyGateExplanation>,
    pub decision: String,
    pub next_step: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyGateExplanation {
    pub gate: String,
    pub status: String,
    pub detail: String,
}

impl PolicyExplanation {
    pub fn to_markdown(&self) -> String {
        let mut lines = vec![
            format!("Tool: {}", self.tool),
            format!("Risk: {}", self.risk),
            "Policy gates:".to_string(),
        ];
        for gate in &self.gates {
            lines.push(format!(
                "- {}: {} — {}",
                gate.gate, gate.status, gate.detail
            ));
        }
        lines.push(format!("Decision: {}", self.decision));
        lines.push(format!("Next step: {}", self.next_step));
        lines.join("\n")
    }
}

pub fn explain_pending_approval(
    request: &ApprovalRequest,
    policy: &PermissionPolicy,
) -> PolicyExplanation {
    let mut gates = common_gates(&request.tool_name, Some(&request.args), None, policy);
    gates.push(PolicyGateExplanation {
        gate: "approval ledger".to_string(),
        status: "queued".to_string(),
        detail: format!("approval_id={} reason={}", request.id, request.reason),
    });
    PolicyExplanation {
        tool: request.tool_name.clone(),
        risk: request.risk_label.clone(),
        gates,
        decision: "queued for human approval".to_string(),
        next_step: format!(
            "review `/approvals show {}` then approve once, approve for session, edit, or deny",
            request.id
        ),
    }
}

pub fn explain_tool_call(
    tool: &Tool,
    args: Option<&Map<String, Value>>,
    policy: &PermissionPolicy,
) -> PolicyExplanation {
    let gates = common_gates(&tool.name, args, Some(tool), policy);
    let risk = if tool.risky {
        risk_label(&tool.name).to_string()
    } else {
        "low".to_string()
    };
    let (decision, next_step) = if policy.bypass_approvals_and_sandbox {
        (
            "allowed by startup dangerous bypass".to_string(),
            "use with caution; bypass cannot be enabled from chat and should be visible in /status and artifacts".to_string(),
        )
    } else if policy.denied_tools.contains(&tool.name) {
        (
            "denied".to_string(),
            "remove the tool from denied policy only through trusted configuration".to_string(),
        )
    } else if tool.risky && policy.require_human_approval && !policy.allow_risky_tools {
        (
            "would queue approval".to_string(),
            "run the tool in a model turn or inspect pending /approvals after it is requested"
                .to_string(),
        )
    } else if tool.risky && !policy.allow_risky_tools {
        (
            "denied until risky tools are enabled or approved".to_string(),
            "use /tools allow-risky only if appropriate, or keep approval mode enabled".to_string(),
        )
    } else {
        (
            "allowed by current policy".to_string(),
            "no approval is currently required for this tool under the supplied arguments"
                .to_string(),
        )
    };
    PolicyExplanation {
        tool: tool.name.clone(),
        risk,
        gates,
        decision,
        next_step,
    }
}

fn common_gates(
    tool_name: &str,
    args: Option<&Map<String, Value>>,
    tool: Option<&Tool>,
    policy: &PermissionPolicy,
) -> Vec<PolicyGateExplanation> {
    let mut gates = Vec::new();
    gates.push(PolicyGateExplanation {
        gate: "dangerous bypass".to_string(),
        status: if policy.bypass_approvals_and_sandbox {
            "enabled"
        } else {
            "disabled"
        }
        .to_string(),
        detail: if policy.bypass_approvals_and_sandbox {
            "approval and sandbox checks are bypassed for this startup-selected high-risk session"
                .to_string()
        } else {
            "normal guardrail checks are active".to_string()
        },
    });
    gates.push(PolicyGateExplanation {
        gate: "tool registry".to_string(),
        status: if tool.is_some() {
            "known"
        } else {
            "pending-record"
        }
        .to_string(),
        detail: tool
            .map(|tool| {
                format!(
                    "description={} risky={} timeout_seconds={:?}",
                    tool.description, tool.risky, tool.timeout_seconds
                )
            })
            .unwrap_or_else(|| "explaining a queued approval request".to_string()),
    });
    gates.push(PolicyGateExplanation {
        gate: "denied tools".to_string(),
        status: if policy.denied_tools.contains(tool_name) {
            "deny"
        } else {
            "pass"
        }
        .to_string(),
        detail: if policy.denied_tools.contains(tool_name) {
            format!("{tool_name} is explicitly denied")
        } else {
            "tool is not explicitly denied".to_string()
        },
    });
    if tool_name == "run_command" {
        let executable = args.and_then(command_name_from_args);
        let allowed = executable
            .as_ref()
            .map(|name| policy.allowed_commands.contains(name))
            .unwrap_or(false);
        gates.push(PolicyGateExplanation {
            gate: "command allow-list".to_string(),
            status: if allowed { "pass" } else { "approval-or-deny" }.to_string(),
            detail: executable
                .map(|name| format!("executable `{name}` allowed={allowed}"))
                .unwrap_or_else(|| "no executable argument supplied".to_string()),
        });
    } else {
        gates.push(PolicyGateExplanation {
            gate: "command allow-list".to_string(),
            status: "n/a".to_string(),
            detail: "not a shell command tool".to_string(),
        });
    }
    gates.push(PolicyGateExplanation {
        gate: "human approval".to_string(),
        status: if policy.require_human_approval {
            "required-for-risky"
        } else {
            "not-required"
        }
        .to_string(),
        detail: format!(
            "allow_risky_tools={} require_human_approval={}",
            policy.allow_risky_tools, policy.require_human_approval
        ),
    });
    gates.push(PolicyGateExplanation {
        gate: "HBSE secret boundary".to_string(),
        status: "ref-only".to_string(),
        detail: "policy explanations record metadata and never plaintext secrets".to_string(),
    });
    gates.push(PolicyGateExplanation {
        gate: "USRL/runtime policy".to_string(),
        status: "checked-by-runtime".to_string(),
        detail: "RuntimePolicy authorizes tool calls after guardrail routing when active contracts exist".to_string(),
    });
    gates
}

fn risk_label(tool_name: &str) -> &'static str {
    match tool_name {
        "run_command" => "command-execution",
        "write_file" => "filesystem-write",
        name if name.starts_with("mcp::") => "external-tool",
        _ => "risky-tool",
    }
}

pub fn explanation_json(explanation: &PolicyExplanation) -> Value {
    json!(explanation)
}
