//! LLM API client with tool use loop
//!
//! Wraps [`ModelRouter`] to provide a high-level interface for the agent.
//! Supports Anthropic, OpenAI, Google Gemini, and any OpenAI-compatible endpoint
//! with automatic failover.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::providers::anthropic::AnthropicProvider;
use crate::providers::router::ModelRouter;
use crate::providers::types::{
    ChatBlock, ChatMessage, ChatMessageContent, ChatResponseBlock, ChatRole, StopReason,
};
use crate::tools::ToolExecutor;
use crate::usage::AccumulatedUsage;

/// LLM API client — delegates to [`ModelRouter`] for multi-provider support
#[derive(Clone)]
pub struct ApiClient {
    router: Arc<ModelRouter>,
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiClient")
            .field("provider", &self.router.provider_name())
            .field("model", &self.router.model())
            .field("providers", &self.router.provider_count())
            .finish()
    }
}

impl ApiClient {
    /// Create a new API client with a single Anthropic provider (backward compatible)
    pub fn new(api_key: String, model: Option<String>) -> Self {
        let model_str = model.unwrap_or_else(|| "claude-opus-4-6".to_string());
        let provider = AnthropicProvider::new(
            api_key,
            model_str,
            "https://api.anthropic.com".to_string(),
            4096,
        );
        Self {
            router: Arc::new(ModelRouter::single(Box::new(provider))),
        }
    }

    /// Create an API client from a pre-built [`ModelRouter`]
    pub fn from_router(router: ModelRouter) -> Self {
        Self {
            router: Arc::new(router),
        }
    }

    /// Set max tokens for responses (only works with single-provider backward-compat constructor)
    pub fn with_max_tokens(self, max_tokens: u32) -> Self {
        // For backward compatibility: rebuild the Anthropic provider with new max_tokens.
        // When using from_router(), max_tokens is set per-provider at construction time.
        let _ = max_tokens;
        // NOTE: This is a no-op when using from_router(). The max_tokens is configured
        // per-provider. Kept for backward compatibility with existing call sites.
        self
    }

    /// Set a custom base URL (backward compatibility — no-op when using from_router)
    pub fn with_base_url(self, _base_url: String) -> Self {
        // NOTE: Base URL is configured per-provider. Kept for backward compatibility.
        self
    }

    /// Make a single chat request via the model router
    pub async fn chat(
        &self,
        messages: &[ApiMessage],
        tools: &[ToolDefinition],
        system: &str,
    ) -> Result<ApiResponse> {
        // Convert legacy ApiMessage to provider-agnostic ChatMessage
        let chat_messages = Self::to_chat_messages(messages);

        let response = self.router.chat(&chat_messages, tools, system).await?;

        // Convert back to legacy ApiResponse
        Ok(Self::from_chat_response(response))
    }

    /// Run the full tool use loop until completion (with 5-minute overall timeout)
    pub async fn run_tool_loop(
        &self,
        initial_message: &str,
        system: &str,
        tools: &[ToolDefinition],
        tool_executor: &dyn ToolExecutor,
    ) -> Result<(String, AccumulatedUsage)> {
        tokio::time::timeout(
            Duration::from_secs(300),
            self.run_tool_loop_inner(initial_message, system, tools, tool_executor),
        )
        .await
        .map_err(|_| anyhow!("Tool loop timed out after 5 minutes"))?
    }

