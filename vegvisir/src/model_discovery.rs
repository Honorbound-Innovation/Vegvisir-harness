use serde_json::{Value, json};

use crate::{
    core::{ModelInfo, ProviderConfig},
    environment::get_env,
    openai_sso::{codex_base_url, load_fresh_tokens_for_metadata},
    provider::{
        direct_provider_auth_allowed, direct_provider_auth_error, hbse_default_or_configured_socket,
    },
};

pub fn discover_provider_models(provider: &ProviderConfig) -> anyhow::Result<Vec<ModelInfo>> {
    match provider.kind.as_str() {
        "demo" => Ok(Vec::new()),
        "openai" | "openai_compatible" => discover_openai_compatible_models(provider),
        "hbse_openai_compatible" | "hbse_anthropic" => {
            discover_hbse_openai_compatible_models(provider)
        }
        "anthropic" => discover_anthropic_models(provider),
        "google" => discover_google_models(provider),
        "hbse_google" => discover_hbse_google_models(provider),
        "cohere" => discover_bearer_models(
            provider,
            &format!(
                "{}/models",
                provider
                    .base_url
                    .as_deref()
                    .unwrap_or("")
                    .trim_end_matches('/')
            ),
        ),
        "ollama" => discover_ollama_models(provider),
        "openai_sso" => discover_openai_sso_models(provider),
        "azure_openai" => {
            anyhow::bail!(
                "Azure OpenAI model discovery needs an Azure endpoint in provider base_url."
            )
        }
        _ => anyhow::bail!(
            "Provider {} does not define model discovery.",
            provider.name
        ),
    }
}

pub fn discover_openai_compatible_models(
    provider: &ProviderConfig,
) -> anyhow::Result<Vec<ModelInfo>> {
    let base_url = provider.base_url.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Provider {} has no model discovery base_url.",
            provider.name
        )
    })?;
    let mut request = ureq::get(&format!("{}/models", base_url.trim_end_matches('/')))
        .set("Accept", "application/json");
    if let Some(env) = &provider.api_key_env {
        if !direct_provider_auth_allowed() {
            return Err(direct_provider_auth_error(provider));
        }
        let Some(api_key) = get_env(env) else {
            anyhow::bail!(
                "Set {env} to refresh models for {}.",
                provider.display_name.as_deref().unwrap_or(&provider.name)
            );
        };
        request = request.set("Authorization", &format!("Bearer {api_key}"));
    }
    let data = get_json(request, &provider.name)?;
    let raw = data
        .get("data")
        .or_else(|| data.get("models"))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(models_from_raw(&provider.name, &raw))
}

pub fn discover_hbse_openai_compatible_models(
    provider: &ProviderConfig,
) -> anyhow::Result<Vec<ModelInfo>> {
    let base_url = provider.base_url.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Provider {} has no model discovery base_url.",
            provider.name
        )
    })?;
    let mut headers = serde_json::Map::new();
    headers.insert("Accept".to_string(), json!("application/json"));
    if let Some(version) = provider
        .metadata
        .get("anthropic_version")
        .and_then(Value::as_str)
    {
        headers.insert("anthropic-version".to_string(), json!(version));
    }
    let body = hbse_model_discovery_request(
        provider,
        "GET",
        &format!("{}/models", base_url.trim_end_matches('/')),
        Value::Object(headers),
        Value::Null,
    )?;
    let data: Value = serde_json::from_str(&body)
        .map_err(|_| anyhow::anyhow!("{} model discovery returned invalid JSON", provider.name))?;
    let raw = data
        .get("data")
        .or_else(|| data.get("models"))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(models_from_raw(&provider.name, &raw))
}

