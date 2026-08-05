use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::Receiver,
    },
    thread,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use cms_v2::prompt_cache::CachedPromptEnvelope;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    context::{ContextBudgetAction, ContextBudgetDecision, ContextBudgetPolicy},
    control_requests::{ApprovalControlPayload, ControlRequest},
    core::{Attachment, ChatMessage, ModelInfo, ProviderConfig, ProviderRegistry, SessionState},
    environment::get_env,
    guardrails::ApprovalResolution,
    openai_sso::{codex_base_url, load_fresh_tokens_for_metadata},
    telemetry::{count_text_tokens, selected_usage_or_counted},
    tools::{CommandOutputSink, ToolExecutor, ToolRegistry, with_command_output_sink},
    types::{Observation, ToolCall},
};

const TOOL_OBSERVATION_MODEL_MAX_BYTES: usize = 24 * 1024;
const OPENAI_TOOL_LOOP_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const PROVIDER_CONTEXT_REPAIR_TARGET_PERCENT: f64 = 75.0;
// Image inputs are billed by dimensions/detail, not by tokenizing their base64 transport.
// This intentionally conservative allowance exceeds typical provider image-token charges.
const PROVIDER_IMAGE_INPUT_TOKEN_ESTIMATE: usize = 4_096;
static RUNTIME_MAX_TOOL_ROUNDS: AtomicUsize = AtomicUsize::new(0);

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

fn max_tool_rounds() -> usize {
    let runtime_limit = RUNTIME_MAX_TOOL_ROUNDS.load(Ordering::Relaxed);
    if runtime_limit > 0 {
        return runtime_limit;
    }
    get_env("VEGVISIR_MAX_TOOL_ROUNDS")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(usize::MAX)
}

pub fn configured_max_tool_rounds() -> Option<usize> {
    let rounds = max_tool_rounds();
    if rounds == usize::MAX {
        None
    } else {
        Some(rounds)
    }
}

pub fn configured_max_tool_rounds_label() -> String {
    configured_max_tool_rounds()
        .map(|rounds| rounds.to_string())
        .unwrap_or_else(|| "unlimited".to_string())
}

pub fn set_runtime_max_tool_rounds(limit: Option<usize>) -> Option<usize> {
    match limit {
        Some(limit) => {
            let limit = limit.max(1);
            RUNTIME_MAX_TOOL_ROUNDS.store(limit, Ordering::Relaxed);
            Some(limit)
        }
        None => {
            RUNTIME_MAX_TOOL_ROUNDS.store(0, Ordering::Relaxed);
            configured_max_tool_rounds()
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl TokenUsage {
    pub fn total(self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderGeneratedArtifact {
    pub kind: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub suggested_filename: Option<String>,
}

impl ProviderGeneratedArtifact {
    fn new(kind: impl Into<String>, mime_type: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            kind: kind.into(),
            mime_type: mime_type.into(),
            bytes,
            suggested_filename: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderResponse {
    pub content: String,
    pub usage: Option<TokenUsage>,
    pub artifacts: Vec<ProviderGeneratedArtifact>,
}

impl ProviderResponse {
    pub fn new(content: String) -> Self {
        Self {
            content,
            usage: None,
            artifacts: Vec::new(),
        }
    }
}

pub trait ProviderAdapter {
    fn config(&self) -> &ProviderConfig;
    fn complete(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        selected_provider: &str,
    ) -> anyhow::Result<String>;

    fn complete_with_usage(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        selected_provider: &str,
    ) -> anyhow::Result<ProviderResponse> {
        self.complete(messages, model, selected_provider)
            .map(ProviderResponse::new)
    }

    fn complete_with_usage_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<ProviderResponse> {
        let response = self.complete_with_usage(messages, model, selected_provider)?;
        if !response.content.is_empty() {
            on_delta(&response.content);
        }
        Ok(response)
    }

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

    fn complete_with_tools_usage(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        tools: &[Value],
        execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
        selected_provider: &str,
    ) -> anyhow::Result<ProviderResponse> {
        self.complete_with_tools(messages, model, tools, execute_tool, selected_provider)
            .map(ProviderResponse::new)
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
        if model_uses_images_generations_api(model) {
            let response = self.post_image_generation(messages, model)?;
            return Ok(response.content);
        }
        if openai_compatible_uses_responses_api(&self.config) {
            return self.post_response_streaming(messages, model, &mut |_| {});
        }
        self.post_chat_completion(model, openai_messages(messages), None)
    }

    fn complete_with_usage(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<ProviderResponse> {
        if model_uses_images_generations_api(model) {
            return self.post_image_generation(messages, model);
        }
        if openai_compatible_uses_responses_api(&self.config) {
            let response =
                self.post_response_stream_json(responses_payload(messages, model), &mut |_| {})?;
            return Ok(responses_provider_response(&response));
        }
        self.post_chat_completion_with_usage(model, openai_messages(messages), None)
    }

    fn complete_with_usage_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<ProviderResponse> {
        if openai_compatible_uses_responses_api(&self.config) {
            let response =
                self.post_response_stream_json(responses_payload(messages, model), on_delta)?;
            return Ok(responses_provider_response(&response));
        }
        if model_outputs_media(model) {
            let response = self.complete_with_usage(messages, model, selected_provider)?;
            if !response.content.is_empty() {
                on_delta(&response.content);
            }
            return Ok(response);
        }
        self.post_chat_completion_streaming(model, openai_messages(messages), None, on_delta)
            .map(ProviderResponse::new)
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
                max_tool_rounds(),
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
                .set("Accept", "text/event-stream")
                // Do not reuse a provider connection for SSE. A stale pooled
                // connection can surface as an intermittent chunk decoder error.
                .set("Connection", "close");
            if let Some(api_key) = &api_key {
                request = request.set("Authorization", &format!("Bearer {api_key}"));
            }
            read_ureq_stream_body_with_retry(|| {
                send_provider_json(request.clone(), payload.clone(), &self.config.name)
            })
        };
        openai_tool_loop_streaming(
            model,
            messages,
            tools,
            execute_tool,
            &mut post,
            max_tool_rounds(),
            on_delta,
        )
    }
}

impl OpenAICompatibleProviderAdapter {
    fn post_image_generation(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
    ) -> anyhow::Result<ProviderResponse> {
        let api_key = optional_provider_env(&self.config)?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let url = format!("{}/images/generations", base_url.trim_end_matches('/'));
        let payload = image_generation_payload(messages, model, &self.config)?;
        let mut request = ureq::post(&url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json");
        if let Some(api_key) = api_key {
            request = request.set("Authorization", &format!("Bearer {api_key}"));
        }
        let response: Value =
            send_provider_json(request, payload, &self.config.name)?.into_json()?;
        let provider_response = image_generation_provider_response(&response);
        if provider_response.artifacts.is_empty() {
            anyhow::bail!(
                "Provider {} image generation response did not include media artifacts",
                self.config.name
            );
        }
        Ok(provider_response)
    }

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
            .set("Accept", "text/event-stream")
            // Do not reuse a provider connection for SSE. A stale pooled
            // connection can surface as an intermittent chunk decoder error.
            .set("Connection", "close");
        if let Some(api_key) = api_key {
            request = request.set("Authorization", &format!("Bearer {api_key}"));
        }
        stream_ureq_response_with_retry(
            || send_provider_json(request.clone(), payload.clone(), &self.config.name),
            |response, callback| {
                parse_response_sse_value_reader(BufReader::new(response.into_reader()), callback)
            },
            on_delta,
        )
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
        apply_chat_reasoning_settings(&mut payload, model);
        let store = provider_store_enabled(&self.config);
        if store {
            payload["store"] = json!(true);
        }
        if store && let Some(metadata) = metadata {
            payload["metadata"] = metadata;
        }
        let mut request = ureq::post(&url)
            .set("Content-Type", "application/json")
            .set("Accept", "text/event-stream")
            // Do not reuse a provider connection for SSE. A stale pooled
            // connection can surface as an intermittent chunk decoder error.
            .set("Connection", "close");
        if let Some(api_key) = api_key {
            request = request.set("Authorization", &format!("Bearer {api_key}"));
        }
        stream_ureq_response_with_retry(
            || send_provider_json(request.clone(), payload.clone(), &self.config.name),
            |response, callback| {
                parse_openai_sse_reader(BufReader::new(response.into_reader()), callback)
            },
            on_delta,
        )
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
        apply_chat_reasoning_settings(&mut payload, model);
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
        if stream {
            request = request.set("Connection", "close");
        }
        if let Some(api_key) = api_key {
            request = request.set("Authorization", &format!("Bearer {api_key}"));
        }
        if stream {
            let body = read_ureq_stream_body_with_retry(|| {
                send_provider_json(request.clone(), payload.clone(), &self.config.name)
            })?;
            parse_openai_sse(&body)
        } else {
            let response = send_provider_json(request, payload, &self.config.name)?;
            let response: Value = response.into_json()?;
            extract_openai_compatible_text(&response).ok_or_else(|| {
                anyhow::anyhow!(
                    "Provider {} response did not include assistant text",
                    self.config.name
                )
            })
        }
    }

    fn post_chat_completion_with_usage(
        &self,
        model: &ModelInfo,
        messages: Vec<Value>,
        metadata: Option<Value>,
    ) -> anyhow::Result<ProviderResponse> {
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
            "stream": false
        });
        apply_chat_reasoning_settings(&mut payload, model);
        let store = provider_store_enabled(&self.config);
        if store {
            payload["store"] = json!(true);
        }
        if store && let Some(metadata) = metadata {
            payload["metadata"] = metadata;
        }
        let mut request = ureq::post(&url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json");
        if let Some(api_key) = api_key {
            request = request.set("Authorization", &format!("Bearer {api_key}"));
        }
        let response: Value =
            send_provider_json(request, payload, &self.config.name)?.into_json()?;
        let provider_response = openai_compatible_provider_response(&response);
        if provider_response.content.is_empty() && provider_response.artifacts.is_empty() {
            anyhow::bail!(
                "Provider {} response did not include assistant text or media artifacts",
                self.config.name
            );
        }
        Ok(provider_response)
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
        if model_uses_images_generations_api(model) {
            let response = self.post_image_generation(messages, model)?;
            return Ok(response.content);
        }
        if openai_compatible_uses_responses_api(&self.config) {
            return self.post_response_streaming(messages, model, &mut |_| {});
        }
        self.post_chat_completion(model, openai_messages(messages), None)
    }

    fn complete_with_usage(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<ProviderResponse> {
        if model_uses_images_generations_api(model) {
            return self.post_image_generation(messages, model);
        }
        if openai_compatible_uses_responses_api(&self.config) {
            let response =
                self.post_response_stream_json(responses_payload(messages, model), &mut |_| {})?;
            return Ok(responses_provider_response(&response));
        }
        self.post_chat_completion_with_usage(model, openai_messages(messages), None)
    }

    fn complete_with_usage_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        selected_provider: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> anyhow::Result<ProviderResponse> {
        if openai_compatible_uses_responses_api(&self.config) {
            let response =
                self.post_response_stream_json(responses_payload(messages, model), on_delta)?;
            return Ok(responses_provider_response(&response));
        }
        if model_outputs_media(model) {
            let response = self.complete_with_usage(messages, model, selected_provider)?;
            if !response.content.is_empty() {
                on_delta(&response.content);
            }
            return Ok(response);
        }
        self.post_chat_completion_streaming(model, openai_messages(messages), None, on_delta)
            .map(ProviderResponse::new)
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
                max_tool_rounds(),
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
            model,
            messages,
            tools,
            execute_tool,
            &mut post,
            max_tool_rounds(),
            on_delta,
        )
    }
}

impl HBSEOpenAICompatibleProviderAdapter {
    fn post_image_generation(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
    ) -> anyhow::Result<ProviderResponse> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let payload = image_generation_payload(messages, model, &self.config)?;
        let response = hbse_provider_http_with_url_and_purpose(
            &self.config,
            &format!("{}/images/generations", base_url.trim_end_matches('/')),
            "application/json",
            serde_json::to_string(&payload)?,
            self.config
                .metadata
                .get("hbse_image_generation_purpose")
                .and_then(Value::as_str)
                .or(Some("model.image_generation")),
        )?;
        let value = provider_http_json_body(&self.config.name, response)?;
        let provider_response = image_generation_provider_response(&value);
        if provider_response.artifacts.is_empty() {
            anyhow::bail!(
                "Provider {} image generation response did not include media artifacts",
                self.config.name
            );
        }
        Ok(provider_response)
    }

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
        apply_chat_reasoning_settings(&mut payload, model);
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
        apply_chat_reasoning_settings(&mut payload, model);
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

    fn post_chat_completion_with_usage(
        &self,
        model: &ModelInfo,
        messages: Vec<Value>,
        metadata: Option<Value>,
    ) -> anyhow::Result<ProviderResponse> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Provider {} has no base_url", self.config.name))?;
        let mut payload = json!({
            "model": model.name,
            "messages": messages,
            "stream": false,
        });
        apply_chat_reasoning_settings(&mut payload, model);
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
            "application/json",
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
        let value: Value = serde_json::from_str(&body)?;
        let provider_response = openai_compatible_provider_response(&value);
        if provider_response.content.is_empty() && provider_response.artifacts.is_empty() {
            anyhow::bail!(
                "Provider {} response did not include assistant text or media artifacts",
                self.config.name
            );
        }
        Ok(provider_response)
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
            model,
            messages,
            tools,
            execute_tool,
            &mut post,
            max_tool_rounds(),
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
        apply_chat_reasoning_settings(&mut payload, model);
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
        selected_provider: &str,
    ) -> anyhow::Result<String> {
        self.complete_with_usage(messages, model, selected_provider)
            .map(|response| response.content)
    }

    fn complete_with_usage(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<ProviderResponse> {
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
            .set("Accept", "text/event-stream")
            .set("Connection", "close");
        let body = read_ureq_stream_body_with_retry(|| {
            send_provider_json(request.clone(), payload.clone(), &self.config.name)
        })?;
        parse_anthropic_sse_response(&body)
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
        anthropic_tool_loop(
            messages,
            model,
            tools,
            execute_tool,
            &mut post,
            max_tool_rounds(),
        )
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
                    "content": anthropic_message_content(message),
                })
            })
            .collect::<Vec<_>>(),
    });
    if !system_prompt.is_empty() {
        payload["system"] = anthropic_cached_text_blocks(&system_prompt);
    }
    apply_anthropic_reasoning_settings(&mut payload, model);
    payload
}

fn anthropic_cached_text_blocks(text: &str) -> Value {
    let text = text.trim();
    if text.is_empty() {
        return Value::Array(Vec::new());
    }
    Value::Array(vec![json!({
        "type": "text",
        "text": text,
        "cache_control": {"type": "ephemeral"},
    })])
}

fn anthropic_apply_cache_control_to_last(items: &mut [Value]) {
    if let Some(Value::Object(object)) = items.last_mut() {
        object.insert("cache_control".to_string(), json!({"type": "ephemeral"}));
    }
}

fn anthropic_message_content(message: &ChatMessage) -> Value {
    let image_attachments = message
        .attachments
        .iter()
        .filter(|attachment| attachment.kind == "image")
        .collect::<Vec<_>>();
    if image_attachments.is_empty() {
        return Value::String(text_with_attachment_refs(message));
    }
    let mut blocks = vec![json!({
        "type": "text",
        "text": text_with_attachment_refs(message),
    })];
    for attachment in image_attachments {
        if let Ok(data) = image_attachment_base64(&attachment.path) {
            blocks.push(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": attachment
                        .mime_type
                        .as_deref()
                        .unwrap_or("application/octet-stream"),
                    "data": data,
                },
            }));
        }
    }
    Value::Array(blocks)
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
    let mut anthropic_tools = tools.iter().map(anthropic_tool_schema).collect::<Vec<_>>();
    anthropic_apply_cache_control_to_last(&mut anthropic_tools);
    let mut observations = Vec::<(String, String)>::new();
    for _ in 0..max_tool_rounds {
        let mut payload = json!({
            "model": model.name,
            "max_tokens": 4096,
            "stream": false,
            "messages": wire_messages,
            "tools": anthropic_tools.clone(),
            "tool_choice": {"type": "auto"},
        });
        apply_anthropic_reasoning_settings(&mut payload, model);
        if !system_prompt.is_empty() {
            payload["system"] = anthropic_cached_text_blocks(&system_prompt);
        }
        enforce_provider_payload_budget(model, &payload)?;
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
        let mut results = Vec::new();
        for (index, tool_use) in tool_uses.into_iter().enumerate() {
            let Some(id) = tool_use
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                continue;
            };
            let Some(name) = tool_use
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
            else {
                continue;
            };
            let args = parse_tool_arguments(tool_use.get("input"));
            let result = execute_tool_call_for_model(
                &name,
                args,
                index,
                execute_tool,
                &mut observations,
                |result| Ok(truncate_model_observation(result)),
            )?;
            results.push(json!({
                "type": "tool_result",
                "tool_use_id": id,
                "content": result,
            }));
        }
        wire_messages.push(json!({"role": "user", "content": results}));
    }
    tool_round_limit_result(&observations, max_tool_rounds)
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
        selected_provider: &str,
    ) -> anyhow::Result<String> {
        self.complete_with_usage(messages, model, selected_provider)
            .map(|response| response.content)
    }

    fn complete_with_usage(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        _selected_provider: &str,
    ) -> anyhow::Result<ProviderResponse> {
        self.post_messages_streaming_with_usage(messages, model)
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
        anthropic_tool_loop(
            messages,
            model,
            tools,
            execute_tool,
            &mut post,
            max_tool_rounds(),
        )
    }
}

