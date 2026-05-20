use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Instant,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use cms_v2::prompt_cache::CachedPromptEnvelope;
use serde_json::{Map, Value, json};

use crate::{
    core::{ChatMessage, ModelInfo, ProviderConfig, ProviderRegistry, SessionState},
    environment::get_env,
    openai_sso::{codex_base_url, load_fresh_tokens_for_metadata},
    tools::{ToolExecutor, ToolRegistry},
    types::{Observation, ToolCall},
};

const TOOL_OBSERVATION_MODEL_MAX_BYTES: usize = 64 * 1024;
const OPENAI_TOOL_LOOP_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

pub fn direct_provider_auth_allowed() -> bool {
    if env_truthy("VEGVISIR_ALLOW_DIRECT_PROVIDER_AUTH") {
        return true;
    }
    !production_auth_required()
}

pub fn production_auth_required() -> bool {
    env_truthy("VEGVISIR_PRODUCTION")
        || get_env("VEGVISIR_AUTH_MODE")
            .map(|mode| {
                matches!(
                    mode.trim().to_ascii_lowercase().as_str(),
                    "production" | "prod" | "hbse" | "hbse-only"
                )
            })
            .unwrap_or(false)
}

pub fn direct_provider_auth_error(config: &ProviderConfig) -> anyhow::Error {
    anyhow::anyhow!(
        "Direct API-key provider auth is disabled in production mode for {}. Configure the secret in HBSE with `/hbse provider {}` and select the HBSE-routed provider.",
        config.display_name.as_deref().unwrap_or(&config.name),
        canonical_hbse_provider_id(&config.name)
    )
}

fn env_truthy(name: &str) -> bool {
    get_env(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on" | "required"
            )
        })
        .unwrap_or(false)
}

pub trait ProviderAdapter {
    fn config(&self) -> &ProviderConfig;
    fn complete(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        selected_provider: &str,
    ) -> anyhow::Result<String>;

    fn complete_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        selected_provider: &str,
    ) -> anyhow::Result<String> {
        let message = ChatMessage {
            role: "system".to_string(),
            content: envelope.model_request.prompt.clone(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        };
        self.complete(&[message], model, selected_provider)
    }

    fn stream_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let response = self.complete_envelope(envelope, model, selected_provider)?;
        on_delta(&response);
        Ok(response)
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        false
    }

    fn complete_with_tools(
        &self,
        _messages: &[ChatMessage],
        model: &ModelInfo,
        _tools: &[Value],
        _execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        anyhow::bail!(
            "Provider {} does not support native tool calls.",
            model.provider
        )
    }

    fn complete_with_tools_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let response =
            self.complete_with_tools(messages, model, tools, execute_tool, selected_provider)?;
        on_delta(&response);
        Ok(response)
    }
}

#[derive(Clone, Debug)]
pub struct DemoProviderAdapter {
    pub config: ProviderConfig,
}

impl DemoProviderAdapter {
    pub fn new() -> Self {
        Self {
            config: ProviderConfig {
                name: "demo".to_string(),
                display_name: Some("Demo Local".to_string()),
                kind: "local".to_string(),
                api_key_env: None,
                base_url: None,
                auth_type: "none".to_string(),
                enabled: true,
                metadata: Default::default(),
            },
        }
    }
}

impl Default for DemoProviderAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for DemoProviderAdapter {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        let latest = messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content.as_str())
            .unwrap_or("");
        let attachment_count: usize = messages
            .iter()
            .map(|message| message.attachments.len())
            .sum();
        Ok(format!(
            "Demo response from {}: received {} characters and {} attachment(s). No external API was called.",
            model.name,
            latest.len(),
            attachment_count,
        ))
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        true
    }

    fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        _execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        selected_provider: &str,
    ) -> anyhow::Result<String> {
        if let Some(prepared_prompt) = messages
            .iter()
            .find(|message| message.role == "system")
            .map(|message| message.content.as_str())
        {
            return Ok(format!(
                "Demo response from {}: received CMS-v2 model request with {} prompt characters. {} tool(s) are exposed. No external API was called.",
                model.name,
                prepared_prompt.len(),
                tools.len(),
            ));
        }
        let mut response = self.complete(messages, model, selected_provider)?;
        response.push_str(&format!(" {} tool(s) are exposed.", tools.len()));
        Ok(response)
    }

    fn complete_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        Ok(format!(
            "Demo response from {}: received CMS-v2 model request with {} prompt characters and cache key {}. No external API was called.",
            model.name,
            envelope.model_request.prompt.len(),
            envelope.manifest.prompt_cache_key,
        ))
    }
}

#[derive(Clone, Debug)]
pub struct OpenAICompatibleProviderAdapter {
    pub config: ProviderConfig,
}

#[derive(Clone, Debug)]
pub struct HBSEOpenAICompatibleProviderAdapter {
    pub config: ProviderConfig,
}

#[derive(Clone, Debug)]
pub struct HBSEAzureOpenAIProviderAdapter {
    pub config: ProviderConfig,
}

#[derive(Clone, Debug)]
pub struct AnthropicProviderAdapter {
    pub config: ProviderConfig,
}

#[derive(Clone, Debug)]
pub struct HBSEAnthropicProviderAdapter {
    pub config: ProviderConfig,
}

#[derive(Clone, Debug)]
pub struct GoogleProviderAdapter {
    pub config: ProviderConfig,
}

#[derive(Clone, Debug)]
pub struct HBSEGoogleProviderAdapter {
    pub config: ProviderConfig,
}

#[derive(Clone, Debug)]
pub struct OpenAISsoProfileAdapter {
    pub config: ProviderConfig,
}

impl ProviderAdapter for OpenAICompatibleProviderAdapter {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        if openai_compatible_uses_responses_api(&self.config) {
            return self.post_response_streaming(messages, model, &mut |_| {});
        }
        self.post_chat_completion(model, openai_messages(messages), None)
    }

    fn complete_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        if openai_compatible_uses_responses_api(&self.config) {
            let message = ChatMessage {
                role: "system".to_string(),
                content: envelope.model_request.prompt.clone(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            };
            return self.post_response_streaming(&[message], model, &mut |_| {});
        }
        self.post_chat_completion(
            model,
            vec![json!({"role": "system", "content": envelope.model_request.prompt})],
            Some(prompt_cache_metadata(envelope)),
        )
    }

    fn stream_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        _selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        if openai_compatible_uses_responses_api(&self.config) {
            let message = ChatMessage {
                role: "user".to_string(),
                content: envelope.model_request.prompt.clone(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            };
            return self.post_response_streaming(&[message], model, on_delta);
        }
        self.post_chat_completion_streaming(
            model,
            vec![json!({"role": "system", "content": envelope.model_request.prompt})],
            Some(prompt_cache_metadata(envelope)),
            on_delta,
        )
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        true
    }

    fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        self.complete_with_tools_streaming(
            messages,
            model,
            tools,
            execute_tool,
            _selected_provider,
            &mut |_| {},
        )
    }

    fn complete_with_tools_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        if openai_compatible_uses_responses_api(&self.config) {
            let mut post = |payload: Value| -> anyhow::Result<Value> {
                self.post_response_stream_json(payload, on_delta)
            };
            return responses_tool_loop_streaming(
                messages,
                model,
                tools,
                execute_tool,
                &mut post,
                8,
            );
        }
        let api_key = optional_provider_env(&self.config)?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let mut post = |payload: Value| -> anyhow::Result<String> {
            let mut request = ureq::post(&url)
                .set("Content-Type", "application/json")
                .set("Accept", "text/event-stream");
            if let Some(api_key) = &api_key {
                request = request.set("Authorization", &format!("Bearer {api_key}"));
            }
            Ok(send_provider_json(request, payload, &self.config.name)?.into_string()?)
        };
        openai_tool_loop_streaming(
            model.name.as_str(),
            messages,
            tools,
            execute_tool,
            &mut post,
            6,
            on_delta,
        )
    }
}

impl OpenAICompatibleProviderAdapter {
    fn post_response_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let response =
            self.post_response_stream_json(responses_payload(messages, model), on_delta)?;
        extract_response_text(&response).ok_or_else(|| {
            anyhow::anyhow!(
                "Provider {} response did not include assistant text",
                self.config.name
            )
        })
    }

    fn post_response_stream_json(
        &self,
        payload: Value,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<Value> {
        let api_key = optional_provider_env(&self.config)?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let url = format!("{}/responses", base_url.trim_end_matches('/'));
        let mut request = ureq::post(&url)
            .set("Content-Type", "application/json")
            .set("Accept", "text/event-stream");
        if let Some(api_key) = api_key {
            request = request.set("Authorization", &format!("Bearer {api_key}"));
        }
        let response = send_provider_json(request, payload, &self.config.name)?;
        parse_response_sse_value_reader(BufReader::new(response.into_reader()), on_delta)
    }

    fn post_chat_completion_streaming(
        &self,
        model: &ModelInfo,
        messages: Vec<Value>,
        metadata: Option<Value>,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let api_key = optional_provider_env(&self.config)?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let mut payload = json!({
            "model": model.name,
            "messages": messages,
            "stream": true
        });
        let store = provider_store_enabled(&self.config);
        if store {
            payload["store"] = json!(true);
        }
        if store && let Some(metadata) = metadata {
            payload["metadata"] = metadata;
        }
        let mut request = ureq::post(&url)
            .set("Content-Type", "application/json")
            .set("Accept", "text/event-stream");
        if let Some(api_key) = api_key {
            request = request.set("Authorization", &format!("Bearer {api_key}"));
        }
        let response = send_provider_json(request, payload, &self.config.name)?;
        parse_openai_sse_reader(BufReader::new(response.into_reader()), on_delta)
    }

    fn post_chat_completion(
        &self,
        model: &ModelInfo,
        messages: Vec<Value>,
        metadata: Option<Value>,
    ) -> anyhow::Result<String> {
        let api_key = optional_provider_env(&self.config)?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let stream = self
            .config
            .metadata
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let mut payload = json!({
            "model": model.name,
            "messages": messages,
            "stream": stream
        });
        let store = provider_store_enabled(&self.config);
        if store {
            payload["store"] = json!(true);
        }
        if store && let Some(metadata) = metadata {
            payload["metadata"] = metadata;
        }
        let mut request = ureq::post(&url)
            .set("Content-Type", "application/json")
            .set(
                "Accept",
                if stream {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            );
        if let Some(api_key) = api_key {
            request = request.set("Authorization", &format!("Bearer {api_key}"));
        }
        let response = send_provider_json(request, payload, &self.config.name)?;
        if stream {
            parse_openai_sse(&response.into_string()?)
        } else {
            let response: Value = response.into_json()?;
            extract_openai_compatible_text(&response).ok_or_else(|| {
                anyhow::anyhow!(
                    "Provider {} response did not include assistant text",
                    self.config.name
                )
            })
        }
    }
}

impl ProviderAdapter for HBSEOpenAICompatibleProviderAdapter {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        if openai_compatible_uses_responses_api(&self.config) {
            return self.post_response_streaming(messages, model, &mut |_| {});
        }
        self.post_chat_completion(model, openai_messages(messages), None)
    }

    fn complete_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        if openai_compatible_uses_responses_api(&self.config) {
            let message = ChatMessage {
                role: "system".to_string(),
                content: envelope.model_request.prompt.clone(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            };
            return self.post_response_streaming(&[message], model, &mut |_| {});
        }
        self.post_chat_completion(
            model,
            vec![json!({"role": "system", "content": envelope.model_request.prompt})],
            Some(prompt_cache_metadata(envelope)),
        )
    }

    fn stream_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        _selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        if openai_compatible_uses_responses_api(&self.config) {
            let message = ChatMessage {
                role: "user".to_string(),
                content: envelope.model_request.prompt.clone(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            };
            return self.post_response_streaming(&[message], model, on_delta);
        }
        self.post_chat_completion_streaming(
            model,
            vec![json!({"role": "system", "content": envelope.model_request.prompt})],
            Some(prompt_cache_metadata(envelope)),
            on_delta,
        )
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        true
    }

    fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        self.complete_with_tools_streaming(
            messages,
            model,
            tools,
            execute_tool,
            _selected_provider,
            &mut |_| {},
        )
    }

    fn complete_with_tools_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        if openai_compatible_uses_responses_api(&self.config) {
            let mut post = |payload: Value| -> anyhow::Result<Value> {
                self.post_response_stream_json(payload, on_delta)
            };
            return responses_tool_loop_streaming(
                messages,
                model,
                tools,
                execute_tool,
                &mut post,
                8,
            );
        }
        let mut post = |payload: Value| -> anyhow::Result<String> {
            let response = hbse_provider_http(
                &self.config,
                "text/event-stream",
                serde_json::to_string(&payload)?,
            )?;
            let status = response
                .get("status_code")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let body = response
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if status >= 400 {
                anyhow::bail!(
                    "{} request failed through HBSE: {} {}",
                    self.config.name,
                    status,
                    body.chars().take(400).collect::<String>()
                );
            }
            Ok(body)
        };
        openai_tool_loop_streaming(
            model.name.as_str(),
            messages,
            tools,
            execute_tool,
            &mut post,
            6,
            on_delta,
        )
    }
}

