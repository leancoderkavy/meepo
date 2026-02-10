//! Tool registry and executor system

use async_trait::async_trait;
use serde_json::Value;
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, warn};

use crate::api::ToolDefinition;

pub mod macos;
pub mod accessibility;
pub mod code;
pub mod memory;
pub mod watchers;
pub mod delegate;
pub mod system;
pub mod search;
pub mod filesystem;
pub mod autonomous;

/// Trait for executing tools
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, tool_name: &str, input: Value) -> Result<String>;
    fn list_tools(&self) -> Vec<ToolDefinition>;
}

/// Individual tool handler
#[async_trait]
pub trait ToolHandler: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;
    async fn execute(&self, input: Value) -> Result<String>;
}

/// Registry of available tools
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ToolHandler>>,
}

impl ToolRegistry {
    /// Create a new empty tool registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool handler
    pub fn register(&mut self, handler: Arc<dyn ToolHandler>) {
        let name = handler.name().to_string();
        debug!("Registering tool: {}", name);
        self.tools.insert(name, handler);
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn ToolHandler>> {
        self.tools.get(name).cloned()
    }

    /// Number of registered tools
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Get tool definitions for only the named tools
    pub fn filter_tools(&self, names: &[String]) -> Vec<ToolDefinition> {
        names.iter()
            .filter_map(|name| self.tools.get(name))
            .map(|handler| ToolDefinition {
                name: handler.name().to_string(),
                description: handler.description().to_string(),
                input_schema: handler.input_schema(),
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolExecutor for ToolRegistry {
    async fn execute(&self, tool_name: &str, input: Value) -> Result<String> {
        debug!("Executing tool: {} with input: {:?}", tool_name, input);

        let handler = self.tools.get(tool_name)
            .ok_or_else(|| anyhow!("Unknown tool: {}", tool_name))?;

        match handler.execute(input).await {
            Ok(result) => {
                debug!("Tool {} succeeded", tool_name);
                Ok(result)
            }
            Err(e) => {
                warn!("Tool {} failed: {}", tool_name, e);
                Err(e)
            }
        }
    }

    fn list_tools(&self) -> Vec<ToolDefinition> {
        self.tools.values()
            .map(|handler| ToolDefinition {
                name: handler.name().to_string(),
                description: handler.description().to_string(),
                input_schema: handler.input_schema(),
            })
            .collect()
    }
}

/// Helper function to create a JSON schema for tool input
pub fn json_schema(properties: Value, required: Vec<&str>) -> Value {
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyTool;

    #[async_trait]
    impl ToolHandler for DummyTool {
        fn name(&self) -> &str {
            "dummy"
        }

        fn description(&self) -> &str {
            "A dummy tool for testing"
        }

        fn input_schema(&self) -> Value {
            json_schema(
                serde_json::json!({
                    "message": {
                        "type": "string",
                        "description": "Test message"
                    }
                }),
                vec!["message"],
            )
        }

        async fn execute(&self, _input: Value) -> Result<String> {
            Ok("dummy result".to_string())
        }
    }

    #[tokio::test]
    async fn test_tool_registry() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(DummyTool));

        assert_eq!(registry.len(), 1);

        let result = registry.execute("dummy", serde_json::json!({"message": "test"})).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "dummy result");
    }

    #[tokio::test]
    async fn test_unknown_tool() {
        let registry = ToolRegistry::new();
        let result = registry.execute("nonexistent", serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_filter_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(DummyTool));

        let filtered = registry.filter_tools(&["dummy".to_string()]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "dummy");

        let filtered_empty = registry.filter_tools(&["nonexistent".to_string()]);
        assert!(filtered_empty.is_empty());
    }
}
