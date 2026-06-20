use std::{collections::BTreeSet, path::Path};

use anyhow::bail;
use serde_json::Value;

use crate::{
    core::{
        AgentProfile, McpConfigStore, ModelInfo, ModelRegistry, ProviderConfig, ProviderRegistry,
        default_tool_definitions, load_skill_definitions, normalize_agent_id,
    },
    model_discovery::discover_provider_models,
};

use super::{ValidationReport, issue_with_details, secret_like, secret_like_pattern};

pub fn validate_profile(
    profile: &AgentProfile,
    workspace: &Path,
    data_root: &Path,
) -> anyhow::Result<ValidationReport> {
    let providers = ProviderRegistry::default_catalog()?;
    let mut models = ModelRegistry::default_catalog()?;
    let tools = default_tool_definitions()?;
    let skills = load_skill_definitions(workspace, data_root)?;
    let mcp_servers = McpConfigStore::new(data_root.join("mcp.json"))
        .load()
        .unwrap_or_default();
    let tool_names = tools
        .into_iter()
        .map(|tool| tool.name)
        .collect::<BTreeSet<_>>();
    let skill_names = skills
        .into_iter()
        .map(|skill| skill.name)
        .collect::<BTreeSet<_>>();
    let mcp_ids = mcp_servers
        .into_iter()
        .map(|server| server.id)
        .collect::<BTreeSet<_>>();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut recommendations = Vec::new();

    if profile.id.trim().is_empty() {
        errors.push(issue_with_details(
            "error",
            "id",
            "agent id is empty",
            ["trigger: id is blank after trimming whitespace"],
        ));
    }
    let normalized_id = normalize_agent_id(&profile.id);
    if normalized_id != profile.id {
        errors.push(issue_with_details(
            "error",
            "id",
            "agent id is not normalized",
            [
                format!("value: {}", quote_detail(&profile.id)),
                format!("expected: {}", quote_detail(&normalized_id)),
                "rule: agent ids must match normalize_agent_id(id)".to_string(),
            ],
        ));
    }
    if profile.display_name.trim().is_empty() {
        errors.push(issue_with_details(
            "error",
            "display_name",
            "display name is empty",
            ["trigger: display_name is blank after trimming whitespace"],
        ));
    }
    if profile.system_prompt.trim().is_empty() {
        errors.push(issue_with_details(
            "error",
            "system_prompt",
            "system prompt is empty",
            ["trigger: system_prompt is blank after trimming whitespace"],
        ));
    }
    if secret_like(&profile.system_prompt) {
        let pattern = secret_like_pattern(&profile.system_prompt).unwrap_or("<unknown>");
        errors.push(issue_with_details(
            "error",
            "system_prompt",
            "prompt appears to contain secret-like material",
            [
                format!("matched pattern: {}", quote_detail(pattern)),
                "snippet: suppressed to avoid echoing possible secret material".to_string(),
                "rule: system_prompt must not contain inline credentials or secret-like tokens"
                    .to_string(),
            ],
        ));
    }
    if profile.description.trim().is_empty() {
        recommendations.push(issue_with_details(
            "recommendation",
            "description",
            "add a concise description for registry operators",
            ["trigger: description is blank after trimming whitespace"],
        ));
    }
    if profile
        .metadata
        .get("primary_scope")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        recommendations.push(issue_with_details(
            "recommendation",
            "metadata.primary_scope",
            "set a primary scope for better delegation and filtering",
            ["trigger: metadata.primary_scope is missing or blank"],
        ));
    }
    if profile.memory_policy.trim().is_empty() {
        warnings.push(issue_with_details(
            "warning",
            "memory_policy",
            "memory policy is empty",
            ["trigger: memory_policy is blank after trimming whitespace"],
        ));
    }
    if profile.cms_user_id.trim().is_empty() || profile.cms_project_id.trim().is_empty() {
        errors.push(issue_with_details(
            "error",
            "cms_scope",
            "CMS user/project ids must be non-empty",
            [
                format!(
                    "cms_user_id blank: {}",
                    profile.cms_user_id.trim().is_empty()
                ),
                format!(
                    "cms_project_id blank: {}",
                    profile.cms_project_id.trim().is_empty()
                ),
            ],
        ));
    }
    if let Some(provider) = &profile.current_provider
        && providers.get(provider).is_none()
    {
        errors.push(issue_with_details(
            "error",
            "current_provider",
            format!("unknown provider: {provider}"),
            [
                format!("value: {}", quote_detail(provider)),
                format!("known providers: {}", provider_names(&providers)),
            ],
        ));
    }
    validate_model_assignment(
        &mut warnings,
        &providers,
        &mut models,
        profile.current_provider.as_deref(),
        profile.current_model.as_deref(),
    );
    for tool in &profile.enabled_tools {
        if tool != "*" && !tool_names.contains(tool) {
            warnings.push(issue_with_details(
                "warning",
                "enabled_tools",
                format!("unknown tool: {tool}"),
                [
                    format!("entry: {}", quote_detail(tool)),
                    format!("known tools: {}", join_limited(&tool_names, 24)),
                ],
            ));
        }
        if tool == "*" {
            warnings.push(issue_with_details(
                "warning",
                "enabled_tools",
                "wildcard tool access should be used only for trusted operator-reviewed agents",
                [
                    "entry: \"*\"".to_string(),
                    "rule: wildcard grants access to every registered tool".to_string(),
                ],
            ));
        }
    }
    for skill in &profile.enabled_skills {
        if !skill_names.contains(skill) {
            warnings.push(issue_with_details(
                "warning",
                "enabled_skills",
                format!("unknown skill in current workspace/data root: {skill}"),
                [
                    format!("entry: {}", quote_detail(skill)),
                    format!("known skills: {}", join_limited(&skill_names, 24)),
                ],
            ));
        }
    }
    for server in &profile.enabled_mcp_servers {
        if !mcp_ids.contains(server) {
            warnings.push(issue_with_details(
                "warning",
                "enabled_mcp_servers",
                format!("unknown MCP server in data root mcp.json: {server}"),
                [
                    format!("entry: {}", quote_detail(server)),
                    format!("known MCP servers: {}", join_limited(&mcp_ids, 24)),
                ],
            ));
        }
    }
    if profile.enabled_tools.is_empty() {
        recommendations.push(issue_with_details(
            "recommendation",
            "enabled_tools",
            "agent has no enabled tools; confirm this is intentional",
            ["trigger: enabled_tools list is empty"],
        ));
    }
    Ok(ValidationReport {
        id: profile.id.clone(),
        status: if errors.is_empty() {
            "ready"
        } else {
            "blocked"
        }
        .to_string(),
        errors,
        warnings,
        recommendations,
    })
}

