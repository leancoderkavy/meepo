//! Watcher management tools

use async_trait::async_trait;
use serde_json::Value;
use anyhow::{Result, Context};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use meepo_knowledge::KnowledgeDb;
use super::{ToolHandler, json_schema};

/// Commands to send to the watcher scheduler
#[derive(Debug, Clone)]
pub enum WatcherCommand {
    Create {
        kind: String,
        config: Value,
        action: String,
        reply_channel: String,
    },
    List,
    Cancel {
        id: String,
    },
}

/// Create a new watcher
pub struct CreateWatcherTool {
    db: Arc<KnowledgeDb>,
    command_tx: mpsc::Sender<WatcherCommand>,
}

impl CreateWatcherTool {
    pub fn new(db: Arc<KnowledgeDb>, command_tx: mpsc::Sender<WatcherCommand>) -> Self {
        Self { db, command_tx }
    }
}

#[async_trait]
impl ToolHandler for CreateWatcherTool {
    fn name(&self) -> &str {
        "create_watcher"
    }

    fn description(&self) -> &str {
        "Create a new watcher to monitor for specific events. \
         Watchers can monitor emails, calendar events, files, GitHub, etc."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "kind": {
                    "type": "string",
                    "description": "Type of watcher: 'email', 'calendar', 'file', 'github', 'time'"
                },
                "config": {
                    "type": "object",
                    "description": "Configuration specific to the watcher type (e.g., file path, email filters)"
                },
                "action": {
                    "type": "string",
                    "description": "Description of what to do when the watcher triggers"
                },
                "reply_channel": {
                    "type": "string",
                    "description": "Channel to send notifications to (e.g., 'slack', 'discord', 'internal')"
                }
            }),
            vec!["kind", "config", "action", "reply_channel"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let kind = input.get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'kind' parameter"))?;
        let config = input.get("config")
            .ok_or_else(|| anyhow::anyhow!("Missing 'config' parameter"))?
            .clone();
        let action = input.get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;
        let reply_channel = input.get("reply_channel")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'reply_channel' parameter"))?;

        debug!("Creating watcher: {} -> {}", kind, action);

        // Store in database
        let watcher_id = self.db.insert_watcher(kind, config.clone(), action, reply_channel)
            .context("Failed to create watcher in database")?;

        // Send command to scheduler
        self.command_tx.send(WatcherCommand::Create {
            kind: kind.to_string(),
            config,
            action: action.to_string(),
            reply_channel: reply_channel.to_string(),
        })
        .await
        .context("Failed to send command to scheduler")?;

        Ok(format!("Created watcher with ID: {}", watcher_id))
    }
}

/// List active watchers
pub struct ListWatchersTool {
    db: Arc<KnowledgeDb>,
}

impl ListWatchersTool {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ToolHandler for ListWatchersTool {
    fn name(&self) -> &str {
        "list_watchers"
    }

    fn description(&self) -> &str {
        "List all currently active watchers and their configurations."
    }

    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({}), vec![])
    }

    async fn execute(&self, _input: Value) -> Result<String> {
        debug!("Listing active watchers");

        let watchers = self.db.get_active_watchers()
            .context("Failed to get active watchers")?;

        if watchers.is_empty() {
            return Ok("No active watchers.".to_string());
        }

        let mut output = format!("Active watchers ({}):\n\n", watchers.len());
        for watcher in watchers {
            output.push_str(&format!("- ID: {}\n", watcher.id));
            output.push_str(&format!("  Kind: {}\n", watcher.kind));
            output.push_str(&format!("  Action: {}\n", watcher.action));
            output.push_str(&format!("  Channel: {}\n", watcher.reply_channel));
            output.push_str(&format!("  Config: {}\n", watcher.config));
            output.push_str(&format!("  Created: {}\n\n", watcher.created_at));
        }

        Ok(output)
    }
}

/// Cancel/deactivate a watcher
pub struct CancelWatcherTool {
    db: Arc<KnowledgeDb>,
    command_tx: mpsc::Sender<WatcherCommand>,
}

impl CancelWatcherTool {
    pub fn new(db: Arc<KnowledgeDb>, command_tx: mpsc::Sender<WatcherCommand>) -> Self {
        Self { db, command_tx }
    }
}

#[async_trait]
impl ToolHandler for CancelWatcherTool {
    fn name(&self) -> &str {
        "cancel_watcher"
    }

    fn description(&self) -> &str {
        "Cancel/deactivate an active watcher by its ID."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "watcher_id": {
                    "type": "string",
                    "description": "ID of the watcher to cancel"
                }
            }),
            vec!["watcher_id"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let watcher_id = input.get("watcher_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'watcher_id' parameter"))?;

        debug!("Canceling watcher: {}", watcher_id);

        // Deactivate in database
        self.db.update_watcher_active(watcher_id, false)
            .context("Failed to deactivate watcher")?;

        // Send cancel command to scheduler
        self.command_tx.send(WatcherCommand::Cancel {
            id: watcher_id.to_string(),
        })
        .await
        .map_err(|e| {
            warn!("Failed to send cancel command: {}", e);
            e
        })
        .ok(); // Don't fail if scheduler is down

        Ok(format!("Canceled watcher: {}", watcher_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolHandler;
    use tempfile::TempDir;

    fn setup() -> (Arc<meepo_knowledge::KnowledgeDb>, mpsc::Sender<WatcherCommand>, mpsc::Receiver<WatcherCommand>, TempDir) {
        let temp = TempDir::new().unwrap();
        let db = Arc::new(meepo_knowledge::KnowledgeDb::new(&temp.path().join("test.db")).unwrap());
        let (tx, rx) = mpsc::channel(100);
        (db, tx, rx, temp)
    }

    #[test]
    fn test_create_watcher_schema() {
        let (db, tx, _rx, _temp) = setup();
        let tool = CreateWatcherTool::new(db, tx);
        assert_eq!(tool.name(), "create_watcher");
        assert!(!tool.description().is_empty());
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_list_watchers_schema() {
        let (db, _tx, _rx, _temp) = setup();
        let tool = ListWatchersTool::new(db);
        assert_eq!(tool.name(), "list_watchers");
    }

    #[test]
    fn test_cancel_watcher_schema() {
        let (db, tx, _rx, _temp) = setup();
        let tool = CancelWatcherTool::new(db, tx);
        assert_eq!(tool.name(), "cancel_watcher");
    }

    #[tokio::test]
    async fn test_list_watchers_empty() {
        let (db, _tx, _rx, _temp) = setup();
        let tool = ListWatchersTool::new(db);
        let result = tool.execute(serde_json::json!({})).await.unwrap();
        assert!(result.contains("No") || result.contains("no") || result.contains("0") || result.is_empty());
    }

    #[tokio::test]
    async fn test_create_and_list_watcher() {
        let (db, tx, _rx, _temp) = setup();
        let create = CreateWatcherTool::new(db.clone(), tx);
        let list = ListWatchersTool::new(db);

        let result = create.execute(serde_json::json!({
            "kind": "scheduled",
            "config": {"cron_expr": "0 * * * *", "task": "test task"},
            "action": "Run a test",
            "reply_channel": "internal"
        })).await.unwrap();
        assert!(result.contains("Created") || result.contains("created") || result.contains("watcher"));

        let result = list.execute(serde_json::json!({})).await.unwrap();
        assert!(result.contains("test") || result.contains("Run"));
    }
}
