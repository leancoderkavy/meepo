//! Action logging and classification for the autonomous agent
//!
//! Classifies tool actions by risk level and logs outcomes for
//! confidence calibration and audit trails.

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::debug;

use meepo_knowledge::KnowledgeDb;

/// Risk level of a tool action
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionRisk {
    /// Read-only, no side effects (e.g. read_file, recall, search_knowledge)
    ReadOnly,
    /// Creates or modifies data but is reversible (e.g. remember, write_file, create_watcher)
    Write,
    /// Sends messages, emails, or interacts with external services (e.g. send_email, send_sms)
    External,
    /// Potentially destructive or irreversible (e.g. run_command, click_element)
    Destructive,
}

impl std::fmt::Display for ActionRisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadOnly => write!(f, "read_only"),
            Self::Write => write!(f, "write"),
            Self::External => write!(f, "external"),
            Self::Destructive => write!(f, "destructive"),
        }
    }
}

/// Classify a tool by its risk level based on its name
pub fn classify_tool(tool_name: &str) -> ActionRisk {
    match tool_name {
        // Read-only tools
        "read_file"
        | "list_directory"
        | "search_files"
        | "recall"
        | "search_knowledge"
        | "smart_recall"
        | "browse_url"
        | "web_search"
        | "get_clipboard"
        | "read_emails"
        | "read_calendar"
        | "list_reminders"
        | "list_notes"
        | "list_watchers"
        | "agent_status"
        | "get_usage_stats"
        | "list_tasks"
        | "project_status"
        | "habit_streak"
        | "habit_report"
        | "spending_summary"
        | "budget_check"
        | "browser_list_tabs"
        | "browser_get_page_content"
        | "browser_get_url"
        | "browser_screenshot"
        | "read_screen"
        | "get_current_track"
        | "search_contacts"
        | "find_free_time"
        | "relationship_summary"
        | "get_weather"
        | "get_directions"
        | "flight_status"
        | "message_summary"
        | "daily_briefing"
        | "weekly_review" => ActionRisk::ReadOnly,

        // Write tools (reversible, local data)
        "write_file"
        | "remember"
        | "link_entities"
        | "ingest_document"
        | "create_watcher"
        | "cancel_watcher"
        | "create_task"
        | "update_task"
        | "complete_task"
        | "log_habit"
        | "log_expense"
        | "parse_receipt"
        | "track_feed"
        | "untrack_feed"
        | "track_topic"
        | "create_note"
        | "create_reminder"
        | "set_auto_reply"
        | "packing_list"
        | "spawn_background_task"
        | "stop_task"
        | "write_code" => ActionRisk::Write,

        // External tools (send data outside the system)
        "send_email" | "send_sms" | "send_notification" | "make_pr" | "review_pr"
        | "create_event" | "reschedule_event" | "schedule_meeting" | "delegate_tasks"
        | "delegate_to_agent" | "email_draft_reply" | "email_unsubscribe" | "suggest_followups" => {
            ActionRisk::External
        }

        // Destructive tools (irreversible or high-impact)
        "run_command"
        | "click_element"
        | "type_text"
        | "browser_click_element"
        | "browser_fill_form"
        | "browser_execute_js"
        | "browser_navigate"
        | "browser_open_tab"
        | "browser_close_tab"
        | "browser_switch_tab"
        | "music_control"
        | "open_app"
        | "screen_capture"
        | "spawn_coding_agent"
        | "email_triage" => ActionRisk::Destructive,

        // Unknown tools default to destructive for safety
        _ => {
            debug!("Unknown tool '{}' classified as destructive", tool_name);
            ActionRisk::Destructive
        }
    }
}

/// Tracks action outcomes for audit and confidence calibration
pub struct ActionLogger {
    db: Arc<KnowledgeDb>,
}

