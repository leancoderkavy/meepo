//! System interaction tools

use async_trait::async_trait;
use serde_json::Value;
use anyhow::{Result, Context};
use tokio::process::Command;
use std::path::Path;
use tracing::{debug, warn};

use super::{ToolHandler, json_schema};

/// Run a shell command (with safety checks)
pub struct RunCommandTool;

#[async_trait]
impl ToolHandler for RunCommandTool {
    fn name(&self) -> &str {
        "run_command"
    }

    fn description(&self) -> &str {
        "Run a shell command safely. Some dangerous commands are blocked for safety."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory (default: current directory)"
                }
            }),
            vec!["command"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let command = input.get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' parameter"))?;
        let working_dir = input.get("working_dir")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        // Safety checks
        let dangerous_patterns = [
            "rm -rf /",
            "rm -rf /*",
            "sudo rm",
            "mkfs",
            "dd if=",
            "> /dev/",
            ":(){ :|:& };:",
        ];

        for pattern in &dangerous_patterns {
            if command.contains(pattern) {
                warn!("Blocked dangerous command: {}", command);
                return Err(anyhow::anyhow!("Command blocked for safety: contains '{}'", pattern));
            }
        }

        debug!("Running command: {} (in {})", command, working_dir);

        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(working_dir)
            .output()
            .await
            .context("Failed to execute command")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str("STDOUT:\n");
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n\n");
            }
            result.push_str("STDERR:\n");
            result.push_str(&stderr);
        }

        if !output.status.success() {
            result.push_str(&format!("\n\nExit code: {}", output.status.code().unwrap_or(-1)));
        }

        Ok(result)
    }
}

/// Read file from disk
pub struct ReadFileTool;

#[async_trait]
impl ToolHandler for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file from disk."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                }
            }),
            vec!["path"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let path = input.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        debug!("Reading file: {}", path);

        let content = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read file: {}", path))?;

        Ok(content)
    }
}

/// Write file to disk
pub struct WriteFileTool;

#[async_trait]
impl ToolHandler for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file on disk. Creates parent directories if needed."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            }),
            vec!["path", "content"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let path = input.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;
        let content = input.get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' parameter"))?;

        debug!("Writing file: {} ({} bytes)", path, content.len());

        // Create parent directories if needed
        if let Some(parent) = Path::new(path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create parent directories")?;
        }

        tokio::fs::write(path, content)
            .await
            .with_context(|| format!("Failed to write file: {}", path))?;

        Ok(format!("Successfully wrote {} bytes to {}", content.len(), path))
    }
}

/// Fetch URL content
pub struct BrowseUrlTool;

#[async_trait]
impl ToolHandler for BrowseUrlTool {
    fn name(&self) -> &str {
        "browse_url"
    }

    fn description(&self) -> &str {
        "Fetch content from a URL and return the text. Useful for reading web pages, APIs, etc."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional HTTP headers to include"
                }
            }),
            vec!["url"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let url = input.get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?;

        debug!("Fetching URL: {}", url);

        let client = reqwest::Client::builder()
            .user_agent("meepo-agent/1.0")
            .build()
            .context("Failed to create HTTP client")?;

        let mut request = client.get(url);

        // Add custom headers if provided
        if let Some(headers) = input.get("headers").and_then(|v| v.as_object()) {
            for (key, value) in headers {
                if let Some(value_str) = value.as_str() {
                    request = request.header(key, value_str);
                }
            }
        }

        let response = request.send()
            .await
            .context("Failed to fetch URL")?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!("HTTP request failed with status: {}", status));
        }

        let content = response.text()
            .await
            .context("Failed to read response body")?;

        // Truncate if too long
        const MAX_LENGTH: usize = 50000;
        if content.len() > MAX_LENGTH {
            Ok(format!("{}\n\n[Content truncated at {} chars]",
                       &content[..MAX_LENGTH], MAX_LENGTH))
        } else {
            Ok(content)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolHandler;
    use tempfile::TempDir;

    #[test]
    fn test_run_command_schema() {
        let tool = RunCommandTool;
        assert_eq!(tool.name(), "run_command");
        assert!(!tool.description().is_empty());
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_read_file_schema() {
        let tool = ReadFileTool;
        assert_eq!(tool.name(), "read_file");
    }

    #[test]
    fn test_write_file_schema() {
        let tool = WriteFileTool;
        assert_eq!(tool.name(), "write_file");
    }

    #[test]
    fn test_browse_url_schema() {
        let tool = BrowseUrlTool;
        assert_eq!(tool.name(), "browse_url");
    }

    #[tokio::test]
    async fn test_run_command_echo() {
        let tool = RunCommandTool;
        let result = tool.execute(serde_json::json!({
            "command": "echo hello_meepo_test"
        })).await.unwrap();
        assert!(result.contains("hello_meepo_test"));
    }

    #[tokio::test]
    async fn test_run_command_missing_param() {
        let tool = RunCommandTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_command_blocks_dangerous() {
        let tool = RunCommandTool;
        let result = tool.execute(serde_json::json!({
            "command": "rm -rf /"
        })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_write_and_read_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        let path_str = path.to_str().unwrap();

        let write_tool = WriteFileTool;
        let result = write_tool.execute(serde_json::json!({
            "path": path_str,
            "content": "hello from meepo"
        })).await.unwrap();
        assert!(result.contains("Wrote") || result.contains("wrote") || result.contains("bytes"));

        let read_tool = ReadFileTool;
        let result = read_tool.execute(serde_json::json!({
            "path": path_str
        })).await.unwrap();
        assert_eq!(result.trim(), "hello from meepo");
    }

    #[tokio::test]
    async fn test_read_file_missing() {
        let tool = ReadFileTool;
        let result = tool.execute(serde_json::json!({
            "path": "/tmp/nonexistent_meepo_test_file_xyz"
        })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_file_missing_param() {
        let tool = ReadFileTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }
}