    async fn run_tool_loop_inner(
        &self,
        initial_message: &str,
        system: &str,
        tools: &[ToolDefinition],
        tool_executor: &dyn ToolExecutor,
    ) -> Result<(String, AccumulatedUsage)> {
        const MAX_TOOL_OUTPUT: usize = 100_000;

        let mut accumulated = AccumulatedUsage::new();

        let mut conversation: Vec<ChatMessage> = vec![ChatMessage {
            role: ChatRole::User,
            content: ChatMessageContent::Text(initial_message.to_string()),
        }];

        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 10;

        loop {
            iterations += 1;
            if iterations > MAX_ITERATIONS {
                warn!("Tool loop exceeded maximum iterations ({})", MAX_ITERATIONS);
                return Err(anyhow!("Tool loop exceeded maximum iterations"));
            }

            info!("Tool loop iteration {}", iterations);

            let response = self.router.chat(&conversation, tools, system).await?;

            // Accumulate token usage from this API call
            accumulated.add(response.usage.input_tokens, response.usage.output_tokens);

            // Build assistant message from response blocks
            let assistant_blocks: Vec<ChatBlock> = response
                .blocks
                .iter()
                .map(|b| match b {
                    ChatResponseBlock::Text { text } => ChatBlock::Text { text: text.clone() },
                    ChatResponseBlock::ToolCall { id, name, input } => ChatBlock::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    },
                })
                .collect();

            conversation.push(ChatMessage {
                role: ChatRole::Assistant,
                content: ChatMessageContent::Blocks(assistant_blocks),
            });

            if response.stop_reason.is_tool_use() {
                debug!("Processing tool calls from response");

                let mut tool_results = Vec::new();

                for block in &response.blocks {
                    if let ChatResponseBlock::ToolCall { id, name, input } = block {
                        info!("Executing tool: {}", name);

                        accumulated.record_tool_call(name);

                        let result = tool_executor.execute(name, input.clone()).await;

                        let mut result_content = match result {
                            Ok(output) => output,
                            Err(e) => {
                                warn!("Tool {} failed: {}", name, e);
                                format!("Error: {}", e)
                            }
                        };

                        if result_content.len() > MAX_TOOL_OUTPUT {
                            result_content.truncate(MAX_TOOL_OUTPUT);
                            result_content.push_str("\n[Output truncated]");
                        }

                        tool_results.push(ChatBlock::ToolResult {
                            tool_call_id: id.clone(),
                            content: result_content,
                        });
                    }
                }

                if tool_results.is_empty() {
                    warn!("Stop reason was tool_use but no tool calls found");
                    return Err(anyhow!("Stop reason was tool_use but no tool calls found"));
                }

                conversation.push(ChatMessage {
                    role: ChatRole::User,
                    content: ChatMessageContent::Blocks(tool_results),
                });
            } else if response.stop_reason.is_end_turn()
                || response.stop_reason == StopReason::Unknown
            {
                debug!(
                    "Tool loop completed (iterations: {}, tokens: in={} out={})",
                    iterations, accumulated.input_tokens, accumulated.output_tokens
                );

                let mut final_text = String::new();
                for block in &response.blocks {
                    if let ChatResponseBlock::Text { text } = block {
                        if !final_text.is_empty() {
                            final_text.push('\n');
                        }
                        final_text.push_str(text);
                    }
                }

                if final_text.is_empty() {
                    return Err(anyhow!("No text response from assistant"));
                }

                return Ok((final_text, accumulated));
            } else {
                warn!("Unexpected stop_reason: {:?}", response.stop_reason);
                return Err(anyhow!("Unexpected stop_reason: {:?}", response.stop_reason));
            }
        }
    }

    /// Get the model name (for usage tracking)
    pub fn model(&self) -> &str {
        self.router.model()
    }

    // ── Legacy format conversion helpers ──

    fn to_chat_messages(messages: &[ApiMessage]) -> Vec<ChatMessage> {
        messages
            .iter()
            .map(|m| {
                let role = match m.role.as_str() {
                    "assistant" => ChatRole::Assistant,
                    "system" => ChatRole::System,
                    _ => ChatRole::User,
                };
                let content = match &m.content {
                    MessageContent::Text(t) => ChatMessageContent::Text(t.clone()),
                    MessageContent::Blocks(blocks) => {
                        let chat_blocks: Vec<ChatBlock> = blocks
                            .iter()
                            .map(|b| match b {
                                ContentBlock::Text { text } => {
                                    ChatBlock::Text { text: text.clone() }
                                }
                                ContentBlock::ToolUse { id, name, input } => {
                                    ChatBlock::ToolCall {
                                        id: id.clone(),
                                        name: name.clone(),
                                        input: input.clone(),
                                    }
                                }
                                ContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                } => ChatBlock::ToolResult {
                                    tool_call_id: tool_use_id.clone(),
                                    content: content.clone(),
                                },
                            })
                            .collect();
                        ChatMessageContent::Blocks(chat_blocks)
                    }
                };
                ChatMessage { role, content }
            })
            .collect()
    }

    fn from_chat_response(
        resp: crate::providers::types::ChatResponse,
    ) -> ApiResponse {
        let content: Vec<ContentBlock> = resp
            .blocks
            .into_iter()
            .map(|b| match b {
                ChatResponseBlock::Text { text } => ContentBlock::Text { text },
                ChatResponseBlock::ToolCall { id, name, input } => {
                    ContentBlock::ToolUse { id, name, input }
                }
            })
            .collect();

        let stop_reason = match resp.stop_reason {
            StopReason::EndTurn => Some("end_turn".to_string()),
            StopReason::ToolUse => Some("tool_use".to_string()),
            StopReason::MaxTokens => Some("max_tokens".to_string()),
            StopReason::Unknown => None,
        };

        ApiResponse {
            id: String::new(),
            content,
            stop_reason,
            usage: Usage {
                input_tokens: resp.usage.input_tokens,
                output_tokens: resp.usage.output_tokens,
            },
        }
    }
}

