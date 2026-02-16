//! Proactive notification service
//!
//! Sends iMessages (or other channel messages) to the user throughout the day
//! when Meepo takes autonomous actions, watchers trigger, tasks complete, etc.
//! Also supports daily digest summaries (morning briefing, evening recap).

use chrono::{NaiveTime, Utc};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::types::{ChannelType, MessageKind, OutgoingMessage};

/// Which kind of event triggered this notification
#[derive(Debug, Clone, PartialEq)]
pub enum NotifyEvent {
    TaskStarted {
        task_id: String,
        description: String,
    },
    TaskCompleted {
        task_id: String,
        description: String,
        result_preview: String,
    },
    TaskFailed {
        task_id: String,
        description: String,
        error: String,
    },
    WatcherTriggered {
        watcher_id: String,
        kind: String,
        payload: String,
    },
    AutonomousAction {
        description: String,
    },
    Error {
        context: String,
        error: String,
    },
    BudgetWarning {
        period: String,
        spent: f64,
        budget: f64,
        percent: f64,
    },
    BudgetExceeded {
        period: String,
        spent: f64,
        budget: f64,
    },
    DigestMorning {
        summary: String,
    },
    DigestEvening {
        summary: String,
    },
}

/// Configuration for the notification service (mirrors config.toml)
#[derive(Debug, Clone)]
pub struct NotifyConfig {
    pub enabled: bool,
    pub channel: ChannelType,
    pub on_task_start: bool,
    pub on_task_complete: bool,
    pub on_task_fail: bool,
    pub on_watcher_triggered: bool,
    pub on_autonomous_action: bool,
    pub on_error: bool,
    pub quiet_hours: Option<(NaiveTime, NaiveTime)>,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            channel: ChannelType::IMessage,
            on_task_start: true,
            on_task_complete: true,
            on_task_fail: true,
            on_watcher_triggered: true,
            on_autonomous_action: true,
            on_error: true,
            quiet_hours: None,
        }
    }
}

/// The notification service — holds config and a sender to the message bus
#[derive(Clone)]
pub struct NotificationService {
    config: NotifyConfig,
    response_tx: mpsc::Sender<OutgoingMessage>,
}

impl NotificationService {
    pub fn new(config: NotifyConfig, response_tx: mpsc::Sender<OutgoingMessage>) -> Self {
        if config.enabled {
            info!(
                "Notification service enabled (channel: {}, quiet_hours: {})",
                config.channel,
                config
                    .quiet_hours
                    .map(|(s, e)| format!("{}-{}", s, e))
                    .unwrap_or_else(|| "none".to_string()),
            );
        }
        Self {
            config,
            response_tx,
        }
    }

    /// Create a no-op notification service (disabled)
    pub fn disabled(response_tx: mpsc::Sender<OutgoingMessage>) -> Self {
        Self::new(NotifyConfig::default(), response_tx)
    }

    /// Send a notification if the event type is enabled and we're not in quiet hours
    pub async fn notify(&self, event: NotifyEvent) {
        if !self.config.enabled {
            return;
        }

        // Check if this event type is enabled
        let is_error = matches!(event, NotifyEvent::Error { .. });
        if !self.should_notify(&event) {
            debug!("Notification suppressed (event type disabled): {:?}", event);
            return;
        }

        // Check quiet hours (errors always go through)
        if !is_error && self.is_quiet_hours() {
            debug!("Notification suppressed (quiet hours): {:?}", event);
            return;
        }

        let content = self.format_message(&event);

        let msg = OutgoingMessage {
            content,
            channel: self.config.channel.clone(),
            reply_to: None,
            kind: MessageKind::Response,
        };

        if let Err(e) = self.response_tx.send(msg).await {
            warn!("Failed to send notification: {}", e);
        }
    }

    /// Check if the given event type is enabled in config
    fn should_notify(&self, event: &NotifyEvent) -> bool {
        match event {
            NotifyEvent::TaskStarted { .. } => self.config.on_task_start,
            NotifyEvent::TaskCompleted { .. } => self.config.on_task_complete,
            NotifyEvent::TaskFailed { .. } => self.config.on_task_fail,
            NotifyEvent::WatcherTriggered { .. } => self.config.on_watcher_triggered,
            NotifyEvent::AutonomousAction { .. } => self.config.on_autonomous_action,
            NotifyEvent::Error { .. } => self.config.on_error,
            NotifyEvent::BudgetWarning { .. } | NotifyEvent::BudgetExceeded { .. } => true,
            NotifyEvent::DigestMorning { .. } | NotifyEvent::DigestEvening { .. } => true,
        }
    }

