use super::super::*;

impl TuiApplication {
    pub(crate) fn model_request_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok("Usage: /model-request <message>".to_string());
        }
        let content = args.join(" ");
        let (model_content, _) = self.prepare_lsl_for_content(&content)?;
        let envelope = self.cms.prepare_cached_prompt(
            model_content,
            self.session.current_provider.clone(),
            self.session.current_model.clone(),
        )?;
        let prompt = if self.session.system_prompt.trim().is_empty() {
            envelope.model_request.prompt.clone()
        } else {
            format!(
                "Harness system prompt:\n{}\n\n{}",
                self.session.system_prompt.trim(),
                envelope.model_request.prompt
            )
        };
        Ok(format!(
            "prompt_cache_key: {}\ncacheable_prefix_tokens: {}\ntotal_prompt_tokens: {}\n\n{}",
            envelope.manifest.prompt_cache_key,
            envelope.manifest.cacheable_prefix_tokens,
            envelope.manifest.total_prompt_tokens,
            prompt
        ))
    }

    pub(crate) fn models_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if !args.is_empty() {
            return self.select_model(args);
        }
        let refresh_note = self.refresh_provider_models(&self.session.current_provider.clone());
        let models = self.models.by_provider(&self.session.current_provider);
        let availability = self.provider_registry.availability();
        let state = if availability
            .get(&self.session.current_provider)
            .copied()
            .unwrap_or(false)
        {
            "ready"
        } else {
            "needs-auth"
        };
        let mut lines = vec![format!(
            "Available models for {} [{}]:",
            self.session.current_provider, state
        )];
        if let Some(refresh_note) = refresh_note {
            lines.push(format!("  {refresh_note}"));
        }
        for model in models {
            let active = if model.name == self.session.current_model {
                "*"
            } else {
                " "
            };
            let context = model
                .context_window
                .map(|value| format!("{value} ctx"))
                .unwrap_or_else(|| "ctx unknown".to_string());
            lines.push(format!("  {active} {:<34} {context}", model.name));
        }
        lines.push(
            "Use /provider <name> to switch provider. Use /model <name> to switch model."
                .to_string(),
        );
        self.input.set_buffer("/model ");
        self.input.paste_char_count = 0;
        self.input.selected_suggestion = 0;
        Ok(lines.join("\n"))
    }

    pub(crate) fn select_model(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok(format!("Current model: {}", self.session.current_model));
        }
        let global = args
            .iter()
            .any(|arg| arg == "--global" || arg == "--default");
        let name = args
            .iter()
            .filter(|arg| !matches!(arg.as_str(), "--global" | "--default"))
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        if self.models.get(&name).is_none() {
            let _ = self.refresh_provider_models(&self.session.current_provider.clone());
        }
        let Some(model) = self.models.get(&name) else {
            let matches = close_model_matches(&self.models, &self.session.current_provider, &name);
            if !matches.is_empty() {
                return Ok(format!(
                    "Unknown model for provider {}. Close matches:\n{}",
                    self.session.current_provider,
                    matches.join("\n")
                ));
            }
            return Ok(format!(
                "Unknown model for provider {}: {name}",
                self.session.current_provider
            ));
        };
        if !self
            .models
            .is_model_allowed_for_provider(model, &self.session.current_provider)
        {
            return Ok(format!(
                "Model {} belongs to provider {}, but selected provider is {}.\nRun /provider {} first, then /model {}.",
                model.name,
                model.provider,
                self.session.current_provider,
                model.provider,
                model.name
            ));
        }
        self.session.current_model = model.name.clone();
        if let Some(context_window) = model.context_window {
            self.session.context_limit = context_window;
        }
        if global {
            self.save_global_model_defaults()?;
            self.clear_workspace_provider_override()?;
        } else {
            self.save_workspace_provider_override()?;
        }
        Ok(format!(
            "Selected model {} via provider {} ({}).",
            model.name,
            model.provider,
            if global {
                "global default"
            } else {
                "project override"
            }
        ))
    }

    pub(crate) fn provider_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok(format!(
                "Current provider: {}",
                self.session.current_provider
            ));
        }
        let global = args
            .iter()
            .any(|arg| arg == "--global" || arg == "--default");
        let filtered = args
            .iter()
            .filter(|arg| !matches!(arg.as_str(), "--global" | "--default"))
            .cloned()
            .collect::<Vec<_>>();
        let Some(name) = filtered.first() else {
            return Ok(format!(
                "Current provider: {}",
                self.session.current_provider
            ));
        };
        let Some(provider) = self.provider_registry.get(name).cloned() else {
            let matches = close_provider_matches(&self.provider_registry, name);
            if !matches.is_empty() {
                return Ok(format!(
                    "Unknown provider. Close matches:\n{}",
                    matches.join("\n")
                ));
            }
            return Ok(format!("Unknown provider: {name}"));
        };
        if provider.auth_type == "api_key" && !direct_provider_auth_allowed() {
            return Ok(format!(
                "Direct API-key provider auth is disabled in production mode for {}.\nConfigure the secret in HBSE with /hbse provider {}, then select the HBSE-routed provider when available, for example /provider {}-hbse.",
                provider.display_name.as_deref().unwrap_or(&provider.name),
                canonical_hbse_provider_id(&provider.name),
                canonical_hbse_provider_id(&provider.name)
            ));
        }
        self.session.current_provider = provider.name.clone();
        let refresh_note = self.refresh_provider_models(&provider.name);
        if let Some(model) = self.models.default_for_provider(&provider.name) {
            self.session.current_model = model.name.clone();
            if let Some(context_window) = model.context_window {
                self.session.context_limit = context_window;
            }
        }
        if global {
            self.save_global_model_defaults()?;
            self.clear_workspace_provider_override()?;
        } else {
            self.save_workspace_provider_override()?;
        }
        if provider.name == "openai-sso" {
            let selected = format!(
                "Selected provider {}; active model is {} ({}).",
                provider.name,
                self.session.current_model,
                if global {
                    "global default"
                } else {
                    "project override"
                }
            );
            if self
                .openai_sso_status()
                .starts_with("OpenAI SSO is logged in")
            {
                return Ok(selected);
            }
            let auth_result = self.openai_sso_login();
            return Ok(format!("{selected}\n{auth_result}"));
        }
        let notice = api_key_notice(&provider);
        Ok([
            format!(
                "Selected provider {}; active model is {} ({}).",
                provider.name,
                self.session.current_model,
                if global {
                    "global default"
                } else {
                    "project override"
                }
            ),
            refresh_note.unwrap_or_default(),
            notice,
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n"))
    }

    pub(crate) fn providers_command(&self) -> String {
        let availability = self.provider_registry.availability();
        let mut lines = vec!["Providers:".to_string()];
        for provider in self.provider_registry.list() {
            let status = if availability.get(&provider.name).copied().unwrap_or(false) {
                "ready"
            } else {
                "needs-auth"
            };
            let auth_detail = provider_auth_detail(provider);
            lines.push(format!(
                "{:<14} {:<10} auth={}{}",
                provider.name, status, provider.auth_type, auth_detail
            ));
        }
        lines.join("\n")
    }

    pub(crate) fn refresh_provider_models(&mut self, provider_name: &str) -> Option<String> {
        let provider = self.provider_registry.get(provider_name)?.clone();
        if provider.kind == "demo" {
            return None;
        }
        match discover_provider_models(&provider) {
            Ok(discovered) if discovered.is_empty() => {
                Some(format!("No models returned by {provider_name}."))
            }
            Ok(discovered) => {
                let count = discovered.len();
                self.models
                    .replace_provider_models(provider_name, discovered);
                if provider_name == self.session.current_provider
                    && self
                        .models
                        .get(&self.session.current_model)
                        .filter(|model| {
                            self.models
                                .is_model_allowed_for_provider(model, provider_name)
                        })
                        .is_none()
                {
                    if let Some(default) = self.models.default_for_provider(provider_name) {
                        self.session.current_model = default.name.clone();
                        if let Some(context_window) = default.context_window {
                            self.session.context_limit = context_window;
                        }
                    }
                }
                Some(format!("Refreshed {count} model(s) from {provider_name}."))
            }
            Err(error) => Some(error.to_string()),
        }
    }

    pub(crate) fn refresh_all_provider_models(&mut self) -> Vec<String> {
        let provider_names: Vec<String> = self
            .provider_registry
            .list()
            .into_iter()
            .filter(|provider| provider.enabled && provider.kind != "demo")
            .map(|provider| provider.name.clone())
            .collect();
        provider_names
            .into_iter()
            .filter_map(|provider| {
                self.refresh_provider_models(&provider)
                    .map(|note| format!("{provider}: {note}"))
            })
            .collect()
    }

    pub(crate) fn auth_command(&self, args: &[String]) -> String {
        let Some(provider_name) = args.first() else {
            return "Usage: /auth <provider>. Example: /auth openai, /auth openai-sso, /auth openai-sso-status, or /auth openai-sso-logout".to_string();
        };
        if provider_name == "openai-sso-status" {
            return self.openai_sso_status();
        }
        if provider_name == "openai-sso-logout" {
            let _ =
                crate::openai_sso::OpenAISsoAuthStore::new(Some(self.data_root.clone())).clear();
            return "OpenAI SSO auth removed from Vegvisir.".to_string();
        }
        let Some(provider) = self.provider_registry.get(provider_name) else {
            return format!("Unknown provider: {provider_name}");
        };
        if provider.auth_type == "none" {
            return format!(
                "{} does not require authentication.",
                provider.display_name.as_deref().unwrap_or(&provider.name)
            );
        }
        if provider.name == "openai-sso" {
            return self.openai_sso_login();
        }
        api_key_notice(provider)
    }

    pub(crate) fn verify_command(&self, args: &[String]) -> String {
        let scope = args.first().map(String::as_str).unwrap_or("all");
        let mut checks = Vec::new();
        if matches!(scope, "all" | "auth") {
            checks.extend(self.verify_auth_checks());
        }
        if matches!(scope, "all" | "mcp") {
            checks.extend(self.verify_mcp_checks());
        }
        if matches!(scope, "all" | "agent") {
            checks.extend(self.verify_agent_checks());
        }
        if matches!(scope, "all" | "memory") {
            checks.extend(self.verify_memory_checks());
        }
        if matches!(scope, "all" | "runtime") {
            checks.extend(self.verify_runtime_checks());
        }
        if matches!(scope, "all" | "evals") {
            checks.extend(self.verify_eval_checks());
        }
        if checks.is_empty() {
            return "Usage: /verify [all|auth|mcp|agent|memory|runtime|evals]".to_string();
        }
        checks.join("\n")
    }

    fn verify_auth_checks(&self) -> Vec<String> {
        let mut checks = Vec::new();
        let hbse = self
            .provider_registry
            .list()
            .into_iter()
            .filter(|provider| provider.auth_type == "hbse")
            .collect::<Vec<_>>();
        for provider in hbse {
            let socket = crate::provider::hbse_default_or_configured_socket(provider);
            let secret_ref = provider
                .metadata
                .get("hbse_secret_ref")
                .and_then(Value::as_str)
                .unwrap_or("");
            let status = if socket.exists() && !secret_ref.is_empty() {
                "ok"
            } else {
                "warn"
            };
            checks.push(format!(
                "{status} auth/hbse {} socket={} secret_ref={}",
                provider.name,
                socket.display(),
                if secret_ref.is_empty() {
                    "missing"
                } else {
                    secret_ref
                }
            ));
        }
        for provider in self
            .provider_registry
            .list()
            .into_iter()
            .filter(|provider| provider.auth_type == "api_key")
        {
            if direct_provider_auth_allowed() {
                checks.push(format!(
                    "warn auth/legacy {} uses direct env fallback {}; production should use /hbse provider {}",
                    provider.name,
                    provider.api_key_env.as_deref().unwrap_or("unknown"),
                    canonical_hbse_provider_id(&provider.name)
                ));
            } else {
                checks.push(format!(
                    "ok auth/legacy {} direct env fallback blocked by production mode; use /hbse provider {}",
                    provider.name,
                    canonical_hbse_provider_id(&provider.name)
                ));
            }
        }
        if self.hbse_services.is_empty() {
            checks.push(
                "warn auth/services no HBSE service refs registered; use /hbse service add for tool/service credentials"
                    .to_string(),
            );
        } else {
            checks.push(format!(
                "ok auth/services registered_refs={}",
                self.hbse_services.len()
            ));
        }
        checks
    }

    pub(crate) fn verify_mcp_checks(&self) -> Vec<String> {
        if self.mcp_servers.is_empty() {
            return vec!["warn mcp no servers configured".to_string()];
        }
        let mut checks = self
            .mcp_servers
            .iter()
            .map(|server| {
                if let Some(error) = &server.discovery_error {
                    format!("warn mcp/{} discovery_error={error}", server.id)
                } else if server.transport == crate::core::McpTransport::Http
                    && server.hbse_secret_refs.is_empty()
                {
                    format!(
                        "fail mcp/{} http transport missing HBSE secret ref",
                        server.id
                    )
                } else {
                    format!(
                        "ok mcp/{} transport={:?} tools={} hbse_refs={}",
                        server.id,
                        server.transport,
                        server.tools.len(),
                        server.hbse_secret_refs.len()
                    )
                }
            })
            .collect::<Vec<_>>();
        checks.extend(self.verify_active_agent_mcp_checks());
        checks
    }

    fn verify_active_agent_mcp_checks(&self) -> Vec<String> {
        let Some(agent_id) = &self.session.active_agent_id else {
            return vec![
                "ok mcp/active all configured servers available; no active agent filter"
                    .to_string(),
            ];
        };
        let Ok(profile) = self.agents.load(agent_id) else {
            return vec![format!(
                "fail mcp/active active agent {agent_id} profile could not be loaded"
            )];
        };
        if profile.enabled_mcp_servers.is_empty() {
            return vec![format!(
                "warn mcp/active agent={agent_id} no MCP servers allowed"
            )];
        }
        let configured = self
            .mcp_servers
            .iter()
            .map(|server| server.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let missing = profile
            .enabled_mcp_servers
            .iter()
            .filter(|server| !configured.contains(*server))
            .cloned()
            .collect::<Vec<_>>();
        let active = self.active_mcp_servers();
        let active_tools = active
            .iter()
            .map(|server| server.tools.len())
            .sum::<usize>();
        let mut checks = vec![format!(
            "ok mcp/active agent={agent_id} servers={} tools={active_tools}",
            active
                .iter()
                .map(|server| server.id.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )];
        if !missing.is_empty() {
            checks.push(format!(
                "fail mcp/active agent={agent_id} missing configured server(s): {}",
                missing.join(",")
            ));
        }
        checks
    }

    fn verify_agent_checks(&self) -> Vec<String> {
        let Some(agent_id) = &self.session.active_agent_id else {
            return vec!["warn agent no active custom agent".to_string()];
        };
        let mut checks = Vec::new();
        checks.push(format!(
            "ok agent active={} mode={}",
            agent_id,
            self.tool_executor
                .runtime_policy
                .active_agent_mode
                .as_deref()
                .unwrap_or("custom")
        ));
        checks.push(format!(
            "{} agent/tools allowed={}",
            if self.tool_executor.runtime_policy.allowed_tools.is_empty() {
                "warn"
            } else {
                "ok"
            },
            if self.tool_executor.runtime_policy.allowed_tools.is_empty() {
                "unrestricted".to_string()
            } else {
                self.tool_executor.runtime_policy.allowed_tools.join(",")
            }
        ));
        checks.push(format!(
            "{} agent/usrl contracts={}",
            if self.tool_executor.runtime_policy.usrl_contracts.is_empty() {
                "warn"
            } else {
                "ok"
            },
            if self.tool_executor.runtime_policy.usrl_contracts.is_empty() {
                "none".to_string()
            } else {
                self.tool_executor.runtime_policy.usrl_contracts.join(",")
            }
        ));
        checks.push(format!(
            "{} agent/usrl constraints={}",
            if self
                .tool_executor
                .runtime_policy
                .usrl_constraints
                .is_empty()
            {
                "warn"
            } else {
                "ok"
            },
            if self
                .tool_executor
                .runtime_policy
                .usrl_constraints
                .is_empty()
            {
                "none".to_string()
            } else {
                self.tool_executor.runtime_policy.usrl_constraints.join(",")
            }
        ));
        checks
    }

    fn verify_memory_checks(&self) -> Vec<String> {
        vec![format!(
            "ok memory cms_v2 db={} user_id={} project_id={}",
            self.cms.config.db_path.display(),
            self.cms.config.user_id,
            self.cms.config.project_id.as_deref().unwrap_or("none")
        )]
    }

    fn verify_runtime_checks(&self) -> Vec<String> {
        let approval_path = self.data_root.join("approvals.json");
        let trace_path = self.data_root.join("traces").join("tui.jsonl");
        let subagent_path = self.subagent_board_path();
        let subagents = self.load_subagent_records().unwrap_or_default();
        let sandbox_status = crate::sandbox::CommandSandboxStatus::current(
            self.dangerously_bypass_approvals_and_sandbox,
            self.cwd.clone(),
        );
        let mut checks = vec![
            format!(
                "ok runtime/approvals path={} pending={}",
                approval_path.display(),
                self.tool_executor.guardrails.approvals.pending_len()
            ),
            format!(
                "ok runtime/traces path={} events={}",
                trace_path.display(),
                self.logger.events().len()
            ),
            format!(
                "ok runtime/subagents path={} tasks={}",
                subagent_path.display(),
                subagents.len()
            ),
            "ok runtime/cancel command=/cancel".to_string(),
            format!(
                "ok runtime/dangerous_bypass {} startup_only=true",
                if self.dangerously_bypass_approvals_and_sandbox {
                    "enabled"
                } else {
                    "disabled"
                }
            ),
            format!(
                "ok runtime/user default={} active={} sessions={}",
                self.default_user_id(),
                self.cms.config.user_id,
                self.sessions.store.root.display()
            ),
        ];
        checks.extend(sandbox_status.verify_runtime_lines());
        checks
    }

    fn verify_eval_checks(&self) -> Vec<String> {
        match crate::evals::run_builtin_evals("golden") {
            Ok(results) => {
                let passed = results.iter().filter(|result| result.passed).count();
                let total = results.len();
                let status = if passed == total { "ok" } else { "fail" };
                vec![format!(
                    "{} evals/golden passed={} total={}",
                    status, passed, total
                )]
            }
            Err(error) => vec![format!("fail evals/golden error={error}")],
        }
    }
}

fn api_key_notice(provider: &ProviderConfig) -> String {
    let Some(env) = &provider.api_key_env else {
        return String::new();
    };
    if provider.auth_type != "api_key" {
        return String::new();
    }
    if !direct_provider_auth_allowed() {
        return format!(
            "{} direct API-key auth is disabled in production mode.\nConfigure the secret in HBSE with /hbse provider {}, then select the HBSE-routed provider when available, for example: /provider {}-hbse",
            provider.display_name.as_deref().unwrap_or(&provider.name),
            canonical_hbse_provider_id(&provider.name),
            canonical_hbse_provider_id(&provider.name)
        );
    }
    if get_env(env).is_some() {
        return format!(
            "{env} environment variable is set for legacy direct-provider use.\nProduction auth should use HBSE instead. Run: /hbse provider {}",
            canonical_hbse_provider_id(&provider.name)
        );
    }
    format!(
        "{} direct API-key auth is a legacy fallback and {env} is not set.\nProduction auth must be configured in HBSE so Vegvisir never sees the secret. Run: /hbse provider {}\nThen select the HBSE-routed provider when available, for example: /provider {}-hbse",
        provider.display_name.as_deref().unwrap_or(&provider.name),
        canonical_hbse_provider_id(&provider.name),
        canonical_hbse_provider_id(&provider.name)
    )
}

fn provider_auth_detail(provider: &ProviderConfig) -> String {
    match provider.auth_type.as_str() {
        "hbse" => provider
            .metadata
            .get("hbse_secret_ref")
            .and_then(Value::as_str)
            .map(|secret_ref| format!(" secret_ref={secret_ref}"))
            .unwrap_or_else(|| " secret_ref=missing".to_string()),
        "api_key" => provider
            .api_key_env
            .as_deref()
            .map(|env| {
                if direct_provider_auth_allowed() {
                    format!(
                        " legacy_env={env} hbse=/hbse provider {}",
                        canonical_hbse_provider_id(&provider.name)
                    )
                } else {
                    format!(
                        " legacy_env={env} blocked_by_production hbse=/hbse provider {}",
                        canonical_hbse_provider_id(&provider.name)
                    )
                }
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn canonical_hbse_provider_id(provider_name: &str) -> &str {
    provider_name.strip_suffix("-hbse").unwrap_or(provider_name)
}

fn close_provider_matches(registry: &ProviderRegistry, name: &str) -> Vec<String> {
    let needle = name.to_ascii_lowercase();
    registry
        .list()
        .into_iter()
        .filter(|provider| provider.name.to_ascii_lowercase().contains(&needle))
        .take(10)
        .map(|provider| provider.name.clone())
        .collect()
}

fn close_model_matches(registry: &ModelRegistry, provider: &str, name: &str) -> Vec<String> {
    let needle = name.to_ascii_lowercase();
    let mut matches = registry
        .by_provider(provider)
        .into_iter()
        .filter(|model| model.name.to_ascii_lowercase().contains(&needle))
        .map(|model| model.name.clone())
        .collect::<Vec<_>>();
    if matches.is_empty() {
        matches = registry
            .list()
            .into_iter()
            .filter(|model| model.name.to_ascii_lowercase().contains(&needle))
            .map(|model| model.name.clone())
            .collect();
    }
    matches.truncate(10);
    matches
}
