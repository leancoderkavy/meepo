//! UI automation and accessibility tools
//!
//! These tools delegate to platform-specific implementations through the platform module.
//! On macOS: AppleScript System Events-based implementations.
//! On Windows: UI Automation-based implementations.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;

use super::{ToolHandler, json_schema};
use crate::platform::UiAutomation;

/// Allowlist of valid UI element types
const VALID_ELEMENT_TYPES: &[&str] = &[
    "button",
    "checkbox",
    "radio button",
    "text field",
    "text area",
    "pop up button",
    "menu item",
    "menu button",
    "slider",
    "tab group",
    "table",
    "outline",
    "list",
    "scroll area",
    "group",
    "window",
    "sheet",
    "toolbar",
    "static text",
    "image",
    "link",
    "cell",
    "row",
    "column",
    "combo box",
    "incrementor",
    "relevance indicator",
];

/// Read screen information (focused app and window)
pub struct ReadScreenTool {
    provider: Box<dyn UiAutomation>,
}

impl Default for ReadScreenTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadScreenTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_ui_automation()
                .expect("UI automation not available on this platform"),
        }
    }
}

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
        self.provider.read_screen().await
    }
}

/// Click UI element by description
pub struct ClickElementTool {
    provider: Box<dyn UiAutomation>,
}

impl Default for ClickElementTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ClickElementTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_ui_automation()
                .expect("UI automation not available on this platform"),
        }
    }
}

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
        let element_name = input
            .get("element_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'element_name' parameter"))?;
        let element_type = input
            .get("element_type")
            .and_then(|v| v.as_str())
            .unwrap_or("button");

        // Input validation: validate element_type against allowlist and normalize to canonical lowercase form
        let element_type_normalized = VALID_ELEMENT_TYPES
            .iter()
            .find(|&&valid| valid.eq_ignore_ascii_case(element_type))
            .ok_or_else(|| anyhow::anyhow!("Invalid element type: {}", element_type))?;

        debug!(
            "Clicking {} element: {}",
            element_type_normalized, element_name
        );
        self.provider
            .click_element(element_name, element_type_normalized)
            .await
    }
}

/// Type text using keyboard simulation
pub struct TypeTextTool {
    provider: Box<dyn UiAutomation>,
}

impl Default for TypeTextTool {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeTextTool {
    pub fn new() -> Self {
        Self {
            provider: crate::platform::create_ui_automation()
                .expect("UI automation not available on this platform"),
        }
    }
}

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
        let text = input
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'text' parameter"))?;

        // Input validation: text length limit
        if text.len() > 50_000 {
            return Err(anyhow::anyhow!(
                "Text too long ({} chars, max 50,000)",
                text.len()
            ));
        }

        debug!("Typing text ({} chars)", text.len());
        self.provider.type_text(text).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolHandler;

    #[test]
    fn test_read_screen_schema() {
        let tool = ReadScreenTool::new();
        assert_eq!(tool.name(), "read_screen");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_click_element_schema() {
        let tool = ClickElementTool::new();
        assert_eq!(tool.name(), "click_element");
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_type_text_schema() {
        let tool = TypeTextTool::new();
        assert_eq!(tool.name(), "type_text");
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[tokio::test]
    async fn test_click_element_missing_params() {
        let tool = ClickElementTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_type_text_missing_params() {
        let tool = TypeTextTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }
}
