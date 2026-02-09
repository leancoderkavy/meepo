//! Filesystem access tools for browsing and searching local directories

use async_trait::async_trait;
use serde_json::Value;
use anyhow::{Result, Context};
use std::path::{Path, PathBuf};
use tracing::debug;

use super::{ToolHandler, json_schema};

/// Validate that a path is within one of the allowed directories
fn validate_allowed_path(path: &str, allowed_dirs: &[PathBuf]) -> Result<PathBuf> {
    if path.contains("..") {
        return Err(anyhow::anyhow!("Path contains '..' which is not allowed"));
    }

    let expanded = shellexpand(path);
    let canonical = expanded.canonicalize()
        .with_context(|| format!("Path does not exist: {}", expanded.display()))?;

    for allowed in allowed_dirs {
        let allowed_canonical = allowed.canonicalize()
            .unwrap_or_else(|_| allowed.clone());
        if canonical.starts_with(&allowed_canonical) {
            return Ok(canonical);
        }
    }

    Err(anyhow::anyhow!(
        "Access denied: '{}' is not within allowed directories",
        canonical.display()
    ))
}

fn shellexpand(s: &str) -> PathBuf {
    let mut result = s.to_string();
    if result.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            result = format!("{}{}", home.display(), &result[1..]);
        }
    }
    PathBuf::from(result)
}

/// List directory contents
pub struct ListDirectoryTool {
    allowed_dirs: Vec<PathBuf>,
}

impl ListDirectoryTool {
    pub fn new(allowed_dirs: Vec<String>) -> Self {
        Self {
            allowed_dirs: allowed_dirs.iter().map(|d| shellexpand(d)).collect(),
        }
    }
}

#[async_trait]
impl ToolHandler for ListDirectoryTool {
    fn name(&self) -> &str {
        "list_directory"
    }

    fn description(&self) -> &str {
        "List files and directories at a given path. Only accessible within configured allowed directories."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "path": {
                    "type": "string",
                    "description": "Directory path to list (supports ~/)"
                },
                "recursive": {
                    "type": "boolean",
                    "description": "List recursively (default: false, max depth: 3)"
                },
                "pattern": {
                    "type": "string",
                    "description": "Optional glob pattern to filter files (e.g. '*.rs', '*.py')"
                }
            }),
            vec!["path"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let path_str = input.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;
        let recursive = input.get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let pattern = input.get("pattern")
            .and_then(|v| v.as_str());

        let validated_path = validate_allowed_path(path_str, &self.allowed_dirs)?;
        debug!("Listing directory: {}", validated_path.display());

        let mut entries = Vec::new();
        list_dir_recursive(&validated_path, &validated_path, recursive, 0, 3, pattern, &mut entries)?;

        if entries.is_empty() {
            return Ok("Directory is empty or no files match the pattern.".to_string());
        }

        Ok(entries.join("\n"))
    }
}

fn list_dir_recursive(
    base: &Path,
    dir: &Path,
    recursive: bool,
    depth: usize,
    max_depth: usize,
    pattern: Option<&str>,
    entries: &mut Vec<String>,
) -> Result<()> {
    let mut dir_entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory: {}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    dir_entries.sort_by_key(|e| e.file_name());

    for entry in dir_entries {
        let path = entry.path();
        let name = path.strip_prefix(base)
            .unwrap_or(&path)
            .display()
            .to_string();

        // Skip hidden files
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }

        let metadata = entry.metadata()?;

        if metadata.is_dir() {
            entries.push(format!("{}/ (dir)", name));
            if recursive && depth < max_depth {
                list_dir_recursive(base, &path, recursive, depth + 1, max_depth, pattern, entries)?;
            }
        } else {
            // Check pattern if provided
            if let Some(pat) = pattern {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if !glob::Pattern::new(pat)
                    .map(|p| p.matches(&file_name))
                    .unwrap_or(false)
                {
                    continue;
                }
            }

            let size = metadata.len();
            let modified = metadata.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| {
                    chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_default()
                })
                .unwrap_or_default();

            let size_str = if size < 1024 {
                format!("{} B", size)
            } else if size < 1024 * 1024 {
                format!("{:.1} KB", size as f64 / 1024.0)
            } else {
                format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
            };

            entries.push(format!("{} ({}, {})", name, size_str, modified));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_list_directory_tool_schema() {
        let tool = ListDirectoryTool::new(vec!["~/Coding".to_string()]);
        assert_eq!(tool.name(), "list_directory");
        assert!(!tool.description().is_empty());
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[tokio::test]
    async fn test_list_directory_allowed() {
        let temp = TempDir::new().unwrap();
        let temp_path = temp.path().to_str().unwrap().to_string();

        std::fs::write(temp.path().join("hello.rs"), "fn main() {}").unwrap();
        std::fs::write(temp.path().join("world.txt"), "hello world").unwrap();
        std::fs::create_dir(temp.path().join("subdir")).unwrap();

        let tool = ListDirectoryTool::new(vec![temp_path.clone()]);
        let result = tool.execute(serde_json::json!({
            "path": temp_path
        })).await.unwrap();

        assert!(result.contains("hello.rs"));
        assert!(result.contains("world.txt"));
        assert!(result.contains("subdir/"));
    }

    #[tokio::test]
    async fn test_list_directory_pattern_filter() {
        let temp = TempDir::new().unwrap();
        let temp_path = temp.path().to_str().unwrap().to_string();

        std::fs::write(temp.path().join("hello.rs"), "fn main() {}").unwrap();
        std::fs::write(temp.path().join("world.txt"), "hello world").unwrap();

        let tool = ListDirectoryTool::new(vec![temp_path.clone()]);
        let result = tool.execute(serde_json::json!({
            "path": temp_path,
            "pattern": "*.rs"
        })).await.unwrap();

        assert!(result.contains("hello.rs"));
        assert!(!result.contains("world.txt"));
    }

    #[tokio::test]
    async fn test_list_directory_denied() {
        let tool = ListDirectoryTool::new(vec!["~/Coding".to_string()]);
        let result = tool.execute(serde_json::json!({
            "path": "/etc"
        })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_directory_path_traversal_blocked() {
        let tool = ListDirectoryTool::new(vec!["~/Coding".to_string()]);
        let result = tool.execute(serde_json::json!({
            "path": "~/Coding/../../etc"
        })).await;
        assert!(result.is_err());
    }
}
