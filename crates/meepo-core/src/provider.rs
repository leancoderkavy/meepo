//! Provider abstraction for LLM APIs (Anthropic, Ollama, etc.)

use anyhow::Result;
use serde_json::Value;

use crate::api::{ApiClient, ToolDefinition as AnthropicToolDefinition};
use crate::ollama::{OllamaClient, ToolDefinition as OllamaToolDefinition};
use crate::tools::ToolExecutor;
use crate::usage::AccumulatedUsage;

/// Unified LLM provider enum that wraps different provider implementations
#[derive(Clone)]
pub enum LlmProvider {
    Anthropic(ApiClient),
    Ollama(OllamaClient),
}

impl LlmProvider {
    /// Run the full tool use loop until completion
    pub async fn run_tool_loop(
        &self,
        initial_message: &str,
        system: &str,
        tools: &[ToolDef],
        tool_executor: &dyn ToolExecutor,
    ) -> Result<(String, AccumulatedUsage)> {
        match self {
            LlmProvider::Anthropic(client) => {
                let anthropic_tools: Vec<AnthropicToolDefinition> = tools
                    .iter()
                    .map(|t| AnthropicToolDefinition {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        input_schema: t.input_schema.clone(),
                    })
                    .collect();
                client
                    .run_tool_loop(initial_message, system, &anthropic_tools, tool_executor)
                    .await
            }
            LlmProvider::Ollama(client) => {
                let ollama_tools: Vec<OllamaToolDefinition> = tools
                    .iter()
                    .map(|t| OllamaToolDefinition {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        input_schema: t.input_schema.clone(),
                    })
                    .collect();
                client
                    .run_tool_loop(initial_message, system, &ollama_tools, tool_executor)
                    .await
            }
        }
    }

    /// Get the model name (for usage tracking)
    pub fn model(&self) -> &str {
        match self {
            LlmProvider::Anthropic(client) => client.model(),
            LlmProvider::Ollama(client) => client.model(),
        }
    }
}

/// Tool definition shared across providers
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