    /// Check if we're currently in quiet hours
    fn is_quiet_hours(&self) -> bool {
        let Some((start, end)) = self.config.quiet_hours else {
            return false;
        };

        let now = Utc::now().time();
        if start < end {
            // e.g., 23:00 - 08:00 doesn't wrap, but 22:00 - 06:00 does
            // Actually start < end means e.g. 08:00 - 17:00 (no wrap)
            now >= start && now < end
        } else {
            // Wraps midnight, e.g., 23:00 - 08:00
            now >= start || now < end
        }
    }

    /// Format a notification event into a user-friendly iMessage
    fn format_message(&self, event: &NotifyEvent) -> String {
        match event {
            NotifyEvent::TaskStarted {
                task_id,
                description,
            } => {
                format!(
                    "🤖 Starting background task\n[{}] {}",
                    task_id,
                    truncate(description, 200)
                )
            }
            NotifyEvent::TaskCompleted {
                task_id,
                description,
                result_preview,
            } => {
                format!(
                    "✅ Task completed\n[{}] {}\n\nResult: {}",
                    task_id,
                    truncate(description, 150),
                    truncate(result_preview, 300)
                )
            }
            NotifyEvent::TaskFailed {
                task_id,
                description,
                error,
            } => {
                format!(
                    "❌ Task failed\n[{}] {}\n\nError: {}",
                    task_id,
                    truncate(description, 150),
                    truncate(error, 200)
                )
            }
            NotifyEvent::WatcherTriggered {
                watcher_id,
                kind,
                payload,
            } => {
                format!(
                    "👁 Watcher triggered\n[{}] {}\n{}",
                    watcher_id,
                    kind,
                    truncate(payload, 300)
                )
            }
            NotifyEvent::AutonomousAction { description } => {
                format!(
                    "🧠 Taking autonomous action\n{}",
                    truncate(description, 400)
                )
            }
            NotifyEvent::Error { context, error } => {
                format!(
                    "⚠️ Error: {}\n{}",
                    truncate(context, 100),
                    truncate(error, 300)
                )
            }
            NotifyEvent::BudgetWarning {
                period,
                spent,
                budget,
                percent,
            } => {
                format!(
                    "💰 Budget warning: {} spending at {:.0}% (${:.2} of ${:.2})",
                    period, percent, spent, budget
                )
            }
            NotifyEvent::BudgetExceeded {
                period,
                spent,
                budget,
            } => {
                format!(
                    "🚨 Budget EXCEEDED: {} spending ${:.2} of ${:.2} limit. API calls paused.",
                    period, spent, budget
                )
            }
            NotifyEvent::DigestMorning { summary } => {
                format!("☀️ Good morning! Here's your briefing:\n\n{}", summary)
            }
            NotifyEvent::DigestEvening { summary } => {
                format!("🌙 End of day recap:\n\n{}", summary)
            }
        }
    }
}

