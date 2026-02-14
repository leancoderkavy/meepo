//! Smart Calendar Manager tools
//!
//! Full calendar autonomy — find free time, schedule/reschedule meetings,
//! generate daily briefings and weekly reviews.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

use crate::platform::{CalendarProvider, ContactsProvider, EmailProvider};
use crate::tools::{ToolHandler, json_schema};
use meepo_knowledge::KnowledgeDb;

/// Find available time slots in the calendar
pub struct FindFreeTimeTool {
    provider: Box<dyn CalendarProvider>,
}

impl FindFreeTimeTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_calendar_provider()
                .expect("Calendar provider not available on this platform"),
        }
    }
}

impl Default for FindFreeTimeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolHandler for FindFreeTimeTool {
    fn name(&self) -> &str {
        "find_free_time"
    }

    fn description(&self) -> &str {
        "Find available time slots in your calendar. Scans upcoming days for gaps between events \
         and returns free blocks of the requested minimum duration. Useful for scheduling meetings \
         or finding focus time."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "days_ahead": {
                    "type": "number",
                    "description": "Number of days to scan (default: 5, max: 14)"
                },
                "min_duration_minutes": {
                    "type": "number",
                    "description": "Minimum free block duration in minutes (default: 30)"
                },
                "working_hours_start": {
                    "type": "string",
                    "description": "Start of working hours in HH:MM format (default: '09:00')"
                },
                "working_hours_end": {
                    "type": "string",
                    "description": "End of working hours in HH:MM format (default: '17:00')"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let days_ahead = input
            .get("days_ahead")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(14);
        let min_duration = input
            .get("min_duration_minutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);
        let work_start = input
            .get("working_hours_start")
            .and_then(|v| v.as_str())
            .unwrap_or("09:00");
        let work_end = input
            .get("working_hours_end")
            .and_then(|v| v.as_str())
            .unwrap_or("17:00");

        debug!(
            "Finding free time: {} days ahead, min {} min, {}-{}",
            days_ahead, min_duration, work_start, work_end
        );

        let events = self.provider.read_events(days_ahead).await?;

        Ok(format!(
            "Calendar events (next {} days):\n\n{}\n\n\
             Working hours: {} - {}\n\
             Minimum free block: {} minutes\n\n\
             Please analyze the events above and identify all free time slots that are:\n\
             1. Within working hours ({} - {})\n\
             2. At least {} minutes long\n\
             3. Not overlapping with any existing events\n\n\
             Format each slot as: DATE | START - END | DURATION",
            days_ahead, events, work_start, work_end, min_duration, work_start, work_end,
            min_duration
        ))
    }
}

/// Schedule a meeting with smart time finding
pub struct ScheduleMeetingTool {
    calendar: Box<dyn CalendarProvider>,
    contacts: Option<Box<dyn ContactsProvider>>,
}

impl ScheduleMeetingTool {
    pub fn new() -> Self {
        Self {
            calendar: crate::platform::create_calendar_provider()
                .expect("Calendar provider not available on this platform"),
            contacts: crate::platform::create_contacts_provider().ok(),
        }
    }
}

impl Default for ScheduleMeetingTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolHandler for ScheduleMeetingTool {
    fn name(&self) -> &str {
        "schedule_meeting"
    }

    fn description(&self) -> &str {
        "Schedule a meeting with smart time finding. Checks your calendar for availability, \
         looks up attendee contact info, creates the calendar event, and optionally sends \
         invitation emails to attendees."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "title": {
                    "type": "string",
                    "description": "Meeting title"
                },
                "attendees": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "List of attendee names or email addresses"
                },
                "duration_minutes": {
                    "type": "number",
                    "description": "Meeting duration in minutes (default: 30)"
                },
                "preferred_time": {
                    "type": "string",
                    "description": "Preferred time slot (e.g., 'tomorrow morning', 'next Tuesday 2pm')"
                },
                "send_invites": {
                    "type": "boolean",
                    "description": "Whether to send email invitations to attendees (default: true)"
                },
                "notes": {
                    "type": "string",
                    "description": "Meeting agenda or notes to include in the invitation"
                }
            }),
            vec!["title", "duration_minutes"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let title = input
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'title' parameter"))?;
        let duration = input
            .get("duration_minutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);
        let preferred_time = input
            .get("preferred_time")
            .and_then(|v| v.as_str())
            .unwrap_or("next available slot");
        let attendees: Vec<String> = input
            .get("attendees")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let send_invites = input
            .get("send_invites")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let notes = input.get("notes").and_then(|v| v.as_str()).unwrap_or("");

        if title.len() > 500 {
            return Err(anyhow::anyhow!("Title too long (max 500 characters)"));
        }

        debug!(
            "Scheduling meeting: {} ({} min) with {} attendees",
            title,
            duration,
            attendees.len()
        );

        // Get current calendar to find availability
        let events = self.calendar.read_events(7).await?;

        // Look up attendee contact info if contacts provider is available
        let mut attendee_info = Vec::new();
        if let Some(ref contacts) = self.contacts {
            for attendee in &attendees {
                if let Ok(info) = contacts.search_contacts(attendee).await {
                    attendee_info.push(format!("{}: {}", attendee, info));
                }
            }
        }

        let attendee_str = if attendee_info.is_empty() {
            attendees.join(", ")
        } else {
            attendee_info.join("\n")
        };

        Ok(format!(
            "Meeting Request:\n\
             - Title: {}\n\
             - Duration: {} minutes\n\
             - Preferred time: {}\n\
             - Attendees: {}\n\
             - Send invites: {}\n\
             - Notes: {}\n\n\
             Current Calendar (next 7 days):\n{}\n\n\
             Please:\n\
             1. Find the best available time slot matching the preference\n\
             2. Create the calendar event using create_calendar_event\n\
             3. {} send invitation emails to attendees using send_email",
            title,
            duration,
            preferred_time,
            attendee_str,
            send_invites,
            notes,
            events,
            if send_invites { "Then" } else { "Do NOT" }
        ))
    }
}

/// Reschedule an existing calendar event
pub struct RescheduleEventTool {
    provider: Box<dyn CalendarProvider>,
}

impl RescheduleEventTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_calendar_provider()
                .expect("Calendar provider not available on this platform"),
        }
    }
}

