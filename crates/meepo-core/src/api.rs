//! Anthropic API client with tool use loop

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use reqwest::Client;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::tools::ToolExecutor;

/// Anthropic API client
#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Mask the API key in debug output
        let masked_key = if self.api_key.len() > 7 {
            format!("{}...{}",
                &self.api_key[..3],
                &self.api_key[self.api_key.len()-4..])
        } else {
            "***".to_string()
        };

        f.debug_struct("ApiClient")
            .field("client", &"<reqwest::Client>")
            .field("api_key", &masked_key)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

impl ApiClient {
    /// Create a new API client
    pub fn new(api_key: String, model: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            api_key,
            base_url: "https://api.anthropic.com".to_string(),
            model: model.unwrap_or_else(|| "claude-opus-4-6".to_string()),
            max_tokens: 4096,
        }
    }

    /// Set max tokens for responses
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Make a single chat request to Claude API
    pub async fn chat(
        &self,
        messages: &[ApiMessage],
        tools: &[ToolDefinition],
        system: &str,
    ) -> Result<ApiResponse> {
        let url = format!("{}/v1/messages", self.base_url);

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system,
            "messages": messages,
            "tools": tools,
        });

        debug!("Sending request to Anthropic API with {} messages", messages.len());

        let response = self.client
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
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("API request failed with status {}: {}", status, error_text));
        }

        let api_response: ApiResponse = response.json().await
            .context("Failed to parse API response")?;

        debug!("Received response with {} content blocks, stop_reason: {:?}",
               api_response.content.len(), api_response.stop_reason);

        Ok(api_response)
    }

    /// Run the full tool use loop until completion
    pub async fn run_tool_loop(
        &self,
        initial_message: &str,
        system: &str,
        tools: &[ToolDefinition],
        tool_executor: &dyn ToolExecutor,
    ) -> Result<String> {
        let mut conversation: Vec<ApiMessage> = vec![
            ApiMessage {
                role: "user".to_string(),
                content: MessageContent::Text(initial_message.to_string()),
            }
        ];

        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 10;

        loop {
            iterations += 1;
            if iterations > MAX_ITERATIONS {
                warn!("Tool loop exceeded maximum iterations ({})", MAX_ITERATIONS);
                return Err(anyhow!("Tool loop exceeded maximum iterations"));
            }

            info!("Tool loop iteration {}", iterations);

            let response = self.chat(&conversation, tools, system).await?;

            // Build assistant message from response content blocks
            let assistant_message = ApiMessage {
                role: "assistant".to_string(),
                content: MessageContent::Blocks(response.content.clone()),
            };
            conversation.push(assistant_message);

            match response.stop_reason.as_deref() {
                Some("tool_use") => {
                    debug!("Processing tool calls from response");

                    // Extract tool use blocks and execute them
                    let mut tool_results = Vec::new();

                    for block in &response.content {
                        if let ContentBlock::ToolUse { id, name, input } = block {
                            info!("Executing tool: {}", name);

                            let result = tool_executor.execute(name, input.clone()).await;

                            let result_content = match result {
                                Ok(output) => output,
                                Err(e) => {
                                    warn!("Tool {} failed: {}", name, e);
                                    format!("Error: {}", e)
                                }
                            };

                            tool_results.push(ContentBlock::ToolResult {
                                tool_use_id: id.clone(),
                                content: result_content,
                            });
                        }
                    }

                    if tool_results.is_empty() {
                        warn!("Stop reason was tool_use but no tool calls found");
                        return Err(anyhow!("Stop reason was tool_use but no tool calls found"));
                    }

                    // Add tool results to conversation
                    conversation.push(ApiMessage {
                        role: "user".to_string(),
                        content: MessageContent::Blocks(tool_results),
                    });

                    // Continue loop to process next response
                }
                Some("end_turn") | None => {
                    debug!("Tool loop completed");

                    // Extract final text response
                    let mut final_text = String::new();
                    for block in &response.content {
                        if let ContentBlock::Text { text } = block {
                            if !final_text.is_empty() {
                                final_text.push('\n');
                            }
                            final_text.push_str(text);
                        }
                    }

                    if final_text.is_empty() {
                        return Err(anyhow!("No text response from assistant"));
                    }

                    return Ok(final_text);
                }
                Some(other) => {
                    warn!("Unexpected stop_reason: {}", other);
                    return Err(anyhow!("Unexpected stop_reason: {}", other));
                }
            }
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
        assert_eq!(client.model, "claude-opus-4-6");
        assert_eq!(client.max_tokens, 4096);
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
    fn test_api_client_debug_masks_key() {
        let client = ApiClient::new("sk-ant-1234567890abcdef".to_string(), None);
        let debug_output = format!("{:?}", client);

        // Should contain masked version
        assert!(debug_output.contains("sk-...cdef"));

        // Should NOT contain the full key
        assert!(!debug_output.contains("sk-ant-1234567890abcdef"));
    }

    #[test]
    fn test_api_client_debug_masks_short_key() {
        let client = ApiClient::new("short".to_string(), None);
        let debug_output = format!("{:?}", client);

        // Should mask short keys as ***
        assert!(debug_output.contains("***"));
        assert!(!debug_output.contains("short"));
    }

    #[test]
    fn test_api_client_clone() {
        let client = ApiClient::new("test-key".to_string(), None);
        let cloned = client.clone();
        assert_eq!(cloned.model, "claude-opus-4-6");
        assert_eq!(cloned.max_tokens, 4096);
    }
}