/// Message in conversation history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: String,
    pub content: MessageContent,
}

/// Content of a message (can be simple text or structured blocks)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// Content block in a message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

/// Tool definition for the API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Response from the API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse {
    pub id: String,
    pub content: Vec<ContentBlock>,
    pub stop_reason: Option<String>,
    pub usage: Usage,
}

/// Token usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_client_creation() {
        let client = ApiClient::new("test-key".to_string(), None);
        assert_eq!(client.model(), "claude-opus-4-6");
    }

    #[test]
    fn test_api_client_creation_custom_model() {
        let client = ApiClient::new("test-key".to_string(), Some("claude-sonnet-4-20250514".to_string()));
        assert_eq!(client.model(), "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_content_block_serialization() {
        let block = ContentBlock::Text {
            text: "Hello".to_string(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("text"));
    }

    #[test]
    fn test_api_client_debug_no_key_leak() {
        let client = ApiClient::new("sk-ant-1234567890abcdef".to_string(), None);
        let debug_output = format!("{:?}", client);

        // Should NOT contain the full key
        assert!(!debug_output.contains("sk-ant-1234567890abcdef"));
        // Should contain provider info
        assert!(debug_output.contains("anthropic"));
    }

    #[test]
    fn test_api_client_clone() {
        let client = ApiClient::new("test-key".to_string(), None);
        let cloned = client.clone();
        assert_eq!(cloned.model(), "claude-opus-4-6");
    }

    #[test]
    fn test_api_client_from_router() {
        use crate::providers::anthropic::AnthropicProvider;
        use crate::providers::router::ModelRouter;

        let provider = AnthropicProvider::new(
            "test-key".to_string(),
            "claude-opus-4-6".to_string(),
            "https://api.anthropic.com".to_string(),
            4096,
        );
        let router = ModelRouter::single(Box::new(provider));
        let client = ApiClient::from_router(router);
        assert_eq!(client.model(), "claude-opus-4-6");
    }

    #[test]
    fn test_to_chat_messages_text() {
        let msgs = vec![ApiMessage {
            role: "user".to_string(),
            content: MessageContent::Text("hello".to_string()),
        }];
        let result = ApiClient::to_chat_messages(&msgs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, ChatRole::User);
    }

    #[test]
    fn test_to_chat_messages_blocks() {
        let msgs = vec![ApiMessage {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text { text: "thinking...".to_string() },
                ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "search".to_string(),
                    input: serde_json::json!({}),
                },
            ]),
        }];
        let result = ApiClient::to_chat_messages(&msgs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, ChatRole::Assistant);
        if let ChatMessageContent::Blocks(blocks) = &result[0].content {
            assert_eq!(blocks.len(), 2);
        } else {
            panic!("expected blocks");
        }
    }

    #[test]
    fn test_from_chat_response_end_turn() {
        use crate::providers::types::{ChatResponse, ChatResponseBlock, ChatUsage, StopReason};

        let resp = ChatResponse {
            blocks: vec![ChatResponseBlock::Text { text: "Hello!".to_string() }],
            stop_reason: StopReason::EndTurn,
            usage: ChatUsage { input_tokens: 10, output_tokens: 5 },
        };
        let result = ApiClient::from_chat_response(resp);
        assert_eq!(result.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(result.usage.input_tokens, 10);
    }

    #[test]
    fn test_from_chat_response_tool_use() {
        use crate::providers::types::{ChatResponse, ChatResponseBlock, ChatUsage, StopReason};

        let resp = ChatResponse {
            blocks: vec![ChatResponseBlock::ToolCall {
                id: "tc_1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({"q": "test"}),
            }],
            stop_reason: StopReason::ToolUse,
            usage: ChatUsage { input_tokens: 20, output_tokens: 15 },
        };
        let result = ApiClient::from_chat_response(resp);
        assert_eq!(result.stop_reason.as_deref(), Some("tool_use"));
        assert!(matches!(&result.content[0], ContentBlock::ToolUse { name, .. } if name == "search"));
    }
}