impl Default for RescheduleEventTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolHandler for RescheduleEventTool {
    fn name(&self) -> &str {
        "reschedule_event"
    }

    fn description(&self) -> &str {
        "Reschedule an existing calendar event. Finds the event by title, checks for conflicts \
         at the new time, and moves it. Can optionally notify attendees of the change."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "event_title": {
                    "type": "string",
                    "description": "Title of the event to reschedule (partial match supported)"
                },
                "new_time": {
                    "type": "string",
                    "description": "New time for the event (ISO8601 or natural language)"
                },
                "notify_attendees": {
                    "type": "boolean",
                    "description": "Whether to notify attendees of the change (default: true)"
                },
                "reason": {
                    "type": "string",
                    "description": "Reason for rescheduling (included in notification)"
                }
            }),
            vec!["event_title", "new_time"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let event_title = input
            .get("event_title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'event_title' parameter"))?;
        let new_time = input
            .get("new_time")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_time' parameter"))?;
        let notify = input
            .get("notify_attendees")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let reason = input
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Schedule conflict");

        if event_title.len() > 500 {
            return Err(anyhow::anyhow!(
                "Event title too long (max 500 characters)"
            ));
        }

        debug!("Rescheduling '{}' to {}", event_title, new_time);

        // Read current calendar to find the event and check conflicts
        let events = self.provider.read_events(14).await?;

        Ok(format!(
            "Reschedule Request:\n\
             - Event: {}\n\
             - New time: {}\n\
             - Notify attendees: {}\n\
             - Reason: {}\n\n\
             Current Calendar:\n{}\n\n\
             Please:\n\
             1. Find the event matching '{}' in the calendar above\n\
             2. Check for conflicts at the new time ({})\n\
             3. Delete the old event and create a new one at the new time\n\
             4. {} notify attendees via email about the change",
            event_title,
            new_time,
            notify,
            reason,
            events,
            event_title,
            new_time,
            if notify { "Then" } else { "Do NOT" }
        ))
    }
}

/// Generate a daily briefing
pub struct DailyBriefingTool {
    calendar: Box<dyn CalendarProvider>,
    email: Box<dyn EmailProvider>,
    db: Arc<KnowledgeDb>,
}

impl DailyBriefingTool {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self {
            calendar: crate::platform::create_calendar_provider()
                .expect("Calendar provider not available on this platform"),
            email: crate::platform::create_email_provider()
                .expect("Email provider not available on this platform"),
            db,
        }
    }
}

#[async_trait]
impl ToolHandler for DailyBriefingTool {
    fn name(&self) -> &str {
        "daily_briefing"
    }