impl HBSEOpenAICompatibleProviderAdapter {
    fn post_response_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let response =
            self.post_response_stream_json(responses_payload(messages, model), on_delta)?;
        extract_response_text(&response).ok_or_else(|| {
            anyhow::anyhow!(
                "Provider {} response did not include assistant text",
                self.config.name
            )
        })
    }

    fn post_response_stream_json(
        &self,
        payload: Value,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<Value> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let response = hbse_provider_http_with_url(
            &self.config,
            &format!("{}/responses", base_url.trim_end_matches('/')),
            "text/event-stream",
            serde_json::to_string(&payload)?,
        )?;
        let status = response
            .get("status_code")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let body = response
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if status >= 400 {
            anyhow::bail!(
                "{} request failed through HBSE: {} {}",
                self.config.name,
                status,
                body.chars().take(400).collect::<String>()
            );
        }
        parse_response_sse_value(&body, on_delta)
    }

    fn post_chat_completion_streaming(
        &self,
        model: &ModelInfo,
        messages: Vec<Value>,
        metadata: Option<Value>,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let mut payload = json!({
            "model": model.name,
            "messages": messages,
            "stream": true,
        });
        let store = provider_store_enabled(&self.config);
        if store {
            payload["store"] = json!(true);
        }
        if store && let Some(metadata) = metadata {
            payload["metadata"] = metadata;
        }
        let response = hbse_provider_http_with_url(
            &self.config,
            &format!("{}/chat/completions", base_url.trim_end_matches('/')),
            "text/event-stream",
            serde_json::to_string(&payload)?,
        )?;
        let status = response
            .get("status_code")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let body = response
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if status >= 400 {
            anyhow::bail!(
                "{} request failed through HBSE: {} {}",
                self.config.name,
                status,
                body.chars().take(400).collect::<String>()
            );
        }
        parse_openai_sse_with_callback(&body, on_delta)
    }

    fn post_chat_completion(
        &self,
        model: &ModelInfo,
        messages: Vec<Value>,
        metadata: Option<Value>,
    ) -> anyhow::Result<String> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let stream = self
            .config
            .metadata
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let mut payload = json!({
            "model": model.name,
            "messages": messages,
            "stream": stream,
        });
        let store = provider_store_enabled(&self.config);
        if store {
            payload["store"] = json!(true);
        }
        if store && let Some(metadata) = metadata {
            payload["metadata"] = metadata;
        }
        let response = hbse_provider_http_with_url(
            &self.config,
            &format!("{}/chat/completions", base_url.trim_end_matches('/')),
            if stream {
                "text/event-stream"
            } else {
                "application/json"
            },
            serde_json::to_string(&payload)?,
        )?;
        let status = response
            .get("status_code")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let body = response
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if status >= 400 {
            anyhow::bail!(
                "{} request failed through HBSE: {} {}",
                self.config.name,
                status,
                body.chars().take(400).collect::<String>()
            );
        }
        if stream {
            parse_openai_sse(&body)
        } else {
            let value: Value = serde_json::from_str(&body)?;
            extract_openai_compatible_text(&value).ok_or_else(|| {
                anyhow::anyhow!(
                    "Provider {} response did not include assistant text",
                    self.config.name
                )
            })
        }
    }
}

impl ProviderAdapter for HBSEAzureOpenAIProviderAdapter {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        self.post_chat_completion(model, openai_messages(messages), None)
    }

    fn complete_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        self.post_chat_completion(
            model,
            vec![json!({"role": "system", "content": envelope.model_request.prompt})],
            Some(prompt_cache_metadata(envelope)),
        )
    }

    fn stream_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        _selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let response = self.post_chat_completion(
            model,
            vec![json!({"role": "system", "content": envelope.model_request.prompt})],
            Some(prompt_cache_metadata(envelope)),
        )?;
        on_delta(&response);
        Ok(response)
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        true
    }

    fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        self.complete_with_tools_streaming(
            messages,
            model,
            tools,
            execute_tool,
            _selected_provider,
            &mut |_| {},
        )
    }

    fn complete_with_tools_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let mut post = |payload: Value| -> anyhow::Result<String> {
            let response = hbse_provider_http_with_url_and_headers(
                &self.config,
                &azure_chat_completions_url(&self.config, model)?,
                "text/event-stream",
                serde_json::to_string(&payload)?,
                json!({"Content-Type": "application/json", "Accept": "text/event-stream"}),
            )?;
            let status = response
                .get("status_code")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let body = response
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if status >= 400 {
                anyhow::bail!(
                    "{} request failed through HBSE: {} {}",
                    self.config.name,
                    status,
                    body.chars().take(400).collect::<String>()
                );
            }
            Ok(body)
        };
        openai_tool_loop_streaming(
            model.name.as_str(),
            messages,
            tools,
            execute_tool,
            &mut post,
            6,
            on_delta,
        )
    }
}

impl HBSEAzureOpenAIProviderAdapter {
    fn post_chat_completion(
        &self,
        model: &ModelInfo,
        messages: Vec<Value>,
        metadata: Option<Value>,
    ) -> anyhow::Result<String> {
        let stream = self
            .config
            .metadata
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let mut payload = json!({
            "messages": messages,
            "stream": stream,
        });
        let store = provider_store_enabled(&self.config);
        if store {
            payload["store"] = json!(true);
        }
        if store && let Some(metadata) = metadata {
            payload["metadata"] = metadata;
        }
        let response = hbse_provider_http_with_url_and_headers(
            &self.config,
            &azure_chat_completions_url(&self.config, model)?,
            if stream {
                "text/event-stream"
            } else {
                "application/json"
            },
            serde_json::to_string(&payload)?,
            json!({
                "Content-Type": "application/json",
                "Accept": if stream { "text/event-stream" } else { "application/json" },
            }),
        )?;
        let status = response
            .get("status_code")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let body = response
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if status >= 400 {
            anyhow::bail!(
                "{} request failed through HBSE: {} {}",
                self.config.name,
                status,
                body.chars().take(400).collect::<String>()
            );
        }
        if stream {
            parse_openai_sse(&body)
        } else {
            let value: Value = serde_json::from_str(&body)?;
            extract_openai_compatible_text(&value).ok_or_else(|| {
                anyhow::anyhow!(
                    "Provider {} response did not include assistant text",
                    self.config.name
                )
            })
        }
    }
}

fn azure_chat_completions_url(
    config: &ProviderConfig,
    model: &ModelInfo,
) -> anyhow::Result<String> {
    let endpoint = config
        .base_url
        .as_deref()
        .or_else(|| {
            config
                .metadata
                .get("azure_endpoint")
                .and_then(Value::as_str)
        })
        .ok_or_else(|| anyhow::anyhow!("Provider {} has no Azure endpoint/base_url", config.name))?
        .trim_end_matches('/');
    let deployment = config
        .metadata
        .get("azure_deployment")
        .and_then(Value::as_str)
        .unwrap_or_else(|| model.name.strip_prefix("azure:").unwrap_or(&model.name));
    let api_version = config
        .metadata
        .get("api_version")
        .or_else(|| config.metadata.get("azure_api_version"))
        .and_then(Value::as_str)
        .unwrap_or("2024-10-21");
    Ok(format!(
        "{endpoint}/openai/deployments/{deployment}/chat/completions?api-version={api_version}"
    ))
}

impl ProviderAdapter for AnthropicProviderAdapter {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        let api_key = required_provider_env(&self.config)?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let payload = anthropic_messages_payload(messages, model);
        let request = ureq::post(&format!("{}/messages", base_url.trim_end_matches('/')))
            .set("x-api-key", &api_key)
            .set("anthropic-version", "2023-06-01")
            .set("Content-Type", "application/json")
            .set("Accept", "text/event-stream");
        let response = send_provider_json(request, payload, &self.config.name)?;
        parse_anthropic_sse(&response.into_string()?)
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        true
    }

    fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        let api_key = required_provider_env(&self.config)?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let mut post = |payload: Value| -> anyhow::Result<Value> {
            let request = ureq::post(&format!("{}/messages", base_url.trim_end_matches('/')))
                .set("x-api-key", &api_key)
                .set("anthropic-version", "2023-06-01")
                .set("Content-Type", "application/json")
                .set("Accept", "application/json");
            Ok(send_provider_json(request, payload, &self.config.name)?.into_json()?)
        };
        anthropic_tool_loop(messages, model, tools, execute_tool, &mut post, 8)
    }
}