impl HBSEAnthropicProviderAdapter {
    fn post_messages_streaming(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
    ) -> anyhow::Result<String> {
        self.post_messages_streaming_with_usage(messages, model)
            .map(|response| response.content)
    }

    fn post_messages_streaming_with_usage(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
    ) -> anyhow::Result<ProviderResponse> {
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
        parse_anthropic_sse_response(&body)
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
        let payload = google_generate_content_payload(messages, model);
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            base_url.trim_end_matches('/'),
            model.name,
            api_key
        );
        let request = ureq::post(&url)
            .set("Content-Type", "application/json")
            .set("Accept", "text/event-stream")
            .set("Connection", "close");
        let body = read_ureq_stream_body_with_retry(|| {
            send_provider_json(request.clone(), payload.clone(), &self.config.name)
        })?;
        parse_google_stream(&body)
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
        google_tool_loop(
            messages,
            model,
            tools,
            execute_tool,
            &mut post,
            max_tool_rounds(),
        )
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
        google_tool_loop(
            messages,
            model,
            tools,
            execute_tool,
            &mut post,
            max_tool_rounds(),
        )
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
            serde_json::to_string(&google_generate_content_payload(messages, model))?,
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

fn google_generate_content_payload(messages: &[ChatMessage], model: &ModelInfo) -> Value {
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
                "parts": google_message_parts(message),
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
    apply_google_reasoning_settings(&mut payload, model);
    payload
}

fn google_message_parts(message: &ChatMessage) -> Vec<Value> {
    let mut parts = vec![json!({"text": text_with_attachment_refs(message)})];
    for attachment in &message.attachments {
        if attachment.kind != "image" {
            continue;
        }
        if let Ok(data) = image_attachment_base64(&attachment.path) {
            parts.push(json!({
                "inlineData": {
                    "mimeType": attachment
                        .mime_type
                        .as_deref()
                        .unwrap_or("application/octet-stream"),
                    "data": data,
                },
            }));
        }
    }
    parts
}

fn google_tool_loop(
    messages: &[ChatMessage],
    model: &ModelInfo,
    tools: &[Value],
    execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
    post: &mut dyn FnMut(Value) -> anyhow::Result<Value>,
    max_tool_rounds: usize,
) -> anyhow::Result<String> {
    let base_payload = google_generate_content_payload(messages, model);
    let system_instruction = base_payload.get("systemInstruction").cloned();
    let mut contents = base_payload
        .get("contents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| vec![json!({"role": "user", "parts": [{"text": ""}]})]);
    let mut observations = Vec::<(String, String)>::new();
    for _ in 0..max_tool_rounds {
        let mut payload = json!({
            "contents": contents,
            "tools": [{"functionDeclarations": tools.iter().map(google_tool_schema).collect::<Vec<_>>()}],
            "toolConfig": {"functionCallingConfig": {"mode": "AUTO"}},
        });
        if let Some(system_instruction) = &system_instruction {
            payload["systemInstruction"] = system_instruction.clone();
        }
        enforce_provider_payload_budget(model, &payload)?;
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
        let mut response_parts = Vec::new();
        for (index, call) in calls.into_iter().enumerate() {
            let Some(name) = call.get("name").and_then(Value::as_str).map(str::to_string) else {
                continue;
            };
            let args = parse_tool_arguments(call.get("args"));
            let result = execute_tool_call_for_model(
                &name,
                args,
                index,
                execute_tool,
                &mut observations,
                |result| Ok(truncate_model_observation(result)),
            )?;
            response_parts.push(json!({
                "functionResponse": {
                    "name": name,
                    "response": {"result": result},
                }
            }));
        }
        contents.push(json!({"role": "user", "parts": response_parts}));
    }
    tool_round_limit_result(&observations, max_tool_rounds)
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
        let max_tool_rounds = max_tool_rounds();
        let mut observations = Vec::<(String, String)>::new();
        for _ in 0..max_tool_rounds {
            enforce_provider_payload_budget(model, &payload)?;
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
            for (index, call) in tool_calls.into_iter().enumerate() {
                let result = execute_tool_call_for_model(
                    &call.name,
                    call.args,
                    index,
                    execute_tool,
                    &mut observations,
                    |result| Ok(truncate_model_observation(result)),
                )?;
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call.call_id,
                    "output": result,
                }));
            }
        }
        let result = tool_round_limit_result(&observations, max_tool_rounds)?;
        on_delta(&result);
        Ok(result)
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
        .set("Accept", "text/event-stream")
        .set("Connection", "close");
        stream_ureq_response_with_retry(
            || match request.clone().send_json(payload.clone()) {
                Ok(response) => Ok(response),
                Err(ureq::Error::Status(401, _)) => {
                    anyhow::bail!(
                        "OpenAI SSO rejected the saved login. Run /auth openai-sso again."
                    )
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
            },
            |response, callback| {
                parse_response_sse_value_reader(BufReader::new(response.into_reader()), callback)
            },
            on_delta,
        )
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
        .set("Accept", "text/event-stream")
        .set("Connection", "close");
        stream_ureq_response_with_retry(
            || match request.clone().send_json(payload.clone()) {
                Ok(response) => Ok(response),
                Err(ureq::Error::Status(401, _)) => {
                    anyhow::bail!(
                        "OpenAI SSO rejected the saved login. Run /auth openai-sso again."
                    )
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
            },
            |response, callback| {
                parse_response_sse_text_reader(BufReader::new(response.into_reader()), callback)
            },
            on_delta,
        )
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
    let mut observations = Vec::<(String, String)>::new();
    for _ in 0..max_tool_rounds {
        enforce_provider_payload_budget(model, &payload)?;
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
        for (index, call) in tool_calls.into_iter().enumerate() {
            let result = execute_tool_call_for_model(
                &call.name,
                call.args,
                index,
                execute_tool,
                &mut observations,
                |result| Ok(truncate_model_observation(result)),
            )?;
            input.push(json!({
                "type": "function_call_output",
                "call_id": call.call_id,
                "output": result,
            }));
        }
    }
    tool_round_limit_result(&observations, max_tool_rounds)
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

const MAX_PROVIDER_STREAM_ATTEMPTS: usize = 2;
const PROVIDER_STREAM_RETRY_DELAY: Duration = Duration::from_millis(75);

fn is_retryable_chunk_decode_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        let message = cause.to_string().to_ascii_lowercase();
        message.contains("decoding chunks")
            || message.contains("chunk decode")
            || message.contains("chunked encoding")
    })
}

fn stream_ureq_response_with_retry<T, Send, Parse>(
    mut send: Send,
    mut parse: Parse,
    on_delta: &mut dyn FnMut(&str),
) -> anyhow::Result<T>
where
    Send: FnMut() -> anyhow::Result<ureq::Response>,
    Parse: FnMut(ureq::Response, &mut dyn FnMut(&str)) -> anyhow::Result<T>,
{
    // A stream can fail after already yielding visible text. If the retry
    // starts from the beginning, suppress the prefix we already displayed so
    // the user sees one continuous answer rather than duplicated text.
    let mut displayed_prefix = String::new();
    let mut replay_offset = 0;
    for attempt in 0..MAX_PROVIDER_STREAM_ATTEMPTS {
        let response = send()?;
        let result = {
            let mut emit = |delta: &str| {
                if attempt == 0 {
                    displayed_prefix.push_str(delta);
                    on_delta(delta);
                } else {
                    emit_replayed_stream_delta(
                        delta,
                        &displayed_prefix,
                        &mut replay_offset,
                        on_delta,
                    );
                }
            };
            parse(response, &mut emit)
        };
        match result {
            Ok(value) => return Ok(value),
            Err(error)
                if attempt + 1 < MAX_PROVIDER_STREAM_ATTEMPTS
                    && is_retryable_chunk_decode_error(&error) =>
            {
                thread::sleep(PROVIDER_STREAM_RETRY_DELAY);
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("provider stream retry loop must return from every attempt")
}

fn emit_replayed_stream_delta(
    delta: &str,
    displayed_prefix: &str,
    replay_offset: &mut usize,
    on_delta: &mut dyn FnMut(&str),
) {
    if delta.is_empty() {
        return;
    }
    if *replay_offset < displayed_prefix.len() {
        let remaining = &displayed_prefix[*replay_offset..];
        if remaining.starts_with(delta) {
            *replay_offset += delta.len();
            return;
        }
        if let Some(suffix) = delta.strip_prefix(remaining) {
            *replay_offset = displayed_prefix.len();
            if !suffix.is_empty() {
                on_delta(suffix);
            }
            return;
        }
        // The provider did not replay the same prefix. Do not hide a new
        // response; forward it rather than risking a truncated answer.
        *replay_offset = displayed_prefix.len();
    }
    on_delta(delta);
}

fn read_ureq_stream_body_with_retry<Send>(mut send: Send) -> anyhow::Result<String>
where
    Send: FnMut() -> anyhow::Result<ureq::Response>,
{
    for attempt in 0..MAX_PROVIDER_STREAM_ATTEMPTS {
        let response = send()?;
        let mut body = String::new();
        match response.into_reader().read_to_string(&mut body) {
            Ok(_) => return Ok(body),
            Err(error) => {
                let error = anyhow::Error::from(error);
                if attempt + 1 < MAX_PROVIDER_STREAM_ATTEMPTS
                    && is_retryable_chunk_decode_error(&error)
                {
                    thread::sleep(PROVIDER_STREAM_RETRY_DELAY);
                    continue;
                }
                return Err(error);
            }
        }
    }
    unreachable!("provider stream retry loop must return from every attempt")
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
    let encoded = image_attachment_base64(path)?;
    Ok(format!(
        "data:{};base64,{}",
        mime_type.unwrap_or("application/octet-stream"),
        encoded
    ))
}

fn image_attachment_base64(path: &str) -> anyhow::Result<String> {
    Ok(STANDARD.encode(fs::read(path)?))
}

fn model_uses_images_generations_api(model: &ModelInfo) -> bool {
    if model
        .metadata
        .get("images_generations_api")
        .or_else(|| model.metadata.get("image_generation_api"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }
    for key in ["api_endpoint", "endpoint", "output_endpoint"] {
        if model
            .metadata
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| value.contains("images/generations"))
        {
            return true;
        }
    }
    let name = model.name.to_ascii_lowercase();
    name.contains("gpt-image")
        || name.contains("dall-e")
        || name.contains("grok-imagine")
        || name.contains("imagen")
        || name.contains("image-generation")
}

fn image_generation_prompt(messages: &[ChatMessage]) -> anyhow::Result<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == "user" && !message.content.trim().is_empty())
        .or_else(|| {
            messages
                .iter()
                .rev()
                .find(|message| !message.content.trim().is_empty())
        })
        .map(text_with_attachment_refs)
        .map(|prompt| prompt.trim().to_string())
        .filter(|prompt| !prompt.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Image generation requires a non-empty prompt."))
}

fn image_generation_payload(
    messages: &[ChatMessage],
    model: &ModelInfo,
    config: &ProviderConfig,
) -> anyhow::Result<Value> {
    let mut payload = json!({
        "model": model.name,
        "prompt": image_generation_prompt(messages)?,
    });
    if let Some(size) = model
        .metadata
        .get("image_size")
        .or_else(|| config.metadata.get("image_size"))
        .and_then(Value::as_str)
    {
        payload["size"] = Value::String(size.to_string());
    }
    if let Some(quality) = model
        .metadata
        .get("image_quality")
        .or_else(|| config.metadata.get("image_quality"))
        .and_then(Value::as_str)
    {
        payload["quality"] = Value::String(quality.to_string());
    }
    if let Some(style) = model
        .metadata
        .get("image_style")
        .or_else(|| config.metadata.get("image_style"))
        .and_then(Value::as_str)
    {
        payload["style"] = Value::String(style.to_string());
    }
    if let Some(n) = model
        .metadata
        .get("image_count")
        .or_else(|| config.metadata.get("image_count"))
        .and_then(Value::as_u64)
    {
        payload["n"] = Value::Number(n.into());
    }
    let response_format = model
        .metadata
        .get("image_response_format")
        .or_else(|| config.metadata.get("image_response_format"))
        .and_then(Value::as_str)
        .or_else(|| {
            (config.name == "xai" || config.name == "xai-hbse" || model.provider == "xai")
                .then_some("b64_json")
        });
    if let Some(response_format) = response_format {
        payload["response_format"] = Value::String(response_format.to_string());
    }
    Ok(payload)
}

fn image_generation_provider_response(response: &Value) -> ProviderResponse {
    let mut provider_response = ProviderResponse {
        content: response
            .get("created")
            .and_then(Value::as_i64)
            .map(|_| "Image generation completed.".to_string())
            .unwrap_or_default(),
        usage: extract_provider_usage(response),
        artifacts: extract_generated_artifacts(response),
    };
    if let Some(revised_prompt) = response
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("revised_prompt").and_then(Value::as_str))
        .next()
    {
        provider_response.content =
            format!("Image generation completed. Revised prompt: {revised_prompt}");
    }
    provider_response
}

fn model_outputs_media(model: &ModelInfo) -> bool {
    if model
        .metadata
        .get("output_media")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }
    let media_value = |value: &Value| -> bool {
        value
            .as_str()
            .map(|text| {
                let text = text.to_ascii_lowercase();
                text.contains("image") || text.contains("video") || text.contains("audio")
            })
            .unwrap_or(false)
    };
    for key in [
        "output_modality",
        "output_modalities",
        "modalities",
        "modality",
        "model_type",
        "type",
    ] {
        if let Some(value) = model.metadata.get(key) {
            if media_value(value) {
                return true;
            }
            if value
                .as_array()
                .map(|items| items.iter().any(media_value))
                .unwrap_or(false)
            {
                return true;
            }
        }
    }
    let name = model.name.to_ascii_lowercase();
    name.contains("gpt-image")
        || name.contains("dall-e")
        || name.contains("imagen")
        || name.contains("image")
        || name.contains("imagine")
        || name.contains("sora")
        || name.contains("video")
}

fn save_generated_artifacts(
    session: &SessionState,
    artifacts: &[ProviderGeneratedArtifact],
) -> anyhow::Result<Vec<Attachment>> {
    if artifacts.is_empty() {
        return Ok(Vec::new());
    }
    let output_dir = Path::new(&session.cwd).join(".vegvisir").join("generated");
    fs::create_dir_all(&output_dir)?;
    let timestamp = chrono::Utc::now()
        .format("%Y%m%dT%H%M%S%.3fZ")
        .to_string()
        .replace(':', "");
    artifacts
        .iter()
        .enumerate()
        .map(|(index, artifact)| {
            let fallback = format!(
                "generated-{}-{:02}.{}",
                timestamp,
                index + 1,
                extension_for_mime(&artifact.mime_type)
            );
            let filename = artifact
                .suggested_filename
                .as_deref()
                .map(sanitize_generated_filename)
                .filter(|name| !name.is_empty())
                .unwrap_or(fallback);
            let path = unique_child_path(&output_dir, &filename);
            fs::write(&path, &artifact.bytes)?;
            Ok(Attachment {
                path: path.display().to_string(),
                kind: artifact.kind.clone(),
                mime_type: Some(artifact.mime_type.clone()),
                name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string()),
                size_bytes: Some(artifact.bytes.len() as u64),
            })
        })
        .collect()
}

