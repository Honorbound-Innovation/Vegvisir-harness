use super::super::*;

impl TuiApplication {
    pub(crate) fn tools_command(&mut self, args: &[String]) -> String {
        if matches!(
            args.first().map(String::as_str),
            Some("max-rounds" | "tool-rounds" | "tool-limit" | "limit")
        ) {
            return self.tool_limit_command(&args[1..]);
        }
        if matches!(
            args.first().map(String::as_str),
            Some("commands" | "command" | "allowed-commands" | "allow-command")
        ) {
            return self.tool_commands_command(&args[1..]);
        }
        if let Some(action) = args.first().map(|arg| arg.as_str()) {
            match action {
                "allow-risky" | "enable-risky" | "deny-risky" | "disable-risky"
                | "require-approval" | "approval" | "no-approval" | "disable-approval"
                    if self
                        .tool_executor
                        .guardrails
                        .policy
                        .bypass_approvals_and_sandbox =>
                {
                    return "Dangerous bypass was enabled at startup and cannot be changed from the TUI.".to_string();
                }
                "allow-risky" | "enable-risky" => {
                    self.risky_tools_enabled = true;
                    self.tool_executor.guardrails.policy.allow_risky_tools = true;
                    self.tool_executor.guardrails.policy.require_human_approval = false;
                    return "Risky tools enabled for this running session.".to_string();
                }
                "deny-risky" | "disable-risky" => {
                    self.risky_tools_enabled = false;
                    self.tool_executor.guardrails.policy.allow_risky_tools = false;
                    return "Risky tools disabled for this running session.".to_string();
                }
                "status" => {
                    let mut commands = self
                        .tool_executor
                        .guardrails
                        .policy
                        .allowed_commands
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>();
                    commands.sort();
                    return format!(
                        "Risky tools: {}\nHuman approval: {}\nDangerous bypass: {}\nPending approvals: {}\nAllowed shell commands: {}",
                        if self.tool_executor.guardrails.policy.allow_risky_tools {
                            "enabled"
                        } else {
                            "disabled"
                        },
                        if self.tool_executor.guardrails.policy.require_human_approval {
                            "required"
                        } else {
                            "not required"
                        },
                        if self
                            .tool_executor
                            .guardrails
                            .policy
                            .bypass_approvals_and_sandbox
                        {
                            "enabled at startup"
                        } else {
                            "disabled"
                        },
                        self.tool_executor.guardrails.approvals.pending_len(),
                        commands.join(", ")
                    );
                }
                "require-approval" | "approval" => {
                    self.tool_executor.guardrails.policy.require_human_approval = true;
                    self.tool_executor.guardrails.policy.allow_risky_tools = false;
                    self.risky_tools_enabled = false;
                    return "Human approval required for risky tools.".to_string();
                }
                "no-approval" | "disable-approval" => {
                    self.tool_executor.guardrails.policy.require_human_approval = false;
                    return "Human approval is no longer required for risky tools.".to_string();
                }
                _ => {}
            }
        }
        let inventory = self
            .session
            .enabled_tools
            .iter()
            .map(|tool| format!("{}: {} - {}", tool.category, tool.name, tool.description))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "{inventory}\nRisky tools: {}\nHuman approval: {}\nDangerous bypass: {}\nPending approvals: {}\nAllowed shell commands: use /tools commands",
            if self.tool_executor.guardrails.policy.allow_risky_tools {
                "enabled"
            } else {
                "disabled"
            },
            if self.tool_executor.guardrails.policy.require_human_approval {
                "required"
            } else {
                "not required"
            },
            if self
                .tool_executor
                .guardrails
                .policy
                .bypass_approvals_and_sandbox
            {
                "enabled at startup"
            } else {
                "disabled"
            },
            self.tool_executor.guardrails.approvals.pending_len()
        )
    }

    pub(crate) fn autonomous_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("status") | Some("show") => self.autonomous_status_message(),
            Some("on") | Some("enable") | Some("enabled") | Some("start") => {
                self.autonomous_mode_enabled = true;
                self.logger.emit(
                    "autonomous_mode_enabled",
                    json!({
                        "session": self.session.session_id,
                        "workspace": self.cwd.display().to_string(),
                    }),
                );
                format!(
                    "Autonomous working mode enabled for this running TUI session.\n\n{}",
                    autonomous_mode_summary()
                )
            }
            Some("off") | Some("disable") | Some("disabled") | Some("stop") => {
                self.autonomous_mode_enabled = false;
                self.logger.emit(
                    "autonomous_mode_disabled",
                    json!({
                        "session": self.session.session_id,
                        "workspace": self.cwd.display().to_string(),
                    }),
                );
                "Autonomous working mode disabled. Vegvisir is back in normal interactive mode."
                    .to_string()
            }
            Some(_) => "Usage: /auto [status|on|off]".to_string(),
        }
    }

    fn autonomous_status_message(&self) -> String {
        format!(
            "Autonomous working mode: {}\n\n{}\n\nNotes:\n- This is not Vegvisir's default mode.\n- It affects new model turns while enabled.\n- Approval and sandbox policy still apply unless dangerous bypass was selected at startup.",
            if self.autonomous_mode_enabled {
                "enabled"
            } else {
                "disabled"
            },
            autonomous_mode_summary()
        )
    }

    fn tool_commands_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("list") | Some("show") | Some("status") => {
                let mut commands = self
                    .tool_executor
                    .guardrails
                    .policy
                    .allowed_commands
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>();
                commands.sort();
                format!(
                    "Allowed shell commands:\n{}\nUsage: /tools commands add <cmd...> | remove <cmd...> | reset",
                    commands.join(", ")
                )
            }
            Some("add") | Some("allow") => {
                if args.len() < 2 {
                    return "Usage: /tools commands add <cmd...>".to_string();
                }
                let mut added = Vec::new();
                let mut rejected = Vec::new();
                for command in args.iter().skip(1) {
                    match normalize_command_name(command) {
                        Some(command) => {
                            if self
                                .tool_executor
                                .guardrails
                                .policy
                                .allowed_commands
                                .insert(command.clone())
                            {
                                added.push(command);
                            }
                        }
                        None => rejected.push(command.clone()),
                    }
                }
                tool_command_update_message("Allowed", "added", added, rejected)
            }
            Some("remove") | Some("revoke") | Some("deny") => {
                if args.len() < 2 {
                    return "Usage: /tools commands remove <cmd...>".to_string();
                }
                let mut removed = Vec::new();
                let mut rejected = Vec::new();
                for command in args.iter().skip(1) {
                    match normalize_command_name(command) {
                        Some(command) => {
                            if self
                                .tool_executor
                                .guardrails
                                .policy
                                .allowed_commands
                                .remove(&command)
                            {
                                removed.push(command);
                            }
                        }
                        None => rejected.push(command.clone()),
                    }
                }
                tool_command_update_message("Removed", "removed", removed, rejected)
            }
            Some("reset") | Some("default") => {
                self.tool_executor.guardrails.policy.allowed_commands = default_allowed_commands();
                "Allowed shell commands reset to Vegvisir defaults.".to_string()
            }
            Some(_) => {
                "Usage: /tools commands [list|add <cmd...>|remove <cmd...>|reset]".to_string()
            }
        }
    }

    pub(crate) fn tool_limit_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("show") | Some("status") => format!(
                "Max tool-call rounds per turn: {}
Default: unlimited
Usage: /tool-limit <rounds>|unlimited",
                configured_max_tool_rounds_label(),
            ),
            Some("default") | Some("reset") | Some("clear") | Some("unlimited") | Some("none") => {
                let effective = set_runtime_max_tool_rounds(None)
                    .map(|rounds| rounds.to_string())
                    .unwrap_or_else(|| "unlimited".to_string());
                format!("Max tool-call rounds reset to {effective}.")
            }
            Some(raw) => match raw.parse::<usize>() {
                Ok(0) => "Tool-call round limit must be at least 1.".to_string(),
                Ok(limit) => {
                    let effective = set_runtime_max_tool_rounds(Some(limit)).unwrap_or(limit);
                    format!(
                        "Max tool-call rounds per turn set to {effective} for this running session."
                    )
                }
                Err(_) => "Usage: /tool-limit <rounds>|unlimited".to_string(),
            },
        }
    }

    pub(crate) fn approvals_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("list") | Some("pending") => {
                let pending = self.tool_executor.guardrails.approvals.pending();
                if pending.is_empty() {
                    return "No pending approvals.".to_string();
                }
                pending
                    .values()
                    .map(|request| {
                        format!(
                            "{}  tool={} risk={} reason={} args={}",
                            request.id,
                            request.tool_name,
                            request.risk_label,
                            request.reason,
                            serde_json::to_string(&request.args).unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Some("show") | Some("view") | Some("inspect") => {
                let Some(id) = args.get(1) else {
                    return "Usage: /approvals show <id>".to_string();
                };
                let pending = self.tool_executor.guardrails.approvals.pending();
                let Some(request) = pending.get(id) else {
                    return format!("Unknown pending approval: {id}");
                };
                serde_json::to_string_pretty(&json!({
                    "id": request.id,
                    "tool": request.tool_name,
                    "risk": request.risk_label,
                    "reason": request.reason,
                    "args": request.args,
                    "actions": {
                        "approve_once": format!("/approvals approve {}", request.id),
                        "approve_for_session": format!("/approvals session {}", request.id),
                        "edit": format!("/approvals edit {} <json-args>", request.id),
                        "deny": format!("/approvals deny {}", request.id),
                    }
                }))
                .unwrap_or_else(|error| format!("Failed to render approval {id}: {error}"))
            }
            Some("approve") | Some("allow") => {
                let Some(id) = args.get(1) else {
                    return "Usage: /approvals approve <id>".to_string();
                };
                match self
                    .tool_executor
                    .guardrails
                    .approvals
                    .approve_once_request(id)
                {
                    Some(request) if self.pending_send.is_some() => {
                        format!(
                            "Approved once: {}. In-flight model run will resume.",
                            request.tool_name
                        )
                    }
                    Some(request) => self.execute_approved_request("Approved once", request),
                    None => format!("Unknown pending approval: {id}"),
                }
            }
            Some("session")
            | Some("approve-session")
            | Some("allow-session")
            | Some("approve-pattern")
            | Some("allow-pattern") => {
                let Some(id) = args.get(1) else {
                    return "Usage: /approvals session <id>".to_string();
                };
                match self
                    .tool_executor
                    .guardrails
                    .approvals
                    .approve_for_session(id)
                {
                    Some(request) => {
                        let mut prefix =
                            "Approved matching call for this running session".to_string();
                        if request.risk_label == "command-allow"
                            && let Some(command) = command_name_from_args(&request.args)
                        {
                            self.tool_executor
                                .guardrails
                                .policy
                                .allowed_commands
                                .insert(command.clone());
                            prefix = format!(
                                "Approved matching call and allowed shell command `{command}` for this running session"
                            );
                        }
                        if self.pending_send.is_some() {
                            format!(
                                "{prefix}: {}. In-flight model run will resume.",
                                request.tool_name
                            )
                        } else {
                            self.execute_approved_request(&prefix, request)
                        }
                    }
                    None => format!("Unknown pending approval: {id}"),
                }
            }
            Some("edit") => {
                let Some(id) = args.get(1) else {
                    return "Usage: /approvals edit <id> <json-args>".to_string();
                };
                let Some(raw_json) = args.get(2) else {
                    return "Usage: /approvals edit <id> <json-args>".to_string();
                };
                let args = match serde_json::from_str::<serde_json::Value>(raw_json) {
                    Ok(serde_json::Value::Object(args)) => args,
                    Ok(_) => return "Approval args must be a JSON object.".to_string(),
                    Err(error) => return format!("Invalid approval args JSON: {error}"),
                };
                match self.tool_executor.guardrails.approvals.edit(id, args) {
                    Some(request) => format!(
                        "Edited approval {id}; new approval id is {} args={}",
                        request.id,
                        serde_json::to_string(&request.args).unwrap_or_default()
                    ),
                    None => format!("Unknown pending approval: {id}"),
                }
            }
            Some("deny") | Some("reject") => {
                let Some(id) = args.get(1) else {
                    return "Usage: /approvals deny <id>".to_string();
                };
                if self.tool_executor.guardrails.approvals.deny(id) {
                    format!("Denied approval {id}.")
                } else {
                    format!("Unknown pending approval: {id}")
                }
            }
            Some(other) => format!("Unknown /approvals command: {other}"),
        }
    }

    pub(crate) fn execute_approved_request(
        &mut self,
        approval_message: &str,
        request: ApprovalRequest,
    ) -> String {
        let tool_name = request.tool_name.clone();
        let observation = self.tool_executor.execute(ToolCall {
            name: request.tool_name,
            args: request.args,
        });
        let status = if observation.ok { "ok" } else { "failed" };
        let body = if observation.content.trim().is_empty() {
            "(no output)".to_string()
        } else {
            observation.content
        };
        format!(
            "{approval_message}: {tool_name}\nTool execution {status}.\n{body}\n\nIf this was part of a model task, send `continue` so the agent can inspect the result and proceed."
        )
    }
}

fn tool_command_update_message(
    action: &str,
    empty_action: &str,
    changed: Vec<String>,
    rejected: Vec<String>,
) -> String {
    let mut lines = Vec::new();
    if !changed.is_empty() {
        lines.push(format!(
            "{action} shell command{} for this running session: {}",
            if changed.len() == 1 { "" } else { "s" },
            changed.join(", ")
        ));
    }
    if !rejected.is_empty() {
        lines.push(format!(
            "Rejected invalid command name{}: {}",
            if rejected.len() == 1 { "" } else { "s" },
            rejected.join(", ")
        ));
    }
    if lines.is_empty() {
        format!("No shell commands were {empty_action}.")
    } else {
        lines.join("\n")
    }
}

fn autonomous_mode_summary() -> &'static str {
    "When enabled, each model turn gets an autonomous-work contract: plan the full workflow, execute available safe steps without waiting for unnecessary chat confirmation, run focused verification, keep progress visible, preserve user work, and stop for approvals, secrets, destructive actions, or unclear scope."
}