fn validate_model_assignment(
    warnings: &mut Vec<super::ValidationIssue>,
    providers: &ProviderRegistry,
    models: &mut ModelRegistry,
    provider_name: Option<&str>,
    model_name: Option<&str>,
) {
    let Some(model_name) = model_name else {
        return;
    };

    let Some(provider_name) = provider_name else {
        if models.get(model_name).is_none() {
            warnings.push(issue_with_details(
                "warning",
                "current_model",
                format!(
                    "model {model_name} is not in the static model catalog; accepting explicit user-supplied model id because provider is inherited at runtime"
                ),
                [
                    format!("current_model: {}", quote_detail(model_name)),
                    "current_provider: inherited at runtime".to_string(),
                    "rule: model cannot be provider-compatibility checked without an explicit provider"
                        .to_string(),
                ],
            ));
        } else {
            warnings.push(issue_with_details(
                "warning",
                "current_model",
                "model is set but provider is inherited at runtime",
                [
                    format!("current_model: {}", quote_detail(model_name)),
                    "current_provider: inherited at runtime".to_string(),
                    "rule: model/provider compatibility is deferred until runtime provider resolution"
                        .to_string(),
                ],
            ));
        }
        return;
    };

    let Some(provider) = providers.get(provider_name) else {
        warnings.push(issue_with_details(
            "warning",
            "current_model",
            format!(
                "model {model_name} could not be checked because provider {provider_name} is unknown"
            ),
            [
                format!("current_model: {}", quote_detail(model_name)),
                format!("current_provider: {}", quote_detail(provider_name)),
                format!("known providers: {}", provider_names(providers)),
            ],
        ));
        return;
    };

    let discovery_error = refresh_provider_models_if_needed(models, provider, model_name);
    match models.get(model_name) {
        Some(model_info) if models.is_model_allowed_for_provider(model_info, provider_name) => {}
        Some(model_info) => warnings.push(issue_with_details(
            "warning",
            "current_model",
            format!(
                "model {model_name} is cataloged for provider {} rather than {provider_name}; accepting explicit agent-admin assignment without blocking",
                model_info.provider
            ),
            [
                format!("current_model: {}", quote_detail(model_name)),
                format!("current_provider: {}", quote_detail(provider_name)),
                format!("catalog provider for model: {}", quote_detail(&model_info.provider)),
                "rule: model catalog provider must match current_provider for a clean validation"
                    .to_string(),
            ],
        )),
        None => warnings.push(issue_with_details(
            "warning",
            "current_model",
            model_discovery_fallback_message(provider_name, model_name, discovery_error.clone()),
            model_discovery_details(provider_name, model_name, discovery_error),
        )),
    }
}

