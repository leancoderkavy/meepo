//! Ollama API client with OpenAI-compatible endpoint

use anyhow::{Context, Result, anyhow};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::tools::ToolExecutor;
use crate::usage::AccumulatedUsage;

/// Ollama API client using OpenAI-compatible format
#[derive(Clone)]
pub struct OllamaClient {
    client: Client,
    base_url: String,
    model: String,
    max_tokens: u32,
}

impl std::fmt::Debug for OllamaClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OllamaClient")
            .field("client", &"<reqwest::Client>")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

impl OllamaClient {
    /// Create a new Ollama API client
    pub fn new(base_url: String, model: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            base_url,
            model,
            max_tokens: 4096,
        }
    }

    /// Set max tokens for responses
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Make a single chat request to Ollama API (OpenAI-compatible endpoint)
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        system: &str,
    ) -> Result<ChatResponse> {
        let url = format!("{}/v1/chat/completions", self.base_url);

        // Prepare messages with system prompt
        let mut all_messages = vec![ChatMessage {
            role: "system".to_string(),
            content: Some(system.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }];
        all_messages.extend_from_slice(messages);

        // Convert tools to OpenAI format
        let openai_tools: Vec<OpenAITool> = tools
            .iter()
            .map(|t| OpenAITool {
                tool_type: "function".to_string(),
                function: FunctionDef {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                },
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": all_messages,
            "max_tokens": self.max_tokens,
        });

        // Only include tools if there are any
        if !openai_tools.is_empty() {
            body["tools"] = serde_json::to_value(&openai_tools)?;
        }

        debug!(
            "Sending request to Ollama API with {} messages",
            all_messages.len()
        );

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Ollama API")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "API request failed with status {}: {}",
                status,
                error_text
            ));
        }

        let api_response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse Ollama API response")?;

        debug!(
            "Received response with finish_reason: {:?}",
            api_response.choices.first().map(|c| &c.finish_reason)
        );

        Ok(api_response)
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
            role: "user".to_string(),
            content: Some(initial_message.to_string()),
            tool_calls: None,
            tool_call_id: None,
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

            let response = self.chat(&conversation, tools, system).await?;

            // Accumulate token usage from this API call
            if let Some(usage) = &response.usage {
                accumulated.add(usage.prompt_tokens, usage.completion_tokens);
            }

            // Extract the first choice
            let choice = response
                .choices
                .first()
                .ok_or_else(|| anyhow!("No choices in response"))?;

            // Build assistant message from response
            let assistant_message = ChatMessage {
                role: "assistant".to_string(),
                content: choice.message.content.clone(),
                tool_calls: choice.message.tool_calls.clone(),
                tool_call_id: None,
            };
            conversation.push(assistant_message);

            match choice.finish_reason.as_deref() {
                Some("tool_calls") => {
                    debug!("Processing tool calls from response");

                    let tool_calls = choice
                        .message
                        .tool_calls
                        .as_ref()
                        .ok_or_else(|| anyhow!("finish_reason was tool_calls but no tool_calls found"))?;

                    for tool_call in tool_calls {
                        info!("Executing tool: {}", tool_call.function.name);

                        // Track tool call in accumulated usage
                        accumulated.record_tool_call(&tool_call.function.name);

                        // Parse function arguments
                        let input: Value = serde_json::from_str(&tool_call.function.arguments)
                            .context("Failed to parse tool call arguments")?;

                        let result = tool_executor.execute(&tool_call.function.name, input).await;

                        let mut result_content = match result {
                            Ok(output) => output,
                            Err(e) => {
                                warn!("Tool {} failed: {}", tool_call.function.name, e);
                                format!("Error: {}", e)
                            }
                        };

                        // Truncate oversized tool outputs to prevent context explosion
                        if result_content.len() > MAX_TOOL_OUTPUT {
                            result_content.truncate(MAX_TOOL_OUTPUT);
                            result_content.push_str("\n[Output truncated]");
                        }

                        // Add tool result to conversation
                        conversation.push(ChatMessage {
                            role: "tool".to_string(),
                            content: Some(result_content),
                            tool_calls: None,
                            tool_call_id: Some(tool_call.id.clone()),
                        });
                    }

                    // Continue loop to process next response
                }
                Some("stop") | None => {
                    debug!(
                        "Tool loop completed (iterations: {}, tokens: in={} out={})",
                        iterations, accumulated.input_tokens, accumulated.output_tokens
                    );

                    // Extract final text response
                    let final_text = choice
                        .message
                        .content
                        .as_ref()
                        .ok_or_else(|| anyhow!("No text response from assistant"))?
                        .clone();

                    if final_text.is_empty() {
                        return Err(anyhow!("Empty text response from assistant"));
                    }

                    return Ok((final_text, accumulated));
                }
                Some(other) => {
                    warn!("Unexpected finish_reason: {}", other);
                    return Err(anyhow!("Unexpected finish_reason: {}", other));
                }
            }
        }
    }

    /// Get the model name (for usage tracking)
    pub fn model(&self) -> &str {
        &self.model
    }
}

/// Message in conversation history (OpenAI format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Tool call in OpenAI format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// Function call details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Tool definition for the API (OpenAI format wrapper)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAITool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDef,
}

/// Function definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Tool definition (same as api.rs for compatibility)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Response from the Ollama API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

/// Choice in response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

/// Token usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ollama_client_creation() {
        let client = OllamaClient::new(
            "http://localhost:11434".to_string(),
            "llama3.2".to_string(),
        );
        assert_eq!(client.model, "llama3.2");
        assert_eq!(client.max_tokens, 4096);
        assert_eq!(client.base_url, "http://localhost:11434");
    }

    #[test]
    fn test_chat_message_serialization() {
        let msg = ChatMessage {
            role: "user".to_string(),
            content: Some("Hello".to_string()),
            tool_calls: None,
            tool_call_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("user"));
        assert!(json.contains("Hello"));
    }

    #[test]
    fn test_ollama_client_debug() {
        let client = OllamaClient::new(
            "http://localhost:11434".to_string(),
            "llama3.2".to_string(),
        );
        let debug_output = format!("{:?}", client);
        assert!(debug_output.contains("OllamaClient"));
        assert!(debug_output.contains("llama3.2"));
    }

    #[test]
    fn test_ollama_client_clone() {
        let client = OllamaClient::new(
            "http://localhost:11434".to_string(),
            "llama3.2".to_string(),
        );
        let cloned = client.clone();
        assert_eq!(cloned.model, "llama3.2");
        assert_eq!(cloned.max_tokens, 4096);
    }
}
