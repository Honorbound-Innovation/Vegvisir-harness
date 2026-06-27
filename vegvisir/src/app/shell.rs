use super::*;

fn agent_selection_prefix(raw: &str) -> Option<&str> {
    if raw == "/agent" || raw == "/agent " {
        return Some("");
    }
    if raw == "/agent use" || raw == "/agent use " {
        return Some("");
    }
    raw.strip_prefix("/agent use ").map(str::trim)
}

impl TuiApplication {
    pub fn render(&mut self) -> String {
        self.expire_ephemeral_notice();
        let suggestions = self.build_suggestions();
        self.input.update_suggestions(suggestions);
        let pending_approvals = self.pending_approval_requests();
        self.renderer.render_startup(
            &self.session,
            &self.commands,
            &self.input,
            &self.input.suggestions,
            self.input.selected_suggestion,
            self.chat_scroll_offset,
            &pending_approvals,
            self.ephemeral_notice
                .as_ref()
                .map(|notice| notice.text.as_str()),
        )
    }

    fn pending_approval_requests(&self) -> Vec<ApprovalRequest> {
        self.tool_executor
            .guardrails
            .approvals
            .pending()
            .into_values()
            .collect()
    }

    pub fn build_suggestions(&self) -> Vec<Suggestion> {
        let raw = &self.input.buffer;
        if !raw.starts_with('/') {
            return Vec::new();
        }
        let parts = raw.split_whitespace().collect::<Vec<_>>();
        let trailing_space = raw.ends_with(' ');
        if let Some(prefix) = agent_selection_prefix(raw) {
            let (profiles, warnings) = self.agents.list_lossy().unwrap_or_default();
            let prefix_lower = prefix.to_ascii_lowercase();
            let mut suggestions = profiles
                .into_iter()
                .filter(|profile| {
                    prefix.is_empty()
                        || profile.id.to_ascii_lowercase().starts_with(&prefix_lower)
                        || profile
                            .display_name
                            .to_ascii_lowercase()
                            .contains(&prefix_lower)
                        || profile.mode.to_ascii_lowercase().starts_with(&prefix_lower)
                })
                .map(|profile| {
                    let active = if self.session.active_agent_id.as_deref() == Some(&profile.id) {
                        "active · "
                    } else {
                        ""
                    };
                    Suggestion::new(
                        profile.id.clone(),
                        format!("{active}{} · mode={}", profile.display_name, profile.mode),
                        Some(format!("/agent use {}", profile.id)),
                    )
                })
                .collect::<Vec<_>>();
            if suggestions.is_empty() && !warnings.is_empty() {
                suggestions.push(Suggestion::new(
                    "agent profile warning",
                    warnings.join("; "),
                    Some("/agent list".to_string()),
                ));
            }
            return suggestions;
        }
        if raw.starts_with("/provider ") || raw == "/provider " {
            let prefix = if trailing_space {
                ""
            } else {
                parts.get(1).copied().unwrap_or("")
            };
            if parts.len() <= 2 && !matches!(parts.get(1), Some(&"compare" | &"diagnose")) {
                let mut suggestions = [
                    Suggestion::new(
                        "compare".to_string(),
                        "compare provider readiness/auth/model catalog".to_string(),
                        Some("/provider compare ".to_string()),
                    ),
                    Suggestion::new(
                        "diagnose".to_string(),
                        "diagnose provider auth and catalog state".to_string(),
                        Some("/provider diagnose ".to_string()),
                    ),
                ]
                .into_iter()
                .filter(|suggestion| suggestion.value.starts_with(prefix))
                .collect::<Vec<_>>();
                suggestions.extend(
                    self.provider_registry
                        .list()
                        .into_iter()
                        .filter(|provider| provider.name.starts_with(prefix))
                        .map(|provider| {
                            Suggestion::new(
                                provider.name.clone(),
                                provider
                                    .display_name
                                    .clone()
                                    .unwrap_or_else(|| provider.name.clone()),
                                Some(format!("/provider {}", provider.name)),
                            )
                        }),
                );
                return suggestions;
            }
            if matches!(parts.get(1), Some(&"compare" | &"diagnose")) {
                let provider_prefix = if trailing_space {
                    ""
                } else {
                    parts.get(2).copied().unwrap_or("")
                };
                return self
                    .provider_registry
                    .list()
                    .into_iter()
                    .filter(|provider| provider.name.starts_with(provider_prefix))
                    .map(|provider| {
                        Suggestion::new(
                            provider.name.clone(),
                            provider
                                .display_name
                                .clone()
                                .unwrap_or_else(|| provider.name.clone()),
                            Some(format!("/provider {} {}", parts[1], provider.name)),
                        )
                    })
                    .collect();
            }
        }
        if raw.starts_with("/runs ") || raw == "/runs " {
            let prefix = if trailing_space {
                ""
            } else {
                parts.get(1).copied().unwrap_or("")
            };
            return [
                ("list", "list recent run artifact bundles"),
                ("show", "show run manifest JSON"),
                ("open", "print run artifact directory"),
                ("diff", "show captured diff.patch"),
                ("result", "show captured result.md"),
                ("context", "show captured context.md"),
                ("memory-used", "show memory-used.json"),
                ("memory-written", "show memory-written.json"),
                ("approvals", "show approvals.json"),
                ("subagents", "show subagents.json"),
                ("verification", "show verification.json"),
                ("failure", "show failure.json"),
                ("export", "print portable bundle path"),
                ("replay-plan", "print manual replay checklist"),
            ]
            .into_iter()
            .filter(|(command, _)| command.starts_with(prefix))
            .map(|(command, description)| {
                Suggestion::new(
                    command.to_string(),
                    description.to_string(),
                    Some(format!("/runs {command} ")),
                )
            })
            .collect();
        }
        if raw.starts_with("/memory ") || raw == "/memory " {
            let prefix = if trailing_space {
                ""
            } else {
                parts.get(1).copied().unwrap_or("")
            };
            return [
                ("status", "show active CMS-v2 scope"),
                ("recent", "list recent memories"),
                ("used-this-turn", "show latest memory-used artifact"),
                ("writes-this-session", "show latest memory-written artifact"),
                ("why", "explain memory id provenance from run artifacts"),
                (
                    "search-chatgpt",
                    "explicitly search imported ChatGPT archive",
                ),
                ("import-chatgpt", "import ChatGPT export archive"),
            ]
            .into_iter()
            .filter(|(command, _)| command.starts_with(prefix))
            .map(|(command, description)| {
                Suggestion::new(
                    command.to_string(),
                    description.to_string(),
                    Some(if command == "why" {
                        "/memory why ".to_string()
                    } else {
                        format!("/memory {command}")
                    }),
                )
            })
            .collect();
        }
        if raw.starts_with("/context ") || raw == "/context " {
            let prefix = if trailing_space {
                ""
            } else {
                parts.get(1).copied().unwrap_or("")
            };
            return [
                ("last", "show latest captured context artifact"),
                ("show-last", "show latest captured context artifact"),
            ]
            .into_iter()
            .filter(|(command, _)| command.starts_with(prefix))
            .map(|(command, description)| {
                Suggestion::new(
                    command.to_string(),
                    description.to_string(),
                    Some(format!("/context {command}")),
                )
            })
            .collect();
        }
        if raw.starts_with("/fast ") || raw == "/fast " {
            if trailing_space && parts.len() >= 2 {
                return Vec::new();
            }
            let prefix = if trailing_space {
                ""
            } else {
                parts.get(1).copied().unwrap_or("")
            };
            return ["on", "off", "status"]
                .into_iter()
                .filter(|mode| mode.starts_with(prefix))
                .map(|mode| {
                    let description = match mode {
                        "on" => "enable fast mode for supported models".to_string(),
                        "off" => "disable fast mode".to_string(),
                        _ => "show fast mode status".to_string(),
                    };
                    Suggestion::new(mode.to_string(), description, Some(format!("/fast {mode}")))
                })
                .collect();
        }
        if raw.starts_with("/effort ") || raw == "/effort " {
            if trailing_space && parts.len() >= 2 {
                return Vec::new();
            }
            let prefix = if trailing_space {
                ""
            } else {
                parts.get(1).copied().unwrap_or("")
            };
            return ["minimal", "low", "medium", "high", "default"]
                .into_iter()
                .filter(|level| level.starts_with(prefix))
                .map(|level| {
                    let description = if level == "default" {
                        "use model catalog default".to_string()
                    } else {
                        format!("set reasoning effort to {level}")
                    };
                    Suggestion::new(
                        level.to_string(),
                        description,
                        Some(format!("/effort {level}")),
                    )
                })
                .collect();
        }
        if raw.starts_with("/auto ")
            || raw == "/auto "
            || raw.starts_with("/autonomous ")
            || raw == "/autonomous "
        {
            let prefix = if trailing_space {
                ""
            } else {
                parts.get(1).copied().unwrap_or("")
            };
            return ["status", "on", "off", "level"]
                .into_iter()
                .filter(|mode| mode.starts_with(prefix))
                .map(|mode| {
                    Suggestion::new(
                        mode.to_string(),
                        match mode {
                            "level" => "set autonomy level 0-6".to_string(),
                            "on" => "enable autonomous prompt-contract mode".to_string(),
                            "off" => "disable autonomous prompt-contract mode".to_string(),
                            _ => "show autonomous mode status".to_string(),
                        },
                        Some(if mode == "level" {
                            "/auto level ".to_string()
                        } else {
                            format!("/auto {mode}")
                        }),
                    )
                })
                .collect();
        }
        if raw.starts_with("/model ")
            || raw == "/model "
            || raw.starts_with("/models ")
            || raw == "/models "
        {
            let prefix = if trailing_space {
                ""
            } else {
                parts.get(1).copied().unwrap_or("")
            };
            let command = if raw.starts_with("/models") {
                "/models"
            } else {
                "/model"
            };
            if command == "/model" && parts.len() <= 2 && parts.get(1) != Some(&"compare") {
                let mut suggestions = if "compare".starts_with(prefix) {
                    vec![Suggestion::new(
                        "compare".to_string(),
                        "compare model context/streaming/reasoning/fast support".to_string(),
                        Some("/model compare ".to_string()),
                    )]
                } else {
                    Vec::new()
                };
                suggestions.extend(self.model_suggestions_for_prefix(prefix, command));
                return suggestions;
            }
            if command == "/model" && parts.get(1) == Some(&"compare") {
                let model_prefix = if trailing_space {
                    ""
                } else {
                    parts.get(2).copied().unwrap_or("")
                };
                return self.model_suggestions_for_prefix(model_prefix, "/model compare");
            }
            return self.model_suggestions_for_prefix(prefix, command);
        }
        self.commands
            .all()
            .into_iter()
            .filter(|command| {
                command_matches_palette_query(&command.name, &command.description, raw)
            })
            .map(|command| {
                Suggestion::new(
                    command.name.clone(),
                    command.description.clone(),
                    Some(command.name.clone()),
                )
            })
            .collect()
    }

