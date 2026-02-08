//! Sub-agent orchestration system
//!
//! Provides task decomposition, parallel execution, and progress reporting
//! for delegated sub-agent work.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, warn};

use crate::api::{ToolDefinition, Usage};
use crate::tools::{ToolExecutor, ToolRegistry};
use crate::types::ChannelType;

/// Execution mode for a task group
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    Parallel,
    Background,
}

/// Status of a completed sub-task
#[derive(Debug, Clone, PartialEq)]
pub enum SubTaskStatus {
    Completed,
    Failed,
    TimedOut,
}

impl std::fmt::Display for SubTaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::TimedOut => write!(f, "timed_out"),
        }
    }
}

/// A single sub-task to be delegated
#[derive(Debug, Clone)]
pub struct SubTask {
    pub task_id: String,
    pub prompt: String,
    pub context_summary: String,
    pub allowed_tools: Vec<String>,
}

/// Result from a completed sub-task
#[derive(Debug, Clone)]
pub struct SubTaskResult {
    pub task_id: String,
    pub status: SubTaskStatus,
    pub output: String,
    pub tokens_used: Usage,
}

/// Tracks a group of sub-tasks
pub struct TaskGroup {
    pub group_id: String,
    pub mode: ExecutionMode,
    pub channel: ChannelType,
    pub reply_to: Option<String>,
    pub tasks: Vec<SubTask>,
    pub created_at: DateTime<Utc>,
}

/// Configuration for the orchestrator
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    pub max_concurrent_subtasks: usize,
    pub max_subtasks_per_request: usize,
    pub parallel_timeout_secs: u64,
    pub background_timeout_secs: u64,
    pub max_background_groups: usize,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_concurrent_subtasks: 5,
            max_subtasks_per_request: 10,
            parallel_timeout_secs: 120,
            background_timeout_secs: 600,
            max_background_groups: 3,
        }
    }
}

/// Wraps a ToolRegistry but only allows execution of specific tools.
/// Implements ToolExecutor so it plugs directly into ApiClient::run_tool_loop.
pub struct FilteredToolExecutor {
    inner: Arc<ToolRegistry>,
    allowed: HashSet<String>,
}

impl FilteredToolExecutor {
    pub fn new(registry: Arc<ToolRegistry>, allowed_tools: &[String]) -> Self {
        let allowed: HashSet<String> = allowed_tools.iter().cloned().collect();
        Self {
            inner: registry,
            allowed,
        }
    }
}

#[async_trait]
impl ToolExecutor for FilteredToolExecutor {
    async fn execute(&self, tool_name: &str, input: Value) -> Result<String> {
        if !self.allowed.contains(tool_name) {
            warn!("Sub-agent attempted to use non-allowed tool: {}", tool_name);
            return Err(anyhow!("Tool '{}' is not available for this sub-agent", tool_name));
        }
        debug!("Sub-agent executing allowed tool: {}", tool_name);
        self.inner.execute(tool_name, input).await
    }

    fn list_tools(&self) -> Vec<ToolDefinition> {
        self.inner.list_tools()
            .into_iter()
            .filter(|t| self.allowed.contains(&t.name))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ToolHandler, ToolRegistry, json_schema};

    struct DummyTool {
        tool_name: String,
    }

    impl DummyTool {
        fn new(name: &str) -> Self {
            Self { tool_name: name.to_string() }
        }
    }

    #[async_trait]
    impl ToolHandler for DummyTool {
        fn name(&self) -> &str { &self.tool_name }
        fn description(&self) -> &str { "dummy" }
        fn input_schema(&self) -> Value {
            json_schema(serde_json::json!({}), vec![])
        }
        async fn execute(&self, _input: Value) -> Result<String> {
            Ok(format!("result from {}", self.tool_name))
        }
    }

    pub fn make_registry_with_tools(names: &[&str]) -> Arc<ToolRegistry> {
        let mut registry = ToolRegistry::new();
        for name in names {
            registry.register(Arc::new(DummyTool::new(name)));
        }
        Arc::new(registry)
    }

    #[test]
    fn test_execution_mode_serde() {
        let json = serde_json::to_string(&ExecutionMode::Parallel).unwrap();
        assert_eq!(json, "\"parallel\"");
        let mode: ExecutionMode = serde_json::from_str("\"background\"").unwrap();
        assert_eq!(mode, ExecutionMode::Background);
    }

    #[test]
    fn test_subtask_status_display() {
        assert_eq!(SubTaskStatus::Completed.to_string(), "completed");
        assert_eq!(SubTaskStatus::Failed.to_string(), "failed");
        assert_eq!(SubTaskStatus::TimedOut.to_string(), "timed_out");
    }

    #[test]
    fn test_orchestrator_config_default() {
        let config = OrchestratorConfig::default();
        assert_eq!(config.max_concurrent_subtasks, 5);
        assert_eq!(config.max_subtasks_per_request, 10);
        assert_eq!(config.parallel_timeout_secs, 120);
        assert_eq!(config.background_timeout_secs, 600);
        assert_eq!(config.max_background_groups, 3);
    }

    #[tokio::test]
    async fn test_filtered_executor_allows_permitted_tool() {
        let registry = make_registry_with_tools(&["read_file", "browse_url", "run_command"]);
        let filtered = FilteredToolExecutor::new(
            registry,
            &["read_file".to_string(), "browse_url".to_string()],
        );

        let result = filtered.execute("read_file", serde_json::json!({})).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "result from read_file");
    }

    #[tokio::test]
    async fn test_filtered_executor_blocks_non_permitted_tool() {
        let registry = make_registry_with_tools(&["read_file", "browse_url", "run_command"]);
        let filtered = FilteredToolExecutor::new(
            registry,
            &["read_file".to_string()],
        );

        let result = filtered.execute("run_command", serde_json::json!({})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not available"));
    }

    #[test]
    fn test_filtered_executor_list_tools_only_returns_allowed() {
        let registry = make_registry_with_tools(&["read_file", "browse_url", "run_command"]);
        let filtered = FilteredToolExecutor::new(
            registry,
            &["read_file".to_string(), "browse_url".to_string()],
        );

        let tools = filtered.list_tools();
        let names: HashSet<String> = tools.iter().map(|t| t.name.clone()).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains("read_file"));
        assert!(names.contains("browse_url"));
        assert!(!names.contains("run_command"));
    }

    #[test]
    fn test_filtered_executor_empty_allowlist() {
        let registry = make_registry_with_tools(&["read_file", "browse_url"]);
        let filtered = FilteredToolExecutor::new(registry, &[]);
        let tools = filtered.list_tools();
        assert!(tools.is_empty());
    }
}
