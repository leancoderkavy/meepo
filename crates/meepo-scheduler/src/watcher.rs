//! Watcher types and definitions
//!
//! This module defines the core types for watchers, which are reactive
//! components that monitor various sources (email, calendar, files, etc.)
//! and emit events when conditions are met.

use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};

/// A watcher monitors a specific source and triggers actions when conditions are met
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Watcher {
    /// Unique identifier for this watcher
    pub id: String,

    /// The type and configuration of the watcher
    pub kind: WatcherKind,

    /// Description of what to do when triggered
    pub action: String,

    /// Which channel to send results to (e.g., "slack-general", "email", "webhook")
    pub reply_channel: String,

    /// Whether this watcher is currently active
    pub active: bool,

    /// When this watcher was created
    pub created_at: DateTime<Utc>,
}

impl Watcher {
    /// Create a new watcher with a generated UUID
    pub fn new(kind: WatcherKind, action: String, reply_channel: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            action,
            reply_channel,
            active: true,
            created_at: Utc::now(),
        }
    }

    /// Get a human-readable description of this watcher
    pub fn description(&self) -> String {
        match &self.kind {
            WatcherKind::EmailWatch { from, subject_contains, interval_secs } => {
                let mut desc = format!("Email watcher (every {}s)", interval_secs);
                if let Some(f) = from {
                    desc.push_str(&format!(" from: {}", f));
                }
                if let Some(s) = subject_contains {
                    desc.push_str(&format!(" subject contains: {}", s));
                }
                desc
            }
            WatcherKind::CalendarWatch { lookahead_hours, interval_secs } => {
                format!("Calendar watcher ({}h lookahead, every {}s)", lookahead_hours, interval_secs)
            }
            WatcherKind::GitHubWatch { repo, events, interval_secs, .. } => {
                format!("GitHub watcher for {} (events: {:?}, every {}s)", repo, events, interval_secs)
            }
            WatcherKind::FileWatch { path } => {
                format!("File watcher for {}", path)
            }
            WatcherKind::MessageWatch { keyword } => {
                format!("Message watcher for keyword: {}", keyword)
            }
            WatcherKind::Scheduled { cron_expr, task } => {
                format!("Scheduled task '{}' (cron: {})", task, cron_expr)
            }
            WatcherKind::OneShot { at, task } => {
                format!("One-shot task '{}' at {}", task, at)
            }
        }
    }
}

/// The different types of watchers available
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WatcherKind {
    /// Watch for emails matching certain criteria
    EmailWatch {
        /// Filter by sender email address
        from: Option<String>,

        /// Filter by subject line containing this text
        subject_contains: Option<String>,

        /// How often to poll for new emails (in seconds)
        interval_secs: u64,
    },

    /// Watch calendar for upcoming events
    CalendarWatch {
        /// How far ahead to look for events (in hours)
        lookahead_hours: u64,

        /// How often to check the calendar (in seconds)
        interval_secs: u64,
    },

    /// Watch GitHub repository for events
    GitHubWatch {
        /// Repository in "owner/repo" format
        repo: String,

        /// Event types to watch for (e.g., "push", "pull_request", "issues")
        events: Vec<String>,

        /// How often to poll GitHub API (in seconds)
        interval_secs: u64,

        /// Optional GitHub token for authenticated API calls (higher rate limits, private repos)
        #[serde(default)]
        github_token: Option<String>,
    },

    /// Watch filesystem for changes
    FileWatch {
        /// Path to file or directory to watch
        path: String,
    },

    /// Watch for messages containing a keyword
    MessageWatch {
        /// Keyword to trigger on
        keyword: String,
    },

    /// Run a task on a schedule (cron expression)
    Scheduled {
        /// Cron expression (e.g., "0 9 * * MON" for 9am every Monday)
        cron_expr: String,

        /// Description of the task to run
        task: String,
    },

    /// Run a task once at a specific time
    OneShot {
        /// When to run the task
        at: DateTime<Utc>,

        /// Description of the task to run
        task: String,
    },
}

impl WatcherKind {
    /// Get the minimum safe polling interval for this watcher type
    pub fn min_interval_secs(&self) -> u64 {
        match self {
            Self::EmailWatch { .. } => 60,      // Email: minimum 1 minute
            Self::CalendarWatch { .. } => 300,  // Calendar: minimum 5 minutes
            Self::GitHubWatch { .. } => 30,     // GitHub: minimum 30 seconds (API rate limits)
            Self::FileWatch { .. } => 0,        // File: event-driven, no polling
            Self::MessageWatch { .. } => 0,     // Message: event-driven
            Self::Scheduled { .. } => 0,        // Scheduled: based on cron
            Self::OneShot { .. } => 0,          // OneShot: fires once
        }
    }