fn response_with_generated_artifact_notice(content: String, attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        return content;
    }
    let mut lines = Vec::new();
    if !content.trim().is_empty() {
        lines.push(content.trim_end().to_string());
    }
    lines.push("Generated media saved by Vegvisir:".to_string());
    lines.extend(attachments.iter().map(|attachment| {
        format!(
            "- {} ({}, {} bytes)",
            attachment.path,
            attachment
                .mime_type
                .as_deref()
                .unwrap_or("application/octet-stream"),
            attachment.size_bytes.unwrap_or(0)
        )
    }));
    lines.join("\n")
}

fn extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type.split(';').next().unwrap_or(mime_type).trim() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "audio/mpeg" => "mp3",
        "audio/wav" | "audio/wave" => "wav",
        "audio/ogg" => "ogg",
        _ => "bin",
    }
}

fn sanitize_generated_filename(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => ch,
            _ => '-',
        })
        .collect::<String>()
        .trim_matches(['.', '-'])
        .to_string();
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        String::new()
    } else {
        sanitized
    }
}

fn unique_child_path(dir: &Path, filename: &str) -> PathBuf {
    let path = dir.join(filename);
    if !path.exists() {
        return path;
    }
    let stem = Path::new(filename)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "generated".to_string());
    let extension = Path::new(filename)
        .extension()
        .map(|extension| extension.to_string_lossy().to_string());
    for counter in 2..10_000 {
        let candidate = match &extension {
            Some(extension) => dir.join(format!("{stem}-{counter}.{extension}")),
            None => dir.join(format!("{stem}-{counter}")),
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}", Uuid::new_v4().simple()))
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
    hbse_provider_http_with_url_and_purpose(config, url, accept, body, None)
}

fn hbse_provider_http_with_url_and_purpose(
    config: &ProviderConfig,
    url: &str,
    accept: &str,
    body: String,
    purpose_override: Option<&str>,
) -> anyhow::Result<Value> {
    hbse_provider_http_with_url_and_headers_and_purpose(
        config,
        url,
        accept,
        body,
        json!({
            "Content-Type": "application/json",
            "Accept": accept,
        }),
        purpose_override,
    )
}

fn hbse_provider_http_with_url_and_headers(
    config: &ProviderConfig,
    url: &str,
    accept: &str,
    body: String,
    headers: Value,
) -> anyhow::Result<Value> {
    hbse_provider_http_with_url_and_headers_and_purpose(config, url, accept, body, headers, None)
}

fn hbse_provider_http_with_url_and_headers_and_purpose(
    config: &ProviderConfig,
    url: &str,
    _accept: &str,
    body: String,
    headers: Value,
    purpose_override: Option<&str>,
) -> anyhow::Result<Value> {
    let socket_path = hbse_socket_path(config);
    let secret_ref = hbse_secret_ref(config)?;
    let consumer = config
        .metadata
        .get("hbse_consumer")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("vegvisir.provider.{}", config.name));
    let purpose = purpose_override.unwrap_or_else(|| {
        config
            .metadata
            .get("hbse_purpose")
            .and_then(Value::as_str)
            .unwrap_or("model.chat")
    });
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

fn extract_provider_usage(response: &Value) -> Option<TokenUsage> {
    let usage = response.get("usage")?;
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_add(
            usage
                .get("cache_creation_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        )
        .saturating_add(
            usage
                .get("cache_read_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if input_tokens == 0 && output_tokens == 0 {
        None
    } else {
        Some(TokenUsage {
            input_tokens,
            output_tokens,
        })
    }
}

fn extract_openai_compatible_text(response: &Value) -> Option<String> {
    openai_compatible_text_parts(response)
        .map(|parts| parts.join(""))
        .filter(|text| !text.is_empty())
}

fn openai_compatible_provider_response(response: &Value) -> ProviderResponse {
    ProviderResponse {
        content: openai_compatible_text_parts(response)
            .map(|parts| parts.join(""))
            .unwrap_or_default(),
        usage: extract_provider_usage(response),
        artifacts: extract_generated_artifacts(response),
    }
}

fn responses_provider_response(response: &Value) -> ProviderResponse {
    ProviderResponse {
        content: extract_response_text(response).unwrap_or_default(),
        usage: extract_provider_usage(response),
        artifacts: extract_generated_artifacts(response),
    }
}

fn openai_compatible_text_parts(response: &Value) -> Option<Vec<String>> {
    if let Some(text) = response
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .or_else(|| response.get("output_text").and_then(Value::as_str))
        .or_else(|| response.pointer("/choices/0/text").and_then(Value::as_str))
    {
        return Some(vec![text.to_string()]);
    }
    let parts = response
        .pointer("/choices/0/message/content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(text_from_content_part)
        .collect::<Vec<_>>();
    (!parts.is_empty()).then_some(parts)
}

fn text_from_content_part(part: &Value) -> Option<String> {
    part.get("text")
        .and_then(Value::as_str)
        .or_else(|| part.get("output_text").and_then(Value::as_str))
        .or_else(|| part.as_str())
        .map(str::to_string)
}

fn extract_generated_artifacts(value: &Value) -> Vec<ProviderGeneratedArtifact> {
    let mut artifacts = Vec::new();
    collect_generated_artifacts(value, &mut artifacts);
    artifacts
}

fn collect_generated_artifacts(value: &Value, artifacts: &mut Vec<ProviderGeneratedArtifact>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_generated_artifacts(item, artifacts);
            }
        }
        Value::Object(object) => {
            if let Some(artifact) = generated_artifact_from_object(object) {
                artifacts.push(artifact);
                return;
            }
            for child in object.values() {
                collect_generated_artifacts(child, artifacts);
            }
        }
        _ => {}
    }
}

fn generated_artifact_from_object(
    object: &Map<String, Value>,
) -> Option<ProviderGeneratedArtifact> {
    let (bytes, declared_mime) = if let Some(encoded) = encoded_media_from_object(object) {
        let (declared_mime, encoded) = split_optional_data_url(encoded);
        (STANDARD.decode(encoded).ok()?, declared_mime)
    } else {
        (bytes_from_media_url_object(object)?, None)
    };
    let mime_type = declared_mime
        .or_else(|| object_mime_type(object))
        .or_else(|| infer_mime_type_from_bytes(&bytes).map(str::to_string))
        .or_else(|| default_mime_type_for_generated_object(object))?;
    if !is_generated_media_mime(&mime_type) {
        return None;
    }
    let kind = media_kind_from_mime(&mime_type).to_string();
    let mut artifact = ProviderGeneratedArtifact::new(kind, mime_type, bytes);
    artifact.suggested_filename = object
        .get("filename")
        .or_else(|| object.get("file_name"))
        .or_else(|| object.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(artifact)
}

fn bytes_from_media_url_object(object: &Map<String, Value>) -> Option<Vec<u8>> {
    let url = object.get("url")?.as_str()?;
    if let Some((_, encoded)) = url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(','))
    {
        return STANDARD.decode(encoded).ok();
    }
    if !url.starts_with("https://") {
        return None;
    }
    let response = ureq::get(url)
        .set("Accept", "image/*,video/*,audio/*")
        .call()
        .ok()?;
    if response.status() >= 400 {
        return None;
    }
    let mut reader = response.into_reader().take(64 * 1024 * 1024);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).ok()?;
    (!bytes.is_empty()).then_some(bytes)
}

fn encoded_media_from_object(object: &Map<String, Value>) -> Option<&str> {
    object
        .get("b64_json")
        .or_else(|| object.get("base64"))
        .or_else(|| object.get("bytes_base64"))
        .or_else(|| object.get("body_base64"))
        .or_else(|| object.get("data"))
        .or_else(|| {
            matches!(
                object.get("type").and_then(Value::as_str),
                Some("image_generation_call") | Some("video_generation_call")
            )
            .then(|| object.get("result"))
            .flatten()
        })
        .and_then(Value::as_str)
}

fn split_optional_data_url(encoded: &str) -> (Option<String>, &str) {
    let Some(rest) = encoded.strip_prefix("data:") else {
        return (None, encoded);
    };
    let Some((metadata, body)) = rest.split_once(',') else {
        return (None, encoded);
    };
    let mime = metadata
        .split(';')
        .next()
        .filter(|mime| mime.contains('/'))
        .map(str::to_string);
    (mime, body)
}

fn infer_mime_type_from_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.get(4..8) == Some(b"ftyp") {
        Some("video/mp4")
    } else {
        None
    }
}

fn default_mime_type_for_generated_object(object: &Map<String, Value>) -> Option<String> {
    match object.get("type").and_then(Value::as_str) {
        Some("image_generation_call") => Some("image/png".to_string()),
        Some("video_generation_call") => Some("video/mp4".to_string()),
        _ if object.contains_key("b64_json") => Some("image/png".to_string()),
        _ => None,
    }
}

fn object_mime_type(object: &Map<String, Value>) -> Option<String> {
    object
        .get("mime_type")
        .or_else(|| object.get("mimeType"))
        .or_else(|| object.get("content_type"))
        .or_else(|| object.get("contentType"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            object
                .get("type")
                .and_then(Value::as_str)
                .filter(|value| value.contains('/'))
                .map(str::to_string)
        })
        .or_else(|| {
            object
                .get("media_type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn is_generated_media_mime(mime_type: &str) -> bool {
    matches!(media_kind_from_mime(mime_type), "image" | "video" | "audio")
}

fn media_kind_from_mime(mime_type: &str) -> &'static str {
    match mime_type.split('/').next().unwrap_or_default() {
        "image" => "image",
        "video" => "video",
        "audio" => "audio",
        _ => "file",
    }
}

fn provider_error_message(value: &Value) -> Option<String> {
    value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/error/error/message")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            value
                .pointer("/response/error/message")
                .and_then(Value::as_str)
        })
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
        if let Some(message) = provider_error_message(&value) {
            anyhow::bail!(message);
        }
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
    parse_anthropic_sse_response(text).map(|response| response.content)
}

fn parse_anthropic_sse_response(text: &str) -> anyhow::Result<ProviderResponse> {
    let mut output = String::new();
    let mut usage = TokenUsage::default();
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
            Some("message_start") => {
                if let Some(event_usage) = value
                    .get("message")
                    .and_then(|message| message.get("usage"))
                {
                    usage = merge_token_usage(usage, anthropic_usage_tokens(event_usage));
                }
            }
            Some("message_delta") => {
                if let Some(event_usage) = value.get("usage") {
                    usage = merge_token_usage(usage, anthropic_usage_tokens(event_usage));
                }
            }
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
    Ok(ProviderResponse {
        content: output,
        usage: (usage.total() > 0).then_some(usage),
        artifacts: Vec::new(),
    })
}

fn merge_token_usage(left: TokenUsage, right: TokenUsage) -> TokenUsage {
    TokenUsage {
        input_tokens: left.input_tokens.saturating_add(right.input_tokens),
        output_tokens: left.output_tokens.saturating_add(right.output_tokens),
    }
}

fn anthropic_usage_tokens(usage: &Value) -> TokenUsage {
    let input_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_add(
            usage
                .get("cache_creation_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        )
        .saturating_add(
            usage
                .get("cache_read_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
    let output_tokens = usage
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    TokenUsage {
        input_tokens,
        output_tokens,
    }
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
                append_google_value(item, &mut output)?;
            }
        } else {
            append_google_value(&value, &mut output)?;
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
    let mut payload = json!({
        "model": model.name,
        "instructions": instructions,
        "input": input,
        "tools": [],
        "tool_choice": "none",
        "parallel_tool_calls": true,
        "store": false,
        "stream": true,
        "include": [],
    });
    apply_responses_reasoning_settings(&mut payload, model);
    payload
}

fn model_reasoning_level(model: &ModelInfo) -> Option<&str> {
    model
        .metadata
        .get("reasoning_level")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn model_reasoning_effort(model: &ModelInfo) -> Option<&str> {
    model
        .metadata
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| match model_reasoning_level(model) {
            Some("minimal") | Some("low") | Some("medium") | Some("high") | Some("xhigh")
            | Some("max") => model_reasoning_level(model),
            _ => None,
        })
}

fn apply_responses_reasoning_settings(payload: &mut Value, model: &ModelInfo) {
    let summary = should_request_reasoning_summary(model);
    let effort = model_reasoning_effort(model);
    if !summary && effort.is_none() {
        return;
    }
    let mut reasoning = Map::new();
    if summary {
        reasoning.insert("summary".to_string(), json!("auto"));
    }
    if let Some(effort) = effort {
        reasoning.insert("effort".to_string(), json!(effort));
    }
    payload["reasoning"] = Value::Object(reasoning);
}

fn apply_chat_reasoning_settings(payload: &mut Value, model: &ModelInfo) {
    if let Some(effort) = model_reasoning_effort(model) {
        payload["reasoning_effort"] = json!(effort);
    }
}

fn anthropic_thinking_budget_tokens(model: &ModelInfo) -> Option<u64> {
    model
        .metadata
        .get("thinking_budget_tokens")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .or_else(|| match model_reasoning_level(model) {
            Some("low") => Some(4_096),
            Some("medium") => Some(8_192),
            Some("high") => Some(16_384),
            Some("xhigh") => Some(24_576),
            Some("max") => Some(32_768),
            _ => None,
        })
}

fn apply_anthropic_reasoning_settings(payload: &mut Value, model: &ModelInfo) {
    if let Some(budget) = anthropic_thinking_budget_tokens(model) {
        payload["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": budget,
        });
    }
}

fn google_thinking_budget(model: &ModelInfo) -> Option<i64> {
    model
        .metadata
        .get("thinking_budget")
        .and_then(Value::as_i64)
        .or_else(|| match model_reasoning_level(model) {
            Some("minimal") => Some(0),
            Some("low") => Some(1_024),
            Some("medium") => Some(8_192),
            // Gemini exposes a provider-specific token budget rather than
            // OpenAI's named effort values. Keep the strongest portable
            // levels within the currently supported 32K budget ceiling.
            Some("high") | Some("xhigh") | Some("max") => Some(32_768),
            _ => None,
        })
}

fn apply_google_reasoning_settings(payload: &mut Value, model: &ModelInfo) {
    if let Some(budget) = google_thinking_budget(model) {
        payload["generationConfig"]["thinkingConfig"]["thinkingBudget"] = json!(budget);
    }
}

fn parse_response_sse_text_reader<R: BufRead>(
    reader: R,
    on_delta: &mut dyn FnMut(&str),
) -> anyhow::Result<String> {
    let mut output = String::new();
    let mut body_lines = Vec::new();
    let mut emitted_reasoning_trace = false;
    let mut emitted_answer_header = false;
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
        handle_response_stream_text_event(
            &value,
            &mut output,
            on_delta,
            &mut emitted_reasoning_trace,
            &mut emitted_answer_header,
        )?;
    }
    if output.is_empty() && !body_lines.is_empty() {
        let value: Value = serde_json::from_str(&body_lines.join("\n"))?;
        if let Some(text) = extract_response_text(&value) {
            emit_response_output_delta(
                &text,
                &mut output,
                on_delta,
                emitted_reasoning_trace,
                &mut emitted_answer_header,
            );
        }
    }
    close_reasoning_trace_if_unanswered(on_delta, emitted_reasoning_trace, emitted_answer_header);
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
    let mut emitted_reasoning_trace = false;
    let mut emitted_answer_header = false;
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
            Some("response.reasoning_summary_text.delta")
            | Some("response.reasoning_text.delta") => emit_reasoning_trace_delta(
                value.get("delta").and_then(Value::as_str).unwrap_or(""),
                on_delta,
                &mut emitted_reasoning_trace,
            ),
            Some("response.output_text.delta") => emit_response_output_delta(
                value.get("delta").and_then(Value::as_str).unwrap_or(""),
                &mut output_text,
                on_delta,
                emitted_reasoning_trace,
                &mut emitted_answer_header,
            ),
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
            emit_response_output_delta(
                &text,
                &mut output_text,
                on_delta,
                emitted_reasoning_trace,
                &mut emitted_answer_header,
            );
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
        close_reasoning_trace_if_unanswered(
            on_delta,
            emitted_reasoning_trace,
            emitted_answer_header,
        );
        return Ok(json!({
            "output": output,
            "output_text": output_text,
        }));
    }
    close_reasoning_trace_if_unanswered(on_delta, emitted_reasoning_trace, emitted_answer_header);
    if !body_lines.is_empty() {
        return Ok(serde_json::from_str(&body_lines.join("\n"))?);
    }
    anyhow::bail!("openai-sso response stream did not contain a completed response.");
}

fn should_request_reasoning_summary(model: &ModelInfo) -> bool {
    model
        .metadata
        .get("reasoning_summary")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            model.provider.contains("openai")
                || model.name.starts_with("gpt-")
                || model.name.starts_with('o')
        })
}