impl ActionLogger {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self { db }
    }

    /// Log an action and its outcome
    pub async fn log_action(
        &self,
        goal_id: Option<&str>,
        action_type: &str,
        description: &str,
        outcome: &str,
    ) -> Result<String> {
        self.db
            .insert_action_log(goal_id, action_type, description, outcome)
            .await
    }

    /// Log a tool execution with risk classification
    pub async fn log_tool_execution(
        &self,
        tool_name: &str,
        goal_id: Option<&str>,
        outcome: &str,
    ) -> Result<String> {
        let risk = classify_tool(tool_name);
        let description = format!("Tool: {} (risk: {})", tool_name, risk);
        self.log_action(goal_id, "tool_execution", &description, outcome)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_read_only() {
        assert_eq!(classify_tool("read_file"), ActionRisk::ReadOnly);
        assert_eq!(classify_tool("recall"), ActionRisk::ReadOnly);
        assert_eq!(classify_tool("web_search"), ActionRisk::ReadOnly);
        assert_eq!(classify_tool("get_usage_stats"), ActionRisk::ReadOnly);
    }

    #[test]
    fn test_classify_write() {
        assert_eq!(classify_tool("write_file"), ActionRisk::Write);
        assert_eq!(classify_tool("remember"), ActionRisk::Write);
        assert_eq!(classify_tool("create_watcher"), ActionRisk::Write);
    }

    #[test]
    fn test_classify_external() {
        assert_eq!(classify_tool("send_email"), ActionRisk::External);
        assert_eq!(classify_tool("make_pr"), ActionRisk::External);
        assert_eq!(classify_tool("delegate_tasks"), ActionRisk::External);
    }

    #[test]
    fn test_classify_destructive() {
        assert_eq!(classify_tool("run_command"), ActionRisk::Destructive);
        assert_eq!(classify_tool("click_element"), ActionRisk::Destructive);
    }

    #[test]
    fn test_classify_unknown_defaults_destructive() {
        assert_eq!(classify_tool("unknown_tool_xyz"), ActionRisk::Destructive);
    }

    #[test]
    fn test_risk_ordering() {
        assert!(ActionRisk::ReadOnly < ActionRisk::Write);
        assert!(ActionRisk::Write < ActionRisk::External);
        assert!(ActionRisk::External < ActionRisk::Destructive);
    }

    #[test]
    fn test_action_risk_display() {
        assert_eq!(ActionRisk::ReadOnly.to_string(), "read_only");
        assert_eq!(ActionRisk::Write.to_string(), "write");
        assert_eq!(ActionRisk::External.to_string(), "external");
        assert_eq!(ActionRisk::Destructive.to_string(), "destructive");
    }

    #[test]
    fn test_action_risk_serde_roundtrip() {
        let risks = [
            ActionRisk::ReadOnly,
            ActionRisk::Write,
            ActionRisk::External,
            ActionRisk::Destructive,
        ];
        for r in &risks {
            let json = serde_json::to_string(r).unwrap();
            let parsed: ActionRisk = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, r);
        }
    }

    #[test]
    fn test_classify_all_read_only_tools() {
        let read_only = [
            "list_directory", "search_files", "smart_recall", "browse_url",
            "get_clipboard", "read_emails", "read_calendar", "list_reminders",
            "list_notes", "list_watchers", "agent_status", "list_tasks",
            "project_status", "habit_streak", "habit_report", "spending_summary",
            "budget_check", "browser_list_tabs", "browser_get_page_content",
            "browser_get_url", "browser_screenshot", "read_screen",
            "get_current_track", "search_contacts", "find_free_time",
            "relationship_summary", "get_weather", "get_directions",
            "flight_status", "message_summary", "daily_briefing", "weekly_review",
        ];
        for tool in &read_only {
            assert_eq!(classify_tool(tool), ActionRisk::ReadOnly, "Expected ReadOnly for {}", tool);
        }
    }

    #[test]
    fn test_classify_all_write_tools() {
        let write = [
            "remember", "link_entities", "ingest_document", "create_watcher",
            "cancel_watcher", "create_task", "update_task", "complete_task",
            "log_habit", "log_expense", "parse_receipt", "track_feed",
            "untrack_feed", "track_topic", "create_note", "create_reminder",
            "set_auto_reply", "packing_list", "spawn_background_task",
            "stop_task", "write_code",
        ];
        for tool in &write {
            assert_eq!(classify_tool(tool), ActionRisk::Write, "Expected Write for {}", tool);
        }
    }

    #[test]
    fn test_classify_all_external_tools() {
        let external = [
            "send_sms", "send_notification", "review_pr",
            "create_event", "reschedule_event", "schedule_meeting",
            "delegate_tasks", "delegate_to_agent", "email_draft_reply",
            "email_unsubscribe", "suggest_followups",
        ];
        for tool in &external {
            assert_eq!(classify_tool(tool), ActionRisk::External, "Expected External for {}", tool);
        }
    }

    #[test]
    fn test_classify_all_destructive_tools() {
        let destructive = [
            "click_element", "type_text", "browser_click_element",
            "browser_fill_form", "browser_execute_js", "browser_navigate",
            "browser_open_tab", "browser_close_tab", "browser_switch_tab",
            "music_control", "open_app", "screen_capture",
            "spawn_coding_agent", "email_triage",
        ];
        for tool in &destructive {
            assert_eq!(classify_tool(tool), ActionRisk::Destructive, "Expected Destructive for {}", tool);
        }
    }

    #[tokio::test]
    async fn test_action_logger_log_action() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Arc::new(KnowledgeDb::new(&dir.path().join("test.db")).unwrap());
        let logger = ActionLogger::new(db.clone());

        let id = logger
            .log_action(None, "test_action", "Did something", "success")
            .await
            .unwrap();
        assert!(!id.is_empty());

        let actions = db.get_recent_actions(10).await.unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_type, "test_action");
        assert_eq!(actions[0].outcome, "success");
    }

    #[tokio::test]
    async fn test_action_logger_log_tool_execution() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Arc::new(KnowledgeDb::new(&dir.path().join("test.db")).unwrap());
        let logger = ActionLogger::new(db.clone());

        let id = logger
            .log_tool_execution("read_file", None, "success")
            .await
            .unwrap();
        assert!(!id.is_empty());

        let actions = db.get_recent_actions(10).await.unwrap();
        assert_eq!(actions.len(), 1);
        assert!(actions[0].description.contains("read_file"));
        assert!(actions[0].description.contains("read_only"));
        assert!(actions[0].goal_id.is_none());
    }
}