fn anthropic_messages_payload(messages: &[ChatMessage], model: &ModelInfo) -> Value {
    let system_prompt = messages
        .iter()
        .filter(|message| message.role == "system")
        .map(text_with_attachment_refs)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    let mut payload = json!({
        "model": model.name,
        "max_tokens": 4096,
        "stream": true,
        "messages": messages
            .iter()
            .filter(|message| message.role != "system")
            .map(|message| {
                json!({
                    "role": if message.role == "assistant" { "assistant" } else { "user" },
                    "content": text_with_attachment_refs(message),
                })
            })
            .collect::<Vec<_>>(),
    });
    if !system_prompt.is_empty() {
        payload["system"] = Value::String(system_prompt);
    }
    payload
}

fn anthropic_tool_loop(
    messages: &[ChatMessage],
    model: &ModelInfo,
    tools: &[Value],
    execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
    post: &mut dyn FnMut(Value) -> anyhow::Result<Value>,
    max_tool_rounds: usize,
) -> anyhow::Result<String> {
    let system_prompt = messages
        .iter()
        .filter(|message| message.role == "system")
        .map(text_with_attachment_refs)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    let mut wire_messages = messages
        .iter()
        .filter(|message| message.role != "system")
        .map(|message| {
            json!({
                "role": if message.role == "assistant" { "assistant" } else { "user" },
                "content": text_with_attachment_refs(message),
            })
        })
        .collect::<Vec<_>>();
    if wire_messages.is_empty() {
        wire_messages.push(json!({"role": "user", "content": "Continue."}));
    }
    for _ in 0..max_tool_rounds {
        let mut payload = json!({
            "model": model.name,
            "max_tokens": 4096,
            "stream": false,
            "messages": wire_messages,
            "tools": tools.iter().map(anthropic_tool_schema).collect::<Vec<_>>(),
            "tool_choice": {"type": "auto"},
        });
        if !system_prompt.is_empty() {
            payload["system"] = Value::String(system_prompt.clone());
        }
        let response = post(payload)?;
        let content = response
            .get("content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let text = content
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("");
        let tool_uses = content
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool_use"))
            .cloned()
            .collect::<Vec<_>>();
        if tool_uses.is_empty() {
            return Ok(text);
        }
        wire_messages.push(json!({"role": "assistant", "content": content}));
        let results = tool_uses
            .into_iter()
            .filter_map(|tool_use| {
                let id = tool_use.get("id").and_then(Value::as_str)?.to_string();
                let name = tool_use.get("name").and_then(Value::as_str)?.to_string();
                let args = tool_use
                    .get("input")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let result = truncate_model_observation(&execute_tool(&name, args));
                Some(json!({
                    "type": "tool_result",
                    "tool_use_id": id,
                    "content": result,
                }))
            })
            .collect::<Vec<_>>();
        wire_messages.push(json!({"role": "user", "content": results}));
    }
    anyhow::bail!("model exceeded Vegvisir tool-call round limit.")
}

fn anthropic_tool_schema(tool: &Value) -> Value {
    let schema = openai_tool_schema(tool);
    let function = schema.get("function").and_then(Value::as_object);
    json!({
        "name": function.and_then(|item| item.get("name")).and_then(Value::as_str).unwrap_or(""),
        "description": function.and_then(|item| item.get("description")).and_then(Value::as_str).unwrap_or(""),
        "input_schema": function.and_then(|item| item.get("parameters")).cloned().unwrap_or_else(|| json!({"type":"object","properties":{},"additionalProperties":false})),
    })
}

impl ProviderAdapter for HBSEAnthropicProviderAdapter {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        self.post_messages_streaming(messages, model)
    }

    fn stream_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        _selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let message = ChatMessage {
            role: "system".to_string(),
            content: envelope.model_request.prompt.clone(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        };
        let response = self.post_messages_streaming(&[message], model)?;
        on_delta(&response);
        Ok(response)
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        true
    }

    fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let mut post = |payload: Value| -> anyhow::Result<Value> {
            let response = hbse_provider_http_with_url_and_headers(
                &self.config,
                &format!("{}/messages", base_url.trim_end_matches('/')),
                "application/json",
                serde_json::to_string(&payload)?,
                json!({
                    "Content-Type": "application/json",
                    "Accept": "application/json",
                    "anthropic-version": self.config
                        .metadata
                        .get("anthropic_version")
                        .and_then(Value::as_str)
                        .unwrap_or("2023-06-01")
                }),
            )?;
            provider_http_json_body(&self.config.name, response)
        };
        anthropic_tool_loop(messages, model, tools, execute_tool, &mut post, 8)
    }
}

impl HBSEAnthropicProviderAdapter {
    fn post_messages_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
    ) -> anyhow::Result<String> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let response = hbse_provider_http_with_url_and_headers(
            &self.config,
            &format!("{}/messages", base_url.trim_end_matches('/')),
            "text/event-stream",
            serde_json::to_string(&anthropic_messages_payload(messages, model))?,
            json!({
                "Content-Type": "application/json",
                "Accept": "text/event-stream",
                "anthropic-version": self.config
                    .metadata
                    .get("anthropic_version")
                    .and_then(Value::as_str)
                    .unwrap_or("2023-06-01")
            }),
        )?;
        let status = response
            .get("status_code")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let body = response
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if status >= 400 {
            anyhow::bail!(
                "{} request failed through HBSE: {} {}",
                self.config.name,
                status,
                body.chars().take(400).collect::<String>()
            );
        }
        parse_anthropic_sse(&body)
    }
}

impl ProviderAdapter for GoogleProviderAdapter {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        let api_key = required_provider_env(&self.config)?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let payload = google_generate_content_payload(messages);
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            base_url.trim_end_matches('/'),
            model.name,
            api_key
        );
        let request = ureq::post(&url)
            .set("Content-Type", "application/json")
            .set("Accept", "text/event-stream");
        let response = send_provider_json(request, payload, &self.config.name)?;
        parse_google_stream(&response.into_string()?)
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        true
    }

    fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        let api_key = required_provider_env(&self.config)?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let url = format!(
            "{}/models/{}:generateContent?key={}",
            base_url.trim_end_matches('/'),
            model.name,
            api_key
        );
        let mut post = |payload: Value| -> anyhow::Result<Value> {
            let request = ureq::post(&url)
                .set("Content-Type", "application/json")
                .set("Accept", "application/json");
            Ok(send_provider_json(request, payload, &self.config.name)?.into_json()?)
        };
        google_tool_loop(messages, tools, execute_tool, &mut post, 8)
    }
}

impl ProviderAdapter for HBSEGoogleProviderAdapter {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        self.post_generate_content_streaming(messages, model)
    }

    fn stream_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        _selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let message = ChatMessage {
            role: "system".to_string(),
            content: envelope.model_request.prompt.clone(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        };
        let response = self.post_generate_content_streaming(&[message], model)?;
        on_delta(&response);
        Ok(response)
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        true
    }

    fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let url = format!(
            "{}/models/{}:generateContent",
            base_url.trim_end_matches('/'),
            model.name
        );
        let mut post = |payload: Value| -> anyhow::Result<Value> {
            let response = hbse_provider_http_with_url_and_headers(
                &self.config,
                &url,
                "application/json",
                serde_json::to_string(&payload)?,
                json!({"Content-Type": "application/json", "Accept": "application/json"}),
            )?;
            provider_http_json_body(&self.config.name, response)
        };
        google_tool_loop(messages, tools, execute_tool, &mut post, 8)
    }
}

impl HBSEGoogleProviderAdapter {
    fn post_generate_content_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
    ) -> anyhow::Result<String> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let response = hbse_provider_http_with_url_and_headers(
            &self.config,
            &format!(
                "{}/models/{}:streamGenerateContent?alt=sse",
                base_url.trim_end_matches('/'),
                model.name
            ),
            "text/event-stream",
            serde_json::to_string(&google_generate_content_payload(messages))?,
            json!({"Content-Type": "application/json", "Accept": "text/event-stream"}),
        )?;
        let status = response
            .get("status_code")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let body = response
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if status >= 400 {
            anyhow::bail!(
                "{} request failed through HBSE: {} {}",
                self.config.name,
                status,
                body.chars().take(400).collect::<String>()
            );
        }
        parse_google_stream(&body)
    }
}

fn google_generate_content_payload(messages: &[ChatMessage]) -> Value {
    let system_prompt = messages
        .iter()
        .filter(|message| message.role == "system")
        .map(text_with_attachment_refs)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    let mut contents = messages
        .iter()
        .filter(|message| message.role != "system")
        .map(|message| {
            json!({
                "role": if message.role == "assistant" { "model" } else { "user" },
                "parts": [{"text": text_with_attachment_refs(message)}],
            })
        })
        .collect::<Vec<_>>();
    if contents.is_empty() {
        contents.push(json!({"role": "user", "parts": [{"text": ""}]}));
    }
    let mut payload = json!({ "contents": contents });
    if !system_prompt.is_empty() {
        payload["systemInstruction"] = json!({"parts": [{"text": system_prompt}]});
    }
    payload
}

