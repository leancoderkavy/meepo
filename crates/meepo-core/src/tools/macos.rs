//! macOS-specific tools using AppleScript

use async_trait::async_trait;
use serde_json::Value;
use anyhow::{Result, Context};
use tokio::process::Command;
use tracing::{debug, warn};

use super::{ToolHandler, json_schema};

/// Sanitize a string for safe use in AppleScript
/// Prevents injection attacks by escaping special characters
pub(crate) fn sanitize_applescript_string(input: &str) -> String {
    input
        .replace('\\', "\\\\")  // Escape backslashes first
        .replace('"', "\\\"")   // Escape double quotes
        .replace('\n', " ")     // Replace newlines with spaces
        .replace('\r', " ")     // Replace carriage returns with spaces
        .chars()
        .filter(|&c| c >= ' ' || c == '\t')  // Remove control characters except tab
        .collect()
}

/// Read emails from Mail.app
pub struct ReadEmailsTool;

#[async_trait]
impl ToolHandler for ReadEmailsTool {
    fn name(&self) -> &str {
        "read_emails"
    }

    fn description(&self) -> &str {
        "Read recent emails from Mail.app. Returns sender, subject, date, and preview for the latest emails."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "limit": {
                    "type": "number",
                    "description": "Number of emails to retrieve (default: 10, max: 50)"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let limit = input.get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .min(50);

        debug!("Reading {} emails from Mail.app", limit);

        let script = format!(r#"
tell application "Mail"
    try
        set msgs to messages 1 thru {} of inbox
        set output to ""
        repeat with m in msgs
            set output to output & "From: " & (sender of m) & "\n"
            set output to output & "Subject: " & (subject of m) & "\n"
            set output to output & "Date: " & (date received of m as string) & "\n"
            set output to output & "---\n"
        end repeat
        return output
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#, limit);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("AppleScript execution timed out after 30 seconds"))?
        .context("Failed to execute osascript")?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(result)
        } else {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            warn!("Failed to read emails: {}", error);
            Err(anyhow::anyhow!("Failed to read emails: {}", error))
        }
    }
}

/// Read calendar events
pub struct ReadCalendarTool;

#[async_trait]
impl ToolHandler for ReadCalendarTool {
    fn name(&self) -> &str {
        "read_calendar"
    }

    fn description(&self) -> &str {
        "Read calendar events from Calendar.app. Returns today's and upcoming events."
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
        let days_ahead = input.get("days_ahead")
            .and_then(|v| v.as_u64())
            .unwrap_or(1);

        debug!("Reading calendar events for next {} days", days_ahead);

        let script = format!(r#"
tell application "Calendar"
    try
        set startDate to current date
        set endDate to (current date) + ({} * days)
        set theEvents to (every event of calendar "Calendar" whose start date is greater than or equal to startDate and start date is less than or equal to endDate)
        set output to ""
        repeat with evt in theEvents
            set output to output & "Event: " & (summary of evt) & "\n"
            set output to output & "Start: " & (start date of evt as string) & "\n"
            set output to output & "End: " & (end date of evt as string) & "\n"
            set output to output & "---\n"
        end repeat
        return output
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#, days_ahead);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("AppleScript execution timed out after 30 seconds"))?
        .context("Failed to execute osascript")?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(result)
        } else {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            warn!("Failed to read calendar: {}", error);
            Err(anyhow::anyhow!("Failed to read calendar: {}", error))
        }
    }
}

/// Send email via Mail.app
pub struct SendEmailTool;

#[async_trait]
impl ToolHandler for SendEmailTool {
    fn name(&self) -> &str {
        "send_email"
    }

    fn description(&self) -> &str {
        "Send an email using Mail.app. Composes and sends a message to the specified recipient."
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
                }
            }),
            vec!["to", "subject", "body"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let to = input.get("to")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'to' parameter"))?;
        let subject = input.get("subject")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'subject' parameter"))?;
        let body = input.get("body")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'body' parameter"))?;

        debug!("Sending email to: {}", to);

        // Sanitize inputs to prevent AppleScript injection
        let safe_to = sanitize_applescript_string(to);
        let safe_subject = sanitize_applescript_string(subject);
        let safe_body = sanitize_applescript_string(body);

        let script = format!(r#"
tell application "Mail"
    try
        set newMessage to make new outgoing message with properties {{subject:"{}", content:"{}", visible:true}}
        tell newMessage
            make new to recipient at end of to recipients with properties {{address:"{}"}}
            send
        end tell
        return "Email sent successfully"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#, safe_subject, safe_body, safe_to);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("AppleScript execution timed out after 30 seconds"))?
        .context("Failed to execute osascript")?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(result)
        } else {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            warn!("Failed to send email: {}", error);
            Err(anyhow::anyhow!("Failed to send email: {}", error))
        }
    }
}

