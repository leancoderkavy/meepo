//! Sub-agent orchestration system
//!
//! Provides task decomposition, parallel execution, and progress reporting
//! for delegated sub-agent work.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, Semaphore};
use tracing::{debug, warn};

use crate::api::{ApiClient, ToolDefinition, Usage};
use crate::tools::{ToolExecutor, ToolRegistry};
use crate::types::{ChannelType, MessageKind, OutgoingMessage};

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

/// The task orchestrator that runs sub-agent task groups.
pub struct TaskOrchestrator {
    api: ApiClient,
    progress_tx: mpsc::Sender<OutgoingMessage>,
    config: OrchestratorConfig,
    active_background_groups: Arc<AtomicUsize>,
}

impl TaskOrchestrator {
    pub fn new(
        api: ApiClient,
        progress_tx: mpsc::Sender<OutgoingMessage>,
        config: OrchestratorConfig,
    ) -> Self {
        Self {
            api,
            progress_tx,
            config,
            active_background_groups: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Execute a single sub-task in isolation. Returns the result.
    async fn run_subtask(
        api: ApiClient,
        registry: Arc<ToolRegistry>,
        task: SubTask,
        timeout_secs: u64,
    ) -> SubTaskResult {
        let system_prompt = format!(
            "You are a focused sub-agent working on a specific task.\n\n\
             ## Context\n{}\n\n\
             ## Your Task\n{}\n\n\
             Respond with your findings/results directly. Be concise.",
            task.context_summary, task.prompt
        );

        let filtered = FilteredToolExecutor::new(registry, &task.allowed_tools);
        let tool_defs = filtered.list_tools();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            api.run_tool_loop(&task.prompt, &system_prompt, &tool_defs, &filtered),
        )
        .await;

        match result {
            Ok(Ok(output)) => SubTaskResult {
                task_id: task.task_id,
                status: SubTaskStatus::Completed,
                output,
                tokens_used: Usage { input_tokens: 0, output_tokens: 0 },
            },
            Ok(Err(e)) => SubTaskResult {
                task_id: task.task_id,
                status: SubTaskStatus::Failed,
                output: format!("Error: {}", e),
                tokens_used: Usage { input_tokens: 0, output_tokens: 0 },
            },
            Err(_) => SubTaskResult {
                task_id: task.task_id,
                status: SubTaskStatus::TimedOut,
                output: "Sub-task timed out".to_string(),
                tokens_used: Usage { input_tokens: 0, output_tokens: 0 },
            },
        }
    }

    /// Format results into a readable markdown string.
    pub fn format_results(results: &[SubTaskResult]) -> String {
        let mut output = String::from("## Results\n\n");
        for result in results {
            output.push_str(&format!("### {} ({})\n", result.task_id, result.status));
            output.push_str(&result.output);
            output.push_str("\n\n");
        }
        output
    }

    /// Send a progress message to the originating channel.
    async fn send_progress(&self, channel: &ChannelType, reply_to: &Option<String>, message: &str) {
        let msg = OutgoingMessage {
            content: message.to_string(),
            channel: channel.clone(),
            reply_to: reply_to.clone(),
            kind: MessageKind::Response,
        };
        if let Err(e) = self.progress_tx.send(msg).await {
            warn!("Failed to send progress message: {}", e);
        }
    }

    /// Execute a task group in parallel mode.
    /// Blocks until all sub-tasks complete and returns combined results.
    pub async fn run_parallel(&self, group: TaskGroup, registry: Arc<ToolRegistry>) -> Result<String> {
        let task_count = group.tasks.len();

        if task_count > self.config.max_subtasks_per_request {
            return Err(anyhow!(
                "Too many sub-tasks: {} (max {})",
                task_count, self.config.max_subtasks_per_request,
            ));
        }

        self.send_progress(
            &group.channel, &group.reply_to,
            &format!("Working on {} tasks...", task_count),
        ).await;

        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent_subtasks));
        let mut handles = Vec::new();
        for task in group.tasks {
            let api = self.api.clone();
            let reg = registry.clone();
            let sem = semaphore.clone();
            let timeout_secs = self.config.parallel_timeout_secs;
            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore closed");
                Self::run_subtask(api, reg, task, timeout_secs).await
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(SubTaskResult {
                    task_id: "unknown".to_string(),
                    status: SubTaskStatus::Failed,
                    output: format!("Task panicked: {}", e),
                    tokens_used: Usage { input_tokens: 0, output_tokens: 0 },
                }),
            }
        }

        Ok(Self::format_results(&results))
    }

    /// Execute a task group in background mode.
    /// Returns immediately with a confirmation. Progress sent via channel.
    pub async fn run_background(&self, group: TaskGroup, registry: Arc<ToolRegistry>) -> Result<String> {
        let task_count = group.tasks.len();

        if task_count > self.config.max_subtasks_per_request {
            return Err(anyhow!(
                "Too many sub-tasks: {} (max {})",
                task_count, self.config.max_subtasks_per_request,
            ));
        }

        // Atomically claim a background group slot using CAS loop
        loop {
            let current = self.active_background_groups.load(Ordering::SeqCst);
            if current >= self.config.max_background_groups {
                return Err(anyhow!(
                    "Too many background task groups running: {} (max {})",
                    current, self.config.max_background_groups,
                ));
            }
            if self.active_background_groups
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
        }
        let active_counter = self.active_background_groups.clone();

        let group_id = group.group_id.clone();
        let channel = group.channel.clone();
        let reply_to = group.reply_to.clone();
        let api = self.api.clone();
        let progress_tx = self.progress_tx.clone();
        let timeout_secs = self.config.background_timeout_secs;
        let max_concurrent = self.config.max_concurrent_subtasks;

        tokio::spawn(async move {
            let _ = progress_tx.send(OutgoingMessage {
                content: format!("Started {} background tasks...", task_count),
                channel: channel.clone(),
                reply_to: reply_to.clone(),
                kind: MessageKind::Response,
            }).await;

            let semaphore = Arc::new(Semaphore::new(max_concurrent));
            let mut handles = Vec::new();
            for task in group.tasks {
                let api = api.clone();
                let reg = registry.clone();
                let sem = semaphore.clone();
                handles.push(tokio::spawn(async move {
                    let _permit = sem.acquire().await.expect("semaphore closed");
                    Self::run_subtask(api, reg, task, timeout_secs).await
                }));
            }

            let mut results = Vec::new();
            for handle in handles {
                match handle.await {
                    Ok(result) => {
                        let update = format!(
                            "Task '{}' {} ({}/{})",
                            result.task_id, result.status,
                            results.len() + 1, task_count,
                        );
                        let _ = progress_tx.send(OutgoingMessage {
                            content: update,
                            channel: channel.clone(),
                            reply_to: reply_to.clone(),
                            kind: MessageKind::Response,
                        }).await;
                        results.push(result);
                    }
                    Err(e) => {
                        let _ = progress_tx.send(OutgoingMessage {
                            content: format!("A background task panicked: {}", e),
                            channel: channel.clone(),
                            reply_to: reply_to.clone(),
                            kind: MessageKind::Response,
                        }).await;
                        results.push(SubTaskResult {
                            task_id: "unknown".to_string(),
                            status: SubTaskStatus::Failed,
                            output: format!("Task panicked: {}", e),
                            tokens_used: Usage { input_tokens: 0, output_tokens: 0 },
                        });
                    }
                }
            }

            let summary = Self::format_results(&results);
            let _ = progress_tx.send(OutgoingMessage {
                content: format!("All background tasks complete:\n\n{}", summary),
                channel: channel.clone(),
                reply_to: reply_to.clone(),
                kind: MessageKind::Response,
            }).await;

            active_counter.fetch_sub(1, Ordering::SeqCst);
        });

        Ok(format!(
            "Started task group {} with {} tasks. The user will be notified on the original channel as tasks complete.",
            group_id, task_count
        ))
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

    use crate::api::ApiClient;

    fn make_orchestrator(
    ) -> (TaskOrchestrator, mpsc::Receiver<OutgoingMessage>) {
        let api = ApiClient::new("test-key".to_string(), None);
        let (tx, rx) = mpsc::channel(100);
        let config = OrchestratorConfig::default();
        (TaskOrchestrator::new(api, tx, config), rx)
    }

    #[test]
    fn test_format_results() {
        let results = vec![
            SubTaskResult {
                task_id: "task_a".to_string(),
                status: SubTaskStatus::Completed,
                output: "Found 3 items".to_string(),
                tokens_used: Usage { input_tokens: 0, output_tokens: 0 },
            },
            SubTaskResult {
                task_id: "task_b".to_string(),
                status: SubTaskStatus::Failed,
                output: "Error: timeout".to_string(),
                tokens_used: Usage { input_tokens: 0, output_tokens: 0 },
            },
        ];
        let formatted = TaskOrchestrator::format_results(&results);
        assert!(formatted.contains("### task_a (completed)"));
        assert!(formatted.contains("Found 3 items"));
        assert!(formatted.contains("### task_b (failed)"));
    }

    #[tokio::test]
    async fn test_parallel_rejects_too_many_tasks() {
        let (orchestrator, _rx) = make_orchestrator();
        let registry = make_registry_with_tools(&["read_file"]);

        let tasks: Vec<SubTask> = (0..11)
            .map(|i| SubTask {
                task_id: format!("task_{}", i),
                prompt: "do something".to_string(),
                context_summary: String::new(),
                allowed_tools: vec!["read_file".to_string()],
            })
            .collect();

        let group = TaskGroup {
            group_id: "test-group".to_string(),
            mode: ExecutionMode::Parallel,
            channel: ChannelType::Internal,
            reply_to: None,
            tasks,
            created_at: Utc::now(),
        };

        let result = orchestrator.run_parallel(group, registry).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Too many sub-tasks"));
    }

    #[tokio::test]
    async fn test_background_rejects_too_many_groups() {
        let api = ApiClient::new("test-key".to_string(), None);
        let (tx, _rx) = mpsc::channel(100);
        let config = OrchestratorConfig {
            max_background_groups: 0,
            ..Default::default()
        };
        let orchestrator = TaskOrchestrator::new(api, tx, config);
        let registry = make_registry_with_tools(&[]);

        let group = TaskGroup {
            group_id: "test-group".to_string(),
            mode: ExecutionMode::Background,
            channel: ChannelType::Internal,
            reply_to: None,
            tasks: vec![SubTask {
                task_id: "t1".to_string(),
                prompt: "test".to_string(),
                context_summary: String::new(),
                allowed_tools: vec![],
            }],
            created_at: Utc::now(),
        };

        let result = orchestrator.run_background(group, registry).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Too many background"));
    }

    #[tokio::test]
    async fn test_send_progress() {
        let (orchestrator, mut rx) = make_orchestrator();

        orchestrator.send_progress(
            &ChannelType::Discord, &None, "Working on 3 tasks...",
        ).await;

        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.content, "Working on 3 tasks...");
        assert_eq!(msg.channel, ChannelType::Discord);
    }
}