    fn model_suggestions_for_prefix(&self, prefix: &str, command: &str) -> Vec<Suggestion> {
        let provider = &self.session.current_provider;
        let mut models = self.models.by_provider(provider);
        if provider.ends_with("-hbse") {
            let direct_provider = provider.trim_end_matches("-hbse");
            let has_hbse_specific_models = models.iter().any(|model| model.provider == *provider);
            if has_hbse_specific_models {
                models.retain(|model| {
                    model.provider == *provider || model.provider != direct_provider
                });
            }
        }
        models
            .into_iter()
            .filter(|model| model.name.starts_with(prefix))
            .map(|model| {
                Suggestion::new(
                    model.name.clone(),
                    format!(
                        "{} · {} ctx",
                        model.provider,
                        model
                            .context_window
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".to_string())
                    ),
                    Some(format!("{command} {}", model.name)),
                )
            })
            .collect()
    }

    pub fn execute_command(&mut self, raw: &str) -> anyhow::Result<Option<String>> {
        let Some((command, args)) = self.commands.parse_with_aliases(raw) else {
            return Ok(None);
        };
        self.logger.emit(
            "command_start",
            json!({
                "command": command.clone(),
                "args": args.clone(),
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
            }),
        );
        let response = match command.as_str() {
            "/new" => self.new_session(&args),
            "/sessions" => self.sessions_command()?,
            "/load" => self.load_session_command(&args)?,
            "/workspace" => self.workspace_command(&args)?,
            "/projects" => self.projects_command(&args)?,
            "/reset" => {
                self.sessions.reset(&mut self.session);
                "Conversation state reset.".to_string()
            }
            "/clear" => {
                self.clear_requested = true;
                "Screen cleared.".to_string()
            }
            "/redraw" => {
                self.clear_requested = true;
                self.redraw_requested = true;
                "Full redraw requested.".to_string()
            }
            "/cancel" => self.cancel_pending_response(),
            "/turn-repair" => self.turn_repair_command(&args),
            "/recover" => self.recover_command(&args)?,
            "/auto" | "/autonomous" => self.autonomous_command(&args),
            "/autonomy" => self.autonomy_command(&args),
            "/history" => self.history(),
            "/status" => self.session_status_command(&args),
            "/diff" => self.diff_command(&args)?,
            "/save" => format!(
                "Saved session to {}",
                self.sessions.save(&self.session)?.display()
            ),
            "/retry" => self.retry()?,
            "/undo" => {
                self.sessions.undo(&mut self.session);
                "Removed last exchange.".to_string()
            }
            "/title" => {
                if !args.is_empty() {
                    self.session.title = args.join(" ");
                }
                format!("Session title: {}", self.session.title)
            }
            "/branch" | "/fork" => self.branch(&args),
            "/compress" => self.compress(&args),
            "/system" => self.system_command(&args)?,
            "/system-prompt" => self.system_command(&[])?,
            "/agent" | "/agents" => self.agent_command(&args)?,
            "/attach" => self.attach_command(&args)?,
            "/ka" => self.persona_command(&args)?,
            "/profile" => self.profile_command(&args)?,
            "/speech" => self.speech_command(&args)?,
            "/tts" => self.tts_command(&args)?,
            "/summary" | "/session-summary" => self.summary_command(&args, false)?,
            "/handoff" => self.summary_command(&args, true)?,
            "/help" => self.help(&args),
            "/commands" => self.commands_command(&args),
            "/tools" => self.tools_command(&args),
            "/tool-limit" => self.tool_limit_command(&args),
            "/approvals" => self.approvals_command(&args),
            "/permissions" => self.permissions_command(&args),
            "/tasks" => self.tasks_command(&args),
            "/runs" => self.runs_command(&args)?,
            "/skills" => self.skills_command(&args)?,
            "/recall" => self.recall_command(&args)?,
            "/memory" => self.memory_command(&args)?,
            "/remember" => self.remember_command(&args)?,
            "/context" => self.context_command(&args)?,
            "/model-request" => self.model_request_command(&args)?,
            "/imagine" => {
                if args.is_empty() {
                    "Usage: /imagine <image prompt>".to_string()
                } else {
                    self.send_imagine(&args.join(" "))?
                }
            }
            "/models" => self.models_command(&args)?,
            "/model" => self.select_model(&args)?,
            "/effort" => self.effort_command(&args)?,
            "/fast" => self.fast_command(&args)?,
            "/provider" => self.provider_command(&args)?,
            "/providers" => self.providers_command(),
            "/auth" => self.auth_command(&args),
            "/verify" => self.verify_command(&args),
            "/eval" => self.eval_command(&args)?,
            "/trace" => self.trace_command(&args)?,
            "/work" => self.work_command(&args),
            "/subagents" => self.subagents_command(&args)?,
            "/mcp" => self.mcp_command(&args)?,
            "/hbse" => self.hbse_command(&args),
            "/config" => self.config_command(&args)?,
            "/exit" => {
                self.running = false;
                "Exiting.".to_string()
            }
            _ => format!("Unknown command: {command}"),
        };
        self.update_command_overlay(&command, &response);
        self.logger.emit(
            "command_finish",
            json!({
                "command": command.clone(),
                "session": self.session.session_id,
                "workspace": self.cwd.display().to_string(),
            }),
        );
        Ok(Some(response))
    }

    fn update_command_overlay(&mut self, command: &str, response: &str) {
        if should_show_info_overlay(command, response) {
            self.info_scroll_offset = 0;
            self.info_overlay = Some(InfoOverlay {
                title: command_overlay_title(command),
                body: response.to_string(),
            });
        }
    }
}

fn command_overlay_title(command: &str) -> String {
    match command {
        // Historical UI compatibility: /agent and /agents share the agent management
        // surface, but they must remain distinct registry commands so /agent does not
        // canonicalize to /agents.
        "/agent" => "agents".to_string(),
        _ => command.trim_start_matches('/').replace('-', " "),
    }
}
