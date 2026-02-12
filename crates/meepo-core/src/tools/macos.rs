//! Platform-abstracted email, calendar, and system tools
//!
//! These tools delegate to platform-specific implementations through the platform module.
//! On macOS: AppleScript-based implementations.
//! On Windows: PowerShell/COM-based implementations.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;

use super::{ToolHandler, json_schema};
use crate::platform::{
    AppLauncher, CalendarProvider, ClipboardProvider, ContactsProvider, EmailProvider,
    MusicProvider, NotesProvider, NotificationProvider, RemindersProvider, ScreenCaptureProvider,
};

/// Read emails from the default email application
pub struct ReadEmailsTool {
    provider: Box<dyn EmailProvider>,
}

impl Default for ReadEmailsTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadEmailsTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_email_provider()
                .expect("Email provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for ReadEmailsTool {
    fn name(&self) -> &str {
        "read_emails"
    }

    fn description(&self) -> &str {
        "Read recent emails. Returns sender, subject, date, and preview for the latest emails."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "limit": {
                    "type": "number",
                    "description": "Number of emails to retrieve (default: 10, max: 50)"
                },
                "mailbox": {
                    "type": "string",
                    "description": "Mailbox to read from (default: 'inbox'). Options: inbox, sent, drafts, trash"
                },
                "search": {
                    "type": "string",
                    "description": "Optional search term to filter by subject or sender"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .min(50);
        let mailbox = input
            .get("mailbox")
            .and_then(|v| v.as_str())
            .unwrap_or("inbox");
        let search = input.get("search").and_then(|v| v.as_str());

        debug!("Reading {} emails from {}", limit, mailbox);
        self.provider.read_emails(limit, mailbox, search).await
    }
}

/// Read calendar events from the default calendar application
pub struct ReadCalendarTool {
    provider: Box<dyn CalendarProvider>,
}

impl Default for ReadCalendarTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadCalendarTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_calendar_provider()
                .expect("Calendar provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for ReadCalendarTool {
    fn name(&self) -> &str {
        "read_calendar"
    }

    fn description(&self) -> &str {
        "Read upcoming calendar events. Returns today's and upcoming events."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "days_ahead": {
                    "type": "number",
                    "description": "Number of days ahead to look (default: 1)"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let days_ahead = input
            .get("days_ahead")
            .and_then(|v| v.as_u64())
            .unwrap_or(1);

        debug!("Reading calendar events for next {} days", days_ahead);
        self.provider.read_events(days_ahead).await
    }
}

/// Send email via the default email application
pub struct SendEmailTool {
    provider: Box<dyn EmailProvider>,
}

impl Default for SendEmailTool {
    fn default() -> Self {
        Self::new()
    }
}

impl SendEmailTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_email_provider()
                .expect("Email provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for SendEmailTool {
    fn name(&self) -> &str {
        "send_email"
    }

    fn description(&self) -> &str {
        "Send an email. Composes and sends a message to the specified recipient."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "to": {
                    "type": "string",
                    "description": "Recipient email address"
                },
                "subject": {
                    "type": "string",
                    "description": "Email subject"
                },
                "body": {
                    "type": "string",
                    "description": "Email body content"
                },
                "cc": {
                    "type": "string",
                    "description": "Optional CC recipient email address"
                },
                "in_reply_to": {
                    "type": "string",
                    "description": "Optional subject line of email to reply to (enables threading)"
                }
            }),
            vec!["to", "subject", "body"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let to = input
            .get("to")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'to' parameter"))?;
        let subject = input
            .get("subject")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'subject' parameter"))?;
        let body = input
            .get("body")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'body' parameter"))?;
        let cc = input.get("cc").and_then(|v| v.as_str());
        let in_reply_to = input.get("in_reply_to").and_then(|v| v.as_str());

        // Input validation: body length limit
        if body.len() > 50_000 {
            return Err(anyhow::anyhow!(
                "Email body too long ({} chars, max 50,000)",
                body.len()
            ));
        }

        debug!("Sending email to: {}", to);
        self.provider
            .send_email(to, subject, body, cc, in_reply_to)
            .await
    }
}

/// Create a calendar event in the default calendar application
pub struct CreateEventTool {
    provider: Box<dyn CalendarProvider>,
}

impl Default for CreateEventTool {
    fn default() -> Self {
        Self::new()
    }
}

impl CreateEventTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_calendar_provider()
                .expect("Calendar provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for CreateEventTool {
    fn name(&self) -> &str {
        "create_calendar_event"
    }

    fn description(&self) -> &str {
        "Create a new calendar event."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "summary": {
                    "type": "string",
                    "description": "Event title/summary"
                },
                "start_time": {
                    "type": "string",
                    "description": "Start time in ISO8601 format or natural language"
                },
                "duration_minutes": {
                    "type": "number",
                    "description": "Duration in minutes (default: 60)"
                }
            }),
            vec!["summary", "start_time"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let summary = input
            .get("summary")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'summary' parameter"))?;
        let start_time = input
            .get("start_time")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'start_time' parameter"))?;
        let duration = input
            .get("duration_minutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(60);

        debug!("Creating calendar event: {}", summary);
        self.provider
            .create_event(summary, start_time, duration)
            .await
    }
}

/// Open an application by name
pub struct OpenAppTool {
    launcher: Box<dyn AppLauncher>,
}

impl Default for OpenAppTool {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenAppTool {
    pub fn new() -> Self {
        Self {
            launcher: crate::platform::create_app_launcher(),
        }
    }
}

#[async_trait]
impl ToolHandler for OpenAppTool {
    fn name(&self) -> &str {
        "open_app"
    }

    fn description(&self) -> &str {
        "Open an application by name."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "app_name": {
                    "type": "string",
                    "description": "Name of the application to open (e.g., 'Safari', 'Terminal')"
                }
            }),
            vec!["app_name"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let app_name = input
            .get("app_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'app_name' parameter"))?;

        // Input validation: prevent path traversal — only allow app names, not paths
        if app_name.contains('/') || app_name.contains('\\') {
            return Err(anyhow::anyhow!("App name cannot contain path separators"));
        }
        if app_name.len() > 100 {
            return Err(anyhow::anyhow!("App name too long (max 100 characters)"));
        }

        debug!("Opening application: {}", app_name);
        self.launcher.open_app(app_name).await
    }
}

