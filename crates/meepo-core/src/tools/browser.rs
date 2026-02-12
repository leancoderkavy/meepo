//! Browser automation tools for Safari and Chrome
//!
//! These tools delegate to platform-specific BrowserProvider implementations.
//! On macOS: AppleScript-based Safari and Chrome automation.

use async_trait::async_trait;
use serde_json::Value;
use anyhow::Result;
use tracing::debug;

use super::{ToolHandler, json_schema};
use crate::platform::BrowserProvider;

/// List all open browser tabs
pub struct BrowserListTabsTool {
    provider: Box<dyn BrowserProvider>,
}

impl BrowserListTabsTool {
    pub fn new(browser: &str) -> Self {
        Self {
            provider: crate::platform::create_browser_provider_for(browser),
        }
    }
}

#[async_trait]
impl ToolHandler for BrowserListTabsTool {
    fn name(&self) -> &str {
        "browser_list_tabs"
    }

    fn description(&self) -> &str {
        "List all open tabs across all browser windows. Returns tab ID, title, URL, and active status for each tab."
    }

    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({}), vec![])
    }

    async fn execute(&self, _input: Value) -> Result<String> {
        debug!("Listing browser tabs");
        let tabs = self.provider.list_tabs().await?;
        if tabs.is_empty() {
            return Ok("No open tabs found".to_string());
        }
        let output = tabs.iter().map(|t| {
            format!("Tab: {}\n  Title: {}\n  URL: {}\n  Active: {}\n  Window: {}",
                    t.id, t.title, t.url, t.is_active, t.window_index)
        }).collect::<Vec<_>>().join("\n---\n");
        Ok(output)
    }
}

/// Open a new browser tab with a URL
pub struct BrowserOpenTabTool {
    provider: Box<dyn BrowserProvider>,
}

impl BrowserOpenTabTool {
    pub fn new(browser: &str) -> Self {
        Self {
            provider: crate::platform::create_browser_provider_for(browser),
        }
    }
}

#[async_trait]
impl ToolHandler for BrowserOpenTabTool {
    fn name(&self) -> &str {
        "browser_open_tab"
    }

    fn description(&self) -> &str {
        "Open a new browser tab with the specified URL. The browser will be activated and the new tab will become the active tab."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "url": {
                    "type": "string",
                    "description": "URL to open in the new tab"
                }
            }),
            vec!["url"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let url = input.get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?;

        if url.len() > 2048 {
            return Err(anyhow::anyhow!("URL too long (max 2048 characters)"));
        }

        debug!("Opening browser tab: {}", url);
        let tab = self.provider.open_tab(url).await?;
        Ok(format!("Opened tab: {} ({})", tab.title, tab.url))
    }
}

/// Close a browser tab
pub struct BrowserCloseTabTool {
    provider: Box<dyn BrowserProvider>,
}

impl BrowserCloseTabTool {
    pub fn new(browser: &str) -> Self {
        Self {
            provider: crate::platform::create_browser_provider_for(browser),
        }
    }
}

#[async_trait]
impl ToolHandler for BrowserCloseTabTool {
    fn name(&self) -> &str {
        "browser_close_tab"
    }

    fn description(&self) -> &str {
        "Close a browser tab by its tab ID. Use browser_list_tabs to get tab IDs."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "tab_id": {
                    "type": "string",
                    "description": "Tab ID to close (e.g., 'safari:1:2' or 'chrome:1:3')"
                }
            }),
            vec!["tab_id"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'tab_id' parameter"))?;

        debug!("Closing browser tab: {}", tab_id);
        self.provider.close_tab(tab_id).await?;
        Ok(format!("Closed tab: {}", tab_id))
    }
}

/// Switch to a browser tab
pub struct BrowserSwitchTabTool {
    provider: Box<dyn BrowserProvider>,
}

impl BrowserSwitchTabTool {
    pub fn new(browser: &str) -> Self {
        Self {
            provider: crate::platform::create_browser_provider_for(browser),
        }
    }
}

#[async_trait]
impl ToolHandler for BrowserSwitchTabTool {
    fn name(&self) -> &str {
        "browser_switch_tab"
    }

