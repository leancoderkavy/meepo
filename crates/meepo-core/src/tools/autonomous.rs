//! Tools for autonomous agent management — spawn tasks, view status, stop anything

use async_trait::async_trait;
use serde_json::Value;
use anyhow::{Result, Context};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::debug;

use meepo_knowledge::KnowledgeDb;
use super::{ToolHandler, json_schema};

/// Commands for background task management
#[derive(Debug, Clone)]
pub enum BackgroundTaskCommand {
    Spawn {
        id: String,
        description: String,
        reply_channel: String,
    },
    Cancel {
        id: String,
    },
}

// ─── spawn_background_task ──────────────────────────────────────────

/// Tool that lets the agent spawn autonomous sub-agents for background work
pub struct SpawnBackgroundTaskTool {
    db: Arc<KnowledgeDb>,
    command_tx: mpsc::Sender<BackgroundTaskCommand>,
}

impl SpawnBackgroundTaskTool {
    pub fn new(db: Arc<KnowledgeDb>, command_tx: mpsc::Sender<BackgroundTaskCommand>) -> Self {
        Self { db, command_tx }
    }
}

#[async_trait]
impl ToolHandler for SpawnBackgroundTaskTool {
    fn name(&self) -> &str {
        "spawn_background_task"
    }

    fn description(&self) -> &str {
        "Spawn an autonomous background task (sub-agent) to work on something independently. \
         The task runs in the background and results are reported to the specified channel when done."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "description": {
                    "type": "string",
                    "description": "What the background task should accomplish"
                },
                "reply_channel": {
                    "type": "string",
                    "description": "Channel to report results to (e.g., 'discord', 'slack', 'imessage'). Defaults to 'internal'."
                }
            }),
            vec!["description"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let description = input.get("description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'description' parameter"))?;
        let reply_channel = input.get("reply_channel")
            .and_then(|v| v.as_str())
            .unwrap_or("internal");

        if description.len() > 10_000 {
            return Err(anyhow::anyhow!("Description too long ({} chars, max 10,000)", description.len()));
        }

        let task_id = format!("t-{}", uuid::Uuid::new_v4());

        debug!("Spawning background task {}: {}", task_id, description);

        // Store in database
        self.db.insert_background_task(&task_id, description, reply_channel, "agent").await
            .context("Failed to create background task in database")?;

        // Send spawn command to main loop
        self.command_tx.send(BackgroundTaskCommand::Spawn {
            id: task_id.clone(),
            description: description.to_string(),
            reply_channel: reply_channel.to_string(),
        })
        .await
        .context("Failed to send background task command")?;

        Ok(format!("Spawned background task [{}]: {}", task_id, description))
    }
}

// ─── agent_status ───────────────────────────────────────────────────

/// Unified view of everything the agent is managing autonomously
pub struct AgentStatusTool {
    db: Arc<KnowledgeDb>,
}

impl AgentStatusTool {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ToolHandler for AgentStatusTool {
    fn name(&self) -> &str {
        "agent_status"
    }

    fn description(&self) -> &str {
        "Show everything the agent is currently managing: active watchers, running background tasks, \
         and recently completed tasks. Use this when the user asks 'what are you doing?' or \
         'what are you watching?'"
    }

    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({}), vec![])
    }

    async fn execute(&self, _input: Value) -> Result<String> {
        let mut output = String::new();

        // Active watchers
        let watchers = self.db.get_active_watchers().await
            .context("Failed to get active watchers")?;

        if watchers.is_empty() {
            output.push_str("## Active Watchers\nNone\n\n");
        } else {
            output.push_str(&format!("## Active Watchers ({})\n", watchers.len()));
            for w in &watchers {
                let age = format_age(w.created_at);
                output.push_str(&format!(
                    "- [{}] {} → {} ({})\n  Action: {}\n",
                    w.id, w.kind, w.reply_channel, age, w.action
                ));
            }
            output.push('\n');
        }

        // Running background tasks
        let tasks = self.db.get_active_background_tasks().await
            .context("Failed to get active background tasks")?;

        if tasks.is_empty() {
            output.push_str("## Running Tasks\nNone\n\n");
        } else {
            output.push_str(&format!("## Running Tasks ({})\n", tasks.len()));
            for t in &tasks {
                let age = format_age(t.created_at);
                output.push_str(&format!(
                    "- [{}] {} → {} ({}, {})\n",
                    t.id, t.description, t.reply_channel, t.status, age
                ));
            }
            output.push('\n');
        }

        // Recently completed tasks
        let recent = self.db.get_recent_background_tasks(5).await
            .context("Failed to get recent background tasks")?;

        if !recent.is_empty() {
            output.push_str(&format!("## Recently Completed ({})\n", recent.len()));
            for t in &recent {
                let age = format_age(t.updated_at);
                let result_preview = t.result.as_deref()
                    .map(|r| if r.len() > 80 { format!("{}...", &r[..80]) } else { r.to_string() })
                    .unwrap_or_default();
                output.push_str(&format!(
                    "- [{}] {} — {} {}{}\n",
                    t.id, t.description, t.status, age,
                    if result_preview.is_empty() { String::new() } else { format!("\n  Result: {}", result_preview) }
                ));
            }
        }

        if output.trim().is_empty() {
            output = "No active watchers or background tasks.".to_string();
        }

        Ok(output)
    }
}

