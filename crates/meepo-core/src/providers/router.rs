//! Model router with automatic failover across providers

use anyhow::{Result, anyhow};
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::api::ToolDefinition;

use super::types::{ChatMessage, ChatResponse, LlmProvider};

/// Routes LLM requests across multiple providers with automatic failover
pub struct ModelRouter {
    /// Providers in failover order (index 0 = primary)
    providers: Vec<Box<dyn LlmProvider>>,
    /// Maximum retries per provider before moving to the next
    max_retries_per_provider: u32,
    /// Base delay for exponential backoff
    base_retry_delay: Duration,
}

impl ModelRouter {
    /// Create a router with a single provider (no failover)
    pub fn single(provider: Box<dyn LlmProvider>) -> Self {
        Self {
            providers: vec![provider],
            max_retries_per_provider: 1,
            base_retry_delay: Duration::from_millis(500),
        }
    }

    /// Create a router with multiple providers in failover order
    pub fn with_failover(providers: Vec<Box<dyn LlmProvider>>) -> Result<Self> {
        if providers.is_empty() {
            return Err(anyhow!("ModelRouter requires at least one provider"));
        }
        Ok(Self {
            providers,
            max_retries_per_provider: 2,
            base_retry_delay: Duration::from_millis(500),
        })
    }

    /// Set the maximum retries per provider
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries_per_provider = max_retries;
        self
    }

    /// Set the base retry delay for exponential backoff
    pub fn with_base_retry_delay(mut self, delay: Duration) -> Self {
        self.base_retry_delay = delay;
        self
    }

    /// Send a chat request, failing over to the next provider on error
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        system: &str,
    ) -> Result<ChatResponse> {
        let mut last_error = None;

        for (idx, provider) in self.providers.iter().enumerate() {
            for attempt in 0..self.max_retries_per_provider {
                debug!(
                    "Trying provider {} ({}/{}) attempt {}/{}",
                    provider.provider_name(),
                    provider.model(),
                    idx + 1,
                    attempt + 1,
                    self.max_retries_per_provider,
                );

                match provider.chat(messages, tools, system).await {
                    Ok(response) => {
                        if idx > 0 {
                            info!(
                                "Request succeeded on failover provider {} ({})",
                                provider.provider_name(),
                                provider.model()
                            );
                        }
                        return Ok(response);
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        let is_retryable = is_retryable_error(&err_str);

                        warn!(
                            "Provider {} ({}) failed (attempt {}, retryable={}): {}",
                            provider.provider_name(),
                            provider.model(),
                            attempt + 1,
                            is_retryable,
                            err_str,
                        );

                        last_error = Some(e);

                        if !is_retryable {
                            break;
                        }

                        // Exponential backoff before retry
                        if attempt + 1 < self.max_retries_per_provider {
                            let delay = self.base_retry_delay * 2u32.pow(attempt);
                            debug!("Backing off for {:?} before retry", delay);
                            tokio::time::sleep(delay).await;
                        }
                    }
                }
            }

            if idx + 1 < self.providers.len() {
                info!(
                    "Failing over from {} to {}",
                    provider.provider_name(),
                    self.providers[idx + 1].provider_name()
                );
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("All providers failed")))
    }

    /// Get the primary provider's model name
    pub fn model(&self) -> &str {
        self.providers
            .first()
            .map(|p| p.model())
            .unwrap_or("unknown")
    }

    /// Get the primary provider's name
    pub fn provider_name(&self) -> &str {
        self.providers
            .first()
            .map(|p| p.provider_name())
            .unwrap_or("unknown")
    }

    /// Number of configured providers
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}