fn quote_detail(value: &str) -> String {
    format!("{:?}", value)
}

fn join_limited(values: &BTreeSet<String>, limit: usize) -> String {
    if values.is_empty() {
        return "<none>".to_string();
    }
    let mut items = values.iter().take(limit).cloned().collect::<Vec<_>>();
    if values.len() > limit {
        items.push(format!("... +{} more", values.len() - limit));
    }
    items.join(", ")
}

fn provider_names(providers: &ProviderRegistry) -> String {
    let names = providers
        .list()
        .into_iter()
        .map(|provider| provider.name.clone())
        .collect::<BTreeSet<_>>();
    join_limited(&names, 24)
}

fn model_discovery_details(
    provider_name: &str,
    model_name: &str,
    discovery_error: Option<String>,
) -> Vec<String> {
    let mut details = vec![
        format!("current_model: {}", quote_detail(model_name)),
        format!("current_provider: {}", quote_detail(provider_name)),
        "static catalog lookup: not found for this provider".to_string(),
    ];
    match discovery_error {
        Some(error) => details.push(format!("live discovery error: {error}")),
        None => details.push("live discovery result: model not returned".to_string()),
    }
    details.push(
        "rule: current_model should be present in static or live discovery for current_provider"
            .to_string(),
    );
    details
}

pub fn refresh_provider_models_if_needed(
    models: &mut ModelRegistry,
    provider: &ProviderConfig,
    model_name: &str,
) -> Option<String> {
    let known_allowed = models
        .get(model_name)
        .is_some_and(|model| models.is_model_allowed_for_provider(model, &provider.name));
    if known_allowed {
        return None;
    }
    match discover_provider_models(provider) {
        Ok(discovered) => {
            models.register_many(discovered);
            None
        }
        Err(error) => Some(error.to_string()),
    }
}

pub fn model_discovery_fallback_message(
    provider_name: &str,
    model_name: &str,
    discovery_error: Option<String>,
) -> String {
    match discovery_error {
        Some(error) => format!(
            "model {model_name} is not in the static model catalog and live discovery for provider {provider_name} failed ({error}); accepting explicit user-supplied model id without blocking"
        ),
        None => format!(
            "model {model_name} was not returned by live/static model discovery for provider {provider_name}; accepting explicit user-supplied model id without blocking"
        ),
    }
}

pub fn user_supplied_model_info(provider_name: &str, model_name: &str) -> ModelInfo {
    ModelInfo {
        name: model_name.to_string(),
        provider: provider_name.to_string(),
        display_name: Some(model_name.to_string()),
        context_window: None,
        supports_streaming: true,
        enabled: true,
        metadata: [(
            "source".to_string(),
            Value::String("agent-admin-user-supplied".to_string()),
        )]
        .into_iter()
        .collect(),
    }
}

pub fn validate_tool_allow_list(tools: &[String]) -> anyhow::Result<()> {
    if tools.iter().any(|tool| tool == "*") && tools.len() > 1 {
        bail!("wildcard tool access '*' must be used alone");
    }
    let known = default_tool_definitions()?
        .into_iter()
        .map(|tool| tool.name)
        .collect::<BTreeSet<_>>();
    for tool in tools {
        if tool != "*" && !known.contains(tool) {
            bail!("unknown tool: {tool}");
        }
    }
    Ok(())
}

pub fn validate_skill_allow_list(
    workspace: &Path,
    data_root: &Path,
    skills: &[String],
) -> anyhow::Result<()> {
    let known = load_skill_definitions(workspace, data_root)?
        .into_iter()
        .map(|skill| skill.name)
        .collect::<BTreeSet<_>>();
    for skill in skills {
        if !known.contains(skill) {
            bail!("unknown skill in current workspace/data root: {skill}");
        }
    }
    Ok(())
}

pub fn validate_mcp_server_allow_list(data_root: &Path, servers: &[String]) -> anyhow::Result<()> {
    let known = McpConfigStore::new(data_root.join("mcp.json"))
        .load()?
        .into_iter()
        .map(|server| server.id)
        .collect::<BTreeSet<_>>();
    for server in servers {
        if !known.contains(server) {
            bail!("unknown MCP server in data root mcp.json: {server}");
        }
    }
    Ok(())
}
