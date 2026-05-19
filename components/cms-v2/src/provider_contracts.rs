use crate::prompt_cache::ModelRequestEnvelope;
use crate::vectors::EmbeddingService;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderEndpointSpec {
    pub provider: String,
    pub model: String,
    pub capability: ProviderCapability,
    pub credential_env: Option<String>,
    pub base_url_env: Option<String>,
    pub default_base_url: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderCapability {
    TextGeneration,
    Embedding,
}

impl ProviderEndpointSpec {
    pub fn openai_responses(model: impl Into<String>) -> Self {
        Self {
            provider: "openai".to_string(),
            model: model.into(),
            capability: ProviderCapability::TextGeneration,
            credential_env: Some("OPENAI_API_KEY".to_string()),
            base_url_env: Some("OPENAI_BASE_URL".to_string()),
            default_base_url: Some("https://api.openai.com/v1".to_string()),
            metadata: BTreeMap::new(),
        }
    }

    pub fn openai_embeddings(model: impl Into<String>) -> Self {
        Self {
            provider: "openai".to_string(),
            model: model.into(),
            capability: ProviderCapability::Embedding,
            credential_env: Some("OPENAI_API_KEY".to_string()),
            base_url_env: Some("OPENAI_BASE_URL".to_string()),
            default_base_url: Some("https://api.openai.com/v1".to_string()),
            metadata: BTreeMap::new(),
        }
    }

    pub fn anthropic_messages(model: impl Into<String>) -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: model.into(),
            capability: ProviderCapability::TextGeneration,
            credential_env: Some("ANTHROPIC_API_KEY".to_string()),
            base_url_env: Some("ANTHROPIC_BASE_URL".to_string()),
            default_base_url: Some("https://api.anthropic.com/v1".to_string()),
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelAdapterRequest {
    pub endpoint: ProviderEndpointSpec,
    pub envelope: ModelRequestEnvelope,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelAdapterResponse {
    pub provider: String,
    pub model: String,
    pub output_text: String,
    pub usage: Option<ProviderUsage>,
    pub raw_metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub cached_input_tokens: usize,
}

pub trait ModelAdapter {
    fn complete(&self, request: ModelAdapterRequest) -> anyhow::Result<ModelAdapterResponse>;
}

pub trait EmbeddingAdapter: EmbeddingService {
    fn endpoint(&self) -> ProviderEndpointSpec;
}
