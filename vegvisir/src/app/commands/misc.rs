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
             dangerous_bypass: {}\n\
             active_subagent_limit: {}\n\n\
             Hardware / parallelism\n\
             available_parallelism: {}\n\
             reserved_cores: {}\n\
             max_workers: {}\n\
             worker_source: {}\n\n\
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
            self.active_subagent_limit,
            self.parallelism.available_parallelism,
            self.parallelism.reserved_cores,
            self.parallelism.max_workers,
            self.parallelism.source_label(),
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
                    "Vegvisir configuration\npath={}\nsessions={}\ndefault_user_id={}\nactive_cms_user_id={}\nprovider={}\nmodel={}\nsubagent_provider={}\nsubagent_model={}\nworkspace={}",
                    self.config.path.display(),
                    self.sessions.store.root.display(),
                    configured_user_id(&defaults),
                    self.cms.config.user_id,
                    self.session.current_provider,
                    self.session.current_model,
                    self.subagent_provider_defaults.provider,
                    self.subagent_provider_defaults.model,
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
            Some("subagents") | Some("subagent") => self.subagents_config_command(&args[1..]),
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
        if let Some(response) = self.try_subagent_limit_command(args)? {
            return Ok(response);
        }
        match args.first().map(String::as_str) {
            Some("policy") | Some("help") => Ok(Self::subagent_policy_help(
                self.active_subagent_limit,
                &self.subagent_provider_defaults,
            )),
            Some("config") | Some("defaults") => self.subagents_config_command(&args[1..]),
            None | Some("list") | Some("tasks") => {
                let records = self.load_subagent_records()?;
                if records.is_empty() {
                    return Ok("No subagent task records.".to_string());
                }
                Ok(records
                    .iter()
                    .map(|record| {
                        format!(
                            "{}  name={} status={:?} workspace={} scope={} budget={} goal={}",
                            record.id,
                            record.name,
                            record.status,
                            record.workspace.display(),
                            record
                                .file_scope
                                .iter()
                                .map(|path| path.display().to_string())
                                .collect::<Vec<_>>()
                                .join(","),
                            serde_json::to_string(&record.work_budget).unwrap_or_default(),
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
            Some("timeline") => self.subagents_timeline(),
            Some("diff") => {
                let Some(id_or_name) = args.get(1) else {
                    return Ok("Usage: /subagents diff <id-or-name>".to_string());
                };
                let Some(record) = self.find_subagent_record(id_or_name)? else {
                    return Ok(format!("Unknown subagent task: {id_or_name}"));
                };
                Ok(format_subagent_diffs(&record))
            }
            Some("events") => {
                let Some(id_or_name) = args.get(1) else {
                    return Ok("Usage: /subagents events <id-or-name>".to_string());
                };
                let Some(record) = self.find_subagent_record(id_or_name)? else {
                    return Ok(format!("Unknown subagent task: {id_or_name}"));
                };
                Ok(format_subagent_events(&record))
            }
            Some("artifacts") => {
                let Some(id_or_name) = args.get(1) else {
                    return Ok("Usage: /subagents artifacts <id-or-name>".to_string());
                };
                let Some(record) = self.find_subagent_record(id_or_name)? else {
                    return Ok(format!("Unknown subagent task: {id_or_name}"));
                };
                Ok(format!(
                    "Subagent artifacts for {}\nparent_run_id={}\nchild_run_id={}\nartifact_dir={}\ncheckpoint={}",
                    record.id,
                    record.parent_run_id.as_deref().unwrap_or("none"),
                    record.child_run_id.as_deref().unwrap_or("none"),
                    record
                        .artifact_dir
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "none".to_string()),
                    record
                        .checkpoint
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "none".to_string())
                ))
            }
            Some("ownership") => self.subagents_ownership(),
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

    fn subagents_timeline(&self) -> anyhow::Result<String> {
        let mut records = self.load_subagent_records()?;
        if records.is_empty() {
            return Ok("No subagent task records.".to_string());
        }
        records.sort_by_key(|record| record.created_at);
        Ok(records
            .iter()
            .map(|record| {
                format!(
                    "{} created={} started={} finished={} status={:?} name={} parent_run={} child_run={}",
                    record.id,
                    record.created_at.to_rfc3339(),
                    record.started_at.map(|ts| ts.to_rfc3339()).unwrap_or_else(|| "none".to_string()),
                    record.finished_at.map(|ts| ts.to_rfc3339()).unwrap_or_else(|| "none".to_string()),
                    record.status,
                    record.name,
                    record.parent_run_id.as_deref().unwrap_or("none"),
                    record.child_run_id.as_deref().unwrap_or("none"),
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn subagents_ownership(&self) -> anyhow::Result<String> {
        let records = self.load_subagent_records()?;
        if records.is_empty() {
            return Ok("No subagent task records.".to_string());
        }
        Ok(records
            .iter()
            .map(|record| {
                let ownership = record.ownership.as_ref();
                let read_scope = ownership
                    .map(|o| &o.read_scope)
                    .unwrap_or(&record.file_scope)
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let write_scope = ownership
                    .map(|o| {
                        o.write_scope
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_else(|| "<none/read-only>".to_string());
                let exclusive = ownership.map(|o| o.exclusive_write).unwrap_or(true);
                format!(
                    "{} name={} status={:?} read_scope={} write_scope={} exclusive_write={}",
                    record.id, record.name, record.status, read_scope, write_scope, exclusive
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub(crate) fn try_subagent_limit_command(
        &mut self,
        args: &[String],
    ) -> anyhow::Result<Option<String>> {
        let Some(first) = args.first().map(String::as_str) else {
            return Ok(None);
        };
        let value = if let Some(value) = first.strip_prefix("max=") {
            Some(value)
        } else if first == "max" || first == "limit" {
            args.get(1).map(String::as_str)
        } else {
            None
        };
        let Some(raw) = value else {
            return Ok(None);
        };
        let limit = raw
            .trim()
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("Usage: /agents max=<n> or /subagents max <n>"))?;
        if limit == 0 {
            anyhow::bail!("Subagent max must be at least 1");
        }
        self.active_subagent_limit = limit;
        self.rebuild_tooling_for_cms()?;
        self.logger.emit(
            "subagent.limit_updated",
            json!({
                "active_subagent_limit": limit,
                "source": "tui-command",
                "spawn_requires_yolo": true,
            }),
        );
        Ok(Some(format!(
            "Active subagent limit set to {limit} for this session. Subagent spawning remains locked to YOLO mode (--dangerously-bypass-approvals-and-sandbox)."
        )))
    }

    pub(crate) fn subagents_config_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("show") | Some("status") => Ok(format!(
                "Subagent configuration
provider={}
model={}
active_limit={}
config_path={}

Set with:
  /subagents config provider <provider>
  /subagents config model <model>
  /subagents config provider <provider> model <model>
  /subagents config max <n>",
                self.subagent_provider_defaults.provider,
                self.subagent_provider_defaults.model,
                self.active_subagent_limit,
                self.config.path.display()
            )),
            Some("provider") | Some("model") | Some("set") | Some("max") | Some("limit") => {
                let mut provider = self.subagent_provider_defaults.provider.clone();
                let mut model = self.subagent_provider_defaults.model.clone();
                let mut active_limit = self.active_subagent_limit;
                let mut index = 0usize;
                while index < args.len() {
                    match args[index].as_str() {
                        "set" => index += 1,
                        "provider" => {
                            let Some(value) = args.get(index + 1) else {
                                return Ok(
                                    "Usage: /subagents config provider <provider> [model <model>]"
                                        .to_string(),
                                );
                            };
                            provider = value.clone();
                            index += 2;
                        }
                        "model" => {
                            let Some(value) = args.get(index + 1) else {
                                return Ok(
                                    "Usage: /subagents config model <model> [provider <provider>]"
                                        .to_string(),
                                );
                            };
                            model = value.clone();
                            index += 2;
                        }
                        "max" | "limit" => {
                            let Some(value) = args.get(index + 1) else {
                                return Ok("Usage: /subagents config max <n>".to_string());
                            };
                            active_limit = value
                                .parse::<usize>()
                                .map_err(|_| anyhow::anyhow!("Usage: /subagents config max <n>"))?
                                .max(1);
                            index += 2;
                        }
                        other if other.starts_with("provider=") => {
                            provider = other.trim_start_matches("provider=").to_string();
                            index += 1;
                        }
                        other if other.starts_with("model=") => {
                            model = other.trim_start_matches("model=").to_string();
                            index += 1;
                        }
                        other if other.starts_with("max=") || other.starts_with("limit=") => {
                            let (_, value) = other.split_once('=').unwrap_or(("max", ""));
                            active_limit = value
                                .parse::<usize>()
                                .map_err(|_| anyhow::anyhow!("Usage: /subagents config max <n>"))?
                                .max(1);
                            index += 1;
                        }
                        other => {
                            return Ok(format!(
                                "Unknown /subagents config token: {other}
Usage: /subagents config provider <provider> model <model>"
                            ));
                        }
                    }
                }
                self.subagent_provider_defaults =
                    crate::tools::SubagentProviderDefaults::new(provider, model);
                self.active_subagent_limit = active_limit;
                let mut defaults = self.config.load().unwrap_or_default();
                defaults.insert(
                    "subagent_provider".to_string(),
                    serde_json::json!(self.subagent_provider_defaults.provider),
                );
                defaults.insert(
                    "subagent_model".to_string(),
                    serde_json::json!(self.subagent_provider_defaults.model),
                );
                defaults.insert(
                    "subagent_active_limit".to_string(),
                    serde_json::json!(self.active_subagent_limit),
                );
                self.config.save(&defaults)?;
                self.rebuild_tooling_for_cms()?;
                self.logger.emit(
                    "subagent.config_updated",
                    serde_json::json!({
                        "provider": self.subagent_provider_defaults.provider,
                        "model": self.subagent_provider_defaults.model,
                        "active_limit": self.active_subagent_limit,
                        "source": "tui-command",
                    }),
                );
                Ok(format!(
                    "Subagent defaults set to provider={} model={} active_limit={}. New spawn_subagent calls inherit these unless provider/model is specified explicitly.",
                    self.subagent_provider_defaults.provider, self.subagent_provider_defaults.model, self.active_subagent_limit
                ))
            }
            Some(other) => Ok(format!(
                "Unknown /subagents config command: {other}
Usage: /subagents config [show|provider <provider>|model <model>|max <n>|provider <provider> model <model>]"
            )),
        }
    }

    fn subagent_policy_help(
        active_subagent_limit: usize,
        defaults: &crate::tools::SubagentProviderDefaults,
    ) -> String {
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
- assign a work_budget for reconnaissance/bug-hunting tasks: max steps, max tool calls, max read bytes, max output bytes, allowed tools, and notes
- use budget notes like "avoid huge raw reads; prefer targeted search/listing; report if more budget is needed"
- assign explicit non-overlapping file_scope values for file-touching work
- never let two active subagents own/edit the same files at the same time
- default active subagent limit is 3; current session limit is {active_subagent_limit}
- default provider/model is {subagent_provider}/{subagent_model} unless a subagent request explicitly sets provider/model
- change the session limit with /agents max=<n> or /subagents max <n>
- change default subagent provider/model with /subagents config provider <provider> model <model>
- subagent spawning remains locked to YOLO mode for now

Commands:
/subagents list
/subagents show <id-or-name>
/subagents timeline
/subagents diff <id-or-name>
/subagents events <id-or-name>
/subagents artifacts <id-or-name>
/subagents ownership
/subagents cancel <id-or-name>
/subagents policy
/subagents max <n>
/subagents config
/subagents config provider <provider> model <model>
/agents max=<n>"#
        .replace("{active_subagent_limit}", &active_subagent_limit.to_string())
        .replace("{subagent_provider}", &defaults.provider)
        .replace("{subagent_model}", &defaults.model)
    }

    pub(crate) fn subagent_board_path(&self) -> PathBuf {
        self.data_root.join("subagents.json")
    }

    pub(crate) fn load_subagent_records(&self) -> anyhow::Result<Vec<SubAgentTaskRecord>> {
        let path = self.subagent_board_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = std::fs::read_to_string(&path)?;
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        match serde_json::from_str::<Vec<SubAgentTaskRecord>>(&text) {
            Ok(records) => Ok(records),
            Err(original_error) => {
                if let Some(records) = recover_subagent_board_records(&text) {
                    let _ = self.save_subagent_records(&records);
                    Ok(records)
                } else {
                    Err(original_error.into())
                }
            }
        }
    }

    fn save_subagent_records(&self, records: &[SubAgentTaskRecord]) -> anyhow::Result<()> {
        let path = self.subagent_board_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write_json(&path, &serde_json::to_string_pretty(records)?)?;
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

fn recover_subagent_board_records(text: &str) -> Option<Vec<SubAgentTaskRecord>> {
    let trimmed = text.trim_start_matches(['\u{feff}', '\u{200b}', '\u{2060}']);
    let start = trimmed.find('[')?;
    let candidate = &trimmed[start..];
    serde_json::from_str::<Vec<SubAgentTaskRecord>>(candidate).ok()
}

fn atomic_write_json(path: &std::path::Path, content: &str) -> anyhow::Result<()> {
    use uuid::Uuid;
    let tmp = path.with_extension(format!("{}.tmp", Uuid::new_v4().simple()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(())
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

fn format_subagent_diffs(record: &SubAgentTaskRecord) -> String {
    let changes = &record.observability.file_changes;
    if changes.is_empty() {
        return format!("No file-change diffs captured for subagent {}.", record.id);
    }
    let mut out = format!("Subagent diffs for {} ({})\n", record.id, record.name);
    for change in changes {
        out.push_str(&format!(
            "\n# {:?}: {} before_bytes={:?} after_bytes={:?}\n",
            change.change,
            change.path.display(),
            change.before_bytes,
            change.after_bytes
        ));
        if let Some(diff) = &change.diff {
            out.push_str(diff.trim());
            out.push('\n');
        } else {
            out.push_str("<no diff text captured>\n");
        }
    }
    out
}

fn format_subagent_events(record: &SubAgentTaskRecord) -> String {
    if record.observability.events.is_empty() {
        return format!("No observed events captured for subagent {}.", record.id);
    }
    record
        .observability
        .events
        .iter()
        .map(|event| {
            format!(
                "{:?} name={} ok={} summary={} args={}",
                event.kind,
                event.name.as_deref().unwrap_or("-"),
                event
                    .ok
                    .map(|ok| ok.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                event.summary.as_deref().unwrap_or("-"),
                event.args.as_deref().unwrap_or("-")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