/// Get clipboard content
pub struct GetClipboardTool {
    provider: Box<dyn ClipboardProvider>,
}

impl Default for GetClipboardTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GetClipboardTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_clipboard_provider(),
        }
    }
}

#[async_trait]
impl ToolHandler for GetClipboardTool {
    fn name(&self) -> &str {
        "get_clipboard"
    }

    fn description(&self) -> &str {
        "Get the current content of the system clipboard."
    }

    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({}), vec![])
    }

    async fn execute(&self, _input: Value) -> Result<String> {
        debug!("Reading clipboard content");
        self.provider.get_clipboard().await
    }
}

/// List reminders from Apple Reminders
pub struct ListRemindersTool {
    provider: Box<dyn RemindersProvider>,
}

impl Default for ListRemindersTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ListRemindersTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_reminders_provider()
                .expect("Reminders provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for ListRemindersTool {
    fn name(&self) -> &str {
        "list_reminders"
    }

    fn description(&self) -> &str {
        "List incomplete reminders from Apple Reminders. Optionally specify a list name."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "list_name": {
                    "type": "string",
                    "description": "Reminders list name (default: default list)"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let list_name = input.get("list_name").and_then(|v| v.as_str());
        debug!("Listing reminders");
        self.provider.list_reminders(list_name).await
    }
}

/// Create a reminder in Apple Reminders
pub struct CreateReminderTool {
    provider: Box<dyn RemindersProvider>,
}

impl Default for CreateReminderTool {
    fn default() -> Self {
        Self::new()
    }
}

impl CreateReminderTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_reminders_provider()
                .expect("Reminders provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for CreateReminderTool {
    fn name(&self) -> &str {
        "create_reminder"
    }

    fn description(&self) -> &str {
        "Create a new reminder in Apple Reminders with optional due date and notes."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "name": {
                    "type": "string",
                    "description": "Reminder title"
                },
                "list_name": {
                    "type": "string",
                    "description": "Reminders list name (default: default list)"
                },
                "due_date": {
                    "type": "string",
                    "description": "Due date (e.g., 'February 10, 2026 at 9:00 AM')"
                },
                "notes": {
                    "type": "string",
                    "description": "Additional notes for the reminder"
                }
            }),
            vec!["name"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'name' parameter"))?;
        let list_name = input.get("list_name").and_then(|v| v.as_str());
        let due_date = input.get("due_date").and_then(|v| v.as_str());
        let notes = input.get("notes").and_then(|v| v.as_str());

        if name.len() > 500 {
            return Err(anyhow::anyhow!(
                "Reminder name too long (max 500 characters)"
            ));
        }

        debug!("Creating reminder: {}", name);
        self.provider
            .create_reminder(name, list_name, due_date, notes)
            .await
    }
}