fn handle_response_stream_text_event(
    value: &Value,
    output: &mut String,
    on_delta: &mut dyn FnMut(&str),
    emitted_reasoning_trace: &mut bool,
    emitted_answer_header: &mut bool,
) -> anyhow::Result<()> {
    match value.get("type").and_then(Value::as_str) {
        Some("response.reasoning_summary_text.delta") | Some("response.reasoning_text.delta") => {
            emit_reasoning_trace_delta(
                value.get("delta").and_then(Value::as_str).unwrap_or(""),
                on_delta,
                emitted_reasoning_trace,
            );
            Ok(())
        }
        Some("response.output_text.delta") => {
            emit_response_output_delta(
                value.get("delta").and_then(Value::as_str).unwrap_or(""),
                output,
                on_delta,
                *emitted_reasoning_trace,
                emitted_answer_header,
            );
            Ok(())
        }
        _ => {
            if let Some(delta) = response_event_text(value)? {
                emit_response_output_delta(
                    &delta,
                    output,
                    on_delta,
                    *emitted_reasoning_trace,
                    emitted_answer_header,
                );
            }
            Ok(())
        }
    }
}

fn emit_reasoning_trace_delta(
    _delta: &str,
    _on_delta: &mut dyn FnMut(&str),
    _emitted_reasoning_trace: &mut bool,
) {
    // Provider reasoning/thinking summaries are intentionally hidden from the
    // chat transcript. Keep accepting these stream events so provider streams
    // parse normally, but do not render or persist them as assistant content.
}

fn emit_response_output_delta(
    delta: &str,
    output: &mut String,
    on_delta: &mut dyn FnMut(&str),
    emitted_reasoning_trace: bool,
    emitted_answer_header: &mut bool,
) {
    if delta.is_empty() {
        return;
    }
    if emitted_reasoning_trace && !*emitted_answer_header {
        on_delta("\n```\n\n**Answer**\n\n");
        *emitted_answer_header = true;
    }
    output.push_str(delta);
    on_delta(delta);
}

fn close_reasoning_trace_if_unanswered(
    on_delta: &mut dyn FnMut(&str),
    emitted_reasoning_trace: bool,
    emitted_answer_header: bool,
) {
    if emitted_reasoning_trace && !emitted_answer_header {
        on_delta("\n```\n");
    }
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
    append_google_value(&value, output)
}

fn append_google_value(value: &Value, output: &mut String) -> anyhow::Result<()> {
    if let Some(message) = google_error_message(value) {
        anyhow::bail!(message);
    }
    let Some(candidates) = value.get("candidates").and_then(Value::as_array) else {
        return Ok(());
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
    Ok(())
}

fn google_error_message(value: &Value) -> Option<String> {
    value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/promptFeedback/blockReasonMessage")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            value
                .pointer("/promptFeedback/blockReason")
                .and_then(Value::as_str)
        })
        .map(str::to_string)
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

    fn complete_with_usage(
        &self,
        messages: &[ChatMessage],
        model: &ModelInfo,
        selected_provider: &str,
    ) -> anyhow::Result<ProviderResponse> {
        match self {
            Self::Demo(adapter) => adapter.complete_with_usage(messages, model, selected_provider),
            Self::OpenAICompatible(adapter) => {
                adapter.complete_with_usage(messages, model, selected_provider)
            }
            Self::HBSEOpenAICompatible(adapter) => {
                adapter.complete_with_usage(messages, model, selected_provider)
            }
            Self::HBSEAzureOpenAI(adapter) => {
                adapter.complete_with_usage(messages, model, selected_provider)
            }
            Self::Anthropic(adapter) => {
                adapter.complete_with_usage(messages, model, selected_provider)
            }
            Self::HBSEAnthropic(adapter) => {
                adapter.complete_with_usage(messages, model, selected_provider)
            }
            Self::Google(adapter) => {
                adapter.complete_with_usage(messages, model, selected_provider)
            }
            Self::HBSEGoogle(adapter) => {
                adapter.complete_with_usage(messages, model, selected_provider)
            }
            Self::OpenAISso(adapter) => {
                adapter.complete_with_usage(messages, model, selected_provider)
            }
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
    model: &ModelInfo,
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
        let mut payload = json!({
            "model": model.name,
            "messages": wire_messages,
            "stream": false,
            "tools": payload_tools,
            "tool_choice": "auto",
            "parallel_tool_calls": true,
        });
        apply_chat_reasoning_settings(&mut payload, model);
        enforce_openai_payload_budget(model, &payload)?;
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
        for (index, tool_call) in tool_calls.into_iter().enumerate() {
            let name = tool_call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let args = parse_tool_arguments(tool_call.pointer("/function/arguments"));
            let tool_call_id = tool_call.get("id").cloned().unwrap_or(Value::Null);
            let result = execute_tool_call_for_model(
                &name,
                args,
                index,
                execute_tool,
                &mut observations,
                |result| {
                    compact_model_observation_for_openai_tool_loop(
                        model,
                        &wire_messages,
                        &payload_tools,
                        tool_call_id,
                        &name,
                        result,
                        false,
                    )
                },
            )?;
            wire_messages.push(json!({
                "role": "tool",
                "tool_call_id": tool_call.get("id").cloned().unwrap_or(Value::Null),
                "name": name,
                "content": result,
            }));
        }
    }
    tool_round_limit_result(&observations, max_tool_rounds)
}

pub fn openai_tool_loop_streaming(
    model: &ModelInfo,
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
        let mut payload = json!({
            "model": model.name,
            "messages": wire_messages,
            "stream": true,
            "tools": payload_tools,
            "tool_choice": "auto",
            "parallel_tool_calls": true,
        });
        apply_chat_reasoning_settings(&mut payload, model);
        enforce_openai_payload_budget(model, &payload)?;
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
        for (index, tool_call) in tool_calls.into_iter().enumerate() {
            let name = tool_call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let args = parse_tool_arguments(tool_call.pointer("/function/arguments"));
            let tool_call_id = tool_call.get("id").cloned().unwrap_or(Value::Null);
            let result = execute_tool_call_for_model(
                &name,
                args,
                index,
                execute_tool,
                &mut observations,
                |result| {
                    compact_model_observation_for_openai_tool_loop(
                        model,
                        &wire_messages,
                        &payload_tools,
                        tool_call_id,
                        &name,
                        result,
                        true,
                    )
                },
            )?;
            wire_messages.push(json!({
                "role": "tool",
                "tool_call_id": tool_call.get("id").cloned().unwrap_or(Value::Null),
                "name": name,
                "content": result,
            }));
        }
    }
    let result = tool_round_limit_result(&observations, max_tool_rounds)?;
    on_delta(&result);
    Ok(result)
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

fn enforce_provider_payload_budget(
    model: &ModelInfo,
    payload: &Value,
) -> anyhow::Result<ContextBudgetDecision> {
    let serialized = serde_json::to_string(payload)?;
    let bytes = serialized.len();
    if bytes > OPENAI_TOOL_LOOP_MAX_BODY_BYTES {
        anyhow::bail!(
            "Vegvisir blocked an oversized model request before provider send: {bytes} bytes exceeds {OPENAI_TOOL_LOOP_MAX_BODY_BYTES} bytes. This usually means tool observations or context are too large."
        );
    }
    let estimated_tokens = estimated_provider_payload_tokens(model, payload)?;
    let decision = evaluate_payload_token_budget(model, estimated_tokens);
    if decision.action == ContextBudgetAction::Block {
        anyhow::bail!(
            "Vegvisir blocked an oversized model request before provider send: estimated {} input tokens for model {} exceeds the active context budget ({}; {:.1}% used). {}",
            estimated_tokens,
            model.name,
            model
                .context_window
                .map(|limit| limit.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            decision.percentage,
            decision.warnings.join(" ")
        );
    }
    Ok(decision)
}

fn enforce_openai_payload_budget(model: &ModelInfo, payload: &Value) -> anyhow::Result<()> {
    enforce_provider_payload_budget(model, payload).map(|_| ())
}

fn evaluate_payload_token_budget(model: &ModelInfo, used_tokens: usize) -> ContextBudgetDecision {
    let max_tokens = model.context_window.unwrap_or(0) as usize;
    ContextBudgetPolicy::default().evaluate(used_tokens, max_tokens)
}

fn estimated_provider_payload_tokens(model: &ModelInfo, payload: &Value) -> anyhow::Result<usize> {
    let mut accounting_payload = payload.clone();
    let image_count = redact_inline_images_for_token_accounting(&mut accounting_payload);
    let serialized = serde_json::to_string(&accounting_payload)?;
    Ok((count_text_tokens(&model.name, &serialized) as usize)
        .saturating_add(image_count.saturating_mul(PROVIDER_IMAGE_INPUT_TOKEN_ESTIMATE)))
}

fn redact_inline_images_for_token_accounting(value: &mut Value) -> usize {
    match value {
        Value::Array(items) => items
            .iter_mut()
            .map(redact_inline_images_for_token_accounting)
            .sum(),
        Value::Object(object) => {
            let mut image_count = 0;

            // OpenAI/Responses-style data URLs.
            if let Some(Value::String(url)) = object.get_mut("image_url")
                && url.starts_with("data:image/")
                && url.contains(";base64,")
            {
                *url = "[inline image omitted from text token accounting]".to_string();
                image_count += 1;
            }

            // Anthropic-style {type: base64, media_type: image/*, data: ...} blocks.
            let anthropic_image = object.get("type").and_then(Value::as_str) == Some("base64")
                && object
                    .get("media_type")
                    .and_then(Value::as_str)
                    .is_some_and(|mime| mime.starts_with("image/"));
            // Google-style {mimeType: image/*, data: ...} inlineData blocks.
            let google_image = object
                .get("mimeType")
                .and_then(Value::as_str)
                .is_some_and(|mime| mime.starts_with("image/"));
            if (anthropic_image || google_image)
                && let Some(Value::String(data)) = object.get_mut("data")
                && !data.is_empty()
            {
                *data = "[inline image omitted from text token accounting]".to_string();
                image_count += 1;
            }

            image_count
                + object
                    .values_mut()
                    .map(redact_inline_images_for_token_accounting)
                    .sum::<usize>()
        }
        _ => 0,
    }
}

fn provider_payload_budget_decision(
    model: &ModelInfo,
    payload: &Value,
) -> anyhow::Result<ContextBudgetDecision> {
    Ok(evaluate_payload_token_budget(
        model,
        estimated_provider_payload_tokens(model, payload)?,
    ))
}

fn provider_message_budget_decision(
    messages: &[ChatMessage],
    model: &ModelInfo,
) -> anyhow::Result<ContextBudgetDecision> {
    provider_payload_budget_decision(model, &provider_budget_probe_payload(messages, model))
}

fn provider_budget_probe_payload(messages: &[ChatMessage], model: &ModelInfo) -> Value {
    match model.provider.as_str() {
        "anthropic" => anthropic_messages_payload(messages, model),
        "google" => google_generate_content_payload(messages, model),
        _ => {
            let mut payload = if model_uses_responses_payload(model) {
                responses_payload(messages, model)
            } else {
                json!({
                    "model": model.name,
                    "messages": openai_messages(messages),
                    "stream": true,
                })
            };
            apply_chat_reasoning_settings(&mut payload, model);
            payload
        }
    }
}

fn model_uses_responses_payload(model: &ModelInfo) -> bool {
    matches!(model.provider.as_str(), "openai" | "openai-sso")
        || model
            .metadata
            .get("responses_api")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn context_repair_target_tokens(model: &ModelInfo) -> usize {
    let max_tokens = model.context_window.unwrap_or(0) as usize;
    if max_tokens == 0 {
        return 0;
    }
    ((max_tokens as f64) * (PROVIDER_CONTEXT_REPAIR_TARGET_PERCENT / 100.0)).floor() as usize
}

fn trim_chat_messages_to_provider_budget(
    messages: Vec<ChatMessage>,
    model: &ModelInfo,
) -> anyhow::Result<(Vec<ChatMessage>, ContextBudgetDecision, bool)> {
    let initial = provider_message_budget_decision(&messages, model)?;
    if initial.action != ContextBudgetAction::CompactRecommended
        && initial.action != ContextBudgetAction::Block
    {
        return Ok((messages, initial, false));
    }
    let target_tokens = context_repair_target_tokens(model);
    if target_tokens == 0 {
        return Ok((messages, initial, false));
    }

    let system_messages = messages
        .iter()
        .filter(|message| message.role == "system")
        .cloned()
        .collect::<Vec<_>>();
    let conversational = messages
        .iter()
        .filter(|message| message.role != "system")
        .cloned()
        .collect::<Vec<_>>();
    let mut kept = Vec::new();
    for message in conversational.iter().rev() {
        kept.push(message.clone());
        let candidate = repaired_provider_messages(&system_messages, &kept, conversational.len());
        let decision = provider_message_budget_decision(&candidate, model)?;
        if decision.action == ContextBudgetAction::Block
            || (decision.percentage > PROVIDER_CONTEXT_REPAIR_TARGET_PERCENT
                && count_probe_tokens(&candidate, model) > target_tokens)
        {
            kept.pop();
            if kept.is_empty() {
                kept.push(truncate_chat_message_for_provider_budget(
                    message,
                    model,
                    target_tokens,
                ));
            }
            break;
        }
    }
    if kept.is_empty() && !conversational.is_empty() {
        if let Some(message) = conversational.last() {
            kept.push(truncate_chat_message_for_provider_budget(
                message,
                model,
                target_tokens,
            ));
        }
    }
    let repaired = repaired_provider_messages(&system_messages, &kept, conversational.len());
    let repaired_decision = provider_message_budget_decision(&repaired, model)?;
    Ok((repaired, repaired_decision, true))
}

fn repaired_provider_messages(
    system_messages: &[ChatMessage],
    kept_reversed: &[ChatMessage],
    original_conversation_len: usize,
) -> Vec<ChatMessage> {
    let mut repaired = system_messages.to_vec();
    let omitted = original_conversation_len.saturating_sub(kept_reversed.len());
    if omitted > 0 {
        repaired.push(ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Earlier conversation history was omitted by Vegvisir before provider send because the active context budget required compaction. Omitted messages: {omitted}."
            ),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
    }
    repaired.extend(kept_reversed.iter().rev().cloned());
    repaired
}

fn truncate_chat_message_for_provider_budget(
    message: &ChatMessage,
    model: &ModelInfo,
    target_tokens: usize,
) -> ChatMessage {
    let marker = "[Message truncated by Vegvisir before provider send to fit the active model context budget.]\n";
    let approx_chars = target_tokens.saturating_mul(3).clamp(512, 64 * 1024);
    let marker_chars = marker.chars().count();
    let available = approx_chars.saturating_sub(marker_chars).max(256);
    let mut truncated = message.clone();
    let suffix = message
        .content
        .chars()
        .rev()
        .take(available)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    truncated.content = format!("{marker}{suffix}");
    if provider_message_budget_decision(&[truncated.clone()], model)
        .map(|decision| decision.action == ContextBudgetAction::Block)
        .unwrap_or(false)
    {
        let hard_suffix = message
            .content
            .chars()
            .rev()
            .take(256)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        truncated.content = format!("{marker}{hard_suffix}");
    }
    truncated
}

fn count_probe_tokens(messages: &[ChatMessage], model: &ModelInfo) -> usize {
    serde_json::to_string(&provider_budget_probe_payload(messages, model))
        .map(|serialized| count_text_tokens(&model.name, &serialized) as usize)
        .unwrap_or(usize::MAX)
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

fn compact_model_observation_for_openai_tool_loop(
    model: &ModelInfo,
    wire_messages: &[Value],
    payload_tools: &[Value],
    tool_call_id: Value,
    name: &str,
    observation: &str,
    stream: bool,
) -> anyhow::Result<String> {
    let target_tokens = context_repair_target_tokens(model);
    let mut max_bytes = TOOL_OBSERVATION_MODEL_MAX_BYTES
        .min(observation.len())
        .max(1024);
    let mut compacted = compact_tool_observation(observation, max_bytes);

    for _ in 0..8 {
        let mut probe_messages = wire_messages.to_vec();
        probe_messages.push(json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "name": name,
            "content": compacted,
        }));
        let mut payload = json!({
            "model": model.name,
            "messages": probe_messages,
            "stream": stream,
            "tools": payload_tools,
            "tool_choice": "auto",
            "parallel_tool_calls": true,
        });
        apply_chat_reasoning_settings(&mut payload, model);
        let decision = provider_payload_budget_decision(model, &payload)?;
        if decision.action != ContextBudgetAction::Block
            && (target_tokens == 0
                || count_text_tokens(&model.name, &serde_json::to_string(&payload)?) as usize
                    <= target_tokens)
        {
            return Ok(compacted);
        }
        if max_bytes <= 1024 {
            break;
        }
        max_bytes = (max_bytes / 2).max(1024);
        compacted = compact_tool_observation(observation, max_bytes);
    }

    let final_budget = 1024.min(observation.len()).max(256);
    let compacted = compact_tool_observation(observation, final_budget);
    let mut probe_messages = wire_messages.to_vec();
    probe_messages.push(json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "name": name,
        "content": compacted,
    }));
    let mut payload = json!({
        "model": model.name,
        "messages": probe_messages,
        "stream": stream,
        "tools": payload_tools,
        "tool_choice": "auto",
        "parallel_tool_calls": true,
    });
    apply_chat_reasoning_settings(&mut payload, model);
    enforce_openai_payload_budget(model, &payload)?;
    Ok(compacted)
}