    fn description(&self) -> &str {
        "Switch to a specific browser tab by its tab ID."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "tab_id": {
                    "type": "string",
                    "description": "Tab ID to switch to (e.g., 'safari:1:2' or 'chrome:1:3')"
                }
            }),
            vec!["tab_id"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'tab_id' parameter"))?;

        debug!("Switching to browser tab: {}", tab_id);
        self.provider.switch_tab(tab_id).await?;
        Ok(format!("Switched to tab: {}", tab_id))
    }
}

/// Get page content from a browser tab
pub struct BrowserGetPageContentTool {
    provider: Box<dyn BrowserProvider>,
}

impl BrowserGetPageContentTool {
    pub fn new(browser: &str) -> Self {
        Self {
            provider: crate::platform::create_browser_provider_for(browser),
        }
    }
}

#[async_trait]
impl ToolHandler for BrowserGetPageContentTool {
    fn name(&self) -> &str {
        "browser_get_page_content"
    }

    fn description(&self) -> &str {
        "Get the text content, title, and URL of a browser tab. Defaults to the active tab if no tab_id is specified."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "tab_id": {
                    "type": "string",
                    "description": "Tab ID to read (default: active tab)"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());

        debug!("Getting page content from browser tab: {:?}", tab_id);
        let content = self.provider.get_page_content(tab_id).await?;
        Ok(format!("Title: {}\nURL: {}\n\n{}", content.title, content.url, content.text))
    }
}

/// Execute JavaScript in a browser tab
pub struct BrowserExecuteJsTool {
    provider: Box<dyn BrowserProvider>,
}

impl BrowserExecuteJsTool {
    pub fn new(browser: &str) -> Self {
        Self {
            provider: crate::platform::create_browser_provider_for(browser),
        }
    }
}

#[async_trait]
impl ToolHandler for BrowserExecuteJsTool {
    fn name(&self) -> &str {
        "browser_execute_js"
    }

    fn description(&self) -> &str {
        "Execute JavaScript code in a browser tab. Returns the result of the script. Defaults to the active tab."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "script": {
                    "type": "string",
                    "description": "JavaScript code to execute"
                },
                "tab_id": {
                    "type": "string",
                    "description": "Tab ID to execute in (default: active tab)"
                }
            }),
            vec!["script"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let script = input.get("script")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'script' parameter"))?;
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());

        if script.len() > 50_000 {
            return Err(anyhow::anyhow!("Script too long ({} chars, max 50,000)", script.len()));
        }

        // Block dangerous JS patterns that could steal credentials or exfiltrate data
        let script_lower = script.to_lowercase();
        let blocked_patterns = [
            ("document.cookie", "accessing cookies"),
            ("localstorage", "accessing localStorage"),
            ("sessionstorage", "accessing sessionStorage"),
            ("indexeddb", "accessing IndexedDB"),
            ("xmlhttprequest", "making network requests"),
            (".fetch(", "making network requests"),
            ("navigator.sendbeacon", "sending beacons"),
            ("new websocket", "opening WebSocket connections"),
            ("new eventsource", "opening EventSource connections"),
            ("importscripts", "importing external scripts"),
            ("eval(", "dynamic code execution"),
            ("function(", "dynamic function creation"),
            ("new function", "dynamic function creation"),
        ];

        for (pattern, reason) in &blocked_patterns {
            if script_lower.contains(pattern) {
                return Err(anyhow::anyhow!(
                    "Script blocked for security: {} is not allowed ({})", pattern, reason
                ));
            }
        }

        debug!("Executing JavaScript in browser tab ({} chars)", script.len());
        self.provider.execute_javascript(tab_id, script).await
    }
}

/// Click an element on a web page by CSS selector
pub struct BrowserClickElementTool {
    provider: Box<dyn BrowserProvider>,
}

impl BrowserClickElementTool {
    pub fn new(browser: &str) -> Self {
        Self {
            provider: crate::platform::create_browser_provider_for(browser),
        }
    }
}

#[async_trait]
impl ToolHandler for BrowserClickElementTool {
    fn name(&self) -> &str {
        "browser_click"
    }

