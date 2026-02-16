//! OpenAI-compatible provider for Ollama, Together, Groq, LM Studio, etc.
//!
//! Reuses the OpenAI wire format with a configurable base URL.

use anyhow::Result;
use async_trait::async_trait;

use crate::api::ToolDefinition;

use super::openai::OpenAiProvider;
use super::types::{ChatMessage, ChatResponse, LlmProvider};

/// OpenAI-compatible provider — wraps [`OpenAiProvider`] with a custom name
pub struct OpenAiCompatProvider {
    inner: OpenAiProvider,
    name: String,
}

impl std::fmt::Debug for OpenAiCompatProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatProvider")
            .field("name", &self.name)
            .field("inner", &self.inner)
            .finish()
    }
}

impl OpenAiCompatProvider {
    /// Create a new OpenAI-compatible provider.
    ///
    /// - `name`: human-readable label (e.g. "ollama", "together", "groq")
    /// - `base_url`: the endpoint root (e.g. `http://localhost:11434/v1`)
    pub fn new(
        name: String,
        api_key: String,
        model: String,
        base_url: String,
        max_tokens: u32,
    ) -> Self {
        Self {
            inner: OpenAiProvider::new(api_key, model, base_url, max_tokens),
            name,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    fn provider_name(&self) -> &str {
        &self.name
    }

    fn model(&self) -> &str {
        self.inner.model()
    }

    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        system: &str,
    ) -> Result<ChatResponse> {
        self.inner.chat(messages, tools, system).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compat_provider_name() {
        let p = OpenAiCompatProvider::new(
            "ollama".to_string(),
            "".to_string(),
            "llama3".to_string(),
            "http://localhost:11434/v1".to_string(),
            4096,
        );
        assert_eq!(p.provider_name(), "ollama");
        assert_eq!(p.model(), "llama3");
    }

    #[test]
    fn test_compat_provider_debug_hides_key() {
        let p = OpenAiCompatProvider::new(
            "groq".to_string(),
            "gsk_secret".to_string(),
            "llama3-70b".to_string(),
            "https://api.groq.com/openai/v1".to_string(),
            4096,
        );
        let debug = format!("{:?}", p);
        assert!(!debug.contains("gsk_secret"));
        assert!(debug.contains("groq"));
    }
}
