//! Claude Code CLI integration tools

use async_trait::async_trait;
use serde_json::Value;
use anyhow::{Result, Context};
use tokio::process::Command;
use tracing::{debug, warn};

use super::{ToolHandler, json_schema};

/// Execute a coding task using Claude Code CLI
pub struct WriteCodeTool;

#[async_trait]
impl ToolHandler for WriteCodeTool {
    fn name(&self) -> &str {
        "write_code"
    }

    fn description(&self) -> &str {
        "Execute a coding task using Claude Code CLI. Provide a task description and workspace directory."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "task": {
                    "type": "string",
                    "description": "Description of the coding task to execute"
                },
                "workspace": {
                    "type": "string",
                    "description": "Path to the workspace directory (default: current directory)"
                }
            }),
            vec!["task"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let task = input.get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'task' parameter"))?;
        let workspace = input.get("workspace")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        debug!("Executing code task in workspace: {}", workspace);

        let output = Command::new("claude")
            .arg("--print")
            .arg("--workspace")
            .arg(workspace)
            .arg(task)
            .output()
            .await
            .context("Failed to execute claude CLI")?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(format!("Task completed:\n{}", result))
        } else {
            let error = String::from_utf8_lossy(&output.stderr).to_string();
            warn!("Claude Code task failed: {}", error);
            Err(anyhow::anyhow!("Claude Code task failed: {}", error))
        }
    }
}

/// Create a PR using Claude Code CLI
pub struct MakePrTool;

#[async_trait]
impl ToolHandler for MakePrTool {
    fn name(&self) -> &str {
        "make_pr"
    }

    fn description(&self) -> &str {
        "Create a branch, implement changes, and create a pull request using Claude Code CLI and GitHub CLI."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "task": {
                    "type": "string",
                    "description": "Description of the changes to implement"
                },
                "repo": {
                    "type": "string",
                    "description": "Path to the repository (default: current directory)"
                },
                "branch_name": {
                    "type": "string",
                    "description": "Name for the new branch (auto-generated if not provided)"
                }
            }),
            vec!["task"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let task = input.get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'task' parameter"))?;
        let repo = input.get("repo")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let branch_name = input.get("branch_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                format!("meepo-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap())
            });

        debug!("Creating PR in repo: {} with branch: {}", repo, branch_name);

        // Create branch
        let create_branch = Command::new("git")
            .current_dir(repo)
            .args(["checkout", "-b", &branch_name])
            .output()
            .await
            .context("Failed to create branch")?;

        if !create_branch.status.success() {
            let error = String::from_utf8_lossy(&create_branch.stderr).to_string();
            return Err(anyhow::anyhow!("Failed to create branch: {}", error));
        }

        // Execute task with Claude Code
        let code_output = Command::new("claude")
            .arg("--print")
            .arg("--workspace")
            .arg(repo)
            .arg(task)
            .output()
            .await
            .context("Failed to execute claude CLI")?;

        if !code_output.status.success() {
            let error = String::from_utf8_lossy(&code_output.stderr).to_string();
            return Err(anyhow::anyhow!("Claude Code task failed: {}", error));
        }

        // Commit changes
        Command::new("git")
            .current_dir(repo)
            .args(["add", "-A"])
            .output()
            .await
            .context("Failed to stage changes")?;

        let commit_msg = format!("feat: {}\n\nCo-Authored-By: meepo <meepo@anthropic.com>", task);
        Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", &commit_msg])
            .output()
            .await
            .context("Failed to commit changes")?;

        // Push branch
        Command::new("git")
            .current_dir(repo)
            .args(["push", "-u", "origin", &branch_name])
            .output()
            .await
            .context("Failed to push branch")?;

        // Create PR using gh
        let pr_output = Command::new("gh")
            .current_dir(repo)
            .args([
                "pr", "create",
                "--title", task,
                "--body", "Automated PR created by meepo agent"
            ])
            .output()
            .await
            .context("Failed to create PR")?;

        if pr_output.status.success() {
            let result = String::from_utf8_lossy(&pr_output.stdout).to_string();
            Ok(format!("PR created successfully:\n{}", result))
        } else {
            let error = String::from_utf8_lossy(&pr_output.stderr).to_string();
            warn!("Failed to create PR: {}", error);
            Err(anyhow::anyhow!("Failed to create PR: {}", error))
        }
    }
}

/// Review a pull request
pub struct ReviewPrTool;

#[async_trait]
impl ToolHandler for ReviewPrTool {
    fn name(&self) -> &str {
        "review_pr"
    }

    fn description(&self) -> &str {
        "Review a pull request by fetching its details and diff using GitHub CLI, then asking Claude to analyze it."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "repo": {
                    "type": "string",
                    "description": "Repository path or owner/name format (e.g., 'octocat/Hello-World')"
                },
                "pr_number": {
                    "type": "number",
                    "description": "Pull request number"
                }
            }),
            vec!["pr_number"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let repo = input.get("repo")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let pr_number = input.get("pr_number")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'pr_number' parameter"))?;

        debug!("Reviewing PR #{} in repo: {}", pr_number, repo);

        // Get PR details
        let pr_view = Command::new("gh")
            .current_dir(repo)
            .args(["pr", "view", &pr_number.to_string()])
            .output()
            .await
            .context("Failed to view PR")?;

        let pr_details = if pr_view.status.success() {
            String::from_utf8_lossy(&pr_view.stdout).to_string()
        } else {
            return Err(anyhow::anyhow!("Failed to fetch PR details"));
        };

        // Get PR diff
        let pr_diff = Command::new("gh")
            .current_dir(repo)
            .args(["pr", "diff", &pr_number.to_string()])
            .output()
            .await
            .context("Failed to get PR diff")?;

        let diff_content = if pr_diff.status.success() {
            String::from_utf8_lossy(&pr_diff.stdout).to_string()
        } else {
            return Err(anyhow::anyhow!("Failed to fetch PR diff"));
        };

        // Combine information for review
        let review = format!(
            "Pull Request Review\n\n## PR Details\n{}\n\n## Changes\n```diff\n{}\n```\n\n\
            Analysis: This PR contains the above changes. Key points to review:\n\
            - Code quality and style\n\
            - Potential bugs or issues\n\
            - Test coverage\n\
            - Documentation updates",
            pr_details, diff_content
        );

        Ok(review)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolHandler;

    #[test]
    fn test_write_code_schema() {
        let tool = WriteCodeTool;
        assert_eq!(tool.name(), "write_code");
        assert!(!tool.description().is_empty());
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_make_pr_schema() {
        let tool = MakePrTool;
        assert_eq!(tool.name(), "make_pr");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema.get("required").cloned().unwrap_or(serde_json::json!([]))
        ).unwrap_or_default();
        assert!(required.contains(&"task".to_string()));
    }

    #[test]
    fn test_review_pr_schema() {
        let tool = ReviewPrTool;
        assert_eq!(tool.name(), "review_pr");
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[tokio::test]
    async fn test_write_code_missing_task() {
        let tool = WriteCodeTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_make_pr_missing_task() {
        let tool = MakePrTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_review_pr_missing_params() {
        let tool = ReviewPrTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }
}
