use serde_json::{Value, json};

use crate::{
    core::{AgentProfile, default_system_prompt},
    memory::VegvisirCms,
    policy::RuntimePolicy,
};

use super::super::{
    TuiApplication, comma_items, configured_user_id, list_or_dash, self_model_invalid,
    workspace_project_id,
};

impl TuiApplication {
    pub(crate) fn try_handle_natural_agent_template_request(
        &mut self,
        content: &str,
    ) -> Option<String> {
        let lower = content.to_ascii_lowercase();
        if !lower.contains("template")
            || !lower.contains("agent")
            || !(lower.contains("create") || lower.contains("make"))
        {
            return None;
        }
        let template = agent_templates()
            .into_iter()
            .find(|template| lower.contains(&format!("{} template", template.mode)))?;
        let specialization = natural_agent_specialization(content);
        let display_name = natural_agent_display_name(&template, &specialization);
        let id = natural_agent_id(&template, &specialization);
        let mut profile = AgentProfile::new(&id, &display_name, &template.system_prompt).ok()?;
        profile.mode = template.mode.clone();
        profile.description = if specialization.is_empty() {
            template.description.clone()
        } else {
            format!(
                "{} Specialization: {}.",
                template.description, specialization
            )
        };
        profile.system_prompt = if specialization.is_empty() {
            template.system_prompt.clone()
        } else {
            format!(
                "{}\n\nSpecialization: {}. Stay within authorized scope, produce concrete findings and mitigations, and do not request or expose secrets.",
                template.system_prompt, specialization
            )
        };
        profile.enabled_tools = template.enabled_tools.clone();
        profile.enabled_skills = template.enabled_skills.clone();
        profile.usrl_contracts = template.usrl_contracts.clone();
        profile.memory_policy = template.memory_policy.clone();
        profile
            .metadata
            .insert("template".to_string(), Value::String(template.mode.clone()));
        profile
            .metadata
            .insert("created_from_natural_request".to_string(), json!(true));
        let path = match self.agents.save(&profile) {
            Ok(path) => path,
            Err(error) => return Some(format!("Command failed: {error}")),
        };
        Some(format!(
            "Created agent {} from template {} at {}\nUse /agent use {} to activate it, or /agent show {} to inspect it.",
            profile.id,
            template.mode,
            path.display(),
            profile.id,
            profile.id
        ))
    }

