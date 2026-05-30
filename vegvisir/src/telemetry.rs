use crate::provider::TokenUsage;

pub fn count_text_tokens(model: &str, text: &str) -> u64 {
    if text.trim().is_empty() {
        return 0;
    }
    if let Ok(bpe) = tiktoken_rs::get_bpe_from_model(model) {
        return bpe.encode_with_special_tokens(text).len() as u64;
    }
    if let Ok(bpe) = tiktoken_rs::cl100k_base() {
        return bpe.encode_with_special_tokens(text).len() as u64;
    }
    fallback_token_estimate(text)
}

pub fn fallback_token_estimate(text: &str) -> u64 {
    // Conservative approximation used only if the tokenizer fails to load.
    let chars = text.chars().count() as u64;
    chars.div_ceil(4).max(1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenCountSource {
    ProviderReported,
    LocalTokenizer,
}

impl TokenCountSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::ProviderReported => "provider-reported",
            Self::LocalTokenizer => "local tiktoken count",
        }
    }
}

pub fn selected_usage_or_counted(
    provider_usage: Option<TokenUsage>,
    model: &str,
    input_text: &str,
    output_text: &str,
) -> (TokenUsage, TokenCountSource) {
    if let Some(usage) = provider_usage
        && usage.total() > 0
    {
        return (usage, TokenCountSource::ProviderReported);
    }
    (
        TokenUsage {
            input_tokens: count_text_tokens(model, input_text),
            output_tokens: count_text_tokens(model, output_text),
        },
        TokenCountSource::LocalTokenizer,
    )
}
