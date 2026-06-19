use super::super::*;

impl TuiApplication {
    pub(crate) fn permissions_command(&mut self, args: &[String]) -> String {
        if wants_json(args) {
            return self.permissions_status_json();
        }
        match args.first().map(String::as_str) {
            None | Some("status" | "show" | "summary" | "policy") => self.permissions_status_text(),
            Some("explain" | "why") => self.permissions_explain_command(&args[1..]),
            Some("pending" | "approval" | "approvals") => {
                self.permissions_pending_command(&args[1..])
            }
            Some("help") => permissions_usage().to_string(),
            Some(other) => format!(
                "Unknown /permissions command: {other}\n{}",
                permissions_usage()
            ),
        }
    }

    fn permissions_explain_command(&self, args: &[String]) -> String {
        let Some(tool_name) = args.first() else {
            return "Usage: /permissions explain <tool-name> [json-args]".to_string();
        };
        let parsed_args = match args.get(1) {
            Some(raw) => match serde_json::from_str::<serde_json::Value>(raw) {
                Ok(serde_json::Value::Object(map)) => Some(map),
                Ok(_) => return "Permission explanation args must be a JSON object.".to_string(),
                Err(error) => return format!("Invalid permission explanation args JSON: {error}"),
            },
            None => None,
        };
        match self.tool_registry.get(tool_name) {
            Ok(tool) => crate::policy_explain::explain_tool_call(
                tool,
                parsed_args.as_ref(),
                &self.tool_executor.guardrails.policy,
            )
            .to_markdown(),
            Err(error) => format!("Unknown tool `{tool_name}`: {error}"),
        }
    }

    fn permissions_pending_command(&self, args: &[String]) -> String {
        let pending = self.tool_executor.guardrails.approvals.pending();
        let Some(id) = args.first() else {
            if pending.is_empty() {
                return "No pending approvals.".to_string();
            }
            let mut lines = vec!["Pending approvals:".to_string()];
            for request in pending.values() {
                lines.push(format!(
                    "  {}  tool={} risk={} reason={}",
                    request.id, request.tool_name, request.risk_label, request.reason
                ));
            }
            lines.push("Use /permissions pending <id> to explain one approval.".to_string());
            return lines.join("\n");
        };
        let Some(request) = pending.get(id) else {
            return format!("Unknown pending approval: {id}");
        };
        crate::policy_explain::explain_pending_approval(
            request,
            &self.tool_executor.guardrails.policy,
        )
        .to_markdown()
    }

    fn permissions_status_text(&self) -> String {
        let policy = &self.tool_executor.guardrails.policy;
        let mut allowed_commands = policy.allowed_commands.iter().cloned().collect::<Vec<_>>();
        allowed_commands.sort();
        let mut denied_tools = policy.denied_tools.iter().cloned().collect::<Vec<_>>();
        denied_tools.sort();
        let pending_ids = self.tool_executor.guardrails.approvals.pending_ids();
        format!(
            "Permission policy:\n  Risky tools: {}\n  Human approval: {}\n  Dangerous bypass: {}\n  Pending approvals: {}\n  Denied tools: {}\n  Allowed shell commands: {}\n\nPolicy gates:\n  - HBSE secret boundary: ref-only; plaintext secrets must not be requested, logged, or serialized.\n  - Command allow-list: run_command checks executable names before execution.\n  - Approval ledger: risky requests can be queued, approved once, approved for session, edited, or denied.\n  - Hard policy: dangerous bypass is startup-only and cannot be enabled from chat.\n\nUsage: /permissions explain <tool-name> [json-args] | pending [approval-id] | --json",
            if policy.allow_risky_tools {
                "enabled"
            } else {
                "disabled"
            },
            if policy.require_human_approval {
                "required for risky tools"
            } else {
                "not required"
            },
            if policy.bypass_approvals_and_sandbox {
                "enabled at startup"
            } else {
                "disabled"
            },
            if pending_ids.is_empty() {
                "none".to_string()
            } else {
                pending_ids.join(", ")
            },
            if denied_tools.is_empty() {
                "none".to_string()
            } else {
                denied_tools.join(", ")
            },
            allowed_commands.join(", ")
        )
    }

    fn permissions_status_json(&self) -> String {
        let policy = &self.tool_executor.guardrails.policy;
        let pending_ids = self.tool_executor.guardrails.approvals.pending_ids();
        serde_json::to_string_pretty(&json!({
            "permission_policy": {
                "allow_risky_tools": policy.allow_risky_tools,
                "require_human_approval": policy.require_human_approval,
                "bypass_approvals_and_sandbox": policy.bypass_approvals_and_sandbox,
                "pending_approval_ids": pending_ids,
                "denied_tools": policy.denied_tools,
                "allowed_commands": policy.allowed_commands,
                "hard_policy": {
                    "dangerous_bypass_startup_only": true,
                    "hbse_secret_boundary": "ref-only"
                }
            }
        }))
        .unwrap_or_else(|error| format!("Failed to serialize permission policy: {error}"))
    }
}

fn permissions_usage() -> &'static str {
    "Usage: /permissions [status|explain <tool-name> [json-args]|pending [approval-id]|--json]"
}