/// Create calendar event
pub struct CreateEventTool;

#[async_trait]
impl ToolHandler for CreateEventTool {
    fn name(&self) -> &str {
        "create_calendar_event"
    }

    fn description(&self) -> &str {
        "Create a new calendar event in Calendar.app."
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
        let summary = input.get("summary")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'summary' parameter"))?;
        let start_time = input.get("start_time")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'start_time' parameter"))?;
        let duration = input.get("duration_minutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(60);

        debug!("Creating calendar event: {}", summary);

        // Sanitize inputs to prevent AppleScript injection
        let safe_summary = sanitize_applescript_string(summary);
        let safe_start_time = sanitize_applescript_string(start_time);

        let script = format!(r#"
tell application "Calendar"
    try
        set startDate to date "{}"
        set endDate to startDate + ({} * minutes)
        tell calendar "Calendar"
            make new event with properties {{summary:"{}", start date:startDate, end date:endDate}}
        end tell
        return "Event created successfully"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#, safe_start_time, duration, safe_summary);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("AppleScript execution timed out after 30 seconds"))?
        .context("Failed to execute osascript")?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(result)
        } else {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            warn!("Failed to create event: {}", error);
            Err(anyhow::anyhow!("Failed to create event: {}", error))
        }
    }
}

/// Open application
pub struct OpenAppTool;

#[async_trait]
impl ToolHandler for OpenAppTool {
    fn name(&self) -> &str {
        "open_app"
    }

    fn description(&self) -> &str {
        "Open a macOS application by name."
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
        let app_name = input.get("app_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'app_name' parameter"))?;

        debug!("Opening application: {}", app_name);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("open")
                .arg("-a")
                .arg(app_name)
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Command execution timed out after 30 seconds"))?
        .context("Failed to execute open command")?;

        if output.status.success() {
            Ok(format!("Successfully opened {}", app_name))
        } else {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            warn!("Failed to open app: {}", error);
            Err(anyhow::anyhow!("Failed to open app: {}", error))
        }
    }
}

/// Get clipboard content
pub struct GetClipboardTool;

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

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("pbpaste")
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Command execution timed out after 30 seconds"))?
        .context("Failed to execute pbpaste")?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(result)
        } else {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            warn!("Failed to read clipboard: {}", error);
            Err(anyhow::anyhow!("Failed to read clipboard: {}", error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolHandler;

    #[test]
    fn test_read_emails_schema() {
        let tool = ReadEmailsTool;
        assert_eq!(tool.name(), "read_emails");
        assert!(!tool.description().is_empty());
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_read_calendar_schema() {
        let tool = ReadCalendarTool;
        assert_eq!(tool.name(), "read_calendar");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_send_email_schema() {
        let tool = SendEmailTool;
        assert_eq!(tool.name(), "send_email");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema.get("required").cloned().unwrap_or(serde_json::json!([]))
        ).unwrap_or_default();
        assert!(required.contains(&"to".to_string()));
        assert!(required.contains(&"subject".to_string()));
        assert!(required.contains(&"body".to_string()));
    }

    #[test]
    fn test_create_event_schema() {
        let tool = CreateEventTool;
        assert_eq!(tool.name(), "create_calendar_event");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema.get("required").cloned().unwrap_or(serde_json::json!([]))
        ).unwrap_or_default();
        assert!(required.contains(&"summary".to_string()));
        assert!(required.contains(&"start_time".to_string()));
    }

    #[test]
    fn test_open_app_schema() {
        let tool = OpenAppTool;
        assert_eq!(tool.name(), "open_app");
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_get_clipboard_schema() {
        let tool = GetClipboardTool;
        assert_eq!(tool.name(), "get_clipboard");
    }

    #[test]
    fn test_sanitize_applescript_string() {
        // Test backslash escaping
        assert_eq!(sanitize_applescript_string("test\\path"), "test\\\\path");

        // Test quote escaping
        assert_eq!(sanitize_applescript_string("test\"quote"), "test\\\"quote");

        // Test newline replacement
        assert_eq!(sanitize_applescript_string("test\nline"), "test line");
        assert_eq!(sanitize_applescript_string("test\rline"), "test line");

        // Test control character removal
        let with_control = "test\x01\x02\x03text";
        assert_eq!(sanitize_applescript_string(with_control), "testtext");

        // Test combined attack string
        let attack = "test\"; do shell script \"rm -rf /\" --\"";
        let safe = sanitize_applescript_string(attack);
        assert!(!safe.contains('\n'));
        assert!(safe.contains("\\\""));
    }

    #[tokio::test]
    async fn test_send_email_missing_params() {
        let tool = SendEmailTool;
        let result = tool.execute(serde_json::json!({
            "to": "test@test.com"
        })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_event_missing_params() {
        let tool = CreateEventTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_open_app_missing_params() {
        let tool = OpenAppTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }
}