// ─── stop_task ──────────────────────────────────────────────────────

/// Cancel any watcher or background task by ID
pub struct StopTaskTool {
    db: Arc<KnowledgeDb>,
    watcher_tx: mpsc::Sender<super::watchers::WatcherCommand>,
    task_tx: mpsc::Sender<BackgroundTaskCommand>,
}

impl StopTaskTool {
    pub fn new(
        db: Arc<KnowledgeDb>,
        watcher_tx: mpsc::Sender<super::watchers::WatcherCommand>,
        task_tx: mpsc::Sender<BackgroundTaskCommand>,
    ) -> Self {
        Self { db, watcher_tx, task_tx }
    }
}

#[async_trait]
impl ToolHandler for StopTaskTool {
    fn name(&self) -> &str {
        "stop_task"
    }

    fn description(&self) -> &str {
        "Stop/cancel any active watcher or background task by its ID. \
         Watcher IDs start with 'w-', background task IDs start with 't-'. \
         Use agent_status to see all active items and their IDs."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "task_id": {
                    "type": "string",
                    "description": "ID of the watcher (w-...) or background task (t-...) to stop"
                }
            }),
            vec!["task_id"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let task_id = input.get("task_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'task_id' parameter"))?;

        debug!("Stopping task: {}", task_id);

        if task_id.starts_with("w-") {
            // Cancel watcher
            self.db.update_watcher_active(task_id, false).await
                .context("Failed to deactivate watcher")?;

            self.watcher_tx.send(super::watchers::WatcherCommand::Cancel {
                id: task_id.to_string(),
            })
            .await
            .context("Failed to send cancel command to scheduler")?;

            Ok(format!("Stopped watcher [{}]", task_id))
        } else if task_id.starts_with("t-") {
            // Cancel background task
            self.db.update_background_task(task_id, "cancelled", None).await
                .context("Failed to cancel background task")?;

            self.task_tx.send(BackgroundTaskCommand::Cancel {
                id: task_id.to_string(),
            })
            .await
            .context("Failed to send cancel command")?;

            Ok(format!("Stopped background task [{}]", task_id))
        } else {
            Err(anyhow::anyhow!("Invalid task ID '{}'. Must start with 'w-' (watcher) or 't-' (background task).", task_id))
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────

fn format_age(dt: chrono::DateTime<chrono::Utc>) -> String {
    let elapsed = chrono::Utc::now().signed_duration_since(dt);
    if elapsed.num_days() > 0 {
        format!("{}d ago", elapsed.num_days())
    } else if elapsed.num_hours() > 0 {
        format!("{}h ago", elapsed.num_hours())
    } else if elapsed.num_minutes() > 0 {
        format!("{}m ago", elapsed.num_minutes())
    } else {
        "just now".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_age() {
        let now = chrono::Utc::now();
        assert_eq!(format_age(now), "just now");

        let two_hours_ago = now - chrono::Duration::hours(2);
        assert_eq!(format_age(two_hours_ago), "2h ago");

        let three_days_ago = now - chrono::Duration::days(3);
        assert_eq!(format_age(three_days_ago), "3d ago");
    }

    #[tokio::test]
    async fn test_agent_status_empty() {
        let temp = tempfile::TempDir::new().unwrap();
        let db = Arc::new(meepo_knowledge::KnowledgeDb::new(temp.path().join("test.db")).unwrap());
        let tool = AgentStatusTool::new(db);

        let result = tool.execute(serde_json::json!({})).await.unwrap();
        assert!(result.contains("None"));
    }

    #[tokio::test]
    async fn test_stop_task_invalid_id() {
        let temp = tempfile::TempDir::new().unwrap();
        let db = Arc::new(meepo_knowledge::KnowledgeDb::new(temp.path().join("test.db")).unwrap());
        let (watcher_tx, _) = mpsc::channel(1);
        let (task_tx, _) = mpsc::channel(1);
        let tool = StopTaskTool::new(db, watcher_tx, task_tx);

        let result = tool.execute(serde_json::json!({"task_id": "invalid-123"})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid task ID"));
    }

    #[tokio::test]
    async fn test_spawn_background_task() {
        let temp = tempfile::TempDir::new().unwrap();
        let db = Arc::new(meepo_knowledge::KnowledgeDb::new(temp.path().join("test.db")).unwrap());
        let (tx, mut rx) = mpsc::channel(1);
        let tool = SpawnBackgroundTaskTool::new(db.clone(), tx);

        let result = tool.execute(serde_json::json!({
            "description": "Research competitors",
            "reply_channel": "slack"
        })).await.unwrap();

        assert!(result.contains("t-"));
        assert!(result.contains("Research competitors"));

        // Check command was sent
        let cmd = rx.try_recv().unwrap();
        match cmd {
            BackgroundTaskCommand::Spawn { id, description, reply_channel } => {
                assert!(id.starts_with("t-"));
                assert_eq!(description, "Research competitors");
                assert_eq!(reply_channel, "slack");
            }
            _ => panic!("Expected Spawn command"),
        }

        // Check DB entry
        let tasks = db.get_active_background_tasks().await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "pending");
    }
}
