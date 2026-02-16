//! Canvas tools — agent-driven visual workspace
//!
//! These tools let the agent push HTML/Markdown content to a canvas
//! rendered in the WebChat UI or companion apps.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;

use super::{ToolHandler, json_schema};

/// Push HTML/Markdown/React content to the canvas
pub struct CanvasPushTool;

impl Default for CanvasPushTool {
    fn default() -> Self {
        Self
    }
}

impl CanvasPushTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolHandler for CanvasPushTool {
    fn name(&self) -> &str {
        "canvas_push"
    }

    fn description(&self) -> &str {
        "Push HTML or Markdown content to the visual canvas. The content will be rendered \
         in the WebChat UI or companion app. Supports HTML, Markdown, Mermaid diagrams, \
         and code blocks. Use this to show visual content like charts, diagrams, tables, \
         or formatted documents."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "content": {
                    "type": "string",
                    "description": "HTML or Markdown content to render on the canvas"
                },
                "content_type": {
                    "type": "string",
                    "enum": ["html", "markdown"],
                    "description": "Type of content being pushed (default: markdown)"
                },
                "title": {
                    "type": "string",
                    "description": "Optional title for the canvas panel"
                },
                "append": {
                    "type": "boolean",
                    "description": "If true, append to existing canvas content instead of replacing"
                }
            }),
            vec!["content"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' parameter"))?;

        let content_type = input
            .get("content_type")
            .and_then(|v| v.as_str())
            .unwrap_or("markdown");

        let title = input.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let append = input.get("append").and_then(|v| v.as_bool()).unwrap_or(false);

        if content.len() > 100_000 {
            return Err(anyhow::anyhow!(
                "Content too large ({} chars, max 100,000)",
                content.len()
            ));
        }

        debug!(
            "Canvas push: type={}, title='{}', append={}, len={}",
            content_type,
            title,
            append,
            content.len()
        );

        // The actual rendering happens client-side via the Gateway event bus.
        // This tool returns a confirmation that the push was dispatched.
        Ok(serde_json::json!({
            "status": "pushed",
            "content_type": content_type,
            "title": title,
            "append": append,
            "content_length": content.len(),
        })
        .to_string())
    }
}

/// Clear the canvas
pub struct CanvasResetTool;

impl Default for CanvasResetTool {
    fn default() -> Self {
        Self
    }
}

impl CanvasResetTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolHandler for CanvasResetTool {
    fn name(&self) -> &str {
        "canvas_reset"
    }

    fn description(&self) -> &str {
        "Clear all content from the visual canvas."
    }

    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({}), vec![])
    }

    async fn execute(&self, _input: Value) -> Result<String> {
        debug!("Canvas reset");
        Ok(r#"{"status": "reset"}"#.to_string())
    }
}

/// Execute JavaScript in the canvas context
pub struct CanvasEvalTool;

impl Default for CanvasEvalTool {
    fn default() -> Self {
        Self
    }
}

impl CanvasEvalTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolHandler for CanvasEvalTool {
    fn name(&self) -> &str {
        "canvas_eval"
    }

    fn description(&self) -> &str {
        "Execute JavaScript code in the canvas context. Use this to update charts, \
         manipulate DOM elements, or run interactive code in the canvas."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "js": {
                    "type": "string",
                    "description": "JavaScript code to execute in the canvas iframe context"
                }
            }),
            vec!["js"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let js = input
            .get("js")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'js' parameter"))?;

        if js.len() > 50_000 {
            return Err(anyhow::anyhow!(
                "JavaScript too large ({} chars, max 50,000)",
                js.len()
            ));
        }

        debug!("Canvas eval: {} chars", js.len());
        Ok(serde_json::json!({
            "status": "evaluated",
            "code_length": js.len(),
        })
        .to_string())
    }
}

/// Request a screenshot of the current canvas state
pub struct CanvasSnapshotTool;

impl Default for CanvasSnapshotTool {
    fn default() -> Self {
        Self
    }
}

impl CanvasSnapshotTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolHandler for CanvasSnapshotTool {
    fn name(&self) -> &str {
        "canvas_snapshot"
    }

    fn description(&self) -> &str {
        "Request a screenshot of the current canvas state. The client will capture \
         the canvas and return it as a base64-encoded image."
    }

    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({}), vec![])
    }

    async fn execute(&self, _input: Value) -> Result<String> {
        debug!("Canvas snapshot requested");
        Ok(serde_json::json!({
            "status": "snapshot_requested",
            "note": "The client will respond with the snapshot via the event bus"
        })
        .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_canvas_push() {
        let tool = CanvasPushTool::new();
        assert_eq!(tool.name(), "canvas_push");

        let result = tool
            .execute(serde_json::json!({
                "content": "<h1>Hello</h1>",
                "content_type": "html",
                "title": "Test"
            }))
            .await
            .unwrap();

        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "pushed");
        assert_eq!(parsed["content_type"], "html");
    }

    #[tokio::test]
    async fn test_canvas_push_missing_content() {
        let tool = CanvasPushTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_canvas_push_too_large() {
        let tool = CanvasPushTool::new();
        let large = "x".repeat(100_001);
        let result = tool
            .execute(serde_json::json!({"content": large}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_canvas_reset() {
        let tool = CanvasResetTool::new();
        assert_eq!(tool.name(), "canvas_reset");
        let result = tool.execute(serde_json::json!({})).await.unwrap();
        assert!(result.contains("reset"));
    }

    #[tokio::test]
    async fn test_canvas_eval() {
        let tool = CanvasEvalTool::new();
        assert_eq!(tool.name(), "canvas_eval");

        let result = tool
            .execute(serde_json::json!({"js": "console.log('hi')"}))
            .await
            .unwrap();

        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "evaluated");
    }

    #[tokio::test]
    async fn test_canvas_eval_missing_js() {
        let tool = CanvasEvalTool::new();
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_canvas_snapshot() {
        let tool = CanvasSnapshotTool::new();
        assert_eq!(tool.name(), "canvas_snapshot");
        let result = tool.execute(serde_json::json!({})).await.unwrap();
        assert!(result.contains("snapshot_requested"));
    }

    #[test]
    fn test_canvas_push_schema() {
        let tool = CanvasPushTool::new();
        let schema = tool.input_schema();
        assert!(schema["properties"]["content"].is_object());
        assert!(schema["required"].as_array().unwrap().contains(&Value::String("content".to_string())));
    }
}
