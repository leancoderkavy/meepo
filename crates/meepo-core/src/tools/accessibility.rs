//! Accessibility API tools for UI automation

use async_trait::async_trait;
use serde_json::Value;
use anyhow::{Result, Context};
use tokio::process::Command;
use std::time::Duration;
use tracing::{debug, warn};

use super::{ToolHandler, json_schema};
use super::macos::sanitize_applescript_string;

/// Allowlist of valid AppleScript UI element types
const VALID_ELEMENT_TYPES: &[&str] = &[
    "button", "checkbox", "radio button", "text field", "text area",
    "pop up button", "menu item", "menu button", "slider", "tab group",
    "table", "outline", "list", "scroll area", "group", "window",
    "sheet", "toolbar", "static text", "image", "link", "cell", "row",
    "column", "combo box", "incrementor", "relevance indicator",
];

/// Read screen information (focused app and window)
pub struct ReadScreenTool;

#[async_trait]
impl ToolHandler for ReadScreenTool {
    fn name(&self) -> &str {
        "read_screen"
    }

    fn description(&self) -> &str {
        "Get information about the currently focused application and window title."
    }

    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({}), vec![])
    }

    async fn execute(&self, _input: Value) -> Result<String> {
        debug!("Reading screen information");

        let script = r#"
tell application "System Events"
    try
        set frontApp to first application process whose frontmost is true
        set appName to name of frontApp
        try
            set windowTitle to name of front window of frontApp
            return "App: " & appName & "\nWindow: " & windowTitle
        on error
            return "App: " & appName & "\nWindow: (no window)"
        end try
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#;

        let output = tokio::time::timeout(
            Duration::from_secs(30),
            Command::new("osascript")
                .arg("-e")
                .arg(script)
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Accessibility command timed out after 30 seconds"))?
        .context("Failed to execute osascript")?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(result)
        } else {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            warn!("Failed to read screen: {}", error);
            Err(anyhow::anyhow!("Failed to read screen: {}", error))
        }
    }
}

/// Click UI element by description
pub struct ClickElementTool;

#[async_trait]
impl ToolHandler for ClickElementTool {
    fn name(&self) -> &str {
        "click_element"
    }

    fn description(&self) -> &str {
        "Click a UI element by its description. Works with buttons, menu items, etc. in the frontmost application."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "element_name": {
                    "type": "string",
                    "description": "Name or description of the UI element to click"
                },
                "element_type": {
                    "type": "string",
                    "description": "Type of element: 'button', 'menu_item', etc. (default: button)"
                }
            }),
            vec!["element_name"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let element_name = input.get("element_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'element_name' parameter"))?;
        let element_type = input.get("element_type")
            .and_then(|v| v.as_str())
            .unwrap_or("button");

        // Validate element_type against allowlist (case-insensitive)
        if !VALID_ELEMENT_TYPES.iter().any(|&valid| valid.eq_ignore_ascii_case(element_type)) {
            return Err(anyhow::anyhow!("Invalid element type: {}", element_type));
        }

        debug!("Clicking {} element: {}", element_type, element_name);

        // Sanitize input to prevent AppleScript injection
        let safe_element_name = sanitize_applescript_string(element_name);

        let script = format!(r#"
tell application "System Events"
    try
        set frontApp to first application process whose frontmost is true
        tell frontApp
            click {} "{}"
        end tell
        return "Clicked successfully"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#, element_type, safe_element_name);

        let output = tokio::time::timeout(
            Duration::from_secs(30),
            Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Accessibility command timed out after 30 seconds"))?
        .context("Failed to execute osascript")?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(result)
        } else {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            warn!("Failed to click element: {}", error);
            Err(anyhow::anyhow!("Failed to click element: {}", error))
        }
    }
}

/// Type text using keyboard simulation
pub struct TypeTextTool;

#[async_trait]
impl ToolHandler for TypeTextTool {
    fn name(&self) -> &str {
        "type_text"
    }

    fn description(&self) -> &str {
        "Type text into the currently focused application using keyboard simulation."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "text": {
                    "type": "string",
                    "description": "Text to type"
                }
            }),
            vec!["text"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let text = input.get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'text' parameter"))?;

        debug!("Typing text ({} chars)", text.len());

        // Sanitize input to prevent AppleScript injection
        let safe_text = sanitize_applescript_string(text);

        let script = format!(r#"
tell application "System Events"
    try
        keystroke "{}"
        return "Text typed successfully"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#, safe_text.replace('\n', "\" & return & \""));

        let output = tokio::time::timeout(
            Duration::from_secs(30),
            Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Accessibility command timed out after 30 seconds"))?
        .context("Failed to execute osascript")?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(result)
        } else {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            warn!("Failed to type text: {}", error);
            Err(anyhow::anyhow!("Failed to type text: {}", error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolHandler;

    #[test]
    fn test_read_screen_schema() {
        let tool = ReadScreenTool;
        assert_eq!(tool.name(), "read_screen");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_click_element_schema() {
        let tool = ClickElementTool;
        assert_eq!(tool.name(), "click_element");
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_type_text_schema() {
        let tool = TypeTextTool;
        assert_eq!(tool.name(), "type_text");
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[tokio::test]
    async fn test_click_element_missing_params() {
        let tool = ClickElementTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_type_text_missing_params() {
        let tool = TypeTextTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_accessibility_uses_sanitization() {
        // Verify that sanitize_applescript_string handles special characters
        let malicious_input = "test\"; do shell script \"rm -rf /\" --\"";
        let sanitized = sanitize_applescript_string(malicious_input);

        // Should have escaped quotes and removed/replaced problematic characters
        assert!(sanitized.contains("\\\""));
        assert!(!sanitized.contains('\n'));

        // Test that newlines are replaced with spaces
        let with_newlines = "line1\nline2\rline3";
        let sanitized = sanitize_applescript_string(with_newlines);
        assert!(!sanitized.contains('\n'));
        assert!(!sanitized.contains('\r'));
    }
}