    fn description(&self) -> &str {
        "Click a web page element by CSS selector in the browser. Defaults to the active tab."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "selector": {
                    "type": "string",
                    "description": "CSS selector for the element to click (e.g., '#submit-btn', '.login-button', 'a[href=\"/about\"]')"
                },
                "tab_id": {
                    "type": "string",
                    "description": "Tab ID (default: active tab)"
                }
            }),
            vec!["selector"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let selector = input.get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'selector' parameter"))?;
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());

        if selector.len() > 500 {
            return Err(anyhow::anyhow!("Selector too long (max 500 characters)"));
        }

        debug!("Clicking element: {}", selector);
        self.provider.click_element(tab_id, selector).await?;
        Ok(format!("Clicked element: {}", selector))
    }
}

/// Fill a form field on a web page
pub struct BrowserFillFormTool {
    provider: Box<dyn BrowserProvider>,
}

impl BrowserFillFormTool {
    pub fn new(browser: &str) -> Self {
        Self {
            provider: crate::platform::create_browser_provider_for(browser),
        }
    }
}

#[async_trait]
impl ToolHandler for BrowserFillFormTool {
    fn name(&self) -> &str {
        "browser_fill_form"
    }

    fn description(&self) -> &str {
        "Fill a form field on a web page by CSS selector. Sets the value and dispatches an input event."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "selector": {
                    "type": "string",
                    "description": "CSS selector for the input element (e.g., '#email', 'input[name=\"username\"]')"
                },
                "value": {
                    "type": "string",
                    "description": "Value to fill into the form field"
                },
                "tab_id": {
                    "type": "string",
                    "description": "Tab ID (default: active tab)"
                }
            }),
            vec!["selector", "value"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let selector = input.get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'selector' parameter"))?;
        let value = input.get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'value' parameter"))?;
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());

        if selector.len() > 500 {
            return Err(anyhow::anyhow!("Selector too long (max 500 characters)"));
        }
        if value.len() > 50_000 {
            return Err(anyhow::anyhow!("Value too long (max 50,000 characters)"));
        }

        debug!("Filling form field: {}", selector);
        self.provider.fill_form(tab_id, selector, value).await?;
        Ok(format!("Filled '{}' into {}", value, selector))
    }
}

/// Navigate browser back/forward/reload
pub struct BrowserNavigateTool {
    provider: Box<dyn BrowserProvider>,
}

impl BrowserNavigateTool {
    pub fn new(browser: &str) -> Self {
        Self {
            provider: crate::platform::create_browser_provider_for(browser),
        }
    }
}

#[async_trait]
impl ToolHandler for BrowserNavigateTool {
    fn name(&self) -> &str {
        "browser_navigate"
    }

    fn description(&self) -> &str {
        "Navigate the browser: go back, go forward, or reload the current page."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "action": {
                    "type": "string",
                    "description": "Navigation action: 'back', 'forward', or 'reload'"
                },
                "tab_id": {
                    "type": "string",
                    "description": "Tab ID (default: active tab)"
                }
            }),
            vec!["action"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input.get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());

        match action.to_lowercase().as_str() {
            "back" => {
                debug!("Navigating back");
                self.provider.go_back(tab_id).await?;
                Ok("Navigated back".to_string())
            }
            "forward" => {
                debug!("Navigating forward");
                self.provider.go_forward(tab_id).await?;
                Ok("Navigated forward".to_string())
            }
            "reload" => {
                debug!("Reloading page");
                self.provider.reload(tab_id).await?;
                Ok("Page reloaded".to_string())
            }
            _ => Err(anyhow::anyhow!("Invalid action: {}. Use: back, forward, reload", action)),
        }
    }
}

/// Get the current URL of a browser tab
pub struct BrowserGetUrlTool {
    provider: Box<dyn BrowserProvider>,
}

impl BrowserGetUrlTool {
    pub fn new(browser: &str) -> Self {
        Self {
            provider: crate::platform::create_browser_provider_for(browser),
        }
    }
}

#[async_trait]
impl ToolHandler for BrowserGetUrlTool {
    fn name(&self) -> &str {
        "browser_get_url"
    }

    fn description(&self) -> &str {
        "Get the current URL of a browser tab. Defaults to the active tab."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "tab_id": {
                    "type": "string",
                    "description": "Tab ID (default: active tab)"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());
        debug!("Getting browser URL");
        self.provider.get_page_url(tab_id).await
    }
}