fn google_tool_loop(
    messages: &[ChatMessage],
    tools: &[Value],
    execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
    post: &mut dyn FnMut(Value) -> anyhow::Result<Value>,
    max_tool_rounds: usize,
) -> anyhow::Result<String> {
    let base_payload = google_generate_content_payload(messages);
    let system_instruction = base_payload.get("systemInstruction").cloned();
    let mut contents = base_payload
        .get("contents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| vec![json!({"role": "user", "parts": [{"text": ""}]})]);
    for _ in 0..max_tool_rounds {
        let mut payload = json!({
            "contents": contents,
            "tools": [{"functionDeclarations": tools.iter().map(google_tool_schema).collect::<Vec<_>>()}],
            "toolConfig": {"functionCallingConfig": {"mode": "AUTO"}},
        });
        if let Some(system_instruction) = &system_instruction {
            payload["systemInstruction"] = system_instruction.clone();
        }
        let response = post(payload)?;
        let parts = response
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let text = parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("");
        let calls = parts
            .iter()
            .filter_map(|part| part.get("functionCall"))
            .cloned()
            .collect::<Vec<_>>();
        if calls.is_empty() {
            return Ok(text);
        }
        contents.push(json!({"role": "model", "parts": parts}));
        let response_parts = calls
            .into_iter()
            .filter_map(|call| {
                let name = call.get("name").and_then(Value::as_str)?.to_string();
                let args = call
                    .get("args")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let result = truncate_model_observation(&execute_tool(&name, args));
                Some(json!({
                    "functionResponse": {
                        "name": name,
                        "response": {"result": result},
                    }
                }))
            })
            .collect::<Vec<_>>();
        contents.push(json!({"role": "user", "parts": response_parts}));
    }
    anyhow::bail!("model exceeded Vegvisir tool-call round limit.")
}

fn google_tool_schema(tool: &Value) -> Value {
    let schema = openai_tool_schema(tool);
    let function = schema.get("function").and_then(Value::as_object);
    json!({
        "name": function.and_then(|item| item.get("name")).and_then(Value::as_str).unwrap_or(""),
        "description": function.and_then(|item| item.get("description")).and_then(Value::as_str).unwrap_or(""),
        "parameters": function.and_then(|item| item.get("parameters")).cloned().unwrap_or_else(|| json!({"type":"object","properties":{},"additionalProperties":false})),
    })
}

impl ProviderAdapter for OpenAISsoProfileAdapter {
    fn config(&self) -> &ProviderConfig {
        &self.config
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        let mut output = String::new();
        self.post_response_streaming(messages, model, &mut |delta| output.push_str(delta))?;
        Ok(output)
    }

    fn stream_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        _selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let message = ChatMessage {
            role: "user".to_string(),
            content: envelope.model_request.prompt.clone(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        };
        self.post_response_streaming(&[message], model, on_delta)
    }

    fn supports_tool_calls(&self, _model: &ModelInfo, _selected_provider: &str) -> bool {
        true
    }

    fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
    ) -> anyhow::Result<String> {
        self.complete_with_tools_streaming(
            messages,
            model,
            tools,
            execute_tool,
            _selected_provider,
            &mut |_| {},
        )
    }

    fn complete_with_tools_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        _selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let mut payload = responses_payload(messages, model);
        payload["stream"] = Value::Bool(true);
        payload["tool_choice"] = Value::String("auto".to_string());
        payload["tools"] = Value::Array(tools.iter().map(responses_tool_schema).collect());
        for _ in 0..8 {
            let response = self.post_response_stream_json(payload.clone(), on_delta)?;
            let tool_calls = response_function_calls(&response);
            if tool_calls.is_empty() {
                if let Some(text) = extract_response_text(&response) {
                    return Ok(text);
                }
                if let Some(error) = response.get("error")
                    && !error.is_null()
                {
                    anyhow::bail!("openai-sso response failed: {error}");
                }
                anyhow::bail!("openai-sso response did not contain assistant text.");
            }
            let input = payload
                .get_mut("input")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| anyhow::anyhow!("openai-sso payload input was not an array"))?;
            if let Some(output) = response.get("output").and_then(Value::as_array) {
                input.extend(output.iter().map(response_output_item_for_followup));
            }
            for call in tool_calls {
                let result = truncate_model_observation(&execute_tool(&call.name, call.args));
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call.call_id,
                    "output": result,
                }));
            }
        }
        anyhow::bail!("model exceeded Vegvisir tool-call round limit.")
    }
}

impl OpenAISsoProfileAdapter {
    fn post_response_stream_json(
        &self,
        payload: Value,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<Value> {
        let tokens = load_fresh_tokens_for_metadata(&self.config.metadata)?;
        let request = ureq::post(&format!(
            "{}/responses",
            codex_base_url(&self.config.metadata)
        ))
        .set("Authorization", &format!("Bearer {}", tokens.access_token))
        .set("ChatGPT-Account-ID", &tokens.account_id)
        .set("Content-Type", "application/json")
        .set("Accept", "text/event-stream");
        match request.send_json(payload) {
            Ok(response) => {
                parse_response_sse_value_reader(BufReader::new(response.into_reader()), on_delta)
            }
            Err(ureq::Error::Status(401, _)) => {
                anyhow::bail!("OpenAI SSO rejected the saved login. Run /auth openai-sso again.")
            }
            Err(ureq::Error::Status(code, response)) => {
                let detail = response.into_string().unwrap_or_default();
                anyhow::bail!(
                    "openai-sso request failed: {} {}",
                    code,
                    detail.chars().take(400).collect::<String>()
                )
            }
            Err(error) => Err(error.into()),
        }
    }

    fn post_response_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        let tokens = load_fresh_tokens_for_metadata(&self.config.metadata)?;
        let payload = responses_payload(messages, model);
        let request = ureq::post(&format!(
            "{}/responses",
            codex_base_url(&self.config.metadata)
        ))
        .set("Authorization", &format!("Bearer {}", tokens.access_token))
        .set("ChatGPT-Account-ID", &tokens.account_id)
        .set("Content-Type", "application/json")
        .set("Accept", "text/event-stream");
        match request.send_json(payload) {
            Ok(response) => {
                parse_response_sse_text_reader(BufReader::new(response.into_reader()), on_delta)
            }
            Err(ureq::Error::Status(401, _)) => {
                anyhow::bail!("OpenAI SSO rejected the saved login. Run /auth openai-sso again.")
            }
            Err(ureq::Error::Status(code, response)) => {
                let detail = response.into_string().unwrap_or_default();
                anyhow::bail!(
                    "openai-sso request failed: {} {}",
                    code,
                    detail.chars().take(400).collect::<String>()
                )
            }
            Err(error) => Err(error.into()),
        }
    }
}

struct ResponseFunctionCall {
    call_id: String,
    name: String,
    args: Map<String, Value>,
}

fn responses_tool_loop_streaming(
    messages: &[ChatMessage],
    model: &ModelInfo,
    tools: &[Value],
    execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
    post_response: &mut dyn FnMut(Value) -> anyhow::Result<Value>,
    max_tool_rounds: usize,
) -> anyhow::Result<String> {
    let mut payload = responses_payload(messages, model);
    payload["stream"] = Value::Bool(true);
    payload["tool_choice"] = Value::String("auto".to_string());
    payload["tools"] = Value::Array(tools.iter().map(responses_tool_schema).collect());
    for _ in 0..max_tool_rounds {
        let response = post_response(payload.clone())?;
        let tool_calls = response_function_calls(&response);
        if tool_calls.is_empty() {
            if let Some(text) = extract_response_text(&response) {
                return Ok(text);
            }
            if let Some(error) = response.get("error")
                && !error.is_null()
            {
                anyhow::bail!("openai-compatible response failed: {error}");
            }
            anyhow::bail!("openai-compatible response did not contain assistant text.");
        }
        let input = payload
            .get_mut("input")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| anyhow::anyhow!("responses payload input was not an array"))?;
        if let Some(output) = response.get("output").and_then(Value::as_array) {
            input.extend(output.iter().map(response_output_item_for_followup));
        }
        for call in tool_calls {
            let result = truncate_model_observation(&execute_tool(&call.name, call.args));
            input.push(json!({
                "type": "function_call_output",
                "call_id": call.call_id,
                "output": result,
            }));
        }
    }
    anyhow::bail!("model exceeded Vegvisir tool-call round limit.")
}

fn response_function_calls(response: &Value) -> Vec<ResponseFunctionCall> {
    response
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .filter_map(|item| {
            let call_id = item.get("call_id").and_then(Value::as_str)?.to_string();
            let name = item.get("name").and_then(Value::as_str)?.to_string();
            Some(ResponseFunctionCall {
                call_id,
                name,
                args: parse_tool_arguments(item.get("arguments")),
            })
        })
        .collect()
}

fn response_output_item_for_followup(item: &Value) -> Value {
    let mut item = item.clone();
    if let Value::Object(object) = &mut item {
        object.remove("id");
    }
    item
}

fn responses_tool_schema(tool: &Value) -> Value {
    let schema = openai_tool_schema(tool);
    let function = schema.get("function").and_then(Value::as_object);
    json!({
        "type": "function",
        "name": function.and_then(|item| item.get("name")).and_then(Value::as_str).unwrap_or(""),
        "description": function.and_then(|item| item.get("description")).and_then(Value::as_str).unwrap_or(""),
        "parameters": function.and_then(|item| item.get("parameters")).cloned().unwrap_or_else(|| json!({"type":"object","properties":{},"additionalProperties":false})),
    })
}

fn send_provider_json(
    request: ureq::Request,
    payload: Value,
    provider_name: &str,
) -> anyhow::Result<ureq::Response> {
    match request.send_json(payload) {
        Ok(response) => Ok(response),
        Err(ureq::Error::Status(code, response)) => {
            let detail = response.into_string().unwrap_or_default();
            let detail = detail.chars().take(400).collect::<String>();
            anyhow::bail!("{provider_name} request failed: {code} {detail}")
        }
        Err(error) => Err(error.into()),
    }
}

fn prompt_cache_metadata(envelope: &CachedPromptEnvelope) -> Value {
    json!({
        "prompt_cache_key": envelope.manifest.prompt_cache_key,
        "cacheable_prefix_tokens": envelope.manifest.cacheable_prefix_tokens.to_string(),
    })
}