fn truncate_model_observation(value: &str) -> String {
    compact_tool_observation(value, TOOL_OBSERVATION_MODEL_MAX_BYTES)
}

fn approval_required_tool_output(content: &str) -> bool {
    let first_line = content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(content)
        .trim_start();
    let lower = first_line.to_ascii_lowercase();
    lower.starts_with("approvalrequired:")
        || lower.starts_with("risky tool requires human approval:")
        || lower.starts_with("risky tool requires permission:")
        || lower.starts_with("command network access requires human approval:")
        || (lower.contains("approval_id=")
            && (lower.contains("requires human approval")
                || lower.contains("requires permission")
                || lower.contains("tool approval")))
}

fn execute_tool_or_stop_for_approval(
    name: &str,
    args: Map<String, Value>,
    execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
) -> anyhow::Result<String> {
    let output = execute_tool(name, args);
    if approval_required_tool_output(&output) {
        anyhow::bail!(output);
    }
    Ok(output)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolBatchDecision {
    Execute,
    Defer,
}

fn read_only_same_round_tool(name: &str) -> bool {
    matches!(
        name,
        "list_files"
            | "read_file"
            | "cms_prepare_context"
            | "cms_prepare_model_request"
            | "cms_recall"
            | "cms_recent"
            | "cms_search_chatgpt_archive"
            | "eternium_prepare_context"
            | "msp_client_check_compatibility"
            | "msp_client_info"
            | "msp_client_load"
            | "msp_client_manifest"
            | "msp_client_search"
            | "msp_client_verify_trust"
            | "skiller_eval"
            | "skiller_load"
            | "skiller_readiness"
            | "skiller_route"
            | "skiller_suspicious_commands"
            | "skiller_validate"
            | "subagents_list"
            | "subagents_show"
    )
}

fn same_round_tool_batch_decision(index: usize, name: &str) -> ToolBatchDecision {
    if index == 0 || read_only_same_round_tool(name) {
        ToolBatchDecision::Execute
    } else {
        ToolBatchDecision::Defer
    }
}

fn execute_tool_call_for_model(
    name: &str,
    args: Map<String, Value>,
    index: usize,
    execute_tool: &mut dyn FnMut(&str, Map<String, Value>) -> String,
    observations: &mut Vec<(String, String)>,
    compact: impl FnOnce(&str) -> anyhow::Result<String>,
) -> anyhow::Result<String> {
    match same_round_tool_batch_decision(index, name) {
        ToolBatchDecision::Execute => {
            let output = execute_tool_or_stop_for_approval(name, args, execute_tool)?;
            let result = completed_tool_observation(name, &output);
            let result = compact(&result)?;
            observations.push((name.to_string(), result.clone()));
            Ok(result)
        }
        ToolBatchDecision::Defer => Ok(deferred_tool_observation(name)),
    }
}

fn completed_tool_observation(name: &str, content: &str) -> String {
    let trimmed = content.trim_end();
    let status = if trimmed
        .lines()
        .next()
        .map(|line| {
            let lower = line.trim_start().to_ascii_lowercase();
            lower.starts_with("toolerror:")
                || lower.starts_with("approvalrequired:")
                || lower.starts_with("permissiondenied:")
                || lower.starts_with("invalidtoolarguments:")
                || lower.starts_with("unknowntool:")
                || lower.starts_with("testsfailed:")
        })
        .unwrap_or(false)
    {
        "failed"
    } else {
        "completed"
    };
    format!(
        "[Vegvisir tool completed]\nname: {name}\nstatus: {status}\n\n{}",
        trimmed
    )
}

fn deferred_tool_observation(name: &str) -> String {
    format!(
        "[Vegvisir tool deferred]\nname: {name}\nstatus: deferred\nreason: Vegvisir only executes same-round sibling calls for tools classified as read-only. Risky, unknown, or side-effecting sibling calls are deferred until the model can review prior completed observations."
    )
}

fn compact_tool_observation(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let head_bytes = max_bytes.saturating_mul(2) / 3;
    let tail_bytes = max_bytes.saturating_sub(head_bytes).saturating_sub(256);
    let head = truncate_utf8(value, head_bytes);
    let tail_start = value.len().saturating_sub(tail_bytes);
    let mut tail_start = tail_start;
    while tail_start < value.len() && !value.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let tail = &value[tail_start..];
    format!(
        "{head}\n[tool observation compacted: omitted {} bytes from middle; showing head and tail; original {} bytes, budget {} bytes]\n{tail}",
        value.len().saturating_sub(head.len() + tail.len()),
        value.len(),
        max_bytes
    )
}

fn tool_round_limit_result(
    observations: &[(String, String)],
    max_tool_rounds: usize,
) -> anyhow::Result<String> {
    let summary = if observations.is_empty() {
        format!(
            "No completed tool observation was recorded before the {max_tool_rounds}-round limit."
        )
    } else {
        observations
            .iter()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|(name, content)| format!("[{name}]\n{content}"))
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    Ok(format!(
        "Tool-call round limit reached before the model produced a final answer. Latest tool observations:\n\n{summary}\n\nRecovery guidance: do not repeat the same failed tool call unless the previous observation shows a clear corrected input. Summarize the failure, switch strategy, or ask the user only if blocked."
    ))
}

fn parse_tool_arguments(value: Option<&Value>) -> Map<String, Value> {
    match value {
        Some(Value::String(raw)) => parse_tool_arguments_str(raw),
        Some(Value::Object(object)) => object.clone(),
        _ => Map::new(),
    }
}

fn parse_tool_arguments_str(raw: &str) -> Map<String, Value> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Map::new();
    }
    if let Ok(args) = serde_json::from_str::<Map<String, Value>>(raw) {
        return args;
    }
    let unwrapped = raw
        .strip_prefix("```json")
        .or_else(|| raw.strip_prefix("```"))
        .and_then(|text| text.strip_suffix("```"))
        .map(str::trim);
    if let Some(unwrapped) = unwrapped
        && let Ok(args) = serde_json::from_str::<Map<String, Value>>(unwrapped)
    {
        return args;
    }
    if let Some(start) = raw.find('{')
        && let Some(end) = raw.rfind('}')
        && start < end
        && let Ok(args) = serde_json::from_str::<Map<String, Value>>(&raw[start..=end])
    {
        return args;
    }
    Map::new()
}

pub struct ConversationRunner<P: ProviderAdapter> {
    pub provider: P,
    pub models: crate::core::ModelRegistry,
    pub tools: Option<ToolRegistry>,
    pub tool_executor: Option<ToolExecutor>,
    pub event_sink: Option<Arc<dyn Fn(ProviderRunEvent) + Send + Sync>>,
    pub cancel_token: Option<Arc<AtomicBool>>,
    pub steering_rx: Option<Receiver<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderRunEvent {
    Activity(String),
    ApprovalRequired {
        request: ControlRequest<ApprovalControlPayload>,
    },
    ToolStart {
        name: String,
        args: String,
    },
    ToolOutput {
        name: String,
        stream: String,
        chunk: String,
        truncated: bool,
    },
    ToolEnd {
        name: String,
        ok: bool,
        summary: String,
        detail: Option<String>,
    },
}

impl serde::Serialize for ProviderRunEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            ProviderRunEvent::Activity(activity) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("kind", "activity")?;
                map.serialize_entry("activity", activity)?;
                map.end()
            }
            ProviderRunEvent::ApprovalRequired { request } => {
                let mut map = serializer.serialize_map(Some(6))?;
                map.serialize_entry("kind", "approval_required")?;
                map.serialize_entry("request_id", &request.request_id)?;
                map.serialize_entry("subtype", &request.subtype)?;
                map.serialize_entry("approval_id", &request.payload.approval_id)?;
                map.serialize_entry("tool_name", &request.payload.tool_name)?;
                map.serialize_entry("risk_label", &request.payload.risk_label)?;
                map.end()
            }
            ProviderRunEvent::ToolStart { name, args } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("kind", "tool_start")?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("args", args)?;
                map.end()
            }
            ProviderRunEvent::ToolOutput {
                name,
                stream,
                chunk,
                truncated,
            } => {
                let mut map = serializer.serialize_map(Some(5))?;
                map.serialize_entry("kind", "tool_output")?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("stream", stream)?;
                map.serialize_entry("chunk", chunk)?;
                map.serialize_entry("truncated", truncated)?;
                map.end()
            }
            ProviderRunEvent::ToolEnd {
                name,
                ok,
                summary,
                detail,
            } => {
                let mut map = serializer.serialize_map(Some(5))?;
                map.serialize_entry("kind", "tool_end")?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("ok", ok)?;
                map.serialize_entry("summary", summary)?;
                map.serialize_entry("detail", detail)?;
                map.end()
            }
        }
    }
}

fn session_conversation_messages(session: &SessionState) -> Vec<ChatMessage> {
    fit_conversation_messages_to_budget(
        session_provider_context_messages(session),
        provider_history_char_budget(session),
    )
}

fn session_provider_context_messages(session: &SessionState) -> Vec<ChatMessage> {
    session
        .messages
        .iter()
        .filter(|message| {
            message.role != "system"
                || message.content.starts_with("Context Capsule:")
                || message
                    .content
                    .starts_with("Earlier conversation history was compacted by Vegvisir")
        })
        .cloned()
        .collect()
}