/// Take a screenshot of the browser page
pub struct BrowserScreenshotTool {
    provider: Box<dyn BrowserProvider>,
}

impl BrowserScreenshotTool {
    pub fn new(browser: &str) -> Self {
        Self {
            provider: crate::platform::create_browser_provider_for(browser),
        }
    }
}

#[async_trait]
impl ToolHandler for BrowserScreenshotTool {
    fn name(&self) -> &str {
        "browser_screenshot"
    }

    fn description(&self) -> &str {
        "Take a screenshot of the current browser page. Returns the file path of the saved image."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "path": {
                    "type": "string",
                    "description": "Output file path (default: /tmp/meepo-browser-screenshot-{timestamp}.png)"
                },
                "tab_id": {
                    "type": "string",
                    "description": "Tab ID (default: active tab)"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let path = input.get("path").and_then(|v| v.as_str());
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());

        if let Some(p) = path {
            if !p.ends_with(".png") && !p.ends_with(".jpg") && !p.ends_with(".pdf") {
                return Err(anyhow::anyhow!("Output path must end with .png, .jpg, or .pdf"));
            }
            if p.len() > 500 {
                return Err(anyhow::anyhow!("Path too long (max 500 characters)"));
            }
        }

        debug!("Taking browser screenshot");
        self.provider.screenshot_page(tab_id, path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolHandler;

    #[cfg(target_os = "macos")]
    #[test]
    fn test_browser_list_tabs_schema() {
        let tool = BrowserListTabsTool::new("safari");
        assert_eq!(tool.name(), "browser_list_tabs");
        assert!(!tool.description().is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_browser_open_tab_schema() {
        let tool = BrowserOpenTabTool::new("safari");
        assert_eq!(tool.name(), "browser_open_tab");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema.get("required").cloned().unwrap_or(serde_json::json!([]))
        ).unwrap_or_default();
        assert!(required.contains(&"url".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_browser_execute_js_schema() {
        let tool = BrowserExecuteJsTool::new("chrome");
        assert_eq!(tool.name(), "browser_execute_js");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema.get("required").cloned().unwrap_or(serde_json::json!([]))
        ).unwrap_or_default();
        assert!(required.contains(&"script".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_browser_click_schema() {
        let tool = BrowserClickElementTool::new("safari");
        assert_eq!(tool.name(), "browser_click");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema.get("required").cloned().unwrap_or(serde_json::json!([]))
        ).unwrap_or_default();
        assert!(required.contains(&"selector".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_browser_fill_form_schema() {
        let tool = BrowserFillFormTool::new("safari");
        assert_eq!(tool.name(), "browser_fill_form");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema.get("required").cloned().unwrap_or(serde_json::json!([]))
        ).unwrap_or_default();
        assert!(required.contains(&"selector".to_string()));
        assert!(required.contains(&"value".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_browser_navigate_schema() {
        let tool = BrowserNavigateTool::new("chrome");
        assert_eq!(tool.name(), "browser_navigate");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema.get("required").cloned().unwrap_or(serde_json::json!([]))
        ).unwrap_or_default();
        assert!(required.contains(&"action".to_string()));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_browser_open_tab_missing_url() {
        let tool = BrowserOpenTabTool::new("safari");
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_browser_execute_js_missing_script() {
        let tool = BrowserExecuteJsTool::new("safari");
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_browser_click_missing_selector() {
        let tool = BrowserClickElementTool::new("safari");
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_browser_fill_form_missing_params() {
        let tool = BrowserFillFormTool::new("safari");
        let result = tool.execute(serde_json::json!({"selector": "#test"})).await;
        assert!(result.is_err());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_browser_navigate_invalid_action() {
        let tool = BrowserNavigateTool::new("safari");
        let result = tool.execute(serde_json::json!({"action": "invalid"})).await;
        assert!(result.is_err());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_browser_screenshot_invalid_extension() {
        let tool = BrowserScreenshotTool::new("safari");
        let result = tool.execute(serde_json::json!({"path": "/tmp/test.txt"})).await;
        assert!(result.is_err());
    }
}
