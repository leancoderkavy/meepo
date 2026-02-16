//! Anthropic Claude provider

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tracing::debug;

use crate::api::ToolDefinition;

use super::types::{
    ChatBlock, ChatMessage, ChatMessageContent, ChatResponse, ChatResponseBlock, ChatRole,
    ChatUsage, LlmProvider, StopReason,
};

/// Anthropic Claude provider
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
}

impl std::fmt::Debug for AnthropicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicProvider")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String, base_url: String, max_tokens: u32) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            api_key,
            base_url,
            model,
            max_tokens,
        }
    }

    /// Convert provider-agnostic messages to Anthropic wire format
    fn to_anthropic_messages(messages: &[ChatMessage]) -> Vec<AnthropicMessage> {
        messages
            .iter()
            .filter(|m| m.role != ChatRole::System)
            .map(|m| {
                let role = match m.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::System => "user",
                };
                let content = match &m.content {
                    ChatMessageContent::Text(t) => AnthropicContent::Text(t.clone()),
                    ChatMessageContent::Blocks(blocks) => {
                        let ab: Vec<AnthropicBlock> = blocks
                            .iter()
                            .map(|b| match b {
                                ChatBlock::Text { text } => AnthropicBlock::Text {
                                    text: text.clone(),
                                },
                                ChatBlock::ToolCall { id, name, input } => {
                                    AnthropicBlock::ToolUse {
                                        id: id.clone(),
                                        name: name.clone(),
                                        input: input.clone(),
                                    }
                                }
                                ChatBlock::ToolResult {
                                    tool_call_id,
                                    content,
                                } => AnthropicBlock::ToolResult {
                                    tool_use_id: tool_call_id.clone(),
                                    content: content.clone(),
                                },
                            })
                            .collect();
                        AnthropicContent::Blocks(ab)
                    }
                };
                AnthropicMessage {
                    role: role.to_string(),
                    content,
                }
            })
            .collect()
    }

    /// Convert Anthropic response to provider-agnostic format
    fn from_anthropic_response(resp: AnthropicApiResponse) -> ChatResponse {
        let blocks = resp
            .content
            .into_iter()
            .map(|b| match b {
                AnthropicBlock::Text { text } => ChatResponseBlock::Text { text },
                AnthropicBlock::ToolUse { id, name, input } => {
                    ChatResponseBlock::ToolCall { id, name, input }
                }
                AnthropicBlock::ToolResult { .. } => {
                    ChatResponseBlock::Text {
                        text: "[tool_result in response]".to_string(),
                    }
                }
            })
            .collect();

        let stop_reason = match resp.stop_reason.as_deref() {
            Some("tool_use") => StopReason::ToolUse,
            Some("end_turn") => StopReason::EndTurn,
            Some("max_tokens") => StopReason::MaxTokens,
            _ => StopReason::Unknown,
        };

        ChatResponse {
            blocks,
            stop_reason,
            usage: ChatUsage {
                input_tokens: resp.usage.input_tokens,
                output_tokens: resp.usage.output_tokens,
            },
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn provider_name(&self) -> &str {
        "anthropic"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        system: &str,
    ) -> Result<ChatResponse> {
        let url = format!("{}/v1/messages", self.base_url);
        let anthropic_messages = Self::to_anthropic_messages(messages);

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system,
            "messages": anthropic_messages,
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)?;
        }

        debug!(
            "Anthropic request: model={}, messages={}",
            self.model,
            anthropic_messages.len()
        );

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Anthropic API")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "Anthropic API request failed with status {}: {}",
                status,
                error_text
            ));
        }

        let api_response: AnthropicApiResponse = response
            .json()
            .await
            .context("Failed to parse Anthropic API response")?;

        debug!(
            "Anthropic response: blocks={}, stop_reason={:?}",
            api_response.content.len(),
            api_response.stop_reason
        );

        Ok(Self::from_anthropic_response(api_response))
    }
}

// ── Anthropic wire types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicBlock {
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

#[derive(Debug, Clone, Deserialize)]
struct AnthropicApiResponse {
    #[allow(dead_code)]
    id: String,
    content: Vec<AnthropicBlock>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_anthropic_messages_text() {
        let msgs = vec![ChatMessage {
            role: ChatRole::User,
            content: ChatMessageContent::Text("hello".to_string()),
        }];
        let result = AnthropicProvider::to_anthropic_messages(&msgs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
    }

    #[test]
    fn test_to_anthropic_messages_filters_system() {
        let msgs = vec![
            ChatMessage {
                role: ChatRole::System,
                content: ChatMessageContent::Text("system prompt".to_string()),
            },
            ChatMessage {
                role: ChatRole::User,
                content: ChatMessageContent::Text("hello".to_string()),
            },
        ];
        let result = AnthropicProvider::to_anthropic_messages(&msgs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
    }

    #[test]
    fn test_to_anthropic_messages_tool_blocks() {
        let msgs = vec![ChatMessage {
            role: ChatRole::Assistant,
            content: ChatMessageContent::Blocks(vec![ChatBlock::ToolCall {
                id: "tc_1".to_string(),
                name: "web_search".to_string(),
                input: serde_json::json!({"query": "test"}),
            }]),
        }];
        let result = AnthropicProvider::to_anthropic_messages(&msgs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "assistant");
    }

    #[test]
    fn test_from_anthropic_response_end_turn() {
        let resp = AnthropicApiResponse {
            id: "msg_1".to_string(),
            content: vec![AnthropicBlock::Text {
                text: "Hello!".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
        };
        let result = AnthropicProvider::from_anthropic_response(resp);
        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert_eq!(result.usage.input_tokens, 10);
        assert_eq!(result.blocks.len(), 1);
    }

    #[test]
    fn test_from_anthropic_response_tool_use() {
        let resp = AnthropicApiResponse {
            id: "msg_2".to_string(),
            content: vec![AnthropicBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({}),
            }],
            stop_reason: Some("tool_use".to_string()),
            usage: AnthropicUsage {
                input_tokens: 20,
                output_tokens: 15,
            },
        };
        let result = AnthropicProvider::from_anthropic_response(resp);
        assert_eq!(result.stop_reason, StopReason::ToolUse);
        assert!(matches!(&result.blocks[0], ChatResponseBlock::ToolCall { name, .. } if name == "search"));
    }

    #[test]
    fn test_anthropic_block_serialization() {
        let block = AnthropicBlock::Text {
            text: "hello".to_string(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"hello\""));
    }

    #[test]
    fn test_anthropic_provider_debug_hides_key() {
        let provider =
            AnthropicProvider::new("sk-secret".to_string(), "claude-opus-4-6".to_string(), "https://api.anthropic.com".to_string(), 4096);
        let debug = format!("{:?}", provider);
        assert!(!debug.contains("sk-secret"));
    }
}
