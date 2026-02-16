//! OpenAI provider (GPT-4o, o3, etc.)

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

/// OpenAI provider
pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
}

impl std::fmt::Debug for OpenAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiProvider")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

impl OpenAiProvider {
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

    /// Convert provider-agnostic messages to OpenAI wire format
    fn to_openai_messages(messages: &[ChatMessage], system: &str) -> Vec<OpenAiMessage> {
        let mut result = vec![OpenAiMessage {
            role: "system".to_string(),
            content: Some(system.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }];

        for msg in messages {
            match (&msg.role, &msg.content) {
                (ChatRole::System, _) => {
                    // Already handled above
                }
                (role, ChatMessageContent::Text(text)) => {
                    result.push(OpenAiMessage {
                        role: role.to_string(),
                        content: Some(text.clone()),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                }
                (ChatRole::Assistant, ChatMessageContent::Blocks(blocks)) => {
                    let mut text_parts = Vec::new();
                    let mut tool_calls = Vec::new();

                    for block in blocks {
                        match block {
                            ChatBlock::Text { text } => text_parts.push(text.clone()),
                            ChatBlock::ToolCall { id, name, input } => {
                                tool_calls.push(OpenAiToolCall {
                                    id: id.clone(),
                                    r#type: "function".to_string(),
                                    function: OpenAiFunction {
                                        name: name.clone(),
                                        arguments: serde_json::to_string(input)
                                            .unwrap_or_default(),
                                    },
                                });
                            }
                            ChatBlock::ToolResult { .. } => {}
                        }
                    }

                    let content = if text_parts.is_empty() {
                        None
                    } else {
                        Some(text_parts.join("\n"))
                    };

                    result.push(OpenAiMessage {
                        role: "assistant".to_string(),
                        content,
                        tool_calls: if tool_calls.is_empty() {
                            None
                        } else {
                            Some(tool_calls)
                        },
                        tool_call_id: None,
                    });
                }
                (ChatRole::User, ChatMessageContent::Blocks(blocks)) => {
                    // Tool results come as separate "tool" role messages in OpenAI
                    let mut text_parts = Vec::new();

                    for block in blocks {
                        match block {
                            ChatBlock::Text { text } => text_parts.push(text.clone()),
                            ChatBlock::ToolResult {
                                tool_call_id,
                                content,
                            } => {
                                result.push(OpenAiMessage {
                                    role: "tool".to_string(),
                                    content: Some(content.clone()),
                                    tool_calls: None,
                                    tool_call_id: Some(tool_call_id.clone()),
                                });
                            }
                            ChatBlock::ToolCall { .. } => {}
                        }
                    }

                    if !text_parts.is_empty() {
                        result.push(OpenAiMessage {
                            role: "user".to_string(),
                            content: Some(text_parts.join("\n")),
                            tool_calls: None,
                            tool_call_id: None,
                        });
                    }
                }
            }
        }

        result
    }

    /// Convert tool definitions to OpenAI function format
    fn to_openai_tools(tools: &[ToolDefinition]) -> Vec<OpenAiToolDef> {
        tools
            .iter()
            .map(|t| OpenAiToolDef {
                r#type: "function".to_string(),
                function: OpenAiToolFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                },
            })
            .collect()
    }

    /// Convert OpenAI response to provider-agnostic format
    fn from_openai_response(resp: OpenAiApiResponse) -> Result<ChatResponse> {
        let choice = resp
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("OpenAI response had no choices"))?;

        let mut blocks = Vec::new();

        if let Some(content) = choice.message.content {
            if !content.is_empty() {
                blocks.push(ChatResponseBlock::Text { text: content });
            }
        }

        if let Some(tool_calls) = choice.message.tool_calls {
            for tc in tool_calls {
                let input: Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Object(
                        serde_json::Map::new(),
                    ));
                blocks.push(ChatResponseBlock::ToolCall {
                    id: tc.id,
                    name: tc.function.name,
                    input,
                });
            }
        }

        let stop_reason = match choice.finish_reason.as_deref() {
            Some("tool_calls") => StopReason::ToolUse,
            Some("stop") => StopReason::EndTurn,
            Some("length") => StopReason::MaxTokens,
            _ => StopReason::Unknown,
        };

        let usage = resp.usage.map_or(ChatUsage::default(), |u| ChatUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        });

        Ok(ChatResponse {
            blocks,
            stop_reason,
            usage,
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn provider_name(&self) -> &str {
        "openai"
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
        let url = format!("{}/v1/chat/completions", self.base_url);
        let openai_messages = Self::to_openai_messages(messages, system);

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": openai_messages,
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(Self::to_openai_tools(tools))?;
        }

        debug!(
            "OpenAI request: model={}, messages={}",
            self.model,
            openai_messages.len()
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to OpenAI API")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "OpenAI API request failed with status {}: {}",
                status,
                error_text
            ));
        }