/// List notes from Apple Notes
pub struct ListNotesTool {
    provider: Box<dyn NotesProvider>,
}

impl Default for ListNotesTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ListNotesTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_notes_provider()
                .expect("Notes provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for ListNotesTool {
    fn name(&self) -> &str {
        "list_notes"
    }

    fn description(&self) -> &str {
        "List recent notes from Apple Notes with title, date, and preview."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "folder": {
                    "type": "string",
                    "description": "Notes folder name (default: all notes)"
                },
                "limit": {
                    "type": "number",
                    "description": "Number of notes to return (default: 10, max: 50)"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let folder = input.get("folder").and_then(|v| v.as_str());
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .min(50);

        debug!("Listing {} notes", limit);
        self.provider.list_notes(folder, limit).await
    }
}

/// Create a note in Apple Notes
pub struct CreateNoteTool {
    provider: Box<dyn NotesProvider>,
}

impl Default for CreateNoteTool {
    fn default() -> Self {
        Self::new()
    }
}

impl CreateNoteTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_notes_provider()
                .expect("Notes provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for CreateNoteTool {
    fn name(&self) -> &str {
        "create_note"
    }

    fn description(&self) -> &str {
        "Create a new note in Apple Notes."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "title": {
                    "type": "string",
                    "description": "Note title"
                },
                "body": {
                    "type": "string",
                    "description": "Note body content"
                },
                "folder": {
                    "type": "string",
                    "description": "Notes folder name (default: default folder)"
                }
            }),
            vec!["title", "body"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let title = input
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'title' parameter"))?;
        let body = input
            .get("body")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'body' parameter"))?;
        let folder = input.get("folder").and_then(|v| v.as_str());

        if body.len() > 100_000 {
            return Err(anyhow::anyhow!(
                "Note body too long (max 100,000 characters)"
            ));
        }

        debug!("Creating note: {}", title);
        self.provider.create_note(title, body, folder).await
    }
}

/// Send a macOS notification
pub struct SendNotificationTool {
    provider: Box<dyn NotificationProvider>,
}

impl Default for SendNotificationTool {
    fn default() -> Self {
        Self::new()
    }
}

impl SendNotificationTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_notification_provider()
                .expect("Notification provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for SendNotificationTool {
    fn name(&self) -> &str {
        "send_notification"
    }

    fn description(&self) -> &str {
        "Send a macOS system notification with title and message."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "title": {
                    "type": "string",
                    "description": "Notification title"
                },
                "message": {
                    "type": "string",
                    "description": "Notification message body"
                },
                "sound": {
                    "type": "string",
                    "description": "Sound name (default: 'default')"
                }
            }),
            vec!["title", "message"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let title = input
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'title' parameter"))?;
        let message = input
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'message' parameter"))?;
        let sound = input.get("sound").and_then(|v| v.as_str());

        if title.len() > 200 {
            return Err(anyhow::anyhow!("Title too long (max 200 characters)"));
        }
        if message.len() > 1000 {
            return Err(anyhow::anyhow!("Message too long (max 1,000 characters)"));
        }

        debug!("Sending notification: {}", title);
        self.provider.send_notification(title, message, sound).await
    }
}

/// Capture the screen
pub struct ScreenCaptureTool {
    provider: Box<dyn ScreenCaptureProvider>,
}

