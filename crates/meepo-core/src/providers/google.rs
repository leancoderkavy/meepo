//! Google Gemini provider

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

/// Google Gemini provider
pub struct GoogleProvider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: u32,
}

impl std::fmt::Debug for GoogleProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleProvider")
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

impl GoogleProvider {
    pub fn new(api_key: String, model: String, max_tokens: u32) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            api_key,
            model,
            max_tokens,
        }
    }

    /// Convert provider-agnostic messages to Gemini wire format
    fn to_gemini_contents(messages: &[ChatMessage]) -> Vec<GeminiContent> {
        messages
            .iter()
            .filter(|m| m.role != ChatRole::System)
            .map(|m| {
                let role = match m.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "model",
                    ChatRole::System => "user",
                };
                let parts = match &m.content {
                    ChatMessageContent::Text(t) => {
                        vec![GeminiPart::Text { text: t.clone() }]
                    }
                    ChatMessageContent::Blocks(blocks) => blocks
                        .iter()
                        .map(|b| match b {
                            ChatBlock::Text { text } => GeminiPart::Text { text: text.clone() },
                            ChatBlock::ToolCall { name, input, .. } => {
                                GeminiPart::FunctionCall {
                                    function_call: GeminiFunctionCall {
                                        name: name.clone(),
                                        args: input.clone(),
                                    },
                                }
                            }
                            ChatBlock::ToolResult { content, tool_call_id } => {
                                GeminiPart::FunctionResponse {
                                    function_response: GeminiFunctionResponse {
                                        name: tool_call_id.clone(),
                                        response: serde_json::json!({"result": content}),
                                    },
                                }
                            }
                        })
                        .collect(),
                };
                GeminiContent {
                    role: role.to_string(),
                    parts,
                }
            })
            .collect()
    }

    /// Convert tool definitions to Gemini function declarations
    fn to_gemini_tools(tools: &[ToolDefinition]) -> Vec<GeminiToolDecl> {
        if tools.is_empty() {
            return vec![];
        }
        vec![GeminiToolDecl {
            function_declarations: tools
                .iter()
                .map(|t| GeminiFunctionDeclaration {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                })
                .collect(),
        }]
    }

    /// Convert Gemini response to provider-agnostic format
    fn from_gemini_response(resp: GeminiApiResponse) -> Result<ChatResponse> {
        let candidate = resp
            .candidates
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("Gemini response had no candidates"))?;

        let mut blocks = Vec::new();
        let mut has_tool_calls = false;

        for part in candidate.content.parts {
            match part {
                GeminiPart::Text { text } => {
                    blocks.push(ChatResponseBlock::Text { text });
                }
                GeminiPart::FunctionCall { function_call } => {
                    has_tool_calls = true;
                    blocks.push(ChatResponseBlock::ToolCall {
                        id: format!("gemini_{}", function_call.name),
                        name: function_call.name,
                        input: function_call.args,
                    });
                }
                GeminiPart::FunctionResponse { .. } => {}
            }
        }

        let stop_reason = if has_tool_calls {
            StopReason::ToolUse
        } else {
            match candidate.finish_reason.as_deref() {
                Some("STOP") => StopReason::EndTurn,
                Some("MAX_TOKENS") => StopReason::MaxTokens,
                _ => StopReason::EndTurn,
            }
        };

        let usage = resp
            .usage_metadata
            .map_or(ChatUsage::default(), |u| ChatUsage {
                input_tokens: u.prompt_token_count.unwrap_or(0),
                output_tokens: u.candidates_token_count.unwrap_or(0),
            });

        Ok(ChatResponse {
            blocks,
            stop_reason,
            usage,
        })
    }
}