fn automatically_compact_session_history(
    session: &mut SessionState,
    model: &ModelInfo,
    stable_system_context: &str,
) -> anyhow::Result<Option<ContextBudgetDecision>> {
    let messages = session_provider_context_messages(session);
    if messages.len() < 2 {
        return Ok(None);
    }

    let mut probe = messages.clone();
    if !stable_system_context.trim().is_empty() {
        probe.insert(
            0,
            ChatMessage {
                role: "system".to_string(),
                content: stable_system_context.to_string(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
        );
    }
    let decision = provider_message_budget_decision(&probe, model)?;
    if !matches!(
        decision.action,
        ContextBudgetAction::CompactRecommended | ContextBudgetAction::Block
    ) {
        return Ok(None);
    }

    let current_chars = messages
        .iter()
        .map(|message| message.content.chars().count())
        .sum::<usize>();
    let system_tokens = count_text_tokens(&model.name, stable_system_context) as usize;
    let token_target_chars = context_repair_target_tokens(model)
        .saturating_sub(system_tokens)
        .saturating_mul(3);
    let ratio_target_chars = if decision.percentage > 0.0 {
        ((current_chars as f64) * (PROVIDER_CONTEXT_REPAIR_TARGET_PERCENT / decision.percentage))
            .floor() as usize
    } else {
        current_chars
    };
    let target_chars = token_target_chars
        .min(ratio_target_chars)
        .min(current_chars.saturating_sub(1));
    if target_chars == 0 {
        return Ok(None);
    }

    let compacted = fit_conversation_messages_to_budget(messages.clone(), target_chars);
    let changed = compacted.len() != messages.len()
        || compacted
            .iter()
            .zip(messages.iter())
            .any(|(left, right)| left.role != right.role || left.content != right.content);
    if !changed {
        return Ok(None);
    }

    session.messages = compacted;
    Ok(Some(decision))
}

fn provider_history_char_budget(session: &SessionState) -> usize {
    let approximate_chars = session.context_limit.saturating_mul(2) as usize;
    approximate_chars.clamp(32_000, 240_000)
}

fn fit_conversation_messages_to_budget(
    messages: Vec<ChatMessage>,
    budget_chars: usize,
) -> Vec<ChatMessage> {
    let total_chars = messages
        .iter()
        .map(|message| message.content.chars().count())
        .sum::<usize>();
    if total_chars <= budget_chars {
        return messages;
    }

    // Reserve part of the provider-history budget for a digest before selecting the
    // recent suffix. Adding a digest after filling the entire budget would make the
    // supposedly fitted request exceed its own limit.
    let summary_budget = (budget_chars / 5).clamp(1_024, 12_000).min(budget_chars);
    let recent_history_budget = budget_chars.saturating_sub(summary_budget);
    let mut kept = Vec::new();
    let mut used_chars = 0usize;
    for message in messages.iter().rev() {
        let message_chars = message.content.chars().count();
        if !kept.is_empty() && used_chars.saturating_add(message_chars) > recent_history_budget {
            break;
        }
        if message_chars > recent_history_budget {
            kept.push(truncate_conversation_message(
                message,
                recent_history_budget,
            ));
            break;
        }
        kept.push(message.clone());
        used_chars = used_chars.saturating_add(message_chars);
    }
    kept.reverse();

    let omitted = messages.len().saturating_sub(kept.len());
    if omitted > 0 {
        kept.insert(
            0,
            ChatMessage {
                role: "system".to_string(),
                content: compact_omitted_conversation(&messages[..omitted], summary_budget),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
        );
    }
    kept
}

fn compact_omitted_conversation(messages: &[ChatMessage], max_chars: usize) -> String {
    let header = format!(
        "Earlier conversation history was compacted by Vegvisir before this provider request. Compacted messages: {}.\n\nCompacted history digest (newest omitted excerpts take priority):",
        messages.len()
    );
    if messages.is_empty() || max_chars <= header.chars().count() {
        return truncate_chars_owned(&header, max_chars);
    }

    let mut excerpts = Vec::new();
    let mut used = header.chars().count();
    for message in messages.iter().rev() {
        let role = if message.role.trim().is_empty() {
            "unknown"
        } else {
            message.role.as_str()
        };
        let compact = message
            .content
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if compact.is_empty() {
            continue;
        }

        let prefix = format!("- [{role}] ");
        let separator_chars = 1;
        let remaining = max_chars.saturating_sub(used + separator_chars);
        if remaining <= prefix.chars().count() {
            break;
        }
        let excerpt_budget = remaining.saturating_sub(prefix.chars().count()).min(900);
        let excerpt = truncate_chars_owned(&compact, excerpt_budget);
        let entry = format!("{prefix}{excerpt}");
        used = used.saturating_add(separator_chars + entry.chars().count());
        excerpts.push(entry);
    }
    excerpts.reverse();

    if excerpts.is_empty() {
        header
    } else {
        format!("{header}\n{}", excerpts.join("\n"))
    }
}

fn truncate_chars_owned(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars <= 1 {
        return "…".chars().take(max_chars).collect();
    }
    let mut truncated = value.chars().take(max_chars - 1).collect::<String>();
    truncated.push('…');
    truncated
}

fn truncate_conversation_message(message: &ChatMessage, budget_chars: usize) -> ChatMessage {
    let marker =
        "\n\n[Message truncated by Vegvisir before provider send to fit the model context budget.]";
    let mut truncated = message.clone();
    let marker_chars = marker.chars().count();
    if budget_chars <= marker_chars {
        truncated.content = truncate_chars_owned(marker, budget_chars);
        return truncated;
    }
    let available = budget_chars - marker_chars;
    let tail = message
        .content
        .chars()
        .rev()
        .take(available)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    truncated.content = format!("{marker}{tail}");
    truncated
}

fn approval_required_observation(observation: &Observation) -> bool {
    observation.error.as_deref() == Some("ApprovalRequired")
        || observation.content.contains("approval_id=")
        || observation
            .content
            .contains("Risky tool requires permission:")
}

fn approval_id_from_observation(observation: &Observation) -> Option<String> {
    let content = observation.content.split_once("approval_id=")?.1;
    Some(
        content
            .split(|ch: char| ch.is_whitespace() || ch == ';' || ch == ',' || ch == ')')
            .next()
            .unwrap_or_default()
            .trim()
            .to_string(),
    )
    .filter(|id| !id.is_empty())
}

fn approval_control_request_from_pending(
    executor: &ToolExecutor,
    run_id: &str,
    approval_id: &str,
) -> Option<ControlRequest<ApprovalControlPayload>> {
    executor
        .guardrails
        .approvals
        .pending()
        .get(approval_id)
        .cloned()
        .map(|request| ControlRequest::approval(run_id.to_string(), request, None))
}

fn wait_for_tool_approval(
    executor: &mut ToolExecutor,
    name: &str,
    args: &Map<String, Value>,
    approval_id: &str,
    cancel_token: Option<&Arc<AtomicBool>>,
) -> anyhow::Result<()> {
    if executor.guardrails.policy.bypass_approvals_and_sandbox {
        return Ok(());
    }
    loop {
        if cancel_token
            .map(|token| token.load(Ordering::SeqCst))
            .unwrap_or(false)
        {
            anyhow::bail!("Cancelled");
        }
        match executor
            .guardrails
            .approvals
            .resolution(approval_id, name, args)
        {
            ApprovalResolution::Approved => return Ok(()),
            ApprovalResolution::Denied => {
                anyhow::bail!("Tool approval denied; approval_id={approval_id}")
            }
            ApprovalResolution::Missing => {
                anyhow::bail!("Tool approval is no longer pending; approval_id={approval_id}")
            }
            ApprovalResolution::Pending => thread::sleep(Duration::from_millis(200)),
        }
    }
}

fn drain_steering_messages(
    steering_rx: &Option<Receiver<String>>,
    session: &mut SessionState,
) -> Vec<String> {
    let Some(receiver) = steering_rx else {
        return Vec::new();
    };
    let mut messages = Vec::new();
    while let Ok(message) = receiver.try_recv() {
        let message = message.trim().to_string();
        if message.is_empty() {
            continue;
        }
        session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: format!("[mid-run steering] {message}"),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        messages.push(message);
    }
    messages
}

fn inject_steering_into_observation(
    steering_rx: &Option<Receiver<String>>,
    session: &mut SessionState,
    observation: String,
) -> String {
    let steering = drain_steering_messages(steering_rx, session);
    if steering.is_empty() {
        return observation;
    }
    let steering = steering
        .into_iter()
        .map(|message| format!("- {message}"))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    format!(
        "{observation}

[User steering received while you were running; adjust your next step accordingly.]
{steering}"
    )
}

fn model_with_session_reasoning(model: &ModelInfo, session: &SessionState) -> ModelInfo {
    let mut model = model.clone();
    if let Some(level) = session
        .current_reasoning_level
        .as_deref()
        .map(str::trim)
        .filter(|level| !level.is_empty())
    {
        model
            .metadata
            .insert("reasoning_level".to_string(), json!(level));
        model.metadata.remove("reasoning_effort");
        model.metadata.remove("thinking_budget_tokens");
        model.metadata.remove("thinking_budget");
    }
    if session.fast_mode && model_supports_fast_mode(&model) {
        apply_fast_mode_to_model(&mut model);
    }
    model
}

fn model_supports_fast_mode(model: &ModelInfo) -> bool {
    model
        .metadata
        .get("fast_capable")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && matches!(model.provider.as_str(), "openai" | "anthropic")
}

fn apply_fast_mode_to_model(model: &mut ModelInfo) {
    model.metadata.insert("fast_mode".to_string(), json!(true));
    match model.provider.as_str() {
        "openai" => {
            model
                .metadata
                .insert("reasoning_level".to_string(), json!("minimal"));
            model
                .metadata
                .insert("reasoning_effort".to_string(), json!("minimal"));
            model.metadata.remove("thinking_budget_tokens");
            model.metadata.remove("thinking_budget");
        }
        "anthropic" => {
            model.metadata.remove("reasoning_level");
            model.metadata.remove("reasoning_effort");
            model.metadata.remove("thinking_budget_tokens");
            model.metadata.remove("thinking_budget");
        }
        _ => {}
    }
}

impl<P: ProviderAdapter> ConversationRunner<P> {
    pub fn send(&mut self, session: &mut SessionState, content: &str) -> anyhow::Result<String> {
        self.send_with_context(session, content, None)
    }

    pub fn imagine(&mut self, session: &mut SessionState, prompt: &str) -> anyhow::Result<String> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            anyhow::bail!("Usage: /imagine <image prompt>");
        }
        session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: format!("/imagine {prompt}"),
            attachments: std::mem::take(&mut session.pending_attachments),
            created_at: chrono::Utc::now(),
        });
        session.status = "streaming".to_string();
        session.activity = "generating image".to_string();
        let catalog_model = self
            .models
            .get(&session.current_model)
            .ok_or_else(|| anyhow::anyhow!("Unknown model: {}", session.current_model))?;
        if !self
            .models
            .is_model_allowed_for_provider(catalog_model, &session.current_provider)
        {
            session.current_provider = catalog_model.provider.clone();
        }
        if let Some(limit) = catalog_model.context_window {
            session.context_limit = limit;
        }
        let mut model = model_with_session_reasoning(catalog_model, session);
        model
            .metadata
            .insert("output_media".to_string(), json!(true));
        model
            .metadata
            .insert("images_generations_api".to_string(), json!(true));
        let provider_messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
            attachments: session
                .messages
                .last()
                .map(|message| message.attachments.clone())
                .unwrap_or_default(),
            created_at: chrono::Utc::now(),
        }];
        let started = Instant::now();
        let provider_response = self.provider.complete_with_usage(
            &provider_messages,
            &model,
            &session.current_provider,
        )?;
        let saved_artifacts = save_generated_artifacts(session, &provider_response.artifacts)?;
        let response =
            response_with_generated_artifact_notice(provider_response.content, &saved_artifacts);
        update_session_token_usage(
            session,
            model.name.as_str(),
            prompt,
            &response,
            provider_response.usage,
        );
        session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.clone(),
            attachments: saved_artifacts,
            created_at: chrono::Utc::now(),
        });
        session.last_latency_ms = started.elapsed().as_millis() as u64;
        session.status = "ready".to_string();
        session.activity.clear();
        Ok(response)
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
        let catalog_model = self
            .models
            .get(&session.current_model)
            .ok_or_else(|| anyhow::anyhow!("Unknown model: {}", session.current_model))?;
        if !self
            .models
            .is_model_allowed_for_provider(catalog_model, &session.current_provider)
        {
            session.current_provider = catalog_model.provider.clone();
        }
        if let Some(limit) = catalog_model.context_window {
            session.context_limit = limit;
        }
        let model = model_with_session_reasoning(catalog_model, session);
        let model = &model;
        if let Some(decision) =
            automatically_compact_session_history(session, model, &session.system_prompt.clone())?
        {
            session.activity = "automatically compacted session context".to_string();
            self.emit_event(ProviderRunEvent::Activity(format!(
                "automatically compacted session context at {:.1}% usage",
                decision.percentage
            )));
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
        let (repaired_messages, budget_decision, repaired) =
            trim_chat_messages_to_provider_budget(provider_messages, model)?;
        provider_messages = repaired_messages;
        if repaired {
            session.activity = "compacted provider context before send".to_string();
            self.emit_event(ProviderRunEvent::Activity(format!(
                "compacted provider context before send: action={} usage={:.1}% remaining={}",
                budget_decision.action.as_str(),
                budget_decision.percentage,
                budget_decision
                    .remaining_tokens
                    .map(|tokens| tokens.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            )));
        }
        if budget_decision.action == ContextBudgetAction::Block {
            anyhow::bail!(
                "Vegvisir blocked provider send after context repair: estimated context usage remains {:.1}% of the active model budget. {}",
                budget_decision.percentage,
                budget_decision.warnings.join(" ")
            );
        }
        let started = Instant::now();
        let event_sink = self.event_sink.clone();
        let provider_response = if self
            .provider
            .supports_tool_calls(model, &session.current_provider)
            && self.tools.is_some()
            && !model_outputs_media(model)
            && let Some(executor) = self.tool_executor.as_mut()
        {
            session.activity = "thinking through tool use".to_string();
            let tools = self
                .tools
                .as_ref()
                .map(ToolRegistry::schemas)
                .unwrap_or_default();
            let session_id = session.session_id.clone();
            let current_provider = session.current_provider.clone();
            let steering_rx = self.steering_rx.take();
            let mut approval_required = None::<String>;
            let mut execute_tool = |name: &str, args: Map<String, Value>| -> String {
                session.activity = format!("using tool {name}");
                let tool_output_sink = command_output_provider_sink(&event_sink, name);
                let mut observation =
                    with_command_output_sink(Some(tool_output_sink.clone()), || {
                        executor.execute(ToolCall {
                            name: name.to_string(),
                            args: args.clone(),
                        })
                    });
                if approval_required_observation(&observation) {
                    approval_required = Some(observation.content.clone());
                    if let Some(approval_id) = approval_id_from_observation(&observation)
                        && self.cancel_token.is_some()
                    {
                        if let Some(request) = approval_control_request_from_pending(
                            executor,
                            &session_id,
                            &approval_id,
                        ) {
                            emit_provider_event(
                                &event_sink,
                                ProviderRunEvent::ApprovalRequired { request },
                            );
                        }
                        session.activity = format!("waiting for approval {approval_id}");
                        match wait_for_tool_approval(
                            executor,
                            name,
                            &args,
                            &approval_id,
                            self.cancel_token.as_ref(),
                        ) {
                            Ok(()) => {
                                session.activity = format!("using approved tool {name}");
                                observation = with_command_output_sink(
                                    Some(tool_output_sink.clone()),
                                    || {
                                        executor.execute(ToolCall {
                                            name: name.to_string(),
                                            args,
                                        })
                                    },
                                );
                                approval_required = None;
                            }
                            Err(error) => {
                                approval_required = Some(error.to_string());
                                return format!("ApprovalRequired: {error}");
                            }
                        }
                    }
                }
                session.activity = format!("finished tool {name}");
                let observation_text = if observation.ok {
                    observation.content
                } else {
                    format!(
                        "{}: {}",
                        observation.error.unwrap_or_else(|| "ToolError".to_string()),
                        observation.content
                    )
                };
                inject_steering_into_observation(&steering_rx, session, observation_text)
            };
            let _ = session_id;
            let response = self.provider.complete_with_tools_usage(
                &provider_messages,
                model,
                &tools,
                &mut execute_tool,
                &current_provider,
            )?;
            if let Some(message) = approval_required {
                anyhow::bail!("{message}");
            }
            let _ = drain_steering_messages(&steering_rx, session);
            response
        } else {
            self.provider.complete_with_usage(
                &provider_messages,
                model,
                &session.current_provider,
            )?
        };
        let _ = drain_steering_messages(&self.steering_rx, session);
        let saved_artifacts = save_generated_artifacts(session, &provider_response.artifacts)?;
        let response =
            response_with_generated_artifact_notice(provider_response.content, &saved_artifacts);
        update_session_token_usage(
            session,
            model.name.as_str(),
            content,
            &response,
            provider_response.usage,
        );
        session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.clone(),
            attachments: saved_artifacts,
            created_at: chrono::Utc::now(),
        });
        session.last_latency_ms = started.elapsed().as_millis() as u64;
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
        let catalog_model = self
            .models
            .get(&session.current_model)
            .ok_or_else(|| anyhow::anyhow!("Unknown model: {}", session.current_model))?;
        if !self
            .models
            .is_model_allowed_for_provider(catalog_model, &session.current_provider)
        {
            session.current_provider = catalog_model.provider.clone();
        }
        if let Some(limit) = catalog_model.context_window {
            session.context_limit = limit;
        }
        let model = model_with_session_reasoning(catalog_model, session);
        let model = &model;
        self.emit_event(ProviderRunEvent::Activity(
            "using CMS-v2 prepared model request".to_string(),
        ));
        let mut envelope = envelope;
        apply_system_prompt_to_envelope(&mut envelope, &session.system_prompt);
        if let Some(decision) =
            automatically_compact_session_history(session, model, &envelope.model_request.prompt)?
        {
            session.activity = "automatically compacted session context".to_string();
            self.emit_event(ProviderRunEvent::Activity(format!(
                "automatically compacted session context at {:.1}% usage",
                decision.percentage
            )));
        }
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
        let (repaired_messages, budget_decision, repaired) =
            trim_chat_messages_to_provider_budget(provider_messages, model)?;
        provider_messages = repaired_messages;
        if repaired {
            session.activity = "compacted provider context before send".to_string();
            self.emit_event(ProviderRunEvent::Activity(format!(
                "compacted provider context before send: action={} usage={:.1}% remaining={}",
                budget_decision.action.as_str(),
                budget_decision.percentage,
                budget_decision
                    .remaining_tokens
                    .map(|tokens| tokens.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            )));
        }
        if budget_decision.action == ContextBudgetAction::Block {
            anyhow::bail!(
                "Vegvisir blocked provider send after context repair: estimated context usage remains {:.1}% of the active model budget. {}",
                budget_decision.percentage,
                budget_decision.warnings.join(" ")
            );
        }
        let started = Instant::now();
        let provider_response = if self
            .provider
            .supports_tool_calls(model, &session.current_provider)
            && self.tools.is_some()
            && !model_outputs_media(model)
            && let Some(executor) = self.tool_executor.as_mut()
        {
            session.activity = "thinking through tool use".to_string();
            let event_sink = self.event_sink.clone();
            emit_provider_event(
                &event_sink,
                ProviderRunEvent::Activity("thinking through tool use".to_string()),
            );
            let tools = self
                .tools
                .as_ref()
                .map(ToolRegistry::schemas)
                .unwrap_or_default();
            let session_id = session.session_id.clone();
            let current_provider = session.current_provider.clone();
            let steering_rx = self.steering_rx.take();
            let mut approval_required = None::<String>;
            let mut execute_tool = |name: &str, args: Map<String, Value>| -> String {
                session.activity = format!("using tool {name}");
                emit_provider_event(
                    &event_sink,
                    ProviderRunEvent::ToolStart {
                        name: name.to_string(),
                        args: summarize_tool_args(&args),
                    },
                );
                let tool_output_sink = command_output_provider_sink(&event_sink, name);
                let mut observation =
                    with_command_output_sink(Some(tool_output_sink.clone()), || {
                        executor.execute(ToolCall {
                            name: name.to_string(),
                            args: args.clone(),
                        })
                    });
                if approval_required_observation(&observation) {
                    approval_required = Some(observation.content.clone());
                    if let Some(approval_id) = approval_id_from_observation(&observation)
                        && self.cancel_token.is_some()
                    {
                        if let Some(request) = approval_control_request_from_pending(
                            executor,
                            &session_id,
                            &approval_id,
                        ) {
                            emit_provider_event(
                                &event_sink,
                                ProviderRunEvent::ApprovalRequired { request },
                            );
                        }
                        session.activity = format!("waiting for approval {approval_id}");
                        emit_provider_event(
                            &event_sink,
                            ProviderRunEvent::Activity(format!(
                                "waiting for approval {approval_id}"
                            )),
                        );
                        match wait_for_tool_approval(
                            executor,
                            name,
                            &args,
                            &approval_id,
                            self.cancel_token.as_ref(),
                        ) {
                            Ok(()) => {
                                session.activity = format!("using approved tool {name}");
                                emit_provider_event(
                                    &event_sink,
                                    ProviderRunEvent::Activity(format!(
                                        "using approved tool {name}"
                                    )),
                                );
                                observation = with_command_output_sink(
                                    Some(tool_output_sink.clone()),
                                    || {
                                        executor.execute(ToolCall {
                                            name: name.to_string(),
                                            args,
                                        })
                                    },
                                );
                                approval_required = None;
                            }
                            Err(error) => {
                                approval_required = Some(error.to_string());
                                return format!("ApprovalRequired: {error}");
                            }
                        }
                    }
                }
                session.activity = format!("finished tool {name}");
                emit_provider_event(
                    &event_sink,
                    ProviderRunEvent::ToolEnd {
                        name: name.to_string(),
                        ok: observation.ok,
                        summary: summarize_observation(&observation),
                        detail: tool_display_detail(name, &observation),
                    },
                );
                let observation_text = if observation.ok {
                    observation.content
                } else {
                    format!(
                        "{}: {}",
                        observation.error.unwrap_or_else(|| "ToolError".to_string()),
                        observation.content
                    )
                };
                inject_steering_into_observation(&steering_rx, session, observation_text)
            };
            let response = self.provider.complete_with_tools_streaming(
                &provider_messages,
                model,
                &tools,
                &mut execute_tool,
                &current_provider,
                on_delta,
            )?;
            if let Some(message) = approval_required {
                anyhow::bail!("{message}");
            }
            let _ = drain_steering_messages(&steering_rx, session);
            ProviderResponse::new(response)
        } else {
            self.provider.complete_with_usage_streaming(
                &provider_messages,
                model,
                &session.current_provider,
                on_delta,
            )?
        };
        let _ = drain_steering_messages(&self.steering_rx, session);
        let saved_artifacts = save_generated_artifacts(session, &provider_response.artifacts)?;
        let response =
            response_with_generated_artifact_notice(provider_response.content, &saved_artifacts);
        session.last_prompt_cache_key = Some(envelope.manifest.prompt_cache_key.clone());
        session.last_prompt_manifest_id = Some(envelope.manifest.manifest_id.clone());
        session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.clone(),
            attachments: saved_artifacts,
            created_at: chrono::Utc::now(),
        });
        session.last_latency_ms = started.elapsed().as_millis() as u64;
        let input_text = format!("{}\n{}", envelope.model_request.prompt, content);
        update_session_token_usage(
            session,
            model.name.as_str(),
            &input_text,
            &response,
            provider_response.usage,
        );
        session.status = "ready".to_string();
        session.activity.clear();
        Ok(response)
    }

    fn emit_event(&self, event: ProviderRunEvent) {
        emit_provider_event(&self.event_sink, event);
    }
}