impl Default for ScreenCaptureTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenCaptureTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_screen_capture_provider()
                .expect("Screen capture provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for ScreenCaptureTool {
    fn name(&self) -> &str {
        "screen_capture"
    }

    fn description(&self) -> &str {
        "Capture a screenshot of the screen. Returns the file path of the saved image."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "path": {
                    "type": "string",
                    "description": "Output file path (default: /tmp/meepo-screenshot-{timestamp}.png)"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let path = input.get("path").and_then(|v| v.as_str());

        if let Some(p) = path {
            if !p.ends_with(".png") && !p.ends_with(".jpg") && !p.ends_with(".pdf") {
                return Err(anyhow::anyhow!(
                    "Output path must end with .png, .jpg, or .pdf"
                ));
            }
            if p.len() > 500 {
                return Err(anyhow::anyhow!("Path too long (max 500 characters)"));
            }
        }

        debug!("Capturing screen");
        self.provider.capture_screen(path).await
    }
}

/// Get the currently playing track from Apple Music
pub struct GetCurrentTrackTool {
    provider: Box<dyn MusicProvider>,
}

impl Default for GetCurrentTrackTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GetCurrentTrackTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_music_provider()
                .expect("Music provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for GetCurrentTrackTool {
    fn name(&self) -> &str {
        "get_current_track"
    }

    fn description(&self) -> &str {
        "Get the currently playing track from Apple Music, including artist, album, and playback position."
    }

    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({}), vec![])
    }

    async fn execute(&self, _input: Value) -> Result<String> {
        debug!("Getting current track");
        self.provider.get_current_track().await
    }
}

/// Control music playback in Apple Music
pub struct MusicControlTool {
    provider: Box<dyn MusicProvider>,
}

impl Default for MusicControlTool {
    fn default() -> Self {
        Self::new()
    }
}

impl MusicControlTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_music_provider()
                .expect("Music provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for MusicControlTool {
    fn name(&self) -> &str {
        "music_control"
    }

    fn description(&self) -> &str {
        "Control Apple Music playback: play, pause, stop, next, or previous."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "action": {
                    "type": "string",
                    "description": "Playback action: play, pause, stop, next, previous"
                }
            }),
            vec!["action"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        debug!("Music control: {}", action);
        self.provider.control_playback(action).await
    }
}

/// Search contacts in Apple Contacts
pub struct SearchContactsTool {
    provider: Box<dyn ContactsProvider>,
}

impl Default for SearchContactsTool {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchContactsTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_contacts_provider()
                .expect("Contacts provider not available on this platform"),
        }
    }
}

#[async_trait]
impl ToolHandler for SearchContactsTool {
    fn name(&self) -> &str {
        "search_contacts"
    }