    fn description(&self) -> &str {
        "Generate a comprehensive daily briefing. Combines today's calendar events, unread emails, \
         pending tasks, active watchers, and any relevant knowledge graph context into a morning \
         digest. Perfect for starting the day informed."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "include_emails": {
                    "type": "boolean",
                    "description": "Include unread email summary (default: true)"
                },
                "include_tasks": {
                    "type": "boolean",
                    "description": "Include pending tasks (default: true)"
                },
                "include_weather": {
                    "type": "boolean",
                    "description": "Include weather forecast if available (default: true)"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let include_emails = input
            .get("include_emails")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let include_tasks = input
            .get("include_tasks")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        debug!("Generating daily briefing");

        // Get today's calendar
        let calendar = self.calendar.read_events(1).await?;

        // Get recent emails
        let emails = if include_emails {
            self.email.read_emails(10, "inbox", None).await?
        } else {
            "Email summary skipped.".to_string()
        };

        // Get pending tasks from knowledge graph
        let tasks = if include_tasks {
            let task_entities = self
                .db
                .search_entities("", Some("task"))
                .await
                .unwrap_or_default();
            if task_entities.is_empty() {
                "No pending tasks.".to_string()
            } else {
                task_entities
                    .iter()
                    .take(10)
                    .map(|e| {
                        let meta = e
                            .metadata
                            .as_ref()
                            .map(|m| m.to_string())
                            .unwrap_or_default();
                        format!("- {} {}", e.name, meta)
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        } else {
            "Task summary skipped.".to_string()
        };

        // Get active goals
        let goals = self
            .db
            .get_active_goals()
            .await
            .unwrap_or_default();
        let goals_str = if goals.is_empty() {
            "No active goals.".to_string()
        } else {
            goals
                .iter()
                .take(5)
                .map(|g| format!("- [P{}] {}", g.priority, g.description))
                .collect::<Vec<_>>()
                .join("\n")
        };

        Ok(format!(
            "# Daily Briefing — {}\n\n\
             ## Today's Calendar\n{}\n\n\
             ## Unread Emails\n{}\n\n\
             ## Pending Tasks\n{}\n\n\
             ## Active Goals\n{}\n\n\
             Please compile this into a concise, actionable morning briefing.",
            chrono::Local::now().format("%A, %B %d, %Y"),
            calendar,
            emails,
            tasks,
            goals_str
        ))
    }
}

/// Generate a weekly review
pub struct WeeklyReviewTool {
    calendar: Box<dyn CalendarProvider>,
    db: Arc<KnowledgeDb>,
}

impl WeeklyReviewTool {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self {
            calendar: crate::platform::create_calendar_provider()
                .expect("Calendar provider not available on this platform"),
            db,
        }
    }
}

#[async_trait]
impl ToolHandler for WeeklyReviewTool {
    fn name(&self) -> &str {
        "weekly_review"
    }

    fn description(&self) -> &str {
        "Generate a weekly review summarizing the past week and planning the next. Reviews \
         completed tasks, meetings attended, goals progress, and upcoming commitments."
    }

    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({}), vec![])
    }

    async fn execute(&self, _input: Value) -> Result<String> {
        debug!("Generating weekly review");

        // Get next week's calendar
        let upcoming = self.calendar.read_events(7).await?;

        // Get completed actions from action log
        let actions = self
            .db
            .get_recent_actions(20)
            .await
            .unwrap_or_default();
        let actions_str = if actions.is_empty() {
            "No logged actions this week.".to_string()
        } else {
            actions
                .iter()
                .map(|a| format!("- [{}] {} — {}", a.outcome, a.action_type, a.description))
                .collect::<Vec<_>>()
                .join("\n")
        };

        // Get goal progress
        let goals = self.db.get_active_goals().await.unwrap_or_default();
        let goals_str = if goals.is_empty() {
            "No active goals.".to_string()
        } else {
            goals
                .iter()
                .map(|g| {
                    format!(
                        "- [P{}] {} (status: {})",
                        g.priority, g.description, g.status
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        Ok(format!(
            "# Weekly Review — Week of {}\n\n\
             ## Actions Taken This Week\n{}\n\n\
             ## Goal Progress\n{}\n\n\
             ## Upcoming Week Calendar\n{}\n\n\
             Please compile a weekly review including:\n\
             1. **Accomplishments** — what was completed\n\
             2. **In Progress** — what's still being worked on\n\
             3. **Blockers** — what's stuck and why\n\
             4. **Next Week Priorities** — top 3-5 items to focus on\n\
             5. **Calendar Prep** — meetings to prepare for",
            chrono::Local::now().format("%B %d, %Y"),
            actions_str,
            goals_str,
            upcoming
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn test_find_free_time_schema() {
        let tool = FindFreeTimeTool::new();
        assert_eq!(tool.name(), "find_free_time");
        assert!(!tool.description().is_empty());
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn test_schedule_meeting_schema() {
        let tool = ScheduleMeetingTool::new();
        assert_eq!(tool.name(), "schedule_meeting");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema.get("required").cloned().unwrap_or(serde_json::json!([])),
        )
        .unwrap_or_default();
        assert!(required.contains(&"title".to_string()));
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn test_reschedule_event_schema() {
        let tool = RescheduleEventTool::new();
        assert_eq!(tool.name(), "reschedule_event");
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn test_daily_briefing_schema() {
        let db = Arc::new(
            KnowledgeDb::new(&std::env::temp_dir().join("test_briefing.db")).unwrap(),
        );
        let tool = DailyBriefingTool::new(db);
        assert_eq!(tool.name(), "daily_briefing");
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn test_weekly_review_schema() {
        let db = Arc::new(
            KnowledgeDb::new(&std::env::temp_dir().join("test_weekly.db")).unwrap(),
        );
        let tool = WeeklyReviewTool::new(db);
        assert_eq!(tool.name(), "weekly_review");
    }
}