fn hbse_model_discovery_request(
    provider: &ProviderConfig,
    method: &str,
    url: &str,
    headers: Value,
    body: Value,
) -> anyhow::Result<String> {
    let secret_ref = provider
        .metadata
        .get("hbse_secret_ref")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| std::env::var("HBSE_PROVIDER_SECRET_REF").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Set HBSE_PROVIDER_SECRET_REF or provider metadata hbse_secret_ref to refresh HBSE-routed models."
            )
        })?;
    let consumer = provider
        .metadata
        .get("hbse_consumer")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("vegvisir.provider.{}", provider.name));
    let purpose = provider
        .metadata
        .get("hbse_model_discovery_purpose")
        .and_then(Value::as_str)
        .unwrap_or("model.discovery");
    let payload = json!({
        "command": "provider_http",
        "secret_ref": secret_ref,
        "consumer": consumer,
        "purpose": purpose,
        "method": method,
        "url": url,
        "headers": headers,
        "body": body,
        "credential_header": provider.metadata.get("credential_header").and_then(Value::as_str).unwrap_or("Authorization"),
        "credential_prefix": provider.metadata.get("credential_prefix").and_then(Value::as_str).unwrap_or("Bearer "),
        "timeout_seconds": 30,
    });
    let response = hbse_request(provider, payload)?;
    let status = response
        .get("status_code")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let body = response.get("body").and_then(Value::as_str).unwrap_or("");
    if status >= 400 {
        anyhow::bail!(
            "{} model discovery failed through HBSE: {} {}",
            provider.name,
            status,
            body.chars().take(300).collect::<String>()
        );
    }
    Ok(body.to_string())
}

pub fn discover_anthropic_models(provider: &ProviderConfig) -> anyhow::Result<Vec<ModelInfo>> {
    let api_key = required_api_key(provider)?;
    let base_url = provider.base_url.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Provider {} has no model discovery base_url.",
            provider.name
        )
    })?;
    let data = get_json(
        ureq::get(&format!("{}/models", base_url.trim_end_matches('/')))
            .set("Accept", "application/json")
            .set("x-api-key", &api_key)
            .set("anthropic-version", "2023-06-01"),
        &provider.name,
    )?;
    Ok(models_from_raw(
        &provider.name,
        data.get("data").unwrap_or(&Value::Null),
    ))
}

pub fn discover_google_models(provider: &ProviderConfig) -> anyhow::Result<Vec<ModelInfo>> {
    let api_key = required_api_key(provider)?;
    let base_url = provider.base_url.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Provider {} has no model discovery base_url.",
            provider.name
        )
    })?;
    let data = get_json(
        ureq::get(&format!(
            "{}/models?key={}",
            base_url.trim_end_matches('/'),
            api_key
        ))
        .set("Accept", "application/json"),
        &provider.name,
    )?;
    let raw = data.get("models").cloned().unwrap_or(Value::Null);
    Ok(parse_google_model_list(provider, raw))
}

fn parse_google_model_list(provider: &ProviderConfig, raw: Value) -> Vec<ModelInfo> {
    let mut discovered = Vec::new();
    if let Some(items) = raw.as_array() {
        for item in items {
            let methods = item
                .get("supportedGenerationMethods")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !methods.is_empty()
                && !methods.iter().any(|method| {
                    matches!(
                        method.as_str(),
                        Some("generateContent" | "streamGenerateContent")
                    )
                })
            {
                continue;
            }
            let Some(value) = item.get("name").and_then(Value::as_str) else {
                continue;
            };
            let model_id = value.strip_prefix("models/").unwrap_or(value).to_string();
            let display = item
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or(&model_id)
                .to_string();
            discovered.push(provider_model(&provider.name, &model_id, Some(display)));
        }
    }
    discovered
}

pub fn discover_hbse_google_models(provider: &ProviderConfig) -> anyhow::Result<Vec<ModelInfo>> {
    let base_url = provider.base_url.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Provider {} has no model discovery base_url.",
            provider.name
        )
    })?;
    let response = hbse_model_discovery_request(
        provider,
        "GET",
        &format!("{}/models", base_url.trim_end_matches('/')),
        json!({"Accept": "application/json"}),
        Value::Null,
    )?;
    let data: Value = serde_json::from_str(&response)?;
    Ok(parse_google_model_list(
        provider,
        data.get("models").cloned().unwrap_or(Value::Null),
    ))
}

pub fn discover_bearer_models(
    provider: &ProviderConfig,
    url: &str,
) -> anyhow::Result<Vec<ModelInfo>> {
    if url.is_empty() {
        anyhow::bail!(
            "Provider {} has no model discovery base_url.",
            provider.name
        );
    }
    let data = get_json(
        ureq::get(url).set("Accept", "application/json").set(
            "Authorization",
            &format!("Bearer {}", required_api_key(provider)?),
        ),
        &provider.name,
    )?;
    let raw = data
        .get("models")
        .or_else(|| data.get("data"))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(models_from_raw(&provider.name, &raw))
}