fn provider_store_enabled(config: &ProviderConfig) -> bool {
    config
        .metadata
        .get("store")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn openai_compatible_uses_responses_api(config: &ProviderConfig) -> bool {
    if let Some(enabled) = config
        .metadata
        .get("responses_api")
        .and_then(Value::as_bool)
    {
        return enabled;
    }
    config.kind == "openai"
        || config.name == "openai"
        || config
            .base_url
            .as_deref()
            .is_some_and(|url| url.contains("api.openai.com"))
}

fn required_provider_env(config: &ProviderConfig) -> anyhow::Result<String> {
    if !direct_provider_auth_allowed() {
        return Err(direct_provider_auth_error(config));
    }
    let Some(env) = &config.api_key_env else {
        anyhow::bail!("Provider {} has no api_key_env", config.name);
    };
    get_env(env).ok_or_else(|| {
        anyhow::anyhow!(
            "Set {env} to use {}.",
            config.display_name.as_deref().unwrap_or(&config.name)
        )
    })
}

fn optional_provider_env(config: &ProviderConfig) -> anyhow::Result<Option<String>> {
    let Some(env) = &config.api_key_env else {
        return Ok(None);
    };
    if !direct_provider_auth_allowed() {
        return Err(direct_provider_auth_error(config));
    }
    let Some(value) = get_env(env) else {
        anyhow::bail!(
            "Set {env} to use {}.",
            config.display_name.as_deref().unwrap_or(&config.name)
        );
    };
    Ok(Some(value))
}

fn canonical_hbse_provider_id(provider_name: &str) -> &str {
    provider_name.strip_suffix("-hbse").unwrap_or(provider_name)
}

fn text_with_attachment_refs(message: &ChatMessage) -> String {
    if message.attachments.is_empty() {
        return message.content.clone();
    }
    let refs = message
        .attachments
        .iter()
        .map(|item| {
            format!(
                "[attachment] {}: {} ({}, {} bytes)",
                item.kind,
                item.name.as_deref().unwrap_or(&item.path),
                item.mime_type.as_deref().unwrap_or("unknown"),
                item.size_bytes.unwrap_or(0)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    [message.content.as_str(), refs.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn data_url(path: &str, mime_type: Option<&str>) -> anyhow::Result<String> {
    let encoded = STANDARD.encode(fs::read(path)?);
    Ok(format!(
        "data:{};base64,{}",
        mime_type.unwrap_or("application/octet-stream"),
        encoded
    ))
}

fn hbse_provider_http(
    config: &ProviderConfig,
    accept: &str,
    body: String,
) -> anyhow::Result<Value> {
    let base_url = config
        .base_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", config.name))?;
    hbse_provider_http_with_url(
        config,
        &format!("{}/chat/completions", base_url.trim_end_matches('/')),
        accept,
        body,
    )
}

fn hbse_provider_http_with_url(
    config: &ProviderConfig,
    url: &str,
    accept: &str,
    body: String,
) -> anyhow::Result<Value> {
    hbse_provider_http_with_url_and_headers(
        config,
        url,
        accept,
        body,
        json!({
            "Content-Type": "application/json",
            "Accept": accept,
        }),
    )
}

fn hbse_provider_http_with_url_and_headers(
    config: &ProviderConfig,
    url: &str,
    _accept: &str,
    body: String,
    headers: Value,
) -> anyhow::Result<Value> {
    let socket_path = hbse_socket_path(config);
    let secret_ref = hbse_secret_ref(config)?;
    let consumer = config
        .metadata
        .get("hbse_consumer")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("vegvisir.provider.{}", config.name));
    let purpose = config
        .metadata
        .get("hbse_purpose")
        .and_then(Value::as_str)
        .unwrap_or("model.chat");
    let payload = json!({
        "command": "provider_http",
        "secret_ref": secret_ref,
        "consumer": consumer,
        "purpose": purpose,
        "method": "POST",
        "url": url,
        "headers": headers,
        "body": body,
        "credential_header": config.metadata.get("credential_header").and_then(Value::as_str).unwrap_or("Authorization"),
        "credential_prefix": config.metadata.get("credential_prefix").and_then(Value::as_str).unwrap_or("Bearer "),
        "timeout_seconds": 120,
    });
    let mut stream = UnixStream::connect(&socket_path)
        .map_err(|error| anyhow::anyhow!("HBSE broker unavailable: {error}"))?;
    stream.write_all(serde_json::to_string(&payload)?.as_bytes())?;
    stream.write_all(b"\n")?;
    let response = read_json_line(&mut stream)?;
    if !response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        let message = response
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| response.get("error").map(Value::to_string))
            .unwrap_or_else(|| "unknown HBSE broker error".to_string());
        anyhow::bail!("HBSE broker denied provider request: {message}");
    }
    Ok(response)
}

fn hbse_socket_path(config: &ProviderConfig) -> PathBuf {
    hbse_default_or_configured_socket(config)
}

pub fn hbse_default_or_configured_socket(config: &ProviderConfig) -> PathBuf {
    if let Some(path) = config.metadata.get("hbse_socket").and_then(Value::as_str) {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("HBSE_BROKER_SOCKET") {
        return PathBuf::from(path);
    }
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("hbse").join("broker.sock");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("share")
        .join("hbse")
        .join("broker.sock")
}

fn hbse_secret_ref(config: &ProviderConfig) -> anyhow::Result<String> {
    config
        .metadata
        .get("hbse_secret_ref")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| std::env::var("HBSE_PROVIDER_SECRET_REF").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Set HBSE_PROVIDER_SECRET_REF or provider metadata hbse_secret_ref to use HBSE-routed providers."
            )
        })
}

fn provider_http_json_body(provider_name: &str, response: Value) -> anyhow::Result<Value> {
    let status = response
        .get("status_code")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let body = response
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if status >= 400 {
        anyhow::bail!(
            "{provider_name} request failed through HBSE: {status} {}",
            body.chars().take(400).collect::<String>()
        );
    }
    Ok(serde_json::from_str(&body)?)
}

fn read_json_line(stream: &mut UnixStream) -> anyhow::Result<Value> {
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
    Ok(serde_json::from_slice(line)?)
}

fn openai_messages(messages: &[ChatMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|message| {
            let role = if matches!(message.role.as_str(), "system" | "user" | "assistant") {
                message.role.as_str()
            } else {
                "user"
            };
            if message.attachments.is_empty() {
                return json!({"role": role, "content": message.content});
            }
            let mut content = vec![json!({
                "type": "text",
                "text": text_with_attachment_refs(message),
            })];
            for attachment in &message.attachments {
                if attachment.kind == "image"
                    && let Ok(url) = data_url(&attachment.path, attachment.mime_type.as_deref())
                {
                    content.push(json!({
                        "type": "image_url",
                        "image_url": {"url": url},
                    }));
                }
            }
            json!({"role": role, "content": content})
        })
        .collect()
}

fn extract_openai_compatible_text(response: &Value) -> Option<String> {
    response
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .or_else(|| response.get("output_text").and_then(Value::as_str))
        .or_else(|| response.pointer("/choices/0/text").and_then(Value::as_str))
        .map(str::to_string)
}

fn parse_openai_sse(text: &str) -> anyhow::Result<String> {
    parse_openai_sse_with_callback(text, &mut |_| {})
}

fn parse_openai_sse_with_callback(
    text: &str,
    on_delta: &mut dyn FnMut(&str),
) -> anyhow::Result<String> {
    parse_openai_sse_reader(BufReader::new(text.as_bytes()), on_delta)
}

fn parse_openai_sse_reader<R: BufRead>(
    reader: R,
    on_delta: &mut dyn FnMut(&str),
) -> anyhow::Result<String> {
    let mut output = String::new();
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let value: Value = serde_json::from_str(data)?;
        if let Some(delta) = value
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
        {
            output.push_str(delta);
            on_delta(delta);
        } else if let Some(text) = value.pointer("/choices/0/text").and_then(Value::as_str) {
            output.push_str(text);
            on_delta(text);
        } else if let Some(delta) = value.get("delta").and_then(Value::as_str) {
            output.push_str(delta);
            on_delta(delta);
        } else if let Some(text) = value.get("output_text").and_then(Value::as_str) {
            output.push_str(text);
            on_delta(text);
        }
    }
    Ok(output)
}

fn parse_anthropic_sse(text: &str) -> anyhow::Result<String> {
    let mut output = String::new();
    for line in text.lines().map(str::trim) {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let value: Value = serde_json::from_str(data)?;
        match value.get("type").and_then(Value::as_str) {
            Some("content_block_delta") => {
                if let Some(delta) = value.pointer("/delta/text").and_then(Value::as_str) {
                    output.push_str(delta);
                }
            }
            Some("error") => {
                let message = value
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("anthropic stream failed.");
                anyhow::bail!("{message}");
            }
            _ => {}
        }
    }
    Ok(output)
}

fn parse_google_stream(text: &str) -> anyhow::Result<String> {
    let mut output = String::new();
    let mut body_lines = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            append_google_json(data, &mut output)?;
        } else {
            body_lines.push(line);
        }
    }
    if !body_lines.is_empty() {
        let body = body_lines.join("\n");
        let value: Value = serde_json::from_str(&body)?;
        if let Some(items) = value.as_array() {
            for item in items {
                append_google_value(item, &mut output);
            }
        } else {
            append_google_value(&value, &mut output);
        }
    }
    Ok(output)
}

fn responses_payload(messages: &[ChatMessage], model: &ModelInfo) -> Value {
    let instructions = messages
        .iter()
        .filter(|message| message.role == "system")
        .map(text_with_attachment_refs)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    let mut input = messages
        .iter()
        .filter(|message| message.role != "system")
        .map(|message| {
            let role = if message.role == "assistant" {
                "assistant"
            } else {
                "user"
            };
            let text_type = if role == "assistant" {
                "output_text"
            } else {
                "input_text"
            };
            let mut content =
                vec![json!({"type": text_type, "text": text_with_attachment_refs(message)})];
            if role == "user" {
                for attachment in &message.attachments {
                    if attachment.kind == "image"
                        && let Ok(url) = data_url(&attachment.path, attachment.mime_type.as_deref())
                    {
                        content.push(json!({"type": "input_image", "image_url": url}));
                    }
                }
            }
            json!({
                "type": "message",
                "role": role,
                "content": content,
            })
        })
        .collect::<Vec<_>>();
    if input.is_empty() {
        let fallback = if instructions.trim().is_empty() {
            "Continue.".to_string()
        } else {
            instructions.clone()
        };
        input.push(json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": fallback}],
        }));
    }
    json!({
        "model": model.name,
        "instructions": instructions,
        "input": input,
        "tools": [],
        "tool_choice": "none",
        "parallel_tool_calls": false,
        "store": false,
        "stream": true,
        "include": [],
    })
}

fn parse_response_sse_text_reader<R: BufRead>(
    reader: R,
    on_delta: &mut dyn FnMut(&str),
) -> anyhow::Result<String> {
    let mut output = String::new();
    let mut body_lines = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(data) = line.strip_prefix("data:") else {
            body_lines.push(line.to_string());
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let value: Value = serde_json::from_str(data)?;
        if let Some(delta) = response_event_text(&value)? {
            output.push_str(&delta);
            on_delta(&delta);
        }
    }
    if output.is_empty() && !body_lines.is_empty() {
        let value: Value = serde_json::from_str(&body_lines.join("\n"))?;
        if let Some(text) = extract_response_text(&value) {
            output.push_str(&text);
            on_delta(&text);
        }
    }
    if output.is_empty() {
        anyhow::bail!("openai-sso response stream did not contain assistant text.");
    }
    Ok(output)
}

fn parse_response_sse_value(text: &str, on_delta: &mut dyn FnMut(&str)) -> anyhow::Result<Value> {
    parse_response_sse_value_reader(BufReader::new(text.as_bytes()), on_delta)
}