        let api_response: OpenAiApiResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI API response")?;

        debug!(
            "OpenAI response: choices={}, finish_reason={:?}",
            api_response.choices.len(),
            api_response.choices.first().map(|c| &c.finish_reason)
        );

        Self::from_openai_response(api_response)
    }
}

// ── OpenAI wire types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiToolCall {
    id: String,
    r#type: String,
    function: OpenAiFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiToolDef {
    r#type: String,
    function: OpenAiToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiToolFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiApiResponse {
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiChoice {
    message: OpenAiChoiceMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiChoiceMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_openai_messages_simple() {
        let msgs = vec![ChatMessage {
            role: ChatRole::User,
            content: ChatMessageContent::Text("hello".to_string()),
        }];
        let result = OpenAiProvider::to_openai_messages(&msgs, "You are helpful.");
        // system + user = 2
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, "system");
        assert_eq!(result[0].content.as_deref(), Some("You are helpful."));
        assert_eq!(result[1].role, "user");
        assert_eq!(result[1].content.as_deref(), Some("hello"));
    }

    #[test]
    fn test_to_openai_messages_with_tool_calls() {
        let msgs = vec![
            ChatMessage {
                role: ChatRole::User,
                content: ChatMessageContent::Text("search for rust".to_string()),
            },
            ChatMessage {
                role: ChatRole::Assistant,
                content: ChatMessageContent::Blocks(vec![ChatBlock::ToolCall {
                    id: "tc_1".to_string(),
                    name: "web_search".to_string(),
                    input: serde_json::json!({"query": "rust"}),
                }]),
            },
            ChatMessage {
                role: ChatRole::User,
                content: ChatMessageContent::Blocks(vec![ChatBlock::ToolResult {
                    tool_call_id: "tc_1".to_string(),
                    content: "Rust is a programming language".to_string(),
                }]),
            },
        ];
        let result = OpenAiProvider::to_openai_messages(&msgs, "sys");
        // system + user + assistant(tool_call) + tool(result) = 4
        assert_eq!(result.len(), 4);
        assert_eq!(result[2].role, "assistant");
        assert!(result[2].tool_calls.is_some());
        assert_eq!(result[3].role, "tool");
        assert_eq!(result[3].tool_call_id.as_deref(), Some("tc_1"));
    }

    #[test]
    fn test_to_openai_tools() {
        let tools = vec![ToolDefinition {
            name: "search".to_string(),
            description: "Search the web".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        }];
        let result = OpenAiProvider::to_openai_tools(&tools);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].r#type, "function");
        assert_eq!(result[0].function.name, "search");
    }

    #[test]
    fn test_from_openai_response_text() {
        let resp = OpenAiApiResponse {
            choices: vec![OpenAiChoice {
                message: OpenAiChoiceMessage {
                    content: Some("Hello!".to_string()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(OpenAiUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
            }),
        };
        let result = OpenAiProvider::from_openai_response(resp).unwrap();
        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert_eq!(result.usage.input_tokens, 10);
        assert_eq!(result.blocks.len(), 1);
    }

    #[test]
    fn test_from_openai_response_tool_calls() {
        let resp = OpenAiApiResponse {
            choices: vec![OpenAiChoice {
                message: OpenAiChoiceMessage {
                    content: None,
                    tool_calls: Some(vec![OpenAiToolCall {
                        id: "call_1".to_string(),
                        r#type: "function".to_string(),
                        function: OpenAiFunction {
                            name: "search".to_string(),
                            arguments: r#"{"q":"test"}"#.to_string(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: Some(OpenAiUsage {
                prompt_tokens: 20,
                completion_tokens: 10,
            }),
        };
        let result = OpenAiProvider::from_openai_response(resp).unwrap();
        assert_eq!(result.stop_reason, StopReason::ToolUse);
        assert!(matches!(&result.blocks[0], ChatResponseBlock::ToolCall { name, .. } if name == "search"));
    }

    #[test]
    fn test_from_openai_response_no_choices() {
        let resp = OpenAiApiResponse {
            choices: vec![],
            usage: None,
        };
        assert!(OpenAiProvider::from_openai_response(resp).is_err());
    }

    #[test]
    fn test_openai_provider_debug_hides_key() {
        let provider = OpenAiProvider::new(
            "sk-secret-key".to_string(),
            "gpt-4o".to_string(),
            "https://api.openai.com".to_string(),
            4096,
        );
        let debug = format!("{:?}", provider);
        assert!(!debug.contains("sk-secret-key"));
    }
}