/// Truncate a string to max_len, appending "..." if truncated
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        // Find a safe char boundary
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello");
    }

    #[test]
    fn test_should_notify_respects_config() {
        let (tx, _rx) = mpsc::channel(16);
        let config = NotifyConfig {
            enabled: true,
            on_task_start: false,
            on_task_complete: true,
            ..Default::default()
        };
        let svc = NotificationService::new(config, tx);

        assert!(!svc.should_notify(&NotifyEvent::TaskStarted {
            task_id: "t-1".into(),
            description: "test".into(),
        }));
        assert!(svc.should_notify(&NotifyEvent::TaskCompleted {
            task_id: "t-1".into(),
            description: "test".into(),
            result_preview: "done".into(),
        }));
    }

    #[test]
    fn test_format_message() {
        let (tx, _rx) = mpsc::channel(16);
        let svc = NotificationService::new(NotifyConfig::default(), tx);

        let msg = svc.format_message(&NotifyEvent::TaskStarted {
            task_id: "t-abc".into(),
            description: "Research competitors".into(),
        });
        assert!(msg.contains("t-abc"));
        assert!(msg.contains("Research competitors"));
    }

    #[tokio::test]
    async fn test_notify_disabled_does_nothing() {
        let (tx, mut rx) = mpsc::channel(16);
        let svc = NotificationService::disabled(tx);

        svc.notify(NotifyEvent::TaskStarted {
            task_id: "t-1".into(),
            description: "test".into(),
        })
        .await;

        // Nothing should be sent
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_notify_enabled_sends_message() {
        let (tx, mut rx) = mpsc::channel(16);
        let config = NotifyConfig {
            enabled: true,
            ..Default::default()
        };
        let svc = NotificationService::new(config, tx);

        svc.notify(NotifyEvent::TaskCompleted {
            task_id: "t-42".into(),
            description: "Build report".into(),
            result_preview: "Report generated".into(),
        })
        .await;

        let msg = rx.try_recv().unwrap();
        assert_eq!(msg.channel, ChannelType::IMessage);
        assert!(msg.content.contains("t-42"));
        assert!(msg.content.contains("Report generated"));
    }

    #[tokio::test]
    async fn test_notify_suppressed_event_type() {
        let (tx, mut rx) = mpsc::channel(16);
        let config = NotifyConfig {
            enabled: true,
            on_task_start: false,
            ..Default::default()
        };
        let svc = NotificationService::new(config, tx);

        svc.notify(NotifyEvent::TaskStarted {
            task_id: "t-1".into(),
            description: "test".into(),
        })
        .await;

        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_should_notify_budget_always_true() {
        let (tx, _rx) = mpsc::channel(16);
        let config = NotifyConfig {
            enabled: true,
            ..Default::default()
        };
        let svc = NotificationService::new(config, tx);

        assert!(svc.should_notify(&NotifyEvent::BudgetWarning {
            period: "daily".into(),
            spent: 5.0,
            budget: 10.0,
            percent: 50.0,
        }));
        assert!(svc.should_notify(&NotifyEvent::BudgetExceeded {
            period: "daily".into(),
            spent: 11.0,
            budget: 10.0,
        }));
        assert!(svc.should_notify(&NotifyEvent::DigestMorning {
            summary: "test".into(),
        }));
        assert!(svc.should_notify(&NotifyEvent::DigestEvening {
            summary: "test".into(),
        }));
    }

    #[test]
    fn test_format_all_event_types() {
        let (tx, _rx) = mpsc::channel(16);
        let svc = NotificationService::new(NotifyConfig::default(), tx);

        let events = vec![
            NotifyEvent::TaskStarted {
                task_id: "t1".into(),
                description: "desc".into(),
            },
            NotifyEvent::TaskCompleted {
                task_id: "t2".into(),
                description: "desc".into(),
                result_preview: "done".into(),
            },
            NotifyEvent::TaskFailed {
                task_id: "t3".into(),
                description: "desc".into(),
                error: "oops".into(),
            },
            NotifyEvent::WatcherTriggered {
                watcher_id: "w1".into(),
                kind: "file".into(),
                payload: "changed".into(),
            },
            NotifyEvent::AutonomousAction {
                description: "doing stuff".into(),
            },
            NotifyEvent::Error {
                context: "ctx".into(),
                error: "err".into(),
            },
            NotifyEvent::BudgetWarning {
                period: "daily".into(),
                spent: 5.0,
                budget: 10.0,
                percent: 50.0,
            },
            NotifyEvent::BudgetExceeded {
                period: "monthly".into(),
                spent: 100.0,
                budget: 50.0,
            },
            NotifyEvent::DigestMorning {
                summary: "morning".into(),
            },
            NotifyEvent::DigestEvening {
                summary: "evening".into(),
            },
        ];

        for event in events {
            let msg = svc.format_message(&event);
            assert!(!msg.is_empty());
        }
    }

    #[test]
    fn test_truncate_unicode_boundary() {
        let s = "héllo wörld";
        let t = truncate(s, 3);
        // Should not panic on multi-byte chars
        assert!(t.len() <= 3);
    }

    #[test]
    fn test_notify_config_default() {
        let config = NotifyConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.channel, ChannelType::IMessage);
        assert!(config.on_task_start);
        assert!(config.on_task_complete);
        assert!(config.on_task_fail);
        assert!(config.on_error);
        assert!(config.quiet_hours.is_none());
    }
}