fn parse_response_sse_value_reader<R: BufRead>(
    reader: R,
    on_delta: &mut dyn FnMut(&str),
) -> anyhow::Result<Value> {
    let mut body_lines = Vec::new();
    let mut completed = None;
    let mut output = Vec::<Value>::new();
    let mut output_index_by_item_id = std::collections::BTreeMap::<String, usize>::new();
    let mut argument_deltas = std::collections::BTreeMap::<String, String>::new();
    let mut output_text = String::new();
    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(data) = line.strip_prefix("data:") else {
            body_lines.push(line.to_string());
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let value: Value = serde_json::from_str(data)?;
        match value.get("type").and_then(Value::as_str) {
            Some("response.completed") => {
                completed = value
                    .get("response")
                    .cloned()
                    .or_else(|| Some(value.clone()));
            }
            Some("response.output_item.added") => {
                if let Some(item) = value.get("item").cloned() {
                    if let Some(id) = item.get("id").and_then(Value::as_str) {
                        output_index_by_item_id.insert(id.to_string(), output.len());
                    }
                    output.push(item);
                }
            }
            Some("response.output_item.done") => {
                if let Some(item) = value.get("item").cloned() {
                    if let Some(id) = item.get("id").and_then(Value::as_str) {
                        if let Some(index) = output_index_by_item_id.get(id).copied() {
                            output[index] = item;
                        } else {
                            output_index_by_item_id.insert(id.to_string(), output.len());
                            output.push(item);
                        }
                    } else {
                        output.push(item);
                    }
                }
            }
            Some("response.function_call_arguments.delta") => {
                if let Some(item_id) = value.get("item_id").and_then(Value::as_str) {
                    let delta = value.get("delta").and_then(Value::as_str).unwrap_or("");
                    argument_deltas
                        .entry(item_id.to_string())
                        .or_default()
                        .push_str(delta);
                }
            }
            Some("response.function_call_arguments.done") => {
                if let Some(item_id) = value.get("item_id").and_then(Value::as_str) {
                    let arguments = value.get("arguments").and_then(Value::as_str).unwrap_or("");
                    argument_deltas.insert(item_id.to_string(), arguments.to_string());
                }
            }
            Some("response.output_text.delta") => {
                if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                    output_text.push_str(delta);
                    on_delta(delta);
                }
            }
            Some("response.failed") => {
                let message = value
                    .pointer("/response/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("openai-sso response failed.");
                anyhow::bail!("{message}")
            }
            _ => {
                if let Some(text) = extract_response_text(&value) {
                    output_text.push_str(&text);
                }
            }
        }
    }
    if let Some(mut response) = completed {
        let completed_output_empty = response
            .get("output")
            .and_then(Value::as_array)
            .map(Vec::is_empty)
            .unwrap_or(true);
        if completed_output_empty && !output.is_empty() {
            response["output"] = Value::Array(output);
        }
        if let Some(items) = response.get_mut("output").and_then(Value::as_array_mut) {
            for item in items {
                if let Some(id) = item.get("id").and_then(Value::as_str)
                    && let Some(arguments) = argument_deltas.get(id)
                {
                    item["arguments"] = Value::String(arguments.clone());
                }
            }
        }
        if output_text.is_empty()
            && let Some(text) = extract_response_text(&response)
            && !text.is_empty()
        {
            on_delta(&text);
            output_text = text;
        }
        if !output_text.is_empty() && response.get("output_text").is_none() {
            response["output_text"] = Value::String(output_text);
        }
        return Ok(response);
    }
    for (item_id, arguments) in argument_deltas {
        if let Some(index) = output_index_by_item_id.get(&item_id)
            && let Some(item) = output.get_mut(*index)
        {
            item["arguments"] = Value::String(arguments);
        }
    }
    if !output.is_empty() || !output_text.is_empty() {
        return Ok(json!({
            "output": output,
            "output_text": output_text,
        }));
    }
    if !body_lines.is_empty() {
        return Ok(serde_json::from_str(&body_lines.join("\n"))?);
    }
    anyhow::bail!("openai-sso response stream did not contain a completed response.");
}

fn response_event_text(value: &Value) -> anyhow::Result<Option<String>> {
    match value.get("type").and_then(Value::as_str) {
        Some("response.output_text.delta") => Ok(value
            .get("delta")
            .and_then(Value::as_str)
            .map(str::to_string)),
        Some("response.failed") => {
            let message = value
                .pointer("/response/error/message")
                .and_then(Value::as_str)
                .unwrap_or("openai-sso response failed.");
            anyhow::bail!("{message}")
        }
        Some("response.completed") | Some("message") => Ok(extract_response_text(value)),
        _ => Ok(extract_response_text(value)),
    }
}

fn extract_response_text(value: &Value) -> Option<String> {
    value
        .get("output_text")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            let output = value.get("output")?.as_array()?;
            let parts = output
                .iter()
                .filter_map(|item| item.get("content"))
                .flat_map(|content| {
                    content
                        .as_array()
                        .cloned()
                        .unwrap_or_else(|| vec![content.clone()])
                })
                .filter_map(|part| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .or_else(|| part.as_str())
                        .map(str::to_string)
                })
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join(""))
        })
}

fn append_google_json(data: &str, output: &mut String) -> anyhow::Result<()> {
    let value: Value = serde_json::from_str(data)?;
    append_google_value(&value, output);
    Ok(())
}