#[async_trait]
impl LlmProvider for GoogleProvider {
    fn provider_name(&self) -> &str {
        "google"
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
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let contents = Self::to_gemini_contents(messages);

        let mut body = serde_json::json!({
            "contents": contents,
            "systemInstruction": {
                "parts": [{"text": system}]
            },
            "generationConfig": {
                "maxOutputTokens": self.max_tokens,
            },
        });

        let gemini_tools = Self::to_gemini_tools(tools);
        if !gemini_tools.is_empty() {
            body["tools"] = serde_json::to_value(&gemini_tools)?;
        }

        debug!(
            "Gemini request: model={}, contents={}",
            self.model,
            contents.len()
        );

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Gemini API")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "Gemini API request failed with status {}: {}",
                status,
                error_text
            ));
        }

        let api_response: GeminiApiResponse = response
            .json()
            .await
            .context("Failed to parse Gemini API response")?;

        debug!(
            "Gemini response: candidates={}",
            api_response.candidates.len()
        );

        Self::from_gemini_response(api_response)
    }
}

// ── Gemini wire types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum GeminiPart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiFunctionResponse {
    name: String,
    response: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiToolDecl {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct GeminiApiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_gemini_contents_text() {
        let msgs = vec![ChatMessage {
            role: ChatRole::User,
            content: ChatMessageContent::Text("hello".to_string()),
        }];
        let result = GoogleProvider::to_gemini_contents(&msgs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, "user");
    }

    #[test]
    fn test_to_gemini_contents_assistant_is_model() {
        let msgs = vec![ChatMessage {
            role: ChatRole::Assistant,
            content: ChatMessageContent::Text("hi".to_string()),
        }];
        let result = GoogleProvider::to_gemini_contents(&msgs);
        assert_eq!(result[0].role, "model");
    }

    #[test]
    fn test_to_gemini_contents_filters_system() {
        let msgs = vec![
            ChatMessage {
                role: ChatRole::System,
                content: ChatMessageContent::Text("sys".to_string()),
            },
            ChatMessage {
                role: ChatRole::User,
                content: ChatMessageContent::Text("hello".to_string()),
            },
        ];
        let result = GoogleProvider::to_gemini_contents(&msgs);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_to_gemini_tools() {
        let tools = vec![ToolDefinition {
            name: "search".to_string(),
            description: "Search".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        }];
        let result = GoogleProvider::to_gemini_tools(&tools);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].function_declarations.len(), 1);
    }

    #[test]
    fn test_to_gemini_tools_empty() {
        let result = GoogleProvider::to_gemini_tools(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_from_gemini_response_text() {
        let resp = GeminiApiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::Text {
                        text: "Hello!".to_string(),
                    }],
                },
                finish_reason: Some("STOP".to_string()),
            }],
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: Some(10),
                candidates_token_count: Some(5),
            }),
        };
        let result = GoogleProvider::from_gemini_response(resp).unwrap();
        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert_eq!(result.usage.input_tokens, 10);
    }

    #[test]
    fn test_from_gemini_response_function_call() {
        let resp = GeminiApiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::FunctionCall {
                        function_call: GeminiFunctionCall {
                            name: "search".to_string(),
                            args: serde_json::json!({"q": "test"}),
                        },
                    }],
                },
                finish_reason: Some("STOP".to_string()),
            }],
            usage_metadata: None,
        };
        let result = GoogleProvider::from_gemini_response(resp).unwrap();
        assert_eq!(result.stop_reason, StopReason::ToolUse);
        assert!(matches!(&result.blocks[0], ChatResponseBlock::ToolCall { name, .. } if name == "search"));
    }

    #[test]
    fn test_from_gemini_response_no_candidates() {
        let resp = GeminiApiResponse {
            candidates: vec![],
            usage_metadata: None,
        };
        assert!(GoogleProvider::from_gemini_response(resp).is_err());
    }

    #[test]
    fn test_google_provider_debug_hides_key() {
        let provider = GoogleProvider::new("AIza-secret".to_string(), "gemini-2.0-flash".to_string(), 4096);
        let debug = format!("{:?}", provider);
        assert!(!debug.contains("AIza-secret"));
    }
}
