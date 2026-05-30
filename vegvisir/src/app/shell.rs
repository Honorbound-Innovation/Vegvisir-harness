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
            if trailing_space && parts.len() >= 2 {
                return Vec::new();
            }
            let prefix = if trailing_space {
                ""
            } else {
                parts.get(1).copied().unwrap_or("")
            };
            return self
                .provider_registry
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
                })
                .collect();
        }
        if raw.starts_with("/model ")
            || raw == "/model "
            || raw.starts_with("/models ")
            || raw == "/models "
        {
            if trailing_space && parts.len() >= 2 {
                return Vec::new();
            }
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
            let provider = &self.session.current_provider;
            let mut models = self.models.by_provider(provider);
            if provider.ends_with("-hbse") {
                let direct_provider = provider.trim_end_matches("-hbse");
                let has_hbse_specific_models =
                    models.iter().any(|model| model.provider == *provider);
                if has_hbse_specific_models {
                    models.retain(|model| {
                        model.provider == *provider || model.provider != direct_provider
                    });
                }
            }
            return models
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
                        Some(if command == "/models" {
                            format!("/models {}", model.name)
                        } else {
                            format!("/model {}", model.name)
                        }),
                    )
                })
                .collect();
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
            "/history" => self.history(),
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
            "/agent" => self.agent_command(&args)?,
            "/attach" => self.attach_command(&args)?,
            "/speech" => self.speech_command(&args)?,
            "/help" => self.help(),
            "/tools" => self.tools_command(&args),
            "/tool-limit" => self.tool_limit_command(&args),
            "/approvals" => self.approvals_command(&args),
            "/skills" => self.skills_command(&args)?,
            "/recall" => self.recall_command(&args)?,
            "/memory" => self.memory_command(&args)?,
            "/remember" => self.remember_command(&args)?,
            "/context" => self.context_command(&args)?,
            "/model-request" => self.model_request_command(&args)?,
            "/models" => self.models_command(&args)?,
            "/model" => self.select_model(&args)?,
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
                title: command.trim_start_matches('/').replace('-', " "),
                body: response.to_string(),
            });
        }
    }
}