pub fn discover_ollama_models(provider: &ProviderConfig) -> anyhow::Result<Vec<ModelInfo>> {
    let base_url = provider.base_url.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Provider {} has no model discovery base_url.",
            provider.name
        )
    })?;
    let data = get_json(
        ureq::get(&format!("{}/api/tags", base_url.trim_end_matches('/')))
            .set("Accept", "application/json"),
        &provider.name,
    )?;
    Ok(models_from_raw(
        &provider.name,
        data.get("models").unwrap_or(&Value::Null),
    ))
}

pub fn discover_openai_sso_models(provider: &ProviderConfig) -> anyhow::Result<Vec<ModelInfo>> {
    let tokens = load_fresh_tokens_for_metadata(&provider.metadata)?;
    let data = get_json(
        ureq::get(&format!("{}/models", codex_base_url(&provider.metadata)))
            .set("Authorization", &format!("Bearer {}", tokens.access_token))
            .set("ChatGPT-Account-ID", &tokens.account_id)
            .set("Accept", "application/json"),
        &provider.name,
    )?;
    let raw = data
        .get("data")
        .or_else(|| data.get("models"))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(models_from_raw(&provider.name, &raw))
}

fn required_api_key(provider: &ProviderConfig) -> anyhow::Result<String> {
    if !direct_provider_auth_allowed() {
        return Err(direct_provider_auth_error(provider));
    }
    let Some(env) = &provider.api_key_env else {
        anyhow::bail!("Provider {} has no api_key_env.", provider.name);
    };
    get_env(env).ok_or_else(|| {
        anyhow::anyhow!(
            "Set {env} to refresh models for {}.",
            provider.display_name.as_deref().unwrap_or(&provider.name)
        )
    })
}

fn get_json(request: ureq::Request, provider_name: &str) -> anyhow::Result<Value> {
    match request.call() {
        Ok(response) => Ok(response.into_json()?),
        Err(ureq::Error::Status(code, response)) => {
            let detail = response.into_string().unwrap_or_default();
            anyhow::bail!(
                "{provider_name} model discovery failed: {code} {}",
                detail.chars().take(300).collect::<String>()
            )
        }
        Err(error) => anyhow::bail!("{provider_name} model discovery failed: {error}"),
    }
}

fn hbse_request(provider: &ProviderConfig, payload: Value) -> anyhow::Result<Value> {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
    };

    let mut stream = UnixStream::connect(hbse_default_or_configured_socket(provider))
        .map_err(|error| anyhow::anyhow!("HBSE broker unavailable: {error}"))?;
    stream.write_all(serde_json::to_string(&payload)?.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let n = stream.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..n]);
        if buffer[..n].contains(&b'\n') {
            break;
        }
    }
    let line = bytes.split(|byte| *byte == b'\n').next().unwrap_or(&bytes);
    let response: Value = serde_json::from_slice(line)?;
    if !response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        let message = response
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| response.get("error").map(Value::to_string))
            .unwrap_or_else(|| "unknown HBSE broker error".to_string());
        anyhow::bail!("HBSE broker denied model discovery: {message}");
    }
    Ok(response)
}

fn models_from_raw(provider_name: &str, raw_models: &Value) -> Vec<ModelInfo> {
    let Some(items) = raw_models.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Value::String(model_id) => Some(provider_model(provider_name, model_id, None)),
            Value::Object(object) => {
                let value = object
                    .get("id")
                    .or_else(|| object.get("name"))
                    .and_then(Value::as_str)?;
                let model_id = value.strip_prefix("models/").unwrap_or(value);
                let display = object
                    .get("display_name")
                    .or_else(|| object.get("displayName"))
                    .or_else(|| object.get("name"))
                    .and_then(Value::as_str)
                    .map(|value| value.strip_prefix("models/").unwrap_or(value).to_string());
                Some(provider_model(provider_name, model_id, display))
            }
            _ => None,
        })
        .collect()
}

fn provider_model(provider_name: &str, model_id: &str, display: Option<String>) -> ModelInfo {
    ModelInfo {
        name: model_id.to_string(),
        provider: provider_name.to_string(),
        display_name: Some(display.unwrap_or_else(|| model_id.to_string())),
        context_window: None,
        supports_streaming: true,
        enabled: true,
        metadata: [("source".to_string(), json!("provider"))]
            .into_iter()
            .collect(),
    }
}