/// Determine if an error is retryable (rate limit, server error, timeout)
fn is_retryable_error(err: &str) -> bool {
    let retryable_patterns = [
        "429",
        "500",
        "502",
        "503",
        "504",
        "rate limit",
        "rate_limit",
        "overloaded",
        "timeout",
        "timed out",
        "connection reset",
        "connection refused",
        "temporarily unavailable",
    ];
    let lower = err.to_lowercase();
    retryable_patterns.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    use super::super::types::{ChatResponseBlock, ChatUsage, StopReason};

    /// Mock provider that succeeds
    struct SuccessProvider {
        name: String,
        model_name: String,
    }

    #[async_trait]
    impl LlmProvider for SuccessProvider {
        fn provider_name(&self) -> &str {
            &self.name
        }
        fn model(&self) -> &str {
            &self.model_name
        }
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
            _system: &str,
        ) -> Result<ChatResponse> {
            Ok(ChatResponse {
                blocks: vec![ChatResponseBlock::Text {
                    text: format!("from {}", self.name),
                }],
                stop_reason: StopReason::EndTurn,
                usage: ChatUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                },
            })
        }
    }

    /// Mock provider that always fails
    struct FailProvider {
        name: String,
        error: String,
    }

    #[async_trait]
    impl LlmProvider for FailProvider {
        fn provider_name(&self) -> &str {
            &self.name
        }
        fn model(&self) -> &str {
            "fail-model"
        }
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
            _system: &str,
        ) -> Result<ChatResponse> {
            Err(anyhow!("{}", self.error))
        }
    }

    #[tokio::test]
    async fn test_single_provider_success() {
        let router = ModelRouter::single(Box::new(SuccessProvider {
            name: "test".to_string(),
            model_name: "test-model".to_string(),
        }));
        let result = router.chat(&[], &[], "system").await.unwrap();
        assert_eq!(result.stop_reason, StopReason::EndTurn);
    }

    #[tokio::test]
    async fn test_failover_to_second_provider() {
        let router = ModelRouter::with_failover(vec![
            Box::new(FailProvider {
                name: "primary".to_string(),
                error: "status 500: server error".to_string(),
            }),
            Box::new(SuccessProvider {
                name: "fallback".to_string(),
                model_name: "fallback-model".to_string(),
            }),
        ])
        .unwrap()
        .with_max_retries(1)
        .with_base_retry_delay(Duration::from_millis(1));

        let result = router.chat(&[], &[], "system").await.unwrap();
        if let ChatResponseBlock::Text { text } = &result.blocks[0] {
            assert_eq!(text, "from fallback");
        } else {
            panic!("expected text block");
        }
    }

    #[tokio::test]
    async fn test_all_providers_fail() {
        let router = ModelRouter::with_failover(vec![
            Box::new(FailProvider {
                name: "a".to_string(),
                error: "auth error 401".to_string(),
            }),
            Box::new(FailProvider {
                name: "b".to_string(),
                error: "auth error 401".to_string(),
            }),
        ])
        .unwrap()
        .with_max_retries(1);

        let result = router.chat(&[], &[], "system").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_providers_rejected() {
        let result = ModelRouter::with_failover(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_model_and_name() {
        let router = ModelRouter::single(Box::new(SuccessProvider {
            name: "anthropic".to_string(),
            model_name: "claude-opus-4-6".to_string(),
        }));
        assert_eq!(router.model(), "claude-opus-4-6");
        assert_eq!(router.provider_name(), "anthropic");
        assert_eq!(router.provider_count(), 1);
    }

    #[test]
    fn test_is_retryable_error() {
        assert!(is_retryable_error("status 429: rate limit exceeded"));
        assert!(is_retryable_error("status 500: internal server error"));
        assert!(is_retryable_error("request timed out"));
        assert!(is_retryable_error("API overloaded"));
        assert!(!is_retryable_error("status 401: unauthorized"));
        assert!(!is_retryable_error("invalid API key"));
    }

    #[tokio::test]
    async fn test_non_retryable_skips_retries() {
        let router = ModelRouter::with_failover(vec![
            Box::new(FailProvider {
                name: "primary".to_string(),
                error: "status 401: unauthorized".to_string(),
            }),
            Box::new(SuccessProvider {
                name: "fallback".to_string(),
                model_name: "model".to_string(),
            }),
        ])
        .unwrap()
        .with_max_retries(3)
        .with_base_retry_delay(Duration::from_millis(1));

        // Should skip retries on 401 and go straight to fallback
        let result = router.chat(&[], &[], "system").await.unwrap();
        if let ChatResponseBlock::Text { text } = &result.blocks[0] {
            assert_eq!(text, "from fallback");
        }
    }
}