fn append_google_value(value: &Value, output: &mut String) {
    let Some(candidates) = value.get("candidates").and_then(Value::as_array) else {
        return;
    };
    for candidate in candidates {
        let Some(parts) = candidate
            .pointer("/content/parts")
            .and_then(Value::as_array)
        else {
            continue;
        };
        for part in parts {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                output.push_str(text);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum ProviderAdapterKind {
    Demo(DemoProviderAdapter),
    OpenAICompatible(OpenAICompatibleProviderAdapter),
    HBSEOpenAICompatible(HBSEOpenAICompatibleProviderAdapter),
    HBSEAzureOpenAI(HBSEAzureOpenAIProviderAdapter),
    Anthropic(AnthropicProviderAdapter),
    HBSEAnthropic(HBSEAnthropicProviderAdapter),
    Google(GoogleProviderAdapter),
    HBSEGoogle(HBSEGoogleProviderAdapter),
    OpenAISso(OpenAISsoProfileAdapter),
}

impl ProviderAdapter for ProviderAdapterKind {
    fn config(&self) -> &ProviderConfig {
        match self {
            Self::Demo(adapter) => adapter.config(),
            Self::OpenAICompatible(adapter) => adapter.config(),
            Self::HBSEOpenAICompatible(adapter) => adapter.config(),
            Self::HBSEAzureOpenAI(adapter) => adapter.config(),
            Self::Anthropic(adapter) => adapter.config(),
            Self::HBSEAnthropic(adapter) => adapter.config(),
            Self::Google(adapter) => adapter.config(),
            Self::HBSEGoogle(adapter) => adapter.config(),
            Self::OpenAISso(adapter) => adapter.config(),
        }
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        selected_provider: &str,
    ) -> anyhow::Result<String> {
        match self {
            Self::Demo(adapter) => adapter.complete(messages, model, selected_provider),
            Self::OpenAICompatible(adapter) => adapter.complete(messages, model, selected_provider),
            Self::HBSEOpenAICompatible(adapter) => {
                adapter.complete(messages, model, selected_provider)
            }
            Self::HBSEAzureOpenAI(adapter) => adapter.complete(messages, model, selected_provider),
            Self::Anthropic(adapter) => adapter.complete(messages, model, selected_provider),
            Self::HBSEAnthropic(adapter) => adapter.complete(messages, model, selected_provider),
            Self::Google(adapter) => adapter.complete(messages, model, selected_provider),
            Self::HBSEGoogle(adapter) => adapter.complete(messages, model, selected_provider),
            Self::OpenAISso(adapter) => adapter.complete(messages, model, selected_provider),
        }
    }

    fn complete_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        selected_provider: &str,
    ) -> anyhow::Result<String> {
        match self {
            Self::Demo(adapter) => adapter.complete_envelope(envelope, model, selected_provider),
            Self::OpenAICompatible(adapter) => {
                adapter.complete_envelope(envelope, model, selected_provider)
            }
            Self::HBSEOpenAICompatible(adapter) => {
                adapter.complete_envelope(envelope, model, selected_provider)
            }
            Self::HBSEAzureOpenAI(adapter) => {
                adapter.complete_envelope(envelope, model, selected_provider)
            }
            Self::Anthropic(adapter) => {
                adapter.complete_envelope(envelope, model, selected_provider)
            }
            Self::HBSEAnthropic(adapter) => {
                adapter.complete_envelope(envelope, model, selected_provider)
            }
            Self::Google(adapter) => adapter.complete_envelope(envelope, model, selected_provider),
            Self::HBSEGoogle(adapter) => {
                adapter.complete_envelope(envelope, model, selected_provider)
            }
            Self::OpenAISso(adapter) => {
                adapter.complete_envelope(envelope, model, selected_provider)
            }
        }
    }

    fn stream_envelope(
        &self,
        envelope: &CachedPromptEnvelope,
        model: &ModelInfo,
        selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        match self {
            Self::Demo(adapter) => {
                adapter.stream_envelope(envelope, model, selected_provider, on_delta)
            }
            Self::OpenAICompatible(adapter) => {
                adapter.stream_envelope(envelope, model, selected_provider, on_delta)
            }
            Self::HBSEOpenAICompatible(adapter) => {
                adapter.stream_envelope(envelope, model, selected_provider, on_delta)
            }
            Self::HBSEAzureOpenAI(adapter) => {
                adapter.stream_envelope(envelope, model, selected_provider, on_delta)
            }
            Self::Anthropic(adapter) => {
                adapter.stream_envelope(envelope, model, selected_provider, on_delta)
            }
            Self::HBSEAnthropic(adapter) => {
                adapter.stream_envelope(envelope, model, selected_provider, on_delta)
            }
            Self::Google(adapter) => {
                adapter.stream_envelope(envelope, model, selected_provider, on_delta)
            }
            Self::HBSEGoogle(adapter) => {
                adapter.stream_envelope(envelope, model, selected_provider, on_delta)
            }
            Self::OpenAISso(adapter) => {
                adapter.stream_envelope(envelope, model, selected_provider, on_delta)
            }
        }
    }

    fn supports_tool_calls(&self, model: &ModelInfo, selected_provider: &str) -> bool {
        match self {
            Self::Demo(adapter) => adapter.supports_tool_calls(model, selected_provider),
            Self::OpenAICompatible(adapter) => {
                adapter.supports_tool_calls(model, selected_provider)
            }
            Self::HBSEOpenAICompatible(adapter) => {
                adapter.supports_tool_calls(model, selected_provider)
            }
            Self::HBSEAzureOpenAI(adapter) => adapter.supports_tool_calls(model, selected_provider),
            Self::Anthropic(adapter) => adapter.supports_tool_calls(model, selected_provider),
            Self::HBSEAnthropic(adapter) => adapter.supports_tool_calls(model, selected_provider),
            Self::Google(adapter) => adapter.supports_tool_calls(model, selected_provider),
            Self::HBSEGoogle(adapter) => adapter.supports_tool_calls(model, selected_provider),
            Self::OpenAISso(adapter) => adapter.supports_tool_calls(model, selected_provider),
        }
    }

    fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        selected_provider: &str,
    ) -> anyhow::Result<String> {
        match self {
            Self::Demo(adapter) => {
                adapter.complete_with_tools(messages, model, tools, execute_tool, selected_provider)
            }
            Self::OpenAICompatible(adapter) => {
                adapter.complete_with_tools(messages, model, tools, execute_tool, selected_provider)
            }
            Self::HBSEOpenAICompatible(adapter) => {
                adapter.complete_with_tools(messages, model, tools, execute_tool, selected_provider)
            }
            Self::HBSEAzureOpenAI(adapter) => {
                adapter.complete_with_tools(messages, model, tools, execute_tool, selected_provider)
            }
            Self::Anthropic(adapter) => {
                adapter.complete_with_tools(messages, model, tools, execute_tool, selected_provider)
            }
            Self::HBSEAnthropic(adapter) => {
                adapter.complete_with_tools(messages, model, tools, execute_tool, selected_provider)
            }
            Self::Google(adapter) => {
                adapter.complete_with_tools(messages, model, tools, execute_tool, selected_provider)
            }
            Self::HBSEGoogle(adapter) => {
                adapter.complete_with_tools(messages, model, tools, execute_tool, selected_provider)
            }
            Self::OpenAISso(adapter) => {
                adapter.complete_with_tools(messages, model, tools, execute_tool, selected_provider)
            }
        }
    }

    fn complete_with_tools_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        match self {
            Self::Demo(adapter) => adapter.complete_with_tools_streaming(
                messages,
                model,
                tools,
                execute_tool,
                selected_provider,
                on_delta,
            ),
            Self::OpenAICompatible(adapter) => adapter.complete_with_tools_streaming(
                messages,
                model,
                tools,
                execute_tool,
                selected_provider,
                on_delta,
            ),
            Self::HBSEOpenAICompatible(adapter) => adapter.complete_with_tools_streaming(
                messages,
                model,
                tools,
                execute_tool,
                selected_provider,
                on_delta,
            ),
            Self::HBSEAzureOpenAI(adapter) => adapter.complete_with_tools_streaming(
                messages,
                model,
                tools,
                execute_tool,
                selected_provider,
                on_delta,
            ),
            Self::Anthropic(adapter) => adapter.complete_with_tools_streaming(
                messages,
                model,
                tools,
                execute_tool,
                selected_provider,
                on_delta,
            ),
            Self::HBSEAnthropic(adapter) => adapter.complete_with_tools_streaming(
                messages,
                model,
                tools,
                execute_tool,
                selected_provider,
                on_delta,
            ),
            Self::Google(adapter) => adapter.complete_with_tools_streaming(
                messages,
                model,
                tools,
                execute_tool,
                selected_provider,
                on_delta,
            ),
            Self::HBSEGoogle(adapter) => adapter.complete_with_tools_streaming(
                messages,
                model,
                tools,
                execute_tool,
                selected_provider,
                on_delta,
            ),
            Self::OpenAISso(adapter) => adapter.complete_with_tools_streaming(
                messages,
                model,
                tools,
                execute_tool,
                selected_provider,
                on_delta,
            ),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProviderRouter {
    providers: std::collections::BTreeMap<String, ProviderAdapterKind>,
}

impl ProviderRouter {
    pub fn from_registry(registry: &ProviderRegistry) -> Self {
        let providers = registry
            .list()
            .into_iter()
            .map(|config| (config.name.clone(), adapter_for_config(config.clone())))
            .collect();
        Self { providers }
    }

    pub fn get(&self, provider: &str) -> Option<&ProviderAdapterKind> {
        self.providers.get(provider)
    }

    pub fn for_model(
        &self,
        model: &ModelInfo,
        selected_provider: &str,
    ) -> Option<&ProviderAdapterKind> {
        if selected_provider == "openai-sso" && model.provider == "openai" {
            return self.get("openai-sso");
        }
        if selected_provider == "azure-openai-hbse" && model.provider == "azure-openai" {
            return self.get("azure-openai-hbse");
        }
        if selected_provider
            .strip_suffix("-hbse")
            .is_some_and(|base_provider| model.provider == base_provider)
        {
            return self.get(selected_provider);
        }
        self.get(&model.provider)
    }
}

fn adapter_for_config(config: ProviderConfig) -> ProviderAdapterKind {
    match config.kind.as_str() {
        "demo" | "local" => ProviderAdapterKind::Demo(DemoProviderAdapter { config }),
        "anthropic" => ProviderAdapterKind::Anthropic(AnthropicProviderAdapter { config }),
        "hbse_anthropic" => {
            ProviderAdapterKind::HBSEAnthropic(HBSEAnthropicProviderAdapter { config })
        }
        "google" => ProviderAdapterKind::Google(GoogleProviderAdapter { config }),
        "hbse_google" => ProviderAdapterKind::HBSEGoogle(HBSEGoogleProviderAdapter { config }),
        "openai_sso" => ProviderAdapterKind::OpenAISso(OpenAISsoProfileAdapter { config }),
        "hbse_openai_compatible" => {
            ProviderAdapterKind::HBSEOpenAICompatible(HBSEOpenAICompatibleProviderAdapter {
                config,
            })
        }
        "hbse_azure_openai" => {
            ProviderAdapterKind::HBSEAzureOpenAI(HBSEAzureOpenAIProviderAdapter { config })
        }
        _ => ProviderAdapterKind::OpenAICompatible(OpenAICompatibleProviderAdapter { config }),
    }
}

pub fn openai_tool_schema(tool: &Value) -> Value {
    let parameters = tool
        .get("parameters")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let normalized = if parameters.contains_key("type") {
        normalize_json_schema(&Value::Object(parameters))
    } else {
        json!({
            "type": "object",
            "properties": normalize_tool_properties(
                parameters
                    .get("properties")
                    .and_then(Value::as_object)
                    .unwrap_or(&parameters),
            ),
            "required": parameters.get("required").cloned().unwrap_or_else(|| json!([])),
            "additionalProperties": false,
        })
    };
    json!({
        "type": "function",
        "function": {
            "name": tool.get("name").and_then(Value::as_str).unwrap_or(""),
            "description": tool.get("description").and_then(Value::as_str).unwrap_or(""),
            "parameters": normalized,
        }
    })
}

fn normalize_tool_properties(properties: &Map<String, Value>) -> Value {
    Value::Object(
        properties
            .iter()
            .map(|(key, value)| (key.clone(), normalize_json_schema(value)))
            .collect(),
    )
}

fn normalize_json_schema(value: &Value) -> Value {
    let mut object = match value {
        Value::String(kind) => {
            let mut object = Map::new();
            object.insert("type".to_string(), Value::String(kind.clone()));
            object
        }
        Value::Object(object) => object.clone(),
        _ => Map::new(),
    };
    match object.get("type").and_then(Value::as_str) {
        Some("array") if !object.contains_key("items") => {
            object.insert("items".to_string(), json!({"type": "string"}));
        }
        Some("object") => {
            let properties = object
                .get("properties")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            object.insert(
                "properties".to_string(),
                normalize_tool_properties(&properties),
            );
            object
                .entry("additionalProperties".to_string())
                .or_insert(Value::Bool(false));
        }
        None => {
            object.insert("type".to_string(), Value::String("string".to_string()));
        }
        _ => {}
    }
    Value::Object(object)
}

pub fn openai_tool_loop(
    model_name: &str,
    messages: &[ChatMessage],
    tools: &[Value],
    execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
    post: &mut dyn FnMut(Value) -> anyhow::Result<Value>,
    max_tool_rounds: usize,
) -> anyhow::Result<String> {
    let mut wire_messages = openai_messages(messages);
    let payload_tools = tools.iter().map(openai_tool_schema).collect::<Vec<_>>();
    let mut observations = Vec::<(String, String)>::new();
    for _ in 0..max_tool_rounds {
        let payload = json!({
            "model": model_name,
            "messages": wire_messages,
            "stream": false,
            "tools": payload_tools,
            "tool_choice": "auto",
            "parallel_tool_calls": false,
        });
        enforce_openai_payload_budget(&payload)?;
        let data = post(payload)?;
        let message = data
            .pointer("/choices/0/message")
            .cloned()
            .unwrap_or_default();
        let tool_calls = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if tool_calls.is_empty() {
            return Ok(content);
        }
        wire_messages.push(json!({
            "role": "assistant",
            "content": content,
            "tool_calls": tool_calls,
        }));
        for tool_call in tool_calls {
            let name = tool_call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let args = parse_tool_arguments(tool_call.pointer("/function/arguments"));
            let result = truncate_model_observation(&execute_tool(&name, args));
            observations.push((name.clone(), result.clone()));
            wire_messages.push(json!({
                "role": "tool",
                "tool_call_id": tool_call.get("id").cloned().unwrap_or(Value::Null),
                "name": name,
                "content": result,
            }));
        }
    }
    if !observations.is_empty() {
        let summary = observations
            .iter()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|(name, content)| format!("[{name}]\n{content}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        return Ok(format!(
            "Tool-call round limit reached before the model produced a final answer. Latest tool observations:\n\n{summary}"
        ));
    }
    anyhow::bail!("model exceeded Vegvisir tool-call round limit.")
}

pub fn openai_tool_loop_streaming(
    model_name: &str,
    messages: &[ChatMessage],
    tools: &[Value],
    execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
    post_stream: &mut dyn FnMut(Value) -> anyhow::Result<String>,
    max_tool_rounds: usize,
    on_delta: &mut dyn FnMut(&str),
) -> anyhow::Result<String> {
    let mut wire_messages = openai_messages(messages);
    let payload_tools = tools.iter().map(openai_tool_schema).collect::<Vec<_>>();
    let mut observations = Vec::<(String, String)>::new();
    for _ in 0..max_tool_rounds {
        let payload = json!({
            "model": model_name,
            "messages": wire_messages,
            "stream": true,
            "tools": payload_tools,
            "tool_choice": "auto",
            "parallel_tool_calls": false,
        });
        enforce_openai_payload_budget(&payload)?;
        let body = post_stream(payload)?;
        let (content, tool_calls) = parse_openai_tool_sse_with_callback(&body, on_delta)?;
        if tool_calls.is_empty() {
            return Ok(content);
        }
        wire_messages.push(json!({
            "role": "assistant",
            "content": content,
            "tool_calls": tool_calls,
        }));
        for tool_call in tool_calls {
            let name = tool_call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let args = parse_tool_arguments(tool_call.pointer("/function/arguments"));
            let result = truncate_model_observation(&execute_tool(&name, args));
            observations.push((name.clone(), result.clone()));
            wire_messages.push(json!({
                "role": "tool",
                "tool_call_id": tool_call.get("id").cloned().unwrap_or(Value::Null),
                "name": name,
                "content": result,
            }));
        }
    }
    if !observations.is_empty() {
        let summary = observations
            .iter()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|(name, content)| format!("[{name}]\n{content}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let message = format!(
            "Tool-call round limit reached before the model produced a final answer. Latest tool observations:\n\n{summary}"
        );
        on_delta(&message);
        return Ok(message);
    }
    anyhow::bail!("model exceeded Vegvisir tool-call round limit.")
}

#[derive(Default)]
struct OpenAiToolCallPart {
    id: String,
    name: String,
    arguments: String,
}

fn parse_openai_tool_sse_with_callback(
    text: &str,
    on_delta: &mut dyn FnMut(&str),
) -> anyhow::Result<(String, Vec<Value>)> {
    let mut output = String::new();
    let mut calls = std::collections::BTreeMap::<usize, OpenAiToolCallPart>::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let value: Value = serde_json::from_str(data)?;
        let Some(delta) = value.pointer("/choices/0/delta") else {
            continue;
        };
        if let Some(content) = delta.get("content").and_then(Value::as_str)
            && !content.is_empty()
        {
            output.push_str(content);
            on_delta(content);
        }
        for item in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let index = item.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let part = calls.entry(index).or_default();
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                part.id = id.to_string();
            }
            if let Some(name) = item.pointer("/function/name").and_then(Value::as_str) {
                part.name.push_str(name);
            }
            if let Some(arguments) = item.pointer("/function/arguments").and_then(Value::as_str) {
                part.arguments.push_str(arguments);
            }
        }
    }
    let tool_calls = calls
        .into_values()
        .filter(|part| !part.name.is_empty())
        .map(|part| {
            json!({
                "id": if part.id.is_empty() { "call".to_string() } else { part.id },
                "type": "function",
                "function": {
                    "name": part.name,
                    "arguments": part.arguments,
                }
            })
        })
        .collect();
    Ok((output, tool_calls))
}

