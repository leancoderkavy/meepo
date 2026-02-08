//! Accessibility API tools for UI automation

use async_trait::async_trait;
use serde_json::Value;
use anyhow::{Result, Context};
use tokio::process::Command;
use tracing::{debug, warn};

use super::{ToolHandler, json_schema};

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

        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .await
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

        debug!("Clicking {} element: {}", element_type, element_name);

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
"#, element_type, element_name.replace('"', "\\\""));

        let output = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await
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

        let script = format!(r#"
tell application "System Events"
    try
        keystroke "{}"
        return "Text typed successfully"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#, text.replace('"', "\\\"").replace('\n', "\" & return & \""));

        let output = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await
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
}