fn update_session_token_usage(
    session: &mut SessionState,
    model: &str,
    input_text: &str,
    output_text: &str,
    provider_usage: Option<TokenUsage>,
) {
    let reported_usage = provider_usage;
    let (usage, source) = selected_usage_or_counted(reported_usage, model, input_text, output_text);
    session.input_tokens_used = session.input_tokens_used.saturating_add(usage.input_tokens);
    session.output_tokens_used = session
        .output_tokens_used
        .saturating_add(usage.output_tokens);
    session.tokens_used = session.tokens_used.saturating_add(usage.total());
    if reported_usage.is_some() && source == crate::telemetry::TokenCountSource::ProviderReported {
        session.provider_reported_input_tokens = session
            .provider_reported_input_tokens
            .saturating_add(usage.input_tokens);
        session.provider_reported_output_tokens = session
            .provider_reported_output_tokens
            .saturating_add(usage.output_tokens);
    }
}

fn emit_provider_event(
    sink: &Option<Arc<dyn Fn(ProviderRunEvent) + Send + Sync>>,
    event: ProviderRunEvent,
) {
    if let Some(sink) = sink {
        sink(event);
    }
}

fn command_output_provider_sink(
    event_sink: &Option<Arc<dyn Fn(ProviderRunEvent) + Send + Sync>>,
    tool_name: &str,
) -> CommandOutputSink {
    let event_sink = event_sink.clone();
    let tool_name = tool_name.to_string();
    Arc::new(move |chunk| {
        emit_provider_event(
            &event_sink,
            ProviderRunEvent::ToolOutput {
                name: tool_name.clone(),
                stream: chunk.stream,
                chunk: chunk.chunk,
                truncated: chunk.truncated,
            },
        );
    })
}

fn summarize_tool_args(args: &Map<String, Value>) -> String {
    let raw = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    truncate_summary(&raw.replace('\n', " "), 240)
}

fn summarize_observation(observation: &Observation) -> String {
    let status = if observation.ok { "ok" } else { "error" };
    let content = observation.content.replace('\n', " ");
    let detail = if content.trim().is_empty() {
        observation.error.clone().unwrap_or_default()
    } else {
        content
    };
    truncate_summary(&format!("{status}: {detail}"), 240)
}

fn tool_display_detail(name: &str, observation: &Observation) -> Option<String> {
    if !observation.ok {
        return None;
    }
    if let Some(diff) = observation.data.get("diff").and_then(Value::as_str)
        && !diff.trim().is_empty()
    {
        return Some(format!("```diff\n{}\n```", diff.trim_end()));
    }
    if name == "read_file" {
        let path = observation
            .data
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("file");
        let language = language_for_path(path);
        return Some(format!(
            "```{}\n{}\n```",
            language,
            observation.content.trim_end()
        ));
    }
    None
}

fn language_for_path(path: &str) -> &'static str {
    match std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "py" => "python",
        "js" => "javascript",
        "ts" => "typescript",
        "tsx" => "tsx",
        "jsx" => "jsx",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "md" => "markdown",
        "html" => "html",
        "css" => "css",
        "sh" | "bash" => "bash",
        "diff" | "patch" => "diff",
        _ => "text",
    }
}

