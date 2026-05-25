use super::super::*;

impl TuiApplication {
    pub(crate) fn mcp_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("list") | Some("servers") => {
                if self.mcp_servers.is_empty() {
                    return Ok("No MCP servers configured. Add servers to $VEGVISIR_HOME/mcp.json with HBSE secret refs for authenticated services.".to_string());
                }
                Ok(self
                    .mcp_servers
                    .iter()
                    .map(|server| {
                        format!(
                            "{:<18} {:?} enabled={} tools={} hbse_refs={}{}",
                            server.id,
                            server.transport,
                            server.enabled,
                            server.tools.len(),
                            server.hbse_secret_refs.len(),
                            server
                                .discovery_error
                                .as_ref()
                                .map(|error| format!(" discovery_error={error}"))
                                .unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            Some("show") | Some("view") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /mcp show <id>".to_string());
                };
                let Some(server) = self.mcp_servers.iter().find(|server| server.id == *id) else {
                    return Ok(format!("Unknown MCP server: {id}"));
                };
                Ok(format!(
                    "id={}\ndisplay_name={}\ntransport={:?}\nenabled={}\nurl={}\ncommand={}\nargs={}\nworking_dir={}\nhbse_secret_refs={}\nconsumer={}\npurpose={}\ntools={}\ndiscovery_error={}",
                    server.id,
                    if server.display_name.is_empty() {
                        "-"
                    } else {
                        &server.display_name
                    },
                    server.transport,
                    server.enabled,
                    server.url.as_deref().unwrap_or("-"),
                    server.command.as_deref().unwrap_or("-"),
                    list_or_dash(&server.args),
                    server.working_dir.as_deref().unwrap_or("-"),
                    list_or_dash(&server.hbse_secret_refs),
                    if server.consumer.is_empty() {
                        "-"
                    } else {
                        &server.consumer
                    },
                    if server.purpose.is_empty() {
                        "-"
                    } else {
                        &server.purpose
                    },
                    server
                        .tools
                        .iter()
                        .map(|tool| tool.name.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                    server.discovery_error.as_deref().unwrap_or("-")
                ))
            }
            Some("tools") => {
                let tools = self
                    .tool_registry
                    .schemas()
                    .into_iter()
                    .filter_map(|tool| {
                        tool.get("name")
                            .and_then(Value::as_str)
                            .filter(|name| name.starts_with("mcp::"))
                            .map(|name| name.to_string())
                    })
                    .collect::<Vec<_>>();
                Ok(if tools.is_empty() {
                    "No MCP tools are registered.".to_string()
                } else {
                    tools.join("\n")
                })
            }
            Some("status") | Some("verify") => Ok(self.verify_mcp_checks().join("\n")),
            Some("reload") => {
                self.mcp_servers = load_mcp_servers(&self.data_root)?;
                self.rebuild_tooling_for_cms()?;
                Ok(format!(
                    "Reloaded {} MCP server(s).",
                    self.mcp_servers.len()
                ))
            }
            Some("add-http") => {
                let Some(id) = args.get(1) else {
                    return Ok(
                        "Usage: /mcp add-http <id> <url> <secret_ref> [consumer] [purpose]"
                            .to_string(),
                    );
                };
                let Some(url) = args.get(2) else {
                    return Ok(
                        "Usage: /mcp add-http <id> <url> <secret_ref> [consumer] [purpose]"
                            .to_string(),
                    );
                };
                let Some(secret_ref) = args.get(3) else {
                    return Ok(
                        "Usage: /mcp add-http <id> <url> <secret_ref> [consumer] [purpose]"
                            .to_string(),
                    );
                };
                let id = crate::core::normalize_agent_id(id);
                if id.is_empty() {
                    return Ok(
                        "MCP server id must contain at least one letter or number.".to_string()
                    );
                }
                if !secret_ref.starts_with("secret://") {
                    return Ok("HTTP MCP credentials must be an HBSE secret ref such as secret://vegvisir/mcp/<server>/default.".to_string());
                }
                if contains_secret_like_value(url)
                    || contains_secret_like_value(secret_ref)
                        && !secret_ref.starts_with("secret://")
                {
                    return Ok(
                        "MCP server configuration must not contain plaintext secrets.".to_string(),
                    );
                }
                let consumer = args
                    .get(4)
                    .cloned()
                    .unwrap_or_else(|| format!("vegvisir.mcp.{id}"));
                let purpose = args
                    .get(5)
                    .cloned()
                    .unwrap_or_else(|| "mcp.tool.call".to_string());
                self.upsert_mcp_server(McpServerConfig {
                    id: id.clone(),
                    display_name: id.clone(),
                    transport: McpTransport::Http,
                    command: None,
                    args: Vec::new(),
                    working_dir: None,
                    url: Some(url.to_string()),
                    enabled: true,
                    hbse_secret_refs: vec![secret_ref.to_string()],
                    consumer,
                    purpose,
                    tools: Vec::new(),
                    metadata: BTreeMap::new(),
                    discovery_error: None,
                })?;
                Ok(format!(
                    "Configured HTTP MCP server {id} with HBSE secret ref {secret_ref}."
                ))
            }
            Some("add-http-service") | Some("add-service-http") => {
                let Some(id) = args.get(1) else {
                    return Ok(
                        "Usage: /mcp add-http-service <id> <url> <hbse-service-name>".to_string(),
                    );
                };
                let Some(url) = args.get(2) else {
                    return Ok(
                        "Usage: /mcp add-http-service <id> <url> <hbse-service-name>".to_string(),
                    );
                };
                let Some(service_name) = args.get(3) else {
                    return Ok(
                        "Usage: /mcp add-http-service <id> <url> <hbse-service-name>".to_string(),
                    );
                };
                if contains_secret_like_value(url) {
                    return Ok(
                        "MCP server configuration must not contain plaintext secrets.".to_string(),
                    );
                }
                let id = crate::core::normalize_agent_id(id);
                if id.is_empty() {
                    return Ok(
                        "MCP server id must contain at least one letter or number.".to_string()
                    );
                }
                let service_name = normalize_hbse_ref_segment(service_name, false);
                let Some(service) = self
                    .hbse_services
                    .iter()
                    .find(|service| service.name == service_name)
                else {
                    return Ok(format!(
                        "Unknown HBSE service ref: {service_name}. Use /hbse service add first."
                    ));
                };
                if !service.enabled {
                    return Ok(format!("HBSE service ref {service_name} is disabled."));
                }
                self.upsert_mcp_server(McpServerConfig {
                    id: id.clone(),
                    display_name: id.clone(),
                    transport: McpTransport::Http,
                    command: None,
                    args: Vec::new(),
                    working_dir: None,
                    url: Some(url.to_string()),
                    enabled: true,
                    hbse_secret_refs: vec![service.secret_ref.clone()],
                    consumer: service.consumer.clone(),
                    purpose: service.purpose.clone(),
                    tools: Vec::new(),
                    metadata: BTreeMap::from([(
                        "hbse_service_ref".to_string(),
                        Value::String(service.name.clone()),
                    )]),
                    discovery_error: None,
                })?;
                Ok(format!(
                    "Configured HTTP MCP server {id} from HBSE service ref {service_name}."
                ))
            }
            Some("add-stdio") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /mcp add-stdio <id> <command> [args...]".to_string());
                };
                let Some(command) = args.get(2) else {
                    return Ok("Usage: /mcp add-stdio <id> <command> [args...]".to_string());
                };
                let id = crate::core::normalize_agent_id(id);
                if id.is_empty() {
                    return Ok(
                        "MCP server id must contain at least one letter or number.".to_string()
                    );
                }
                let command_args = args.iter().skip(3).cloned().collect::<Vec<_>>();
                if contains_secret_like_value(command)
                    || command_args
                        .iter()
                        .any(|arg| contains_secret_like_value(arg))
                {
                    return Ok("MCP stdio command configuration must not contain plaintext secrets. Put service credentials in HBSE and reference them through tools or service policies.".to_string());
                }
                self.upsert_mcp_server(McpServerConfig {
                    id: id.clone(),
                    display_name: id.clone(),
                    transport: McpTransport::Stdio,
                    command: Some(command.to_string()),
                    args: command_args,
                    working_dir: None,
                    url: None,
                    enabled: true,
                    hbse_secret_refs: Vec::new(),
                    consumer: String::new(),
                    purpose: String::new(),
                    tools: Vec::new(),
                    metadata: BTreeMap::new(),
                    discovery_error: None,
                })?;
                Ok(format!("Configured stdio MCP server {id}."))
            }
            Some("add-tool") => {
                let Some(server_id) = args.get(1) else {
                    return Ok(
                        "Usage: /mcp add-tool <server-id> <tool-name> [description]".to_string()
                    );
                };
                let Some(tool_name) = args.get(2) else {
                    return Ok(
                        "Usage: /mcp add-tool <server-id> <tool-name> [description]".to_string()
                    );
                };
                let description = args
                    .get(3..)
                    .map(|items| items.join(" "))
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| format!("MCP tool {tool_name}"));
                let Some(server) = self
                    .mcp_servers
                    .iter_mut()
                    .find(|server| server.id == *server_id)
                else {
                    return Ok(format!("Unknown MCP server: {server_id}"));
                };
                if !server.tools.iter().any(|tool| tool.name == *tool_name) {
                    server.tools.push(McpToolConfig {
                        name: tool_name.to_string(),
                        description,
                        schema: json!({"properties": {}}),
                    });
                }
                self.save_mcp_config_and_reload()?;
                Ok(format!("Added MCP tool {tool_name} to server {server_id}."))
            }
            Some("remove-tool") | Some("delete-tool") => {
                let Some(server_id) = args.get(1) else {
                    return Ok("Usage: /mcp remove-tool <server-id> <tool-name>".to_string());
                };
                let Some(tool_name) = args.get(2) else {
                    return Ok("Usage: /mcp remove-tool <server-id> <tool-name>".to_string());
                };
                let Some(server) = self
                    .mcp_servers
                    .iter_mut()
                    .find(|server| server.id == *server_id)
                else {
                    return Ok(format!("Unknown MCP server: {server_id}"));
                };
                let before = server.tools.len();
                server.tools.retain(|tool| tool.name != *tool_name);
                if server.tools.len() == before {
                    return Ok(format!(
                        "Unknown MCP tool {tool_name} on server {server_id}."
                    ));
                }
                self.save_mcp_config_and_reload()?;
                Ok(format!(
                    "Removed MCP tool {tool_name} from server {server_id}."
                ))
            }
            Some("remove") | Some("delete") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /mcp remove <id>".to_string());
                };
                let before = self.mcp_servers.len();
                self.mcp_servers.retain(|server| server.id != *id);
                if self.mcp_servers.len() == before {
                    return Ok(format!("Unknown MCP server: {id}"));
                }
                self.save_mcp_config_and_reload()?;
                Ok(format!("Removed MCP server {id}."))
            }
            Some("enable") | Some("disable") => {
                let enable = args.first().map(String::as_str) == Some("enable");
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /mcp <enable|disable> <id>".to_string());
                };
                let Some(server) = self.mcp_servers.iter_mut().find(|server| server.id == *id)
                else {
                    return Ok(format!("Unknown MCP server: {id}"));
                };
                server.enabled = enable;
                self.save_mcp_config_and_reload()?;
                Ok(format!(
                    "{} MCP server {id}.",
                    if enable { "Enabled" } else { "Disabled" }
                ))
            }
            Some(other) => Ok(format!("Unknown /mcp command: {other}")),
        }
    }

    fn upsert_mcp_server(&mut self, server: McpServerConfig) -> anyhow::Result<()> {
        if let Some(existing) = self
            .mcp_servers
            .iter_mut()
            .find(|existing| existing.id == server.id)
        {
            *existing = server;
        } else {
            self.mcp_servers.push(server);
        }
        self.save_mcp_config_and_reload()
    }

    fn save_mcp_config_and_reload(&mut self) -> anyhow::Result<()> {
        McpConfigStore::new(self.data_root.join("mcp.json")).save(&self.mcp_servers)?;
        self.mcp_servers = load_mcp_servers(&self.data_root)?;
        self.rebuild_tooling_for_cms()?;
        Ok(())
    }

    pub(crate) fn hbse_command(&mut self, args: &[String]) -> String {
        match args.first().map(String::as_str) {
            None | Some("status") => {
                let providers = self
                    .provider_registry
                    .list()
                    .into_iter()
                    .filter(|provider| provider.auth_type == "hbse")
                    .map(|provider| {
                        let socket = crate::provider::hbse_default_or_configured_socket(provider);
                        format!(
                            "{:<14} socket={} exists={}",
                            provider.name,
                            socket.display(),
                            socket.exists()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    "HBSE is the auth/secrets layer. Vegvisir only handles secret refs and broker policies; plaintext secrets must be entered into HBSE.\n{providers}\nregistered_services={}\nUsage: /hbse onboard [provider|all]\nUsage: /hbse provider <openai|xai|openrouter|groq|mistral|deepseek|together|perplexity|anthropic|google>\nUsage: /hbse mcp <server> [url] [consumer] [purpose]\nUsage: /hbse service <name> [consumer] [purpose]\nUsage: /hbse service add <name> <secret_ref> [consumer] [purpose]\nUsage: /hbse services",
                    self.hbse_services.len()
                )
            }
            Some("onboard") | Some("setup") => {
                let provider = args.get(1).map(String::as_str).unwrap_or("all");
                hbse_onboarding_script_setup(provider)
            }
            Some("provider") => {
                let Some(provider_id) = args.get(1) else {
                    return "Usage: /hbse provider <provider-id>".to_string();
                };
                hbse_model_provider_setup(provider_id)
            }
            Some("service") | Some("tool") => {
                if args.get(1).map(String::as_str) == Some("add") {
                    return self.hbse_service_add_command(args);
                }
                if matches!(args.get(1).map(String::as_str), Some("show" | "get")) {
                    return self.hbse_service_show_command(args);
                }
                if matches!(args.get(1).map(String::as_str), Some("enable" | "disable")) {
                    return self.hbse_service_toggle_command(args);
                }
                if matches!(args.get(1).map(String::as_str), Some("remove" | "delete")) {
                    return self.hbse_service_remove_command(args);
                }
                let Some(name) = args.get(1) else {
                    return "Usage: /hbse service <name> [consumer] [purpose] | /hbse service add|show|enable|disable|remove".to_string();
                };
                let consumer = args
                    .get(2)
                    .cloned()
                    .unwrap_or_else(|| format!("vegvisir.service.{name}"));
                let purpose = args
                    .get(3)
                    .cloned()
                    .unwrap_or_else(|| "service.access".to_string());
                hbse_service_setup(name, &consumer, &purpose)
            }
            Some("services") => self.hbse_services_command(),
            Some("mcp") => {
                let Some(name) = args.get(1) else {
                    return "Usage: /hbse mcp <server> [url] [consumer] [purpose]".to_string();
                };
                let maybe_url = args
                    .get(2)
                    .filter(|value| value.starts_with("http://") || value.starts_with("https://"));
                let consumer_index = if maybe_url.is_some() { 3 } else { 2 };
                let purpose_index = consumer_index + 1;
                let consumer = args
                    .get(consumer_index)
                    .cloned()
                    .unwrap_or_else(|| format!("vegvisir.mcp.{name}"));
                let purpose = args
                    .get(purpose_index)
                    .cloned()
                    .unwrap_or_else(|| "mcp.tool.call".to_string());
                hbse_mcp_setup(name, maybe_url.map(String::as_str), &consumer, &purpose)
            }
            Some(other) => format!("Unknown /hbse command: {other}"),
        }
    }

    fn hbse_services_command(&self) -> String {
        if self.hbse_services.is_empty() {
            return "No HBSE service refs registered. Use /hbse service add <name> <secret_ref> [consumer] [purpose].".to_string();
        }
        self.hbse_services
            .iter()
            .map(|service| {
                format!(
                    "{:<18} enabled={} secret_ref={} consumer={} purpose={}",
                    service.name,
                    service.enabled,
                    service.secret_ref,
                    service.consumer,
                    service.purpose
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn hbse_service_show_command(&self, args: &[String]) -> String {
        let Some(name) = args.get(2) else {
            return "Usage: /hbse service show <name>".to_string();
        };
        let name = normalize_hbse_ref_segment(name, false);
        let Some(service) = self
            .hbse_services
            .iter()
            .find(|service| service.name == name)
        else {
            return format!("Unknown HBSE service ref: {name}");
        };
        format!(
            "name={}\nenabled={}\nsecret_ref={}\nconsumer={}\npurpose={}\nmetadata_keys={}",
            service.name,
            service.enabled,
            service.secret_ref,
            service.consumer,
            service.purpose,
            service
                .metadata
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    fn hbse_service_toggle_command(&mut self, args: &[String]) -> String {
        let Some(action) = args.get(1).map(String::as_str) else {
            return "Usage: /hbse service <enable|disable> <name>".to_string();
        };
        let Some(name) = args.get(2) else {
            return "Usage: /hbse service <enable|disable> <name>".to_string();
        };
        let name = normalize_hbse_ref_segment(name, false);
        let Some(service) = self
            .hbse_services
            .iter_mut()
            .find(|service| service.name == name)
        else {
            return format!("Unknown HBSE service ref: {name}");
        };
        service.enabled = action == "enable";
        match self.save_hbse_services() {
            Ok(()) => format!(
                "{} HBSE service ref {name}.",
                if action == "enable" {
                    "Enabled"
                } else {
                    "Disabled"
                }
            ),
            Err(error) => format!("Failed to save HBSE service refs: {error}"),
        }
    }

    fn hbse_service_add_command(&mut self, args: &[String]) -> String {
        let Some(name) = args.get(2) else {
            return "Usage: /hbse service add <name> <secret_ref> [consumer] [purpose]".to_string();
        };
        let Some(secret_ref) = args.get(3) else {
            return "Usage: /hbse service add <name> <secret_ref> [consumer] [purpose]".to_string();
        };
        if !secret_ref.starts_with("secret://") {
            return "HBSE service refs must use secret:// references, not plaintext credentials."
                .to_string();
        }
        let name = normalize_hbse_ref_segment(name, false);
        if name.is_empty() {
            return "HBSE service name must contain at least one letter or number.".to_string();
        }
        let consumer = args
            .get(4)
            .cloned()
            .unwrap_or_else(|| format!("vegvisir.service.{name}"));
        let purpose = args
            .get(5)
            .cloned()
            .unwrap_or_else(|| "service.access".to_string());
        let service = HbseServiceRef {
            name: name.clone(),
            secret_ref: secret_ref.to_string(),
            consumer,
            purpose,
            enabled: true,
            metadata: BTreeMap::new(),
        };
        if let Some(existing) = self
            .hbse_services
            .iter_mut()
            .find(|existing| existing.name == service.name)
        {
            *existing = service;
        } else {
            self.hbse_services.push(service);
        }
        match self.save_hbse_services() {
            Ok(()) => format!("Registered HBSE service ref {name}."),
            Err(error) => format!("Failed to save HBSE service ref {name}: {error}"),
        }
    }

    fn hbse_service_remove_command(&mut self, args: &[String]) -> String {
        let Some(name) = args.get(2) else {
            return "Usage: /hbse service remove <name>".to_string();
        };
        let name = normalize_hbse_ref_segment(name, false);
        let before = self.hbse_services.len();
        self.hbse_services.retain(|service| service.name != name);
        if self.hbse_services.len() == before {
            return format!("Unknown HBSE service ref: {name}");
        }
        match self.save_hbse_services() {
            Ok(()) => format!("Removed HBSE service ref {name}."),
            Err(error) => format!("Failed to save HBSE service refs: {error}"),
        }
    }

    fn save_hbse_services(&self) -> anyhow::Result<()> {
        HbseServiceRefStore::new(self.data_root.join("hbse-services.json"))
            .save(&self.hbse_services)?;
        Ok(())
    }
}

fn hbse_model_provider_setup(provider_id: &str) -> String {
    let secret_ref = match provider_id {
        "openai" => "secret://vegvisir/providers/openai/default".to_string(),
        "xai" => "secret://vegvisir/providers/xai/default".to_string(),
        other => format!("secret://vegvisir/providers/{other}/default"),
    };
    let consumer = match provider_id {
        "openai" => "vegvisir.provider.openai-hbse".to_string(),
        "xai" => "vegvisir.provider.xai-hbse".to_string(),
        other => format!("vegvisir.provider.{other}-hbse"),
    };
    format!(
        "Vegvisir will not read or store the provider secret. Prefer the deterministic onboarding helper when available:\n\nscripts/hbse-provider-onboard.sh {provider_id}\n\nManual equivalent:\n\nhbse model-provider setup {provider_id} --stdin --secret-ref {secret_ref} --consumer {consumer} --purpose model.chat --model-discovery-purpose model.discovery\n\nAfter setup, select the HBSE-routed provider in Vegvisir, for example /provider {provider_id}-hbse when that provider exists in the catalog."
    )
}

fn hbse_onboarding_script_setup(provider: &str) -> String {
    format!(
        "Use the deterministic HBSE onboarding helper from the Vegvisir repo root:\n\nscripts/hbse-provider-onboard.sh {provider}\n\nIt prompts for provider secrets outside model chat, writes them into HBSE, installs chat/discovery broker policies, and verifies model.discovery policy access. T3 can call the same onboarding metadata through the hbse.onboarding.providers bridge method."
    )
}

fn hbse_service_setup(name: &str, consumer: &str, purpose: &str) -> String {
    let normalized = normalize_hbse_ref_segment(name, true);
    let secret_ref = format!("secret://vegvisir/services/{normalized}/default");
    let policy_id = format!("vegvisir-service-{normalized}");
    let policy = json!({
        "policy_id": policy_id,
        "secret_refs": [secret_ref],
        "allowed_consumers": [consumer],
        "denied_consumers": [],
        "allowed_purposes": [purpose],
        "denied_purposes": [],
        "allowed_delivery_modes": ["brokered_operation"],
        "allowed_http_hosts": [],
        "denied_http_hosts": [],
        "allowed_http_methods": [],
        "denied_http_methods": [],
        "allowed_http_path_prefixes": [],
        "denied_http_path_prefixes": [],
        "require_https_for_brokered_http": true,
        "max_http_request_body_bytes": null,
        "allowed_os_uids": [],
        "denied_os_uids": [],
        "allowed_executable_paths": [],
        "denied_executable_paths": [],
        "allowed_executable_sha256": [],
        "denied_executable_sha256": [],
        "exportable": false,
        "max_ticket_ttl_seconds": 60,
        "max_uses": 1,
        "minimum_provider_assurance": "A1",
        "require_mfa": false,
        "expires_at": null
    });
    format!(
        "Vegvisir will not read or store the service secret. Run these in a trusted terminal and paste the secret into HBSE stdin, then press Ctrl-D:\n\nhbse secret put {secret_ref} --stdin\n\nhbse policy put --stdin <<'JSON'\n{}\nJSON\n\nUse secret_ref={secret_ref}, consumer={consumer}, purpose={purpose} from Vegvisir tools or service adapters.",
        serde_json::to_string_pretty(&policy).unwrap_or_else(|_| policy.to_string())
    )
}

fn hbse_mcp_setup(name: &str, url: Option<&str>, consumer: &str, purpose: &str) -> String {
    let normalized = normalize_hbse_ref_segment(name, false);
    let secret_ref = format!("secret://vegvisir/mcp/{normalized}/default");
    let policy_id = format!("vegvisir-mcp-{normalized}");
    let host = url.and_then(http_host_from_url);
    let path_prefix = url
        .and_then(http_path_prefix_from_url)
        .map(|prefix| vec![prefix])
        .unwrap_or_default();
    let policy = json!({
        "policy_id": policy_id,
        "secret_refs": [secret_ref],
        "allowed_consumers": [consumer],
        "denied_consumers": [],
        "allowed_purposes": [purpose],
        "denied_purposes": [],
        "allowed_delivery_modes": ["brokered_http"],
        "allowed_http_hosts": host.map(|value| vec![value]).unwrap_or_default(),
        "denied_http_hosts": [],
        "allowed_http_methods": ["GET", "POST", "DELETE"],
        "denied_http_methods": [],
        "allowed_http_path_prefixes": path_prefix,
        "denied_http_path_prefixes": [],
        "require_https_for_brokered_http": url.map(|value| value.starts_with("https://")).unwrap_or(true),
        "max_http_request_body_bytes": 10485760,
        "allowed_os_uids": [],
        "denied_os_uids": [],
        "allowed_executable_paths": [],
        "denied_executable_paths": [],
        "allowed_executable_sha256": [],
        "denied_executable_sha256": [],
        "exportable": false,
        "max_ticket_ttl_seconds": 60,
        "max_uses": 1,
        "minimum_provider_assurance": "A1",
        "require_mfa": false,
        "expires_at": null
    });
    let url_hint = url
        .map(|value| format!(" for {value}"))
        .unwrap_or_else(|| " for the MCP server URL you configure in /mcp add-http".to_string());
    format!(
        "Vegvisir will not read or store the MCP service credential. Run these in a trusted terminal and paste the credential into HBSE stdin, then press Ctrl-D:\n\nhbse secret put {secret_ref} --stdin\n\nhbse policy put --stdin <<'JSON'\n{}\nJSON\n\nThen configure the MCP server in Vegvisir:\n/mcp add-http {normalized} <url> {secret_ref} {consumer} {purpose}\n\nThis grants only brokered HTTP access{url_hint}; Vegvisir stores the secret ref, consumer, and purpose, never the credential.",
        serde_json::to_string_pretty(&policy).unwrap_or_else(|_| policy.to_string())
    )
}

fn normalize_hbse_ref_segment(value: &str, allow_slash: bool) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric()
                || matches!(ch, '-' | '_' | '.')
                || (allow_slash && ch == '/')
            {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
}

fn http_host_from_url(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority)
        .split(':')
        .next()
        .unwrap_or(authority)
        .trim();
    (!host.is_empty()).then(|| host.to_string())
}

fn http_path_prefix_from_url(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://")?.1;
    let path = after_scheme
        .split_once('/')
        .map(|(_, path)| format!("/{path}"))
        .unwrap_or_else(|| "/".to_string());
    let path = path.split(['?', '#']).next().unwrap_or("/");
    Some(if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    })
}