    fn description(&self) -> &str {
        "Search for contacts by name in Apple Contacts. Returns name, email, and phone."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "query": {
                    "type": "string",
                    "description": "Search term to match against contact names"
                }
            }),
            vec!["query"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;

        if query.len() > 200 {
            return Err(anyhow::anyhow!("Query too long (max 200 characters)"));
        }

        debug!("Searching contacts: {}", query);
        self.provider.search_contacts(query).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolHandler;

    #[test]
    fn test_read_emails_schema() {
        let tool = ReadEmailsTool::new();
        assert_eq!(tool.name(), "read_emails");
        assert!(!tool.description().is_empty());
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_read_calendar_schema() {
        let tool = ReadCalendarTool::new();
        assert_eq!(tool.name(), "read_calendar");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_send_email_schema() {
        let tool = SendEmailTool::new();
        assert_eq!(tool.name(), "send_email");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema
                .get("required")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .unwrap_or_default();
        assert!(required.contains(&"to".to_string()));
        assert!(required.contains(&"subject".to_string()));
        assert!(required.contains(&"body".to_string()));
    }

    #[test]
    fn test_create_event_schema() {
        let tool = CreateEventTool::new();
        assert_eq!(tool.name(), "create_calendar_event");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema
                .get("required")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .unwrap_or_default();
        assert!(required.contains(&"summary".to_string()));
        assert!(required.contains(&"start_time".to_string()));
    }

    #[test]
    fn test_open_app_schema() {
        let tool = OpenAppTool::new();
        assert_eq!(tool.name(), "open_app");
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_get_clipboard_schema() {
        let tool = GetClipboardTool::new();
        assert_eq!(tool.name(), "get_clipboard");
    }

    #[tokio::test]
    async fn test_send_email_missing_params() {
        let tool = SendEmailTool::new();
        let result = tool
            .execute(serde_json::json!({
                "to": "test@test.com"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_event_missing_params() {
        let tool = CreateEventTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_open_app_missing_params() {
        let tool = OpenAppTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    // --- Reminders ---
    #[cfg(target_os = "macos")]
    #[test]
    fn test_list_reminders_schema() {
        let tool = ListRemindersTool::new();
        assert_eq!(tool.name(), "list_reminders");
        assert!(!tool.description().is_empty());
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_create_reminder_schema() {
        let tool = CreateReminderTool::new();
        assert_eq!(tool.name(), "create_reminder");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema
                .get("required")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .unwrap_or_default();
        assert!(required.contains(&"name".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_create_reminder_missing_name() {
        let tool = CreateReminderTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    // --- Notes ---
    #[cfg(target_os = "macos")]
    #[test]
    fn test_list_notes_schema() {
        let tool = ListNotesTool::new();
        assert_eq!(tool.name(), "list_notes");
        assert!(!tool.description().is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_create_note_schema() {
        let tool = CreateNoteTool::new();
        assert_eq!(tool.name(), "create_note");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema
                .get("required")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .unwrap_or_default();
        assert!(required.contains(&"title".to_string()));
        assert!(required.contains(&"body".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_create_note_missing_params() {
        let tool = CreateNoteTool::new();
        let result = tool.execute(serde_json::json!({"title": "test"})).await;
        assert!(result.is_err());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_create_note_body_too_long() {
        let tool = CreateNoteTool::new();
        let long_body = "x".repeat(100_001);
        let result = tool
            .execute(serde_json::json!({
                "title": "test",
                "body": long_body
            }))
            .await;
        assert!(result.is_err());
    }

    // --- Notifications ---
    #[cfg(target_os = "macos")]
    #[test]
    fn test_send_notification_schema() {
        let tool = SendNotificationTool::new();
        assert_eq!(tool.name(), "send_notification");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema
                .get("required")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .unwrap_or_default();
        assert!(required.contains(&"title".to_string()));
        assert!(required.contains(&"message".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_send_notification_missing_params() {
        let tool = SendNotificationTool::new();
        let result = tool.execute(serde_json::json!({"title": "test"})).await;
        assert!(result.is_err());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_send_notification_title_too_long() {
        let tool = SendNotificationTool::new();
        let long_title = "x".repeat(201);
        let result = tool
            .execute(serde_json::json!({
                "title": long_title,
                "message": "test"
            }))
            .await;
        assert!(result.is_err());
    }

    // --- Screen Capture ---
    #[cfg(target_os = "macos")]
    #[test]
    fn test_screen_capture_schema() {
        let tool = ScreenCaptureTool::new();
        assert_eq!(tool.name(), "screen_capture");
        assert!(!tool.description().is_empty());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_screen_capture_invalid_extension() {
        let tool = ScreenCaptureTool::new();
        let result = tool
            .execute(serde_json::json!({"path": "/tmp/test.txt"}))
            .await;
        assert!(result.is_err());
    }

    // --- Music ---
    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_current_track_schema() {
        let tool = GetCurrentTrackTool::new();
        assert_eq!(tool.name(), "get_current_track");
        assert!(!tool.description().is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_music_control_schema() {
        let tool = MusicControlTool::new();
        assert_eq!(tool.name(), "music_control");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema
                .get("required")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .unwrap_or_default();
        assert!(required.contains(&"action".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_music_control_missing_action() {
        let tool = MusicControlTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    // --- Contacts ---
    #[cfg(target_os = "macos")]
    #[test]
    fn test_search_contacts_schema() {
        let tool = SearchContactsTool::new();
        assert_eq!(tool.name(), "search_contacts");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema
                .get("required")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .unwrap_or_default();
        assert!(required.contains(&"query".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_search_contacts_missing_query() {
        let tool = SearchContactsTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_search_contacts_query_too_long() {
        let tool = SearchContactsTool::new();
        let long_query = "x".repeat(201);
        let result = tool.execute(serde_json::json!({"query": long_query})).await;
        assert!(result.is_err());
    }
}
