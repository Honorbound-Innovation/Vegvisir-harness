use super::super::*;

fn command_available(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn command_output(mut command: std::process::Command, label: &str) -> anyhow::Result<String> {
    let output = command.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(
            "{label} failed{}",
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }
    Ok(strip_ansi(&String::from_utf8_lossy(&output.stdout)))
}

fn strip_ansi(text: &str) -> String {
    static ANSI_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    ANSI_RE
        .get_or_init(|| regex::Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("valid ansi regex"))
        .replace_all(text, "")
        .to_string()
}

impl TuiApplication {
    pub(crate) fn session_status_command(&mut self, _args: &[String]) -> String {
        let body = self.session_status_report();
        self.info_scroll_offset = 0;
        self.info_overlay = Some(InfoOverlay {
            title: "session status".to_string(),
            body: body.clone(),
        });
        body
    }

    fn session_status_report(&self) -> String {
        let message_count = self.session.messages.len();
        let user_messages = self
            .session
            .messages
            .iter()
            .filter(|message| message.role == "user")
            .count();
        let assistant_messages = self
            .session
            .messages
            .iter()
            .filter(|message| message.role == "assistant")
            .count();
        let system_messages = self
            .session
            .messages
            .iter()
            .filter(|message| message.role == "system")
            .count();
        let attachment_count: usize = self
            .session
            .messages
            .iter()
            .map(|message| message.attachments.len())
            .sum::<usize>()
            + self.session.pending_attachments.len();
        let session_age = chrono::Utc::now()
            .signed_duration_since(self.session.created_at)
            .num_seconds()
            .max(0);
        let provider_reported_total = self
            .session
            .provider_reported_input_tokens
            .saturating_add(self.session.provider_reported_output_tokens);
        let local_total = self
            .session
            .input_tokens_used
            .saturating_add(self.session.output_tokens_used);
        let context_percent = if self.session.context_limit > 0 {
            (local_total as f64 / self.session.context_limit as f64) * 100.0
        } else {
            0.0
        };
        let pending_approvals = self.tool_executor.guardrails.approvals.pending_len();
        let recent_events = self.logger.events().len();
        let token_source = if provider_reported_total > 0 {
            "mixed: provider-reported where available, local tiktoken for streaming/unsupported providers"
        } else {
            "local tiktoken count"
        };
        let active_agent = self
            .session
            .active_agent_name
            .as_deref()
            .or(self.session.active_agent_id.as_deref())
            .unwrap_or("none");
        format!(
            "Session status\n\
             session_id: {}\n\
             title: {}\n\
             workspace: {}\n\
             created_at: {}\n\
             age: {}\n\
             status: {}\n\
             activity: {}\n\
             provider: {}\n\
             model: {}\n\
             active_agent: {}\n\
             autonomous_mode: {}\n\
             risky_tools: {}\n\
             dangerous_bypass: {}\n\n\
             Token telemetry\n\
             source: {}\n\
             input_tokens: {}\n\
             output_tokens: {}\n\
             total_tokens: {}\n\
             provider_reported_input_tokens: {}\n\
             provider_reported_output_tokens: {}\n\
             provider_reported_total_tokens: {}\n\
             context_limit: {}\n\
             context_used_estimate: {:.1}%\n\n\
             Session telemetry\n\
             messages: {} total / {} user / {} assistant / {} system\n\
             attachments: {} active+pending\n\
             pending_approvals: {}\n\
             pending_model_response: {}\n\
             pending_background_jobs: {}\n\
             last_latency_ms: {}\n\
             trace_events: {}",
            self.session.session_id,
            self.session.title,
            self.cwd.display(),
            self.session.created_at.to_rfc3339(),
            format_duration(session_age),
            self.session.status,
            if self.session.activity.trim().is_empty() {
                "none"
            } else {
                self.session.activity.as_str()
            },
            self.session.current_provider,
            self.session.current_model,
            active_agent,
            if self.autonomous_mode_enabled {
                "enabled"
            } else {
                "disabled"
            },
            if self.tool_executor.guardrails.policy.allow_risky_tools {
                "enabled"
            } else {
                "disabled"
            },
            if self.dangerously_bypass_approvals_and_sandbox {
                "enabled"
            } else {
                "disabled"
            },
            token_source,
            self.session.input_tokens_used,
            self.session.output_tokens_used,
            local_total,
            self.session.provider_reported_input_tokens,
            self.session.provider_reported_output_tokens,
            provider_reported_total,
            self.session.context_limit,
            context_percent,
            message_count,
            user_messages,
            assistant_messages,
            system_messages,
            attachment_count,
            pending_approvals,
            if self.pending_send.is_some() {
                "yes"
            } else {
                "no"
            },
            self.pending_background_jobs.len(),
            self.session.last_latency_ms,
            recent_events,
        )
    }

    pub(crate) fn work_command(&mut self, args: &[String]) -> String {
        let limit = parse_limit(args, 40);
        let body = self.work_activity_report(limit);
        self.info_scroll_offset = 0;
        self.info_overlay = Some(InfoOverlay {
            title: "work activity".to_string(),
            body: body.clone(),
        });
        body
    }

    fn work_activity_report(&self, limit: usize) -> String {
        let mut events = self.logger.events();
        if events.len() > limit {
            events = events.split_off(events.len() - limit);
        }
        let mut lines = vec![
            format!("Work activity for session {}", self.session.session_id),
            format!("workspace: {}", self.cwd.display()),
            format!("status: {}", self.session.status),
            String::new(),
        ];
        if self.pending_send.is_some() {
            lines.push("running: model response in progress".to_string());
        }
        if !self.session.activity.trim().is_empty() {
            lines.push(format!("activity: {}", self.session.activity));
        }
        let pending = self.tool_executor.guardrails.approvals.pending();
        if !pending.is_empty() {
            lines.push(String::new());
            lines.push("Pending approvals".to_string());
            for approval in pending.values() {
                lines.push(format!(
                    "? {} {} approval_id={}",
                    approval.risk_label, approval.tool_name, approval.id
                ));
            }
        }
        lines.push(String::new());
        lines.push("Recent events".to_string());
        if events.is_empty() {
            lines.push("No trace events recorded yet.".to_string());
        } else {
            for event in events {
                lines.push(format!(
                    "{} {} {}",
                    event.timestamp.format("%H:%M:%S"),
                    event.name,
                    compact_json(&event.payload)
                ));
            }
        }
        lines.join("\n")
    }

    pub(crate) fn trace_command(&self, args: &[String]) -> anyhow::Result<String> {
        let json_output = args.iter().any(|arg| arg == "--json" || arg == "json");
        let limit = parse_limit(args, 20);
        let mut events = self.logger.events();
        let total = events.len();
        if total > limit {
            events = events.split_off(total - limit);
        }
        if json_output {
            return Ok(serde_json::to_string_pretty(&events)?);
        }
        if events.is_empty() {
            return Ok("No trace events recorded.".to_string());
        }
        Ok(events
            .iter()
            .map(|event| {
                format!(
                    "{} {} {}",
                    event.timestamp.to_rfc3339(),
                    event.name,
                    compact_json(&event.payload)
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub(crate) fn diff_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let staged = args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--staged" | "--cached" | "staged" | "cached"));
        let stat = args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--stat" | "stat"));
        let renderer = args
            .iter()
            .find_map(|arg| match arg.as_str() {
                "semantic" | "difftastic" | "--semantic" | "--difftastic" => {
                    Some(DiffRenderer::Difftastic)
                }
                "delta" | "--delta" => Some(DiffRenderer::Delta),
                "unified" | "--unified" | "patch" | "--patch" => Some(DiffRenderer::Unified),
                _ => None,
            })
            .unwrap_or(DiffRenderer::Unified);
        let paths = args
            .iter()
            .filter(|arg| {
                !matches!(
                    arg.as_str(),
                    "--staged"
                        | "--cached"
                        | "staged"
                        | "cached"
                        | "--stat"
                        | "stat"
                        | "semantic"
                        | "difftastic"
                        | "--semantic"
                        | "--difftastic"
                        | "delta"
                        | "--delta"
                        | "unified"
                        | "--unified"
                        | "patch"
                        | "--patch"
                )
            })
            .collect::<Vec<_>>();

        let diff = match renderer {
            DiffRenderer::Unified => self.git_diff_output(staged, stat, &paths)?,
            DiffRenderer::Delta if stat => self.git_diff_output(staged, stat, &paths)?,
            DiffRenderer::Difftastic if stat => self.git_diff_output(staged, stat, &paths)?,
            DiffRenderer::Delta => match self.delta_diff_output(staged, &paths)? {
                Some(rendered) => rendered,
                None => {
                    let unified = self.git_diff_output(staged, false, &paths)?;
                    format!(
                        "delta is not installed or failed; showing unified diff instead.\n\n{unified}"
                    )
                }
            },
            DiffRenderer::Difftastic => match self.difftastic_diff_output(staged, &paths)? {
                Some(rendered) => rendered,
                None => {
                    let unified = self.git_diff_output(staged, false, &paths)?;
                    format!(
                        "difft/difftastic is not installed or failed; showing unified diff instead.\n\n{unified}"
                    )
                }
            },
        };

        if diff.trim().is_empty() {
            return Ok(if staged {
                "No staged changes.".to_string()
            } else {
                "No workspace changes.".to_string()
            });
        }
        if stat {
            return Ok(format!("Git diff stat\n\n```text\n{diff}\n```"));
        }
        let title = match renderer {
            DiffRenderer::Unified => "Git diff",
            DiffRenderer::Delta => "Git diff (delta)",
            DiffRenderer::Difftastic => "Git diff (difftastic)",
        };
        let overlay = if renderer == DiffRenderer::Unified {
            diff_overlay_from_patch(title, &diff)
        } else {
            diff_overlay_from_rendered(title, &diff, renderer)
        };
        self.diff_scroll_offset = 0;
        self.diff_overlay = Some(overlay);
        let fence = if renderer == DiffRenderer::Unified {
            "diff"
        } else {
            "text"
        };
        Ok(format!("{title}\n\n```{fence}\n{diff}\n```"))
    }

    fn git_diff_output(
        &self,
        staged: bool,
        stat: bool,
        paths: &[&String],
    ) -> anyhow::Result<String> {
        let mut command = std::process::Command::new("git");
        command
            .arg("-C")
            .arg(&self.cwd)
            .arg("--no-pager")
            .arg("diff")
            .arg("--no-ext-diff")
            .arg("--color=never");
        if staged {
            command.arg("--cached");
        }
        if stat {
            command.arg("--stat");
        }
        if !paths.is_empty() {
            command.arg("--");
            for path in paths {
                command.arg(path);
            }
        }
        command_output(command, "git diff")
    }

    fn delta_diff_output(&self, staged: bool, paths: &[&String]) -> anyhow::Result<Option<String>> {
        if !command_available("delta") {
            return Ok(None);
        }
        let unified = self.git_diff_output(staged, false, paths)?;
        if unified.trim().is_empty() {
            return Ok(Some(unified));
        }
        let mut child = std::process::Command::new("delta")
            .arg("--color=never")
            .arg("--line-numbers")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            stdin.write_all(unified.as_bytes())?;
        }
        let output = child.wait_with_output()?;
        if output.status.success() {
            Ok(Some(strip_ansi(&String::from_utf8_lossy(&output.stdout))))
        } else {
            Ok(None)
        }
    }

    fn difftastic_diff_output(
        &self,
        staged: bool,
        paths: &[&String],
    ) -> anyhow::Result<Option<String>> {
        let executable = if command_available("difft") {
            "difft"
        } else if command_available("difftastic") {
            "difftastic"
        } else {
            return Ok(None);
        };
        let mut command = std::process::Command::new("git");
        command
            .arg("-C")
            .arg(&self.cwd)
            .arg("--no-pager")
            .arg("diff")
            .arg("--ext-diff")
            .arg("--color=never")
            .env("GIT_EXTERNAL_DIFF", executable)
            .env("DFT_COLOR", "never")
            .env("DFT_DISPLAY", "inline");
        if staged {
            command.arg("--cached");
        }
        if !paths.is_empty() {
            command.arg("--");
            for path in paths {
                command.arg(path);
            }
        }
        match command_output(command, "git diff with difftastic") {
            Ok(output) => Ok(Some(strip_ansi(&output))),
            Err(_) => Ok(None),
        }
    }

    pub(crate) fn config_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("status") | Some("show") => {
                let defaults = self.config.load().unwrap_or_default();
                Ok(format!(
                    "Vegvisir configuration\npath={}\nsessions={}\ndefault_user_id={}\nactive_cms_user_id={}\nprovider={}\nmodel={}\nworkspace={}",
                    self.config.path.display(),
                    self.sessions.store.root.display(),
                    configured_user_id(&defaults),
                    self.cms.config.user_id,
                    self.session.current_provider,
                    self.session.current_model,
                    self.cwd.display()
                ))
            }
            Some("user") | Some("set-user") => {
                let Some(user_id) = args.get(1) else {
                    return Ok(format!("Default user id: {}", self.default_user_id()));
                };
                validate_user_id(user_id)?;
                let mut defaults = self.config.load().unwrap_or_default();
                defaults.insert(
                    "current_user_id".to_string(),
                    Value::String(user_id.clone()),
                );
                self.config.save(&defaults)?;
                self.autosave_session();
                self.sessions.store =
                    SessionStore::new(session_root_for_user(&self.data_root, user_id))?;
                self.sessions.cwd = self.cwd.clone();
                if self.session.active_agent_id.is_none() {
                    let previous = self.session.clone();
                    let mut config = self.cms.config.clone();
                    config.user_id = user_id.clone();
                    config.project_id = Some(workspace_project_id(&self.cwd));
                    self.cms = VegvisirCms::open(config)?;
                    self.rebuild_tooling_for_cms()?;
                    if let Some(restored) = self.session_for_workspace(&self.cwd)? {
                        self.session = restored;
                    } else {
                        let mut next = self.sessions.create(
                            workspace_title(&self.cwd),
                            previous.current_provider,
                            previous.current_model,
                            previous.enabled_tools,
                            previous.enabled_skills,
                        );
                        next.system_prompt = previous.system_prompt;
                        next.context_limit = previous.context_limit;
                        self.session = next;
                    }
                    self.session.cwd = self.cwd.display().to_string();
                    self.input.history = self.session.input_history.clone();
                    self.autosave_session();
                }
                Ok(format!("Default user id set to {user_id}."))
            }
            Some("provider") => {
                let Some(provider) = args.get(1) else {
                    return Ok("Usage: /config provider <provider>".to_string());
                };
                self.provider_command(&["--global".to_string(), provider.clone()])
            }
            Some("model") => {
                let Some(model) = args.get(1) else {
                    return Ok("Usage: /config model <model>".to_string());
                };
                self.select_model(&["--global".to_string(), model.clone()])
            }
            Some("skills") | Some("lsl") => self.skills_config_command(&args[1..]),
            Some("path") => Ok(self.config.path.display().to_string()),
            Some(other) => Ok(format!("Unknown /config command: {other}")),
        }
    }

    pub(crate) fn eval_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.first().map(String::as_str) == Some("file") {
            let path = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("Usage: /eval file <path>"))?;
            let results = crate::evals::run_eval_file(path)?;
            return Ok(crate::evals::format_eval_results(&results));
        }
        let scope = args.first().map(String::as_str).unwrap_or("all");
        let results = crate::evals::run_builtin_evals(scope)?;
        Ok(crate::evals::format_eval_results(&results))
    }

    pub(crate) fn subagents_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            Some("policy") | Some("help") => Ok(Self::subagent_policy_help()),
            None | Some("list") | Some("tasks") => {
                let records = self.load_subagent_records()?;
                if records.is_empty() {
                    return Ok("No subagent task records.".to_string());
                }
                Ok(records
                    .iter()
                    .map(|record| {
                        format!(
                            "{}  name={} status={:?} workspace={} goal={}",
                            record.id,
                            record.name,
                            record.status,
                            record.workspace.display(),
                            record.goal
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            Some("show") => {
                let Some(id_or_name) = args.get(1) else {
                    return Ok("Usage: /subagents show <id-or-name>".to_string());
                };
                let Some(record) = self.find_subagent_record(id_or_name)? else {
                    return Ok(format!("Unknown subagent task: {id_or_name}"));
                };
                Ok(serde_json::to_string_pretty(&record)?)
            }
            Some("cancel") => {
                let Some(id_or_name) = args.get(1) else {
                    return Ok("Usage: /subagents cancel <id-or-name>".to_string());
                };
                let mut records = self.load_subagent_records()?;
                let Some(record) = records
                    .iter_mut()
                    .find(|record| record.id == *id_or_name || record.name == *id_or_name)
                else {
                    return Ok(format!("Unknown subagent task: {id_or_name}"));
                };
                if matches!(
                    record.status,
                    SubAgentStatus::Completed | SubAgentStatus::Failed | SubAgentStatus::Cancelled
                ) {
                    return Ok(format!(
                        "Subagent task {} is already {:?}.",
                        record.id, record.status
                    ));
                }
                record.status = SubAgentStatus::Cancelled;
                record.finished_at = Some(chrono::Utc::now());
                let id = record.id.clone();
                let name = record.name.clone();
                self.save_subagent_records(&records)?;
                self.logger.emit(
                    "subagent.cancelled",
                    json!({
                        "id": id,
                        "name": name,
                        "source": "tui-command",
                    }),
                );
                Ok(format!("Cancelled subagent task {id}."))
            }
            Some(other) => Ok(format!("Unknown /subagents command: {other}")),
        }
    }

    fn subagent_policy_help() -> String {
        r#"Subagent delegation policy

Vegvisir exposes `spawn_subagent` as a normal bounded delegation tool. The model receives hidden task-local orchestration guidance encouraging subagents for complex, multi-part, evidence-seeking work.

Good subagent tasks:
- codebase reconnaissance
- focused test investigation
- documentation review
- compatibility checks
- security review
- design critique
- migration impact analysis

Boundaries:
- do not spawn for trivial single-step tasks
- do not delegate plaintext secrets or credential handling
- do not delegate destructive actions or ambiguous external side effects
- keep goals bounded, evidence-oriented, and low-step by default
- assign explicit non-overlapping file_scope values for file-touching work
- never let two active subagents own/edit the same files at the same time
- never more than three active subagents at once

Commands:
/subagents list
/subagents show <id-or-name>
/subagents cancel <id-or-name>
/subagents policy"#
        .to_string()
    }

    pub(crate) fn subagent_board_path(&self) -> PathBuf {
        self.data_root.join("subagents.json")
    }

    pub(crate) fn load_subagent_records(&self) -> anyhow::Result<Vec<SubAgentTaskRecord>> {
        let path = self.subagent_board_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    fn save_subagent_records(&self, records: &[SubAgentTaskRecord]) -> anyhow::Result<()> {
        let path = self.subagent_board_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(records)?)?;
        Ok(())
    }

    fn find_subagent_record(&self, id_or_name: &str) -> anyhow::Result<Option<SubAgentTaskRecord>> {
        Ok(self
            .load_subagent_records()?
            .into_iter()
            .find(|record| record.id == id_or_name || record.name == id_or_name))
    }

    pub(crate) fn attach_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            if self.session.pending_attachments.is_empty() {
                return Ok(
                    "No pending attachments. Drag/drop a file path into the input or use /attach <path>."
                        .to_string(),
                );
            }
            return Ok(self
                .session
                .pending_attachments
                .iter()
                .map(|item| format!("{}: {}", item.kind, item.path))
                .collect::<Vec<_>>()
                .join("\n"));
        }
        if args[0] == "clear" {
            self.session.pending_attachments.clear();
            return Ok("Pending attachments cleared.".to_string());
        }
        let mut path = PathBuf::from(args.join(" "));
        if !path.is_absolute() {
            path = self.cwd.join(path);
        }
        if !path.exists() || !path.is_file() {
            return Ok(format!("Attachment not found: {}", path.display()));
        }
        let attachment = attachment_for(&path)?;
        let name = attachment
            .name
            .clone()
            .unwrap_or_else(|| path.display().to_string());
        self.session.pending_attachments.push(attachment);
        Ok(format!("Attached {name}"))
    }
}

fn format_duration(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {secs}s")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}