    pub(crate) fn agent_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("list") => {
                let profiles = self.agents.list()?;
                if profiles.is_empty() {
                    return Ok("No custom agents are defined. Use /agent create <id> | <mode> | <display name> | <system prompt>.".to_string());
                }
                Ok(profiles
                    .into_iter()
                    .map(|profile| {
                        let active = if self.session.active_agent_id.as_deref() == Some(&profile.id)
                        {
                            "*"
                        } else {
                            " "
                        };
                        format!(
                            "{active} {:<20} mode={:<14} {} cms={}/{}",
                            profile.id,
                            profile.mode,
                            profile.display_name,
                            profile.cms_user_id,
                            profile.cms_project_id
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            Some("templates") | Some("modes") => Ok(agent_templates()
                .into_iter()
                .map(|template| {
                    format!(
                        "{:<14} {:<22} tools={}",
                        template.mode,
                        template.display_name,
                        template.enabled_tools.join(",")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")),
            Some("design") | Some("designer") => self.agent_design_command(args),
            Some("create-template") | Some("from-template") => {
                let Some(mode) = args.get(1) else {
                    return Ok(
                        "Usage: /agent create-template <mode> <id> [display name]".to_string()
                    );
                };
                let Some(id) = args.get(2) else {
                    return Ok(
                        "Usage: /agent create-template <mode> <id> [display name]".to_string()
                    );
                };
                let Some(template) = agent_template(mode) else {
                    return Ok(format!(
                        "Unknown agent template: {mode}\nAvailable templates:\n{}",
                        agent_templates()
                            .into_iter()
                            .map(|template| template.mode)
                            .collect::<Vec<_>>()
                            .join("\n")
                    ));
                };
                let display_name = if args.len() > 3 {
                    args[3..].join(" ")
                } else {
                    template.display_name.clone()
                };
                let mut profile = AgentProfile::new(id, &display_name, &template.system_prompt)?;
                profile.mode = template.mode.clone();
                profile.description = template.description.clone();
                profile.enabled_tools = template.enabled_tools.clone();
                profile.enabled_skills = template.enabled_skills.clone();
                profile.usrl_contracts = template.usrl_contracts.clone();
                profile.memory_policy = template.memory_policy.clone();
                profile
                    .metadata
                    .insert("template".to_string(), Value::String(template.mode.clone()));
                let path = self.agents.save(&profile)?;
                Ok(format!(
                    "Created agent {} from template {} at {}",
                    profile.id,
                    template.mode,
                    path.display()
                ))
            }
            Some("create") | Some("new") => {
                let raw = args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
                let parts = raw.split('|').map(str::trim).collect::<Vec<_>>();
                if parts.len() < 3 || parts.iter().any(|part| part.is_empty()) {
                    return Ok(
                        "Usage: /agent create <id> | <mode> | <display name> | <system prompt>"
                            .to_string(),
                    );
                }
                let mut profile = if parts.len() >= 4 {
                    let mut profile =
                        AgentProfile::new(parts[0], parts[2], parts[3..].join(" | "))?;
                    profile.mode = parts[1].to_string();
                    profile
                } else {
                    AgentProfile::new(parts[0], parts[1], parts[2..].join(" | "))?
                };
                if profile.mode.trim().is_empty() {
                    profile.mode = "custom".to_string();
                }
                let path = self.agents.save(&profile)?;
                Ok(format!(
                    "Created agent {} ({}, mode={}) at {}",
                    profile.id,
                    profile.display_name,
                    profile.mode,
                    path.display()
                ))
            }
            Some("use") | Some("select") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent use <id>".to_string());
                };
                let profile = self.agents.load(id)?;
                self.apply_agent_profile(&profile)
            }
            Some("clone") => {
                let Some(source_id) = args.get(1) else {
                    return Ok(
                        "Usage: /agent clone <source-id> <new-id> [display name]".to_string()
                    );
                };
                let Some(new_id) = args.get(2) else {
                    return Ok(
                        "Usage: /agent clone <source-id> <new-id> [display name]".to_string()
                    );
                };
                let mut profile = self.agents.load(source_id)?;
                let normalized = crate::core::normalize_agent_id(new_id);
                if normalized.is_empty() {
                    return Ok(
                        "New agent id must contain at least one letter or number.".to_string()
                    );
                }
                profile.id = normalized;
                if args.len() > 3 {
                    profile.display_name = args[3..].join(" ");
                }
                let cms_scope = format!("agent:{}", profile.id);
                profile.cms_user_id = cms_scope.clone();
                profile.cms_project_id = cms_scope;
                profile.created_at = chrono::Utc::now();
                profile.updated_at = profile.created_at;
                let path = self.agents.save(&profile)?;
                Ok(format!(
                    "Cloned agent {source_id} to {} at {}",
                    profile.id,
                    path.display()
                ))
            }
            Some("export") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent export <id> [path]".to_string());
                };
                let profile = self.agents.load(id)?;
                if let Some(path) = args.get(2) {
                    let path = self.resolve_workspace_path(path);
                    std::fs::write(&path, serde_json::to_string_pretty(&profile)?)?;
                    Ok(format!(
                        "Exported agent {} to {}",
                        profile.id,
                        path.display()
                    ))
                } else {
                    Ok(serde_json::to_string_pretty(&profile)?)
                }
            }
            Some("import") => {
                let Some(path) = args.get(1) else {
                    return Ok("Usage: /agent import <path>".to_string());
                };
                let path = self.resolve_workspace_path(path);
                let mut profile: AgentProfile =
                    serde_json::from_str(&std::fs::read_to_string(&path)?)?;
                profile.id = crate::core::normalize_agent_id(&profile.id);
                if profile.id.is_empty() {
                    return Ok(
                        "Imported agent id must contain at least one letter or number.".to_string(),
                    );
                }
                profile.updated_at = chrono::Utc::now();
                let saved = self.agents.save(&profile)?;
                Ok(format!(
                    "Imported agent {} to {}",
                    profile.id,
                    saved.display()
                ))
            }
            Some("show") | Some("view") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent show <id>".to_string());
                };
                let profile = self.agents.load(id)?;
                Ok(format!(
                    "agent: {}\nmode: {}\nname: {}\ndescription: {}\ncms_user_id: {}\ncms_project_id: {}\nprovider: {}\nmodel: {}\ntools: {}\nskills: {}\nmcp_servers: {}\nusrl_contracts: {}\nsystem_prompt:\n{}",
                    profile.id,
                    profile.mode,
                    profile.display_name,
                    if profile.description.is_empty() {
                        "-"
                    } else {
                        &profile.description
                    },
                    profile.cms_user_id,
                    profile.cms_project_id,
                    profile.current_provider.as_deref().unwrap_or("-"),
                    profile.current_model.as_deref().unwrap_or("-"),
                    list_or_dash(&profile.enabled_tools),
                    list_or_dash(&profile.enabled_skills),
                    list_or_dash(&profile.enabled_mcp_servers),
                    list_or_dash(&profile.usrl_contracts),
                    profile.system_prompt
                ))
            }
            Some("mode") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent mode <id> <mode>".to_string());
                };
                let Some(mode) = args.get(2) else {
                    return Ok("Usage: /agent mode <id> <mode>".to_string());
                };
                let mut profile = self.agents.load(id)?;
                profile.mode = mode.to_string();
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Set agent {} mode to {}.",
                    profile.id, profile.mode
                ))
            }
            Some("provider") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent provider <id> <provider|->".to_string());
                };
                let Some(provider) = args.get(2) else {
                    return Ok("Usage: /agent provider <id> <provider|->".to_string());
                };
                let mut profile = self.agents.load(id)?;
                if provider == "-" || provider == "clear" {
                    profile.current_provider = None;
                    profile.current_model = None;
                } else {
                    let Some(config) = self.provider_registry.get(provider) else {
                        return Ok(format!("Unknown provider: {provider}"));
                    };
                    profile.current_provider = Some(config.name.clone());
                    if profile
                        .current_model
                        .as_deref()
                        .and_then(|model| self.models.get(model))
                        .filter(|model| self.models.is_model_allowed_for_provider(model, provider))
                        .is_none()
                    {
                        profile.current_model = self
                            .models
                            .default_for_provider(provider)
                            .map(|model| model.name.clone());
                    }
                }
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Set agent {} provider to {}.",
                    profile.id,
                    profile.current_provider.as_deref().unwrap_or("-")
                ))
            }
            Some("model") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent model <id> <model|->".to_string());
                };
                let Some(model) = args.get(2) else {
                    return Ok("Usage: /agent model <id> <model|->".to_string());
                };
                let mut profile = self.agents.load(id)?;
                if model == "-" || model == "clear" {
                    profile.current_model = None;
                } else {
                    let Some(model_info) = self.models.get(model) else {
                        return Ok(format!("Unknown model: {model}"));
                    };
                    let provider = profile
                        .current_provider
                        .as_deref()
                        .unwrap_or(&self.session.current_provider);
                    if !self
                        .models
                        .is_model_allowed_for_provider(model_info, provider)
                    {
                        return Ok(format!(
                            "Model {} is not available for agent provider {}. Set /agent provider {} {} first.",
                            model_info.name, provider, profile.id, model_info.provider
                        ));
                    }
                    profile.current_model = Some(model_info.name.clone());
                    if profile.current_provider.is_none() {
                        profile.current_provider = Some(provider.to_string());
                    }
                }
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Set agent {} model to {}.",
                    profile.id,
                    profile.current_model.as_deref().unwrap_or("-")
                ))
            }
            Some("prompt") | Some("system") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent prompt <id> <system prompt>".to_string());
                };
                if args.len() < 3 {
                    return Ok("Usage: /agent prompt <id> <system prompt>".to_string());
                }
                let mut profile = self.agents.load(id)?;
                profile.system_prompt = args[2..].join(" ");
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!("Updated agent {} system prompt.", profile.id))
            }
            Some("describe") | Some("description") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent describe <id> <description>".to_string());
                };
                if args.len() < 3 {
                    return Ok("Usage: /agent describe <id> <description>".to_string());
                }
                let mut profile = self.agents.load(id)?;
                profile.description = args[2..].join(" ");
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!("Updated agent {} description.", profile.id))
            }
            Some("bind-usrl") | Some("usrl") => {
                let Some(id) = args.get(1) else {
                    return Ok(
                        "Usage: /agent bind-usrl <id> <contract-id-or-skill-name>".to_string()
                    );
                };
                let Some(contract) = args.get(2) else {
                    return Ok(
                        "Usage: /agent bind-usrl <id> <contract-id-or-skill-name>".to_string()
                    );
                };
                let mut profile = self.agents.load(id)?;
                let contracts = self.resolve_usrl_contract_refs(contract);
                for resolved in &contracts {
                    if !profile.usrl_contracts.contains(resolved) {
                        profile.usrl_contracts.push(resolved.clone());
                    }
                }
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Bound USRL contract(s) {} to agent {}.",
                    contracts.join(","),
                    profile.id
                ))
            }
            Some("unbind-usrl") => {
                let Some(id) = args.get(1) else {
                    return Ok(
                        "Usage: /agent unbind-usrl <id> <contract-id-or-skill-name>".to_string()
                    );
                };
                let Some(contract) = args.get(2) else {
                    return Ok(
                        "Usage: /agent unbind-usrl <id> <contract-id-or-skill-name>".to_string()
                    );
                };
                let mut profile = self.agents.load(id)?;
                let contracts = self.resolve_usrl_contract_refs(contract);
                profile
                    .usrl_contracts
                    .retain(|item| !contracts.iter().any(|contract| contract == item));
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Unbound USRL contract(s) {} from agent {}.",
                    contracts.join(","),
                    profile.id
                ))
            }
            Some("allow-mcp") | Some("mcp") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent allow-mcp <id> <server-id>".to_string());
                };
                let Some(server) = args.get(2) else {
                    return Ok("Usage: /agent allow-mcp <id> <server-id>".to_string());
                };
                let mut profile = self.agents.load(id)?;
                if !profile.enabled_mcp_servers.contains(server) {
                    profile.enabled_mcp_servers.push(server.to_string());
                }
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Allowed MCP server {server} for agent {}.",
                    profile.id
                ))
            }
            Some("revoke-mcp") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent revoke-mcp <id> <server-id>".to_string());
                };
                let Some(server) = args.get(2) else {
                    return Ok("Usage: /agent revoke-mcp <id> <server-id>".to_string());
                };
                let mut profile = self.agents.load(id)?;
                profile.enabled_mcp_servers.retain(|item| item != server);
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!(
                    "Revoked MCP server {server} from agent {}.",
                    profile.id
                ))
            }
            Some("enable-skill") | Some("skill") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent enable-skill <id> <skill-name>".to_string());
                };
                let Some(skill) = args.get(2) else {
                    return Ok("Usage: /agent enable-skill <id> <skill-name>".to_string());
                };
                if !self
                    .session
                    .enabled_skills
                    .iter()
                    .any(|item| item.name == *skill)
                {
                    return Ok(format!("Unknown skill: {skill}"));
                }
                let mut profile = self.agents.load(id)?;
                if !profile.enabled_skills.contains(skill) {
                    profile.enabled_skills.push(skill.to_string());
                }
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!("Enabled skill {skill} for agent {}.", profile.id))
            }
            Some("disable-skill") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent disable-skill <id> <skill-name>".to_string());
                };
                let Some(skill) = args.get(2) else {
                    return Ok("Usage: /agent disable-skill <id> <skill-name>".to_string());
                };
                let mut profile = self.agents.load(id)?;
                profile.enabled_skills.retain(|item| item != skill);
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!("Disabled skill {skill} for agent {}.", profile.id))
            }
            Some("allow-tool") | Some("tool") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent allow-tool <id> <tool-name>".to_string());
                };
                let Some(tool) = args.get(2) else {
                    return Ok("Usage: /agent allow-tool <id> <tool-name>".to_string());
                };
                if tool != "*" && self.tool_registry.get(tool).is_err() {
                    return Ok(format!("Unknown tool: {tool}"));
                }
                let mut profile = self.agents.load(id)?;
                if !profile.enabled_tools.contains(tool) {
                    profile.enabled_tools.push(tool.to_string());
                }
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!("Allowed tool {tool} for agent {}.", profile.id))
            }
            Some("revoke-tool") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent revoke-tool <id> <tool-name>".to_string());
                };
                let Some(tool) = args.get(2) else {
                    return Ok("Usage: /agent revoke-tool <id> <tool-name>".to_string());
                };
                let mut profile = self.agents.load(id)?;
                profile.enabled_tools.retain(|item| item != tool);
                profile.updated_at = chrono::Utc::now();
                self.save_agent_profile_and_refresh_if_active(&profile)?;
                Ok(format!("Revoked tool {tool} from agent {}.", profile.id))
            }
            Some("clear") | Some("default") => {
                self.clear_active_agent()?;
                Ok("Custom agent cleared. Using default Vegvisir memory scope.".to_string())
            }
            Some("delete") | Some("remove") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /agent delete <id>".to_string());
                };
                let path = self.agents.delete(id)?;
                if self.session.active_agent_id.as_deref() == Some(id) {
                    self.clear_active_agent()?;
                }
                Ok(format!("Deleted agent {id} at {}", path.display()))
            }
            Some(other) => match self.agents.load(other) {
                Ok(profile) => self.apply_agent_profile(&profile),
                Err(_) => Ok(format!(
                    "Unknown /agent command or agent id: {other}\nUse /agent templates to list templates, /agent list to list saved agents, or /agent use <id> to activate an agent."
                )),
            },
        }
    }

    fn agent_design_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let raw = args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
        let parts = raw.split('|').map(str::trim).collect::<Vec<_>>();
        if parts.len() < 4 || parts[..4].iter().any(|part| part.is_empty()) {
            return Ok("Usage: /agent design <id> | <mode> | <display name> | <system prompt> | tools=a,b skills=s,b mcp=server usrl=contract provider=id model=id use=true".to_string());
        }
        let id = parts[0];
        let mode = crate::core::normalize_agent_id(parts[1]);
        if mode.is_empty() {
            return Ok("Agent mode must contain at least one letter or number.".to_string());
        }
        let display_name = parts[2];
        let system_prompt = parts[3];
        let mut profile = AgentProfile::new(id, display_name, system_prompt)?;
        profile.mode = mode.clone();
        profile.description = format!("Designed {} agent.", mode);
        if let Some(template) = agent_template(&mode) {
            profile.description = template.description;
            profile.enabled_tools = template.enabled_tools;
            profile.enabled_skills = template.enabled_skills;
            profile.usrl_contracts = template.usrl_contracts;
            profile.memory_policy = template.memory_policy;
            profile
                .metadata
                .insert("template".to_string(), Value::String(template.mode));
        }

        let options = parts
            .get(4..)
            .map(|items| items.join(" "))
            .unwrap_or_default();
        let mut activate = false;
        for token in options.split_whitespace() {
            if matches!(token, "use" | "activate" | "use=true" | "activate=true") {
                activate = true;
                continue;
            }
            let Some((key, value)) = token.split_once('=') else {
                continue;
            };
            match key {
                "description" | "desc" => {
                    profile.description = value.replace('_', " ");
                }
                "tools" => {
                    let tools = comma_items(value);
                    for tool in &tools {
                        if tool != "*" && self.tool_registry.get(tool).is_err() {
                            return Ok(format!("Unknown tool for designed agent: {tool}"));
                        }
                    }
                    profile.enabled_tools = tools;
                }
                "skills" => {
                    let skills = comma_items(value);
                    for skill in &skills {
                        if !self
                            .session
                            .enabled_skills
                            .iter()
                            .any(|item| item.name == *skill)
                        {
                            return Ok(format!("Unknown skill for designed agent: {skill}"));
                        }
                    }
                    profile.enabled_skills = skills;
                }
                "mcp" | "mcp_servers" => {
                    let servers = comma_items(value);
                    for server in &servers {
                        if !self.mcp_servers.iter().any(|item| item.id == *server) {
                            return Ok(format!("Unknown MCP server for designed agent: {server}"));
                        }
                    }
                    profile.enabled_mcp_servers = servers;
                }
                "usrl" | "contracts" => {
                    let mut contracts = Vec::new();
                    for item in comma_items(value) {
                        for resolved in self.resolve_usrl_contract_refs(&item) {
                            if !contracts.contains(&resolved) {
                                contracts.push(resolved);
                            }
                        }
                    }
                    profile.usrl_contracts = contracts;
                }
                "provider" => {
                    if value != "-" && self.provider_registry.get(value).is_none() {
                        return Ok(format!("Unknown provider for designed agent: {value}"));
                    }
                    profile.current_provider = (value != "-").then(|| value.to_string());
                }
                "model" => {
                    if value != "-" && self.models.get(value).is_none() {
                        return Ok(format!("Unknown model for designed agent: {value}"));
                    }
                    profile.current_model = (value != "-").then(|| value.to_string());
                }
                "memory" | "memory_policy" => {
                    profile.memory_policy = value.to_string();
                }
                _ => {}
            }
        }
        if let (Some(provider), Some(model)) = (&profile.current_provider, &profile.current_model)
            && self_model_invalid(&self.models, provider, model)
        {
            return Ok(format!(
                "Model {model} is not available for designed agent provider {provider}."
            ));
        }
        profile
            .metadata
            .insert("designed".to_string(), Value::Bool(true));
        profile.updated_at = chrono::Utc::now();
        let path = self.agents.save(&profile)?;
        if activate {
            let message = self.apply_agent_profile(&profile)?;
            Ok(format!(
                "Designed agent {} at {}\n{}",
                profile.id,
                path.display(),
                message
            ))
        } else {
            Ok(format!(
                "Designed agent {} ({}, mode={}) at {}",
                profile.id,
                profile.display_name,
                profile.mode,
                path.display()
            ))
        }
    }

    pub(crate) fn apply_agent_profile(&mut self, profile: &AgentProfile) -> anyhow::Result<String> {
        self.session.active_agent_id = Some(profile.id.clone());
        self.session.active_agent_name = Some(profile.display_name.clone());
        self.session.system_prompt = self.effective_agent_system_prompt(profile);
        if let Some(provider) = &profile.current_provider
            && self.provider_registry.get(provider).is_some()
        {
            self.session.current_provider = provider.clone();
        }
        if let Some(model) = &profile.current_model
            && let Some(model_info) = self.models.get(model)
            && self
                .models
                .is_model_allowed_for_provider(model_info, &self.session.current_provider)
        {
            self.session.current_model = model.clone();
            if let Some(context_window) = model_info.context_window {
                self.session.context_limit = context_window;
            }
        }
        let mut config = self.cms.config.clone();
        config.user_id = profile.cms_user_id.clone();
        config.project_id = Some(profile.cms_project_id.clone());
        self.cms = VegvisirCms::open(config)?;
        self.tool_executor.runtime_policy = RuntimePolicy {
            active_agent_id: Some(profile.id.clone()),
            active_agent_mode: Some(profile.mode.clone()),
            allowed_tools: profile.enabled_tools.clone(),
            usrl_contracts: profile.usrl_contracts.clone(),
            usrl_rules: self.usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_rules"),
            usrl_constraints: self
                .usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_constraints"),
            usrl_stages: self.usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_stages"),
            usrl_triggers: self
                .usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_triggers"),
            strict_usrl: !profile.usrl_contracts.is_empty(),
        };
        self.rebuild_tooling_for_cms()?;
        Ok(format!(
            "Using agent {} ({}, mode={}). System prompt and CMS memory scope applied.",
            profile.id, profile.display_name, profile.mode
        ))
    }

    fn clear_active_agent(&mut self) -> anyhow::Result<()> {
        self.session.active_agent_id = None;
        self.session.active_agent_name = None;
        self.session.system_prompt = self
            .config
            .load()
            .ok()
            .and_then(|defaults| {
                defaults
                    .get("system_prompt")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(default_system_prompt);
        let mut config = self.cms.config.clone();
        config.user_id = self.default_user_id();
        config.project_id = Some(workspace_project_id(&self.cwd));
        self.cms = VegvisirCms::open(config)?;
        self.tool_executor.runtime_policy = RuntimePolicy::default();
        self.rebuild_tooling_for_cms()?;
        Ok(())
    }

    pub(crate) fn default_user_id(&self) -> String {
        self.config
            .load()
            .ok()
            .map(|defaults| configured_user_id(&defaults))
            .unwrap_or_else(|| "local-user".to_string())
    }

    fn save_agent_profile_and_refresh_if_active(
        &mut self,
        profile: &AgentProfile,
    ) -> anyhow::Result<()> {
        self.agents.save(profile)?;
        if self.session.active_agent_id.as_deref() == Some(&profile.id) {
            self.refresh_active_agent_profile(profile)?;
        }
        Ok(())
    }

    fn refresh_active_agent_profile(&mut self, profile: &AgentProfile) -> anyhow::Result<()> {
        self.session.active_agent_name = Some(profile.display_name.clone());
        self.session.system_prompt = self.effective_agent_system_prompt(profile);
        self.tool_executor.runtime_policy = RuntimePolicy {
            active_agent_id: Some(profile.id.clone()),
            active_agent_mode: Some(profile.mode.clone()),
            allowed_tools: profile.enabled_tools.clone(),
            usrl_contracts: profile.usrl_contracts.clone(),
            usrl_rules: self.usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_rules"),
            usrl_constraints: self
                .usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_constraints"),
            usrl_stages: self.usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_stages"),
            usrl_triggers: self
                .usrl_metadata_for_contracts(&profile.usrl_contracts, "usrl_triggers"),
            strict_usrl: !profile.usrl_contracts.is_empty(),
        };
        self.rebuild_tooling_for_cms()?;
        Ok(())
    }

    fn effective_agent_system_prompt(&self, profile: &AgentProfile) -> String {
        let mut sections = vec![profile.system_prompt.trim().to_string()]
            .into_iter()
            .filter(|section| !section.is_empty())
            .collect::<Vec<_>>();
        let skill_sections = profile
            .enabled_skills
            .iter()
            .filter_map(|name| {
                self.session
                    .enabled_skills
                    .iter()
                    .find(|skill| skill.name == *name)
            })
            .filter(|skill| {
                skill.kind == "markdown"
                    || skill.kind == "lsl_subskill"
                    || skill.metadata.get("format").and_then(Value::as_str) == Some("markdown")
                    || skill.metadata.get("format").and_then(Value::as_str) == Some("lsl")
            })
            .filter_map(|skill| {
                let body = skill.metadata.get("body").and_then(Value::as_str)?;
                let body = body.trim();
                if body.is_empty() {
                    None
                } else {
                    Some(format!("Skill: {}\n{}", skill.name, body))
                }
            })
            .collect::<Vec<_>>();
        if !skill_sections.is_empty() {
            sections.push(format!(
                "Enabled agent skills:\n{}",
                skill_sections.join("\n\n")
            ));
        }
        sections.join("\n\n")
    }

    fn resolve_usrl_contract_refs(&self, value: &str) -> Vec<String> {
        let Some(skill) = self.session.enabled_skills.iter().find(|skill| {
            skill.name == value
                && (skill.kind == "usrl_contract"
                    || skill.metadata.get("format").and_then(Value::as_str) == Some("usrl"))
        }) else {
            return vec![value.to_string()];
        };
        let contracts = skill
            .metadata
            .get("usrl_contracts")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if contracts.is_empty() {
            vec![value.to_string()]
        } else {
            contracts
        }
    }
}

#[derive(Clone)]
struct AgentTemplate {
    mode: String,
    display_name: String,
    description: String,
    system_prompt: String,
    enabled_tools: Vec<String>,
    enabled_skills: Vec<String>,
    usrl_contracts: Vec<String>,
    memory_policy: String,
}

fn agent_template(mode: &str) -> Option<AgentTemplate> {
    let normalized = crate::core::normalize_agent_id(mode);
    agent_templates()
        .into_iter()
        .find(|template| template.mode == normalized)
}

fn agent_templates() -> Vec<AgentTemplate> {
    vec![
        template(
            "planner",
            "Planner",
            "Decomposes goals into staged, verifiable plans.",
            "You are a planning specialist. Convert ambiguous goals into concrete phases, dependencies, risks, acceptance checks, and next actions. Do not edit files unless explicitly asked through an enabled tool path.",
            &[
                "list_files",
                "read_file",
                "cms_recall",
                "cms_recent",
                "cms_search_chatgpt_archive",
                "cms_prepare_context",
                "save_session",
            ],
        ),
        template(
            "researcher",
            "Researcher",
            "Finds, compares, and summarizes project evidence.",
            "You are a research specialist. Gather relevant local context, distinguish evidence from inference, cite files or memories when available, and produce concise findings with uncertainty called out.",
            &[
                "list_files",
                "read_file",
                "cms_recall",
                "cms_recent",
                "cms_search_chatgpt_archive",
                "cms_remember",
                "cms_prepare_context",
            ],
        ),
        template(
            "orchestrator",
            "Orchestrator",
            "Coordinates specialist agents and tracks execution state.",
            "You are an orchestration specialist. Break work into bounded tasks, delegate when useful, merge results, maintain task state, and keep execution aligned with the user's current objective.",
            &[
                "list_files",
                "read_file",
                "cms_recall",
                "cms_recent",
                "cms_search_chatgpt_archive",
                "cms_prepare_context",
                "spawn_subagent",
                "save_session",
                "audit_log",
            ],
        ),
        template(
            "engineer",
            "Engineer",
            "Implements scoped code changes with verification.",
            "You are an engineering specialist. Read the surrounding code before changing it, make minimal coherent edits, preserve existing behavior unless intentionally changed, and verify with focused tests.",
            &[
                "list_files",
                "read_file",
                "write_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_search_chatgpt_archive",
                "cms_remember",
                "cms_prepare_context",
                "audit_log",
            ],
        ),
        template(
            "coder",
            "Coder",
            "Focuses on implementation details and local patches.",
            "You are a coding specialist. Implement the requested behavior directly, keep patches small, follow local style, and report the exact verification performed.",
            &[
                "list_files",
                "read_file",
                "write_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_search_chatgpt_archive",
                "cms_remember",
            ],
        ),
        template(
            "tester",
            "Tester",
            "Designs and runs verification for changed behavior.",
            "You are a testing specialist. Identify behavioral risk, add or run targeted tests, explain failures in terms of expected versus actual behavior, and avoid unrelated rewrites.",
            &[
                "list_files",
                "read_file",
                "write_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_search_chatgpt_archive",
                "cms_remember",
                "audit_log",
            ],
        ),
        template(
            "agent-red",
            "Agent Red",
            "Security-oriented review and adversarial analysis.",
            "You are Agent Red, a security specialist. Focus on abuse cases, privilege boundaries, secret handling, injection paths, unsafe execution, and concrete mitigations. Treat secrets and credentials as out of scope for direct access.",
            &[
                "list_files",
                "read_file",
                "run_command",
                "run_tests",
                "cms_recall",
                "cms_search_chatgpt_archive",
                "cms_remember",
                "cms_prepare_context",
                "audit_log",
            ],
        ),
    ]
}

fn natural_agent_specialization(content: &str) -> String {
    let lower = content.to_ascii_lowercase();
    let start = lower
        .find("specializing in")
        .map(|index| index + "specializing in".len())
        .or_else(|| {
            lower
                .find("specialized in")
                .map(|index| index + "specialized in".len())
        })
        .or_else(|| {
            lower
                .find("focused on")
                .map(|index| index + "focused on".len())
        });
    start
        .map(|index| {
            content[index..]
                .trim()
                .trim_matches(|ch: char| ch == '.' || ch == ',' || ch == ';' || ch.is_whitespace())
                .to_string()
        })
        .unwrap_or_default()
}

fn natural_agent_display_name(template: &AgentTemplate, specialization: &str) -> String {
    if template.mode == "agent-red" && specialization.to_ascii_lowercase().contains("security") {
        "Agent Red Security Auditor".to_string()
    } else if specialization.is_empty() {
        template.display_name.clone()
    } else {
        let words = specialization
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .take(4)
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>();
        if words.is_empty() {
            template.display_name.clone()
        } else {
            format!("{} {}", template.display_name, words.join(" "))
        }
    }
}

fn natural_agent_id(template: &AgentTemplate, specialization: &str) -> String {
    if template.mode == "agent-red" && specialization.to_ascii_lowercase().contains("security") {
        "agent-red-security-auditor".to_string()
    } else {
        let suffix = crate::core::normalize_agent_id(specialization);
        if suffix.is_empty() {
            template.mode.clone()
        } else {
            format!("{}-{}", template.mode, suffix)
        }
    }
}

fn template(
    mode: &str,
    display_name: &str,
    description: &str,
    system_prompt: &str,
    enabled_tools: &[&str],
) -> AgentTemplate {
    AgentTemplate {
        mode: mode.to_string(),
        display_name: display_name.to_string(),
        description: description.to_string(),
        system_prompt: system_prompt.to_string(),
        enabled_tools: enabled_tools.iter().map(|tool| tool.to_string()).collect(),
        enabled_skills: Vec::new(),
        usrl_contracts: Vec::new(),
        memory_policy: "agent-scoped".to_string(),
    }
}