fn enforce_openai_payload_budget(payload: &Value) -> anyhow::Result<()> {
    let bytes = serde_json::to_string(payload)?.len();
    if bytes > OPENAI_TOOL_LOOP_MAX_BODY_BYTES {
        anyhow::bail!(
            "Vegvisir blocked an oversized model request before provider send: {bytes} bytes exceeds {OPENAI_TOOL_LOOP_MAX_BODY_BYTES} bytes. This usually means tool observations or context are too large."
        );
    }
    Ok(())
}

fn truncate_model_observation(value: &str) -> String {
    if value.len() <= TOOL_OBSERVATION_MODEL_MAX_BYTES {
        return value.to_string();
    }
    format!(
        "{}\n[tool observation truncated at {} of {} bytes before model resend]",
        truncate_utf8(value, TOOL_OBSERVATION_MODEL_MAX_BYTES),
        TOOL_OBSERVATION_MODEL_MAX_BYTES,
        value.len()
    )
}

fn parse_tool_arguments(value: Option<&Value>) -> Map<String, Value> {
    match value {
        Some(Value::String(raw)) => {
            serde_json::from_str::<Map<String, Value>>(raw).unwrap_or_default()
        }
        Some(Value::Object(object)) => object.clone(),
        _ => Map::new(),
    }
}

pub struct ConversationRunner<P: ProviderAdapter> {
    pub provider: P,
    pub models: crate::core::ModelRegistry,
    pub tools: Option<ToolRegistry>,
    pub tool_executor: Option<ToolExecutor>,
}

fn session_conversation_messages(session: &SessionState) -> Vec<ChatMessage> {
    session
        .messages
        .iter()
        .filter(|message| message.role != "system")
        .cloned()
        .collect()
}

fn approval_required_observation(observation: &Observation) -> bool {
    observation.error.as_deref() == Some("ApprovalRequired")
        || observation.content.contains("approval_id=")
        || observation
            .content
            .contains("Risky tool requires permission:")
}

impl<P: ProviderAdapter> ConversationRunner<P> {
    pub fn send(&mut self, session: &mut SessionState, content: &str) -> anyhow::Result<String> {
        self.send_with_context(session, content, None)
    }

    pub fn send_with_context(
        &mut self,
        session: &mut SessionState,
        content: &str,
        prepared_context: Option<String>,
    ) -> anyhow::Result<String> {
        session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: content.to_string(),
            attachments: std::mem::take(&mut session.pending_attachments),
            created_at: chrono::Utc::now(),
        });
        session.status = "streaming".to_string();
        session.activity = "thinking through the request".to_string();
        let model = self
            .models
            .get(&session.current_model)
            .ok_or_else(|| anyhow::anyhow!("Unknown model: {}", session.current_model))?;
        if !self
            .models
            .is_model_allowed_for_provider(model, &session.current_provider)
        {
            session.current_provider = model.provider.clone();
        }
        if let Some(limit) = model.context_window {
            session.context_limit = limit;
        }
        let mut provider_messages = session_conversation_messages(session);
        if !session.system_prompt.is_empty() {
            provider_messages.insert(
                0,
                ChatMessage {
                    role: "system".to_string(),
                    content: session.system_prompt.clone(),
                    attachments: Vec::new(),
                    created_at: chrono::Utc::now(),
                },
            );
        }
        if let Some(prepared_context) = prepared_context.filter(|text| !text.trim().is_empty()) {
            let insertion_index = if session.system_prompt.is_empty() {
                0
            } else {
                1
            };
            provider_messages.insert(
                insertion_index,
                ChatMessage {
                    role: "system".to_string(),
                    content: prepared_context,
                    attachments: Vec::new(),
                    created_at: chrono::Utc::now(),
                },
            );
        }
        let started = Instant::now();
        let response = if self
            .provider
            .supports_tool_calls(model, &session.current_provider)
            && self.tools.is_some()
            && self.tool_executor.is_some()
        {
            session.activity = "thinking through tool use".to_string();
            let tools = self
                .tools
                .as_ref()
                .map(ToolRegistry::schemas)
                .unwrap_or_default();
            let executor = self.tool_executor.as_mut().expect("checked above");
            let session_id = session.session_id.clone();
            let mut approval_required = None::<String>;
            let mut execute_tool = |name: &str, args: Map<String, Value>| -> String {
                session.activity = format!("using tool {name}");
                let observation = executor.execute(ToolCall {
                    name: name.to_string(),
                    args,
                });
                session.activity = format!("finished tool {name}");
                if approval_required_observation(&observation) {
                    approval_required = Some(observation.content.clone());
                }
                if observation.ok {
                    observation.content
                } else {
                    format!(
                        "{}: {}",
                        observation.error.unwrap_or_else(|| "ToolError".to_string()),
                        observation.content
                    )
                }
            };
            let _ = session_id;
            let response = self.provider.complete_with_tools(
                &provider_messages,
                model,
                &tools,
                &mut execute_tool,
                &session.current_provider,
            )?;
            if let Some(message) = approval_required {
                anyhow::bail!("{message}");
            }
            response
        } else {
            self.provider
                .complete(&provider_messages, model, &session.current_provider)?
        };
        session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.clone(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        session.last_latency_ms = started.elapsed().as_millis() as u64;
        session.tokens_used += (content.split_whitespace().count()
            + response.split_whitespace().count())
        .max(1) as u64;
        session.status = "ready".to_string();
        session.activity.clear();
        Ok(response)
    }

    pub fn send_with_envelope(
        &mut self,
        session: &mut SessionState,
        content: &str,
        envelope: CachedPromptEnvelope,
    ) -> anyhow::Result<String> {
        self.send_with_envelope_streaming(session, content, envelope, &mut |_| {})
    }

    pub fn send_with_envelope_streaming(
        &mut self,
        session: &mut SessionState,
        content: &str,
        envelope: CachedPromptEnvelope,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<String> {
        session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: content.to_string(),
            attachments: std::mem::take(&mut session.pending_attachments),
            created_at: chrono::Utc::now(),
        });
        session.status = "streaming".to_string();
        session.activity = "using CMS-v2 prepared model request".to_string();
        let model = self
            .models
            .get(&session.current_model)
            .ok_or_else(|| anyhow::anyhow!("Unknown model: {}", session.current_model))?;
        if !self
            .models
            .is_model_allowed_for_provider(model, &session.current_provider)
        {
            session.current_provider = model.provider.clone();
        }
        if let Some(limit) = model.context_window {
            session.context_limit = limit;
        }
        let mut envelope = envelope;
        apply_system_prompt_to_envelope(&mut envelope, &session.system_prompt);
        let mut provider_messages = session_conversation_messages(session);
        provider_messages.insert(
            0,
            ChatMessage {
                role: "system".to_string(),
                content: envelope.model_request.prompt.clone(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
        );
        let started = Instant::now();
        let response = if self
            .provider
            .supports_tool_calls(model, &session.current_provider)
            && self.tools.is_some()
            && self.tool_executor.is_some()
        {
            session.activity = "thinking through tool use".to_string();
            let tools = self
                .tools
                .as_ref()
                .map(ToolRegistry::schemas)
                .unwrap_or_default();
            let executor = self.tool_executor.as_mut().expect("checked above");
            let mut approval_required = None::<String>;
            let mut execute_tool = |name: &str, args: Map<String, Value>| -> String {
                session.activity = format!("using tool {name}");
                let observation = executor.execute(ToolCall {
                    name: name.to_string(),
                    args,
                });
                session.activity = format!("finished tool {name}");
                if approval_required_observation(&observation) {
                    approval_required = Some(observation.content.clone());
                }
                if observation.ok {
                    observation.content
                } else {
                    format!(
                        "{}: {}",
                        observation.error.unwrap_or_else(|| "ToolError".to_string()),
                        observation.content
                    )
                }
            };
            let response = self.provider.complete_with_tools_streaming(
                &provider_messages,
                model,
                &tools,
                &mut execute_tool,
                &session.current_provider,
                on_delta,
            )?;
            if let Some(message) = approval_required {
                anyhow::bail!("{message}");
            }
            response
        } else {
            let response =
                self.provider
                    .complete(&provider_messages, model, &session.current_provider)?;
            on_delta(&response);
            response
        };
        session.last_prompt_cache_key = Some(envelope.manifest.prompt_cache_key.clone());
        session.last_prompt_manifest_id = Some(envelope.manifest.manifest_id.clone());
        session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.clone(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        session.last_latency_ms = started.elapsed().as_millis() as u64;
        session.tokens_used += (content.split_whitespace().count()
            + response.split_whitespace().count()
            + envelope.manifest.total_prompt_tokens)
            .max(1) as u64;
        session.status = "ready".to_string();
        session.activity.clear();
        Ok(response)
    }
}

fn apply_system_prompt_to_envelope(envelope: &mut CachedPromptEnvelope, system_prompt: &str) {
    let system_prompt = system_prompt.trim();
    if system_prompt.is_empty() {
        return;
    }
    envelope.model_request.prompt = format!(
        "Harness system prompt:\n{system_prompt}\n\n{}",
        envelope.model_request.prompt
    );
    envelope.manifest.total_prompt_tokens = envelope
        .manifest
        .total_prompt_tokens
        .saturating_add(system_prompt.split_whitespace().count());
}

fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