    /// Check if this is a polling-based watcher
    pub fn is_polling(&self) -> bool {
        matches!(
            self,
            Self::EmailWatch { .. } | Self::CalendarWatch { .. } | Self::GitHubWatch { .. }
        )
    }

    /// Check if this is an event-driven watcher
    pub fn is_event_driven(&self) -> bool {
        matches!(
            self,
            Self::FileWatch { .. } | Self::MessageWatch { .. }
        )
    }

    /// Check if this is a scheduled task
    pub fn is_scheduled(&self) -> bool {
        matches!(
            self,
            Self::Scheduled { .. } | Self::OneShot { .. }
        )
    }
}

/// An event emitted by a watcher when triggered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherEvent {
    /// The ID of the watcher that emitted this event
    pub watcher_id: String,

    /// The kind of event (e.g., "email_received", "file_changed", "task_scheduled")
    pub kind: String,

    /// Event-specific payload data
    pub payload: serde_json::Value,

    /// When this event occurred
    pub timestamp: DateTime<Utc>,
}

impl WatcherEvent {
    /// Create a new watcher event
    pub fn new(watcher_id: String, kind: String, payload: serde_json::Value) -> Self {
        Self {
            watcher_id,
            kind,
            payload,
            timestamp: Utc::now(),
        }
    }

    /// Create an email event
    pub fn email(watcher_id: String, from: String, subject: String, body: String) -> Self {
        Self::new(
            watcher_id,
            "email_received".to_string(),
            serde_json::json!({
                "from": from,
                "subject": subject,
                "body": body,
            }),
        )
    }

    /// Create a calendar event
    pub fn calendar(watcher_id: String, event_title: String, event_time: DateTime<Utc>) -> Self {
        Self::new(
            watcher_id,
            "calendar_event".to_string(),
            serde_json::json!({
                "title": event_title,
                "time": event_time,
            }),
        )
    }

    /// Create a file change event
    pub fn file_changed(watcher_id: String, path: String, change_type: String) -> Self {
        Self::new(
            watcher_id,
            "file_changed".to_string(),
            serde_json::json!({
                "path": path,
                "change_type": change_type,
            }),
        )
    }

    /// Create a GitHub event
    pub fn github(watcher_id: String, event_type: String, data: serde_json::Value) -> Self {
        Self::new(
            watcher_id,
            format!("github_{}", event_type),
            data,
        )
    }

    /// Create a task execution event
    pub fn task(watcher_id: String, task_name: String) -> Self {
        Self::new(
            watcher_id,
            "task_triggered".to_string(),
            serde_json::json!({
                "task": task_name,
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watcher_creation() {
        let watcher = Watcher::new(
            WatcherKind::EmailWatch {
                from: Some("boss@company.com".to_string()),
                subject_contains: Some("urgent".to_string()),
                interval_secs: 300,
            },
            "Notify on urgent emails".to_string(),
            "slack-alerts".to_string(),
        );

        assert!(watcher.active);
        assert!(!watcher.id.is_empty());
        assert_eq!(watcher.action, "Notify on urgent emails");
    }

    #[test]
    fn test_watcher_kind_min_intervals() {
        let email_watch = WatcherKind::EmailWatch {
            from: None,
            subject_contains: None,
            interval_secs: 30,
        };
        assert_eq!(email_watch.min_interval_secs(), 60);

        let file_watch = WatcherKind::FileWatch {
            path: "/tmp/test".to_string(),
        };
        assert_eq!(file_watch.min_interval_secs(), 0);
    }

    #[test]
    fn test_watcher_kind_classification() {
        let email = WatcherKind::EmailWatch {
            from: None,
            subject_contains: None,
            interval_secs: 60,
        };
        assert!(email.is_polling());
        assert!(!email.is_event_driven());
        assert!(!email.is_scheduled());

        let file = WatcherKind::FileWatch {
            path: "/tmp".to_string(),
        };
        assert!(!file.is_polling());
        assert!(file.is_event_driven());

        let scheduled = WatcherKind::Scheduled {
            cron_expr: "0 9 * * *".to_string(),
            task: "Daily backup".to_string(),
        };
        assert!(scheduled.is_scheduled());
    }

    #[test]
    fn test_watcher_event_creation() {
        let event = WatcherEvent::email(
            "watcher-123".to_string(),
            "sender@example.com".to_string(),
            "Test Subject".to_string(),
            "Test body".to_string(),
        );

        assert_eq!(event.watcher_id, "watcher-123");
        assert_eq!(event.kind, "email_received");
        assert!(event.payload.get("from").is_some());
    }
}