fn truncate_summary(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
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

#[doc(hidden)]
pub mod test_support {
    use super::*;

    pub fn openai_messages_for_test(messages: &[ChatMessage]) -> Vec<Value> {
        openai_messages(messages)
    }

    pub fn responses_payload_for_test(messages: &[ChatMessage], model: &ModelInfo) -> Value {
        responses_payload(messages, model)
    }

    pub fn anthropic_messages_payload_for_test(
        messages: &[ChatMessage],
        model: &ModelInfo,
    ) -> Value {
        anthropic_messages_payload(messages, model)
    }

    pub fn google_generate_content_payload_for_test(
        messages: &[ChatMessage],
        model: &ModelInfo,
    ) -> Value {
        google_generate_content_payload(messages, model)
    }

    pub fn parse_tool_arguments_for_test(value: Option<&Value>) -> Map<String, Value> {
        parse_tool_arguments(value)
    }

    pub fn tool_round_limit_result_for_test(
        observations: &[(String, String)],
        max_tool_rounds: usize,
    ) -> anyhow::Result<String> {
        tool_round_limit_result(observations, max_tool_rounds)
    }

    pub fn parse_openai_sse_for_test(text: &str) -> anyhow::Result<String> {
        parse_openai_sse(text)
    }

    pub fn parse_responses_sse_for_test(text: &str) -> anyhow::Result<String> {
        parse_response_sse_text_reader(BufReader::new(text.as_bytes()), &mut |_| {})
    }

    pub fn parse_anthropic_sse_for_test(text: &str) -> anyhow::Result<String> {
        parse_anthropic_sse(text)
    }

    pub fn parse_google_stream_for_test(text: &str) -> anyhow::Result<String> {
        parse_google_stream(text)
    }

    pub fn anthropic_tool_schema_for_test(tool: &Value) -> Value {
        anthropic_tool_schema(tool)
    }

    pub fn google_tool_schema_for_test(tool: &Value) -> Value {
        google_tool_schema(tool)
    }

    pub fn responses_tool_schema_for_test(tool: &Value) -> Value {
        responses_tool_schema(tool)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        net::TcpListener,
    };

    use super::*;
    use std::sync::Mutex;

    static TOOL_ROUND_LIMIT_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn malformed_chunked_stream_is_retried_before_turn_failure() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let server = thread::spawn(move || {
            for (index, incoming) in listener.incoming().take(2).enumerate() {
                let Ok(mut stream) = incoming else {
                    return;
                };
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let response = if index == 0 {
                    concat!(
                        "HTTP/1.1 200 OK\r\n",
                        "Transfer-Encoding: chunked\r\n",
                        "Connection: close\r\n",
                        "\r\n",
                        "not-a-chunk\r\n",
                        "broken\r\n"
                    )
                } else {
                    concat!(
                        "HTTP/1.1 200 OK\r\n",
                        "Transfer-Encoding: chunked\r\n",
                        "Connection: close\r\n",
                        "\r\n",
                        "5\r\nhello\r\n",
                        "0\r\n\r\n"
                    )
                };
                let _ = stream.write_all(response.as_bytes());
            }
        });

        let body = read_ureq_stream_body_with_retry(|| {
            Ok(ureq::get(&format!("http://{address}/stream"))
                .set("Connection", "close")
                .call()?)
        })?;

        assert_eq!(body, "hello");
        server
            .join()
            .map_err(|_| anyhow::anyhow!("chunk test server panicked"))?;
        Ok(())
    }

    #[test]
    fn mid_stream_chunk_failure_retries_without_duplicate_visible_text() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let server = thread::spawn(move || {
            for (index, incoming) in listener.incoming().take(2).enumerate() {
                let Ok(mut stream) = incoming else {
                    return;
                };
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let response = if index == 0 {
                    let first_event =
                        "data: {\"choices\":[{\"delta\":{\"content\":\"hello \"}}]}\n\n";
                    format!(
                        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{}\r\nnot-a-chunk\r\nbroken\r\n",
                        first_event.len(),
                        first_event
                    )
                } else {
                    let body = concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"hello world\"}}]}\n\n",
                        "data: [DONE]\n\n"
                    );
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                };
                let _ = stream.write_all(response.as_bytes());
            }
        });

        let mut visible = String::new();
        let response = stream_ureq_response_with_retry(
            || {
                Ok(ureq::get(&format!("http://{address}/stream"))
                    .set("Connection", "close")
                    .call()?)
            },
            |response, callback| {
                parse_openai_sse_reader(BufReader::new(response.into_reader()), callback)
            },
            &mut |delta| visible.push_str(delta),
        )?;

        assert_eq!(response, "hello world");
        assert_eq!(visible, "hello world");
        server
            .join()
            .map_err(|_| anyhow::anyhow!("chunk test server panicked"))?;
        Ok(())
    }

    #[test]
    fn provider_payload_budget_blocks_oversized_request() {
        let model = ModelInfo {
            name: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            display_name: None,
            context_window: Some(256),
            supports_streaming: true,
            enabled: true,
            metadata: Default::default(),
        };
        let payload = json!({
            "model": model.name,
            "messages": [{"role": "user", "content": "large context ".repeat(800)}],
            "stream": false,
        });

        let error = enforce_provider_payload_budget(&model, &payload)
            .expect_err("oversized prompt should be blocked before provider send");

        assert!(
            error
                .to_string()
                .contains("blocked an oversized model request before provider send")
        );
    }

    #[test]
    fn provider_payload_budget_does_not_tokenize_inline_image_base64_as_text() -> anyhow::Result<()>
    {
        let model = ModelInfo {
            name: "gpt-5.6-sol".to_string(),
            provider: "openai-sso".to_string(),
            display_name: None,
            context_window: Some(372_000),
            supports_streaming: true,
            enabled: true,
            metadata: Default::default(),
        };
        // Mirrors the transport size of the 614,555-byte PNG from the reported failure.
        let image_base64 = "A".repeat(820_000);
        let payload = json!({
            "model": model.name,
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "inspect this screenshot"},
                    {
                        "type": "input_image",
                        "image_url": format!("data:image/png;base64,{image_base64}")
                    }
                ]
            }]
        });
        let estimated = estimated_provider_payload_tokens(&model, &payload)?;
        assert!(
            estimated < 10_000,
            "unexpected image-aware estimate: {estimated}"
        );
        assert_ne!(
            provider_payload_budget_decision(&model, &payload)?.action,
            ContextBudgetAction::Block
        );
        enforce_provider_payload_budget(&model, &payload)?;
        Ok(())
    }

    #[test]
    fn image_token_accounting_handles_openai_anthropic_and_google_shapes() -> anyhow::Result<()> {
        let model = ModelInfo {
            name: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            display_name: None,
            context_window: Some(128_000),
            supports_streaming: true,
            enabled: true,
            metadata: Default::default(),
        };
        let payload = json!({
            "openai": {"image_url": format!("data:image/png;base64,{}", "A".repeat(20_000))},
            "anthropic": {
                "type": "base64",
                "media_type": "image/png",
                "data": "B".repeat(20_000)
            },
            "google": {
                "inlineData": {
                    "mimeType": "image/png",
                    "data": "C".repeat(20_000)
                }
            }
        });

        let estimated = estimated_provider_payload_tokens(&model, &payload)?;
        assert!(estimated >= 3 * PROVIDER_IMAGE_INPUT_TOKEN_ESTIMATE);
        assert!(estimated < 4 * PROVIDER_IMAGE_INPUT_TOKEN_ESTIMATE);
        Ok(())
    }

    #[test]
    fn provider_message_budget_repair_keeps_recent_context() -> anyhow::Result<()> {
        let model = ModelInfo {
            name: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            display_name: None,
            context_window: Some(1024),
            supports_streaming: true,
            enabled: true,
            metadata: Default::default(),
        };
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "stable system prompt".to_string(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "old context ".repeat(400),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "old answer ".repeat(400),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "latest request: keep me".to_string(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
        ];

        let (repaired, decision, changed) =
            trim_chat_messages_to_provider_budget(messages, &model)?;

        assert!(changed);
        assert_ne!(decision.action, ContextBudgetAction::Block);
        assert!(
            repaired
                .iter()
                .any(|message| message.content.contains("Omitted messages"))
        );
        assert_eq!(
            repaired.last().map(|message| message.content.as_str()),
            Some("latest request: keep me")
        );
        assert!(
            repaired
                .iter()
                .all(|message| !message.content.contains("old context old context"))
        );
        Ok(())
    }

    #[test]
    fn compacted_history_digest_prioritizes_recent_omitted_context_and_stays_bounded() {
        let messages = (0..20)
            .map(|index| ChatMessage {
                role: if index % 2 == 0 { "user" } else { "assistant" }.to_string(),
                content: format!("history item {index} {}", "detail ".repeat(200)),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            })
            .collect::<Vec<_>>();

        let digest = compact_omitted_conversation(&messages, 1_200);

        assert!(digest.contains("Compacted messages: 20"));
        assert!(digest.contains("history item 19"));
        assert!(digest.chars().count() <= 1_200);
    }

    #[test]
    fn fitted_conversation_includes_digest_without_exceeding_budget() {
        let messages = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "old task: repair context compaction".to_string(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "a".repeat(3_500),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "continue".to_string(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
        ];

        let fitted = fit_conversation_messages_to_budget(messages, 4_000);
        let total_chars = fitted
            .iter()
            .map(|message| message.content.chars().count())
            .sum::<usize>();

        assert!(total_chars <= 4_000);
        assert!(fitted[0].content.contains("repair context compaction"));
        assert_eq!(
            fitted.last().map(|message| message.content.as_str()),
            Some("continue")
        );
    }

    #[test]
    fn truncating_a_message_honors_even_a_tiny_character_budget() {
        let message = ChatMessage {
            role: "assistant".to_string(),
            content: "content that cannot fit".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        };

        for budget in [0, 1, 16, 128] {
            let truncated = truncate_conversation_message(&message, budget);
            assert!(truncated.content.chars().count() <= budget);
        }
    }

    #[test]
    fn default_tool_round_limit_is_unlimited() {
        let _guard = TOOL_ROUND_LIMIT_TEST_LOCK.lock().unwrap();
        let previous = RUNTIME_MAX_TOOL_ROUNDS.swap(0, Ordering::Relaxed);
        assert_eq!(configured_max_tool_rounds(), None);
        assert_eq!(configured_max_tool_rounds_label(), "unlimited");
        RUNTIME_MAX_TOOL_ROUNDS.store(previous, Ordering::Relaxed);
    }

    #[test]
    fn runtime_tool_round_limit_override_still_applies() {
        let _guard = TOOL_ROUND_LIMIT_TEST_LOCK.lock().unwrap();
        let previous = RUNTIME_MAX_TOOL_ROUNDS.swap(0, Ordering::Relaxed);
        assert_eq!(set_runtime_max_tool_rounds(Some(3)), Some(3));
        assert_eq!(configured_max_tool_rounds(), Some(3));
        RUNTIME_MAX_TOOL_ROUNDS.store(previous, Ordering::Relaxed);
    }

    #[test]
    fn completed_tool_observation_marks_tool_errors_failed() {
        let observation = completed_tool_observation("run_tests", "TestsFailed: one test failed");
        assert!(observation.contains("status: failed"));
    }

    #[test]
    fn same_round_batch_decision_executes_read_only_siblings_only() {
        assert_eq!(
            same_round_tool_batch_decision(0, "write_file"),
            ToolBatchDecision::Execute
        );
        assert_eq!(
            same_round_tool_batch_decision(1, "read_file"),
            ToolBatchDecision::Execute
        );
        assert_eq!(
            same_round_tool_batch_decision(1, "list_files"),
            ToolBatchDecision::Execute
        );
        assert_eq!(
            same_round_tool_batch_decision(1, "cms_recall"),
            ToolBatchDecision::Execute
        );
        assert_eq!(
            same_round_tool_batch_decision(1, "write_file"),
            ToolBatchDecision::Defer
        );
        assert_eq!(
            same_round_tool_batch_decision(1, "run_command"),
            ToolBatchDecision::Defer
        );
    }

    #[test]
    fn openai_tool_loop_executes_read_only_sibling_same_round_and_defers_write()
    -> anyhow::Result<()> {
        let model = ModelInfo {
            name: "gpt-test".to_string(),
            provider: "openai".to_string(),
            display_name: None,
            context_window: Some(200000),
            supports_streaming: false,
            enabled: true,
            metadata: Default::default(),
        };
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "inspect files".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        }];
        let tools = vec![
            json!({
                "name": "read_file",
                "description": "read",
                "parameters": {"type": "object", "properties": {}},
            }),
            json!({
                "name": "list_files",
                "description": "list",
                "parameters": {"type": "object", "properties": {}},
            }),
            json!({
                "name": "write_file",
                "description": "write",
                "parameters": {"type": "object", "properties": {}},
            }),
        ];
        let mut post_calls = 0usize;
        let mut payload_parallel = None;
        let mut post = |payload: Value| -> anyhow::Result<Value> {
            post_calls += 1;
            if post_calls == 1 {
                payload_parallel = payload.get("parallel_tool_calls").and_then(Value::as_bool);
                Ok(json!({
                    "choices": [{
                        "message": {
                            "content": "",
                            "tool_calls": [
                                {"id": "call_1", "type": "function", "function": {"name": "read_file", "arguments": "{\"path\":\"a.rs\"}"}},
                                {"id": "call_2", "type": "function", "function": {"name": "list_files", "arguments": "{\"path\":\"src\"}"}},
                                {"id": "call_3", "type": "function", "function": {"name": "write_file", "arguments": "{\"path\":\"x\",\"content\":\"y\"}"}}
                            ]
                        }
                    }]
                }))
            } else {
                Ok(json!({"choices": [{"message": {"content": "done"}}]}))
            }
        };
        let mut executed = Vec::<String>::new();
        let mut execute_tool = |name: &str, _args: Map<String, Value>| -> String {
            executed.push(name.to_string());
            format!("ok from {name}")
        };

        let answer = openai_tool_loop(&model, &messages, &tools, &mut execute_tool, &mut post, 3)?;

        assert_eq!(answer, "done");
        assert_eq!(payload_parallel, Some(true));
        assert_eq!(executed, vec!["read_file", "list_files"]);
        Ok(())
    }

    #[test]
    fn approval_required_tool_output_is_detected_before_model_followup() {
        assert!(approval_required_tool_output(
            "Risky tool requires human approval: write_file; approval_id=apr_123"
        ));
        assert!(approval_required_tool_output(
            "ApprovalRequired: Human approval required; approval_id=apr_456"
        ));
        assert!(approval_required_tool_output(
            "Command network access requires human approval: git fetch; approval_id=apr_789"
        ));
        assert!(!approval_required_tool_output("normal tool output"));
        assert!(!approval_required_tool_output(
            r#"ok: read file

```typescript
const message = "approval_id=apr_123";
```"#
        ));
    }

    #[test]
    fn tool_round_limit_returns_recovery_guidance_instead_of_error_when_observations_exist() {
        let observations = vec![(
            "run_tests".to_string(),
            completed_tool_observation("run_tests", "TestsFailed: one test failed"),
        )];
        let result = tool_round_limit_result(&observations, 1)
            .expect("observed tool loops should produce a recoverable summary");
        assert!(result.contains("Tool-call round limit reached"));
        assert!(result.contains("Recovery guidance"));
        assert!(result.contains("do not repeat the same failed tool call"));
    }

    #[test]
    fn tool_round_limit_without_observations_is_recoverable() {
        let result = tool_round_limit_result(&[], 12)
            .expect("tool loop limit should not fail the whole turn");
        assert!(result.contains("Tool-call round limit reached"));
        assert!(result.contains("No completed tool observation was recorded"));
        assert!(result.contains("12-round limit"));
    }

    #[test]
    fn responses_stream_hides_reasoning_summary_and_streams_answer() -> anyhow::Result<()> {
        let body = concat!(
            r#"data: {"type":"response.reasoning_summary_text.delta","delta":"Checking context."}

"#,
            r#"data: {"type":"response.output_text.delta","delta":"Final answer."}

"#,
            r#"data: {"type":"response.completed","response":{"id":"resp_1","output_text":"Final answer.","output":[]}}

"#,
            "data: [DONE]\n\n"
        );
        let mut visible = String::new();
        let value = parse_response_sse_value(body, &mut |delta| visible.push_str(delta))?;

        assert_eq!(
            value.get("output_text").and_then(Value::as_str),
            Some("Final answer.")
        );
        assert_eq!(visible, "Final answer.");
        assert!(!visible.contains("Thinking trace"));
        assert!(!visible.contains("Checking context."));
        Ok(())
    }

    #[test]
    fn openai_compatible_provider_response_extracts_base64_media_artifact() {
        let response = json!({
            "choices": [{
                "message": {
                    "content": [
                        {"type": "text", "text": "Here is the image."},
                        {
                            "type": "image/png",
                            "data": STANDARD.encode(b"fake png bytes"),
                            "filename": "render.png"
                        }
                    ]
                }
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 2}
        });

        let parsed = openai_compatible_provider_response(&response);

        assert_eq!(parsed.content, "Here is the image.");
        assert_eq!(parsed.artifacts.len(), 1);
        assert_eq!(parsed.artifacts[0].kind, "image");
        assert_eq!(parsed.artifacts[0].mime_type, "image/png");
        assert_eq!(parsed.artifacts[0].bytes, b"fake png bytes");
        assert_eq!(
            parsed.artifacts[0].suggested_filename.as_deref(),
            Some("render.png")
        );
        assert_eq!(
            parsed.usage,
            Some(TokenUsage {
                input_tokens: 1,
                output_tokens: 2,
            })
        );
    }

    #[test]
    fn responses_provider_response_extracts_image_generation_result_without_mime() {
        let png = b"\x89PNG\r\n\x1a\nrest";
        let response = json!({
            "output": [{
                "type": "image_generation_call",
                "result": STANDARD.encode(png)
            }]
        });

        let parsed = responses_provider_response(&response);

        assert_eq!(parsed.artifacts.len(), 1);
        assert_eq!(parsed.artifacts[0].kind, "image");
        assert_eq!(parsed.artifacts[0].mime_type, "image/png");
        assert_eq!(parsed.artifacts[0].bytes, png);
    }

    #[test]
    fn grok_imagine_uses_images_generations_payload_without_system_prompt() -> anyhow::Result<()> {
        let model = ModelInfo {
            name: "grok-imagine".to_string(),
            provider: "xai".to_string(),
            display_name: None,
            context_window: None,
            supports_streaming: false,
            enabled: true,
            metadata: Default::default(),
        };
        let config = ProviderConfig {
            name: "xai-hbse".to_string(),
            display_name: None,
            kind: "hbse_openai_compatible".to_string(),
            api_key_env: None,
            base_url: Some("https://api.x.ai/v1".to_string()),
            auth_type: "hbse".to_string(),
            enabled: true,
            metadata: Default::default(),
        };
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "do not include me".to_string(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "create a highly detailed Vegvisir".to_string(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
        ];

        assert!(model_uses_images_generations_api(&model));
        let payload = image_generation_payload(&messages, &model, &config)?;

        assert_eq!(payload["model"], "grok-imagine");
        assert_eq!(payload["prompt"], "create a highly detailed Vegvisir");
        assert_eq!(payload["response_format"], "b64_json");
        assert!(!payload.to_string().contains("do not include me"));
        Ok(())
    }

    #[test]
    fn image_generation_provider_response_extracts_data_url_artifact() {
        let png = b"\x89PNG\r\n\x1a\nrest";
        let response = json!({
            "created": 123,
            "data": [{
                "url": format!("data:image/png;base64,{}", STANDARD.encode(png)),
                "revised_prompt": "ornate Vegvisir"
            }]
        });

        let parsed = image_generation_provider_response(&response);

        assert!(parsed.content.contains("ornate Vegvisir"));
        assert_eq!(parsed.artifacts.len(), 1);
        assert_eq!(parsed.artifacts[0].kind, "image");
        assert_eq!(parsed.artifacts[0].mime_type, "image/png");
        assert_eq!(parsed.artifacts[0].bytes, png);
    }

    #[test]
    fn generated_artifacts_are_saved_under_workspace_generated_dir() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let session = SessionState::new(temp.path(), Vec::new(), Vec::new());
        let artifacts = vec![ProviderGeneratedArtifact {
            kind: "video".to_string(),
            mime_type: "video/mp4".to_string(),
            bytes: b"video bytes".to_vec(),
            suggested_filename: Some("../clip.mp4".to_string()),
        }];

        let attachments = save_generated_artifacts(&session, &artifacts)?;

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].kind, "video");
        assert_eq!(attachments[0].mime_type.as_deref(), Some("video/mp4"));
        assert_eq!(attachments[0].size_bytes, Some(11));
        let saved_path = PathBuf::from(&attachments[0].path);
        assert!(saved_path.starts_with(temp.path().join(".vegvisir/generated")));
        assert_eq!(fs::read(saved_path)?, b"video bytes");
        Ok(())
    }

    #[test]
    fn anthropic_sse_usage_includes_prompt_cache_tokens() -> anyhow::Result<()> {
        let body = concat!(
            r#"data: {"type":"message_start","message":{"usage":{"input_tokens":10,"cache_creation_input_tokens":20,"cache_read_input_tokens":30}}}"#,
            "

",
            r#"data: {"type":"content_block_delta","delta":{"text":"cached answer"}}"#,
            "

",
            r#"data: {"type":"message_delta","usage":{"output_tokens":7}}"#,
            "

",
            "data: [DONE]

"
        );
        let response = parse_anthropic_sse_response(body)?;

        assert_eq!(response.content, "cached answer");
        assert_eq!(
            response.usage,
            Some(TokenUsage {
                input_tokens: 60,
                output_tokens: 7,
            })
        );
        Ok(())
    }

    #[test]
    fn anthropic_cache_control_applies_only_to_anthropic_payloads() {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "Stable Vegvisir system prompt".to_string(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Dynamic turn".to_string(),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            },
        ];
        let model = ModelInfo {
            name: "claude-test".to_string(),
            provider: "anthropic".to_string(),
            display_name: None,
            context_window: Some(200000),
            supports_streaming: true,
            enabled: true,
            metadata: Default::default(),
        };

        let anthropic = anthropic_messages_payload(&messages, &model);
        assert_eq!(
            anthropic.pointer("/system/0/cache_control"),
            Some(&json!({"type": "ephemeral"}))
        );
        assert!(
            anthropic
                .pointer("/messages/0/content/cache_control")
                .is_none()
        );

        let openai = openai_messages(&messages);
        assert!(
            openai
                .iter()
                .all(|message| message.get("cache_control").is_none())
        );
        let responses = responses_payload(&messages, &model);
        assert!(responses.get("cache_control").is_none());
    }

    #[test]
    fn session_fast_mode_minimizes_supported_openai_reasoning() {
        let model = ModelInfo {
            name: "gpt-fast".to_string(),
            provider: "openai".to_string(),
            display_name: None,
            context_window: Some(400000),
            supports_streaming: true,
            enabled: true,
            metadata: BTreeMap::from([
                ("fast_capable".to_string(), json!(true)),
                ("reasoning_level".to_string(), json!("high")),
                ("reasoning_summary".to_string(), json!(true)),
            ]),
        };
        let mut session = SessionState::new("/tmp/workspace", Vec::new(), Vec::new());
        session.fast_mode = true;

        let fast_model = model_with_session_reasoning(&model, &session);

        assert_eq!(
            fast_model
                .metadata
                .get("reasoning_effort")
                .and_then(Value::as_str),
            Some("minimal")
        );
        assert_eq!(fast_model.metadata.get("fast_mode"), Some(&json!(true)));
    }

    #[test]
    fn session_fast_mode_disables_supported_anthropic_thinking() {
        let model = ModelInfo {
            name: "claude-fast".to_string(),
            provider: "anthropic".to_string(),
            display_name: None,
            context_window: Some(200000),
            supports_streaming: true,
            enabled: true,
            metadata: BTreeMap::from([
                ("fast_capable".to_string(), json!(true)),
                ("reasoning_level".to_string(), json!("high")),
            ]),
        };
        let mut session = SessionState::new("/tmp/workspace", Vec::new(), Vec::new());
        session.fast_mode = true;

        let fast_model = model_with_session_reasoning(&model, &session);

        assert!(anthropic_thinking_budget_tokens(&fast_model).is_none());
        assert_eq!(fast_model.metadata.get("fast_mode"), Some(&json!(true)));
    }

    #[test]
    fn openai_responses_payload_requests_reasoning_summary_for_reasoning_models() {
        let model = ModelInfo {
            name: "gpt-5.5".to_string(),
            provider: "openai".to_string(),
            display_name: None,
            context_window: Some(400000),
            supports_streaming: true,
            enabled: true,
            metadata: Default::default(),
        };
        let payload = responses_payload(&[], &model);

        assert_eq!(
            payload
                .pointer("/reasoning/summary")
                .and_then(Value::as_str),
            Some("auto")
        );
    }

    #[test]
    fn session_conversation_messages_includes_manual_context_capsule() {
        let mut session = SessionState::new("/tmp/workspace", Vec::new(), Vec::new());
        session.messages.push(ChatMessage {
            role: "system".to_string(),
            content: "Context Capsule: manual repair\nCurrent Objective:\n- keep working"
                .to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        session.messages.push(ChatMessage {
            role: "system".to_string(),
            content: "Tool finished: read_file - ok".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: "continue".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        let provider_messages = session_conversation_messages(&session);

        assert!(provider_messages.iter().any(|message| {
            message.role == "system" && message.content.starts_with("Context Capsule:")
        }));
        assert!(
            provider_messages
                .iter()
                .all(|message| !message.content.starts_with("Tool finished:"))
        );
    }

    #[test]
    fn automatic_compaction_mutates_session_and_keeps_latest_request() -> anyhow::Result<()> {
        let model = ModelInfo {
            name: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            display_name: None,
            context_window: Some(1024),
            supports_streaming: true,
            enabled: true,
            metadata: Default::default(),
        };
        let mut session = SessionState::new("/tmp/workspace", Vec::new(), Vec::new());
        for index in 0..8 {
            session.messages.push(ChatMessage {
                role: if index % 2 == 0 { "user" } else { "assistant" }.to_string(),
                content: format!("old-{index} {}", "context ".repeat(220)),
                attachments: Vec::new(),
                created_at: chrono::Utc::now(),
            });
        }
        session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: "latest request must survive".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        let original_count = session.messages.len();

        let decision =
            automatically_compact_session_history(&mut session, &model, "stable system context")?
                .expect("oversized session should compact automatically");

        assert!(matches!(
            decision.action,
            ContextBudgetAction::CompactRecommended | ContextBudgetAction::Block
        ));
        assert!(session.messages.len() < original_count);
        assert!(session.messages.first().is_some_and(|message| {
            message
                .content
                .starts_with("Earlier conversation history was compacted by Vegvisir")
        }));
        assert_eq!(
            session
                .messages
                .last()
                .map(|message| message.content.as_str()),
            Some("latest request must survive")
        );
        Ok(())
    }

    #[test]
    fn session_conversation_messages_uses_recent_bounded_suffix() {
        let mut session = SessionState::new("/tmp/workspace", Vec::new(), Vec::new());
        session.context_limit = 20_000;
        session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: "old request".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        session.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: "x".repeat(60_000),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        session.messages.push(ChatMessage {
            role: "system".to_string(),
            content: "Tool finished: read_file - ok: ".to_string() + &"y".repeat(60_000),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        session.messages.push(ChatMessage {
            role: "user".to_string(),
            content: "latest request".to_string(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
        });

        let provider_messages = session_conversation_messages(&session);
        let total_chars = provider_messages
            .iter()
            .map(|message| message.content.chars().count())
            .sum::<usize>();

        assert!(total_chars <= provider_history_char_budget(&session));
        assert!(provider_messages.first().is_some_and(|message| {
            message.role == "system"
                && message
                    .content
                    .contains("Earlier conversation history was compacted")
                && message.content.contains("old request")
        }));
        assert!(
            provider_messages[0].content.chars().count()
                <= (provider_history_char_budget(&session) / 5).clamp(1_024, 12_000)
        );
        assert!(
            provider_messages
                .iter()
                .all(|message| !message.content.starts_with("Tool finished: read_file"))
        );
        assert_eq!(
            provider_messages
                .last()
                .map(|message| message.content.as_str()),
            Some("latest request")
        );
    }
}
