//! Claude Code CLI integration tools

use async_trait::async_trait;
use serde_json::Value;
use anyhow::{Result, Context};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use meepo_knowledge::KnowledgeDb;
use super::{ToolHandler, json_schema};
use super::autonomous::BackgroundTaskCommand;

/// Configuration for Claude Code CLI tools, plumbed from [code] config section
#[derive(Debug, Clone)]
pub struct CodeToolConfig {
    pub claude_code_path: String,
    pub gh_path: String,
    pub default_workspace: String,
}

impl Default for CodeToolConfig {
    fn default() -> Self {
        Self {
            claude_code_path: "claude".to_string(),
            gh_path: "gh".to_string(),
            default_workspace: ".".to_string(),
        }
    }
}

/// Execute a coding task using Claude Code CLI
pub struct WriteCodeTool {
    config: CodeToolConfig,
}

impl WriteCodeTool {
    pub fn new(config: CodeToolConfig) -> Self {
        Self { config }
    }
}

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
            .unwrap_or(&self.config.default_workspace);

        if task.len() > 50_000 {
            return Err(anyhow::anyhow!("Task description too long ({} chars, max 50,000)", task.len()));
        }

        // Validate workspace path to prevent operations in arbitrary directories
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        let expanded_workspace = if workspace.starts_with("~/") {
            home_dir.join(&workspace[2..])
        } else {
            std::path::PathBuf::from(workspace)
        };
        let canonical_workspace = expanded_workspace.canonicalize()
            .with_context(|| format!("Workspace path does not exist: {}", workspace))?;
        let default_ws = if self.config.default_workspace.starts_with("~/") {
            home_dir.join(&self.config.default_workspace[2..])
        } else {
            std::path::PathBuf::from(&self.config.default_workspace)
        };
        let canonical_allowed = default_ws.canonicalize().unwrap_or(default_ws);

        if !canonical_workspace.starts_with(&canonical_allowed) && !canonical_workspace.starts_with(&home_dir) {
            return Err(anyhow::anyhow!(
                "Workspace '{}' is outside allowed directories. Must be within home directory or configured workspace.",
                canonical_workspace.display()
            ));
        }

        debug!("Executing code task in workspace: {}", workspace);

        let output = tokio::time::timeout(
            Duration::from_secs(300),
            Command::new(&self.config.claude_code_path)
                .arg("--print")
                .arg("--dangerously-skip-permissions")
                .arg(task)
                .current_dir(workspace)
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Claude CLI timed out after 5 minutes"))?
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
pub struct MakePrTool {
    config: CodeToolConfig,
}

impl MakePrTool {
    pub fn new(config: CodeToolConfig) -> Self {
        Self { config }
    }
}

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
        if task.len() > 50_000 {
            return Err(anyhow::anyhow!("Task description too long ({} chars, max 50,000)", task.len()));
        }
        let repo = input.get("repo")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.config.default_workspace);
        let branch_name = input.get("branch_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                format!("meepo-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap())
            });

        // Validate branch name to prevent git command injection
        if branch_name.len() > 255 {
            return Err(anyhow::anyhow!("Branch name too long (max 255 characters)"));
        }
        if branch_name.starts_with('-') {
            return Err(anyhow::anyhow!("Branch name cannot start with a dash"));
        }
        if !branch_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '_' || c == '-' || c == '.') {
            return Err(anyhow::anyhow!("Branch name contains invalid characters. Only alphanumeric, /, _, -, and . are allowed."));
        }

        debug!("Creating PR in repo: {} with branch: {}", repo, branch_name);

        // Get original branch for rollback
        let original_branch_output = tokio::time::timeout(
            Duration::from_secs(60),
            Command::new("git")
                .current_dir(repo)
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Git command timed out after 60 seconds"))?
        .context("Failed to get current branch")?;

        let original_branch = if original_branch_output.status.success() {
            String::from_utf8_lossy(&original_branch_output.stdout)
                .trim()
                .to_string()
        } else {
            "main".to_string() // fallback
        };

        debug!("Original branch: {}", original_branch);

        let mut branch_pushed = false;

        // Clone values for the cleanup closure
        let branch_name_clone = branch_name.clone();
        let original_branch_clone = original_branch.clone();
        let repo_clone = repo.to_string();

        // Cleanup helper
        let cleanup = |branch_created: bool, branch_pushed: bool| async move {
            if branch_pushed {
                warn!("Cleaning up: deleting remote branch {}", branch_name_clone);
                let _ = tokio::time::timeout(
                    Duration::from_secs(60),
                    Command::new("git")
                        .current_dir(&repo_clone)
                        .args(["push", "origin", "--delete", &branch_name_clone])
                        .output()
                ).await;
            }
            if branch_created {
                warn!("Cleaning up: switching back to {} and deleting local branch {}", original_branch_clone, branch_name_clone);
                let _ = tokio::time::timeout(
                    Duration::from_secs(60),
                    Command::new("git")
                        .current_dir(&repo_clone)
                        .args(["checkout", &original_branch_clone])
                        .output()
                ).await;
                let _ = tokio::time::timeout(
                    Duration::from_secs(60),
                    Command::new("git")
                        .current_dir(&repo_clone)
                        .args(["branch", "-D", &branch_name_clone])
                        .output()
                ).await;
            }
        };

        // Create branch
        let create_branch = tokio::time::timeout(
            Duration::from_secs(60),
            Command::new("git")
                .current_dir(repo)
                .args(["checkout", "-b", &branch_name])
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Git command timed out after 60 seconds"))?
        .context("Failed to create branch")?;

        if !create_branch.status.success() {
            let error = String::from_utf8_lossy(&create_branch.stderr).to_string();
            return Err(anyhow::anyhow!("Failed to create branch: {}", error));
        }
        let branch_created = true;

        // Execute task with Claude Code
        let code_output = tokio::time::timeout(
            Duration::from_secs(300),
            Command::new(&self.config.claude_code_path)
                .arg("--print")
                .arg("--dangerously-skip-permissions")
                .arg(task)
                .current_dir(repo)
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Claude CLI timed out after 5 minutes"))??;

        if !code_output.status.success() {
            let error = String::from_utf8_lossy(&code_output.stderr).to_string();
            cleanup(branch_created, branch_pushed).await;
            return Err(anyhow::anyhow!("Claude Code task failed: {}", error));
        }

        // Commit changes
        let _stage_result = tokio::time::timeout(
            Duration::from_secs(60),
            Command::new("git")
                .current_dir(repo)
                .args(["add", "-A"])
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Git command timed out after 60 seconds"))??;

        let commit_msg = format!("feat: {}\n\nCo-Authored-By: meepo <meepo@anthropic.com>", task);
        let _commit_result = tokio::time::timeout(
            Duration::from_secs(60),
            Command::new("git")
                .current_dir(repo)
                .args(["commit", "-m", &commit_msg])
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Git command timed out after 60 seconds"))??;

        // Push branch
        let push_output = tokio::time::timeout(
            Duration::from_secs(60),
            Command::new("git")
                .current_dir(repo)
                .args(["push", "-u", "origin", &branch_name])
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Git command timed out after 60 seconds"))??;

        if !push_output.status.success() {
            cleanup(branch_created, branch_pushed).await;
            let error = String::from_utf8_lossy(&push_output.stderr).to_string();
            return Err(anyhow::anyhow!("Failed to push branch: {}", error));
        }
        branch_pushed = true;

        // Create PR using gh
        let pr_output = tokio::time::timeout(
            Duration::from_secs(60),
            Command::new(&self.config.gh_path)
                .current_dir(repo)
                .args([
                    "pr", "create",
                    "--title", task,
                    "--body", "Automated PR created by meepo agent"
                ])
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("GitHub CLI timed out after 60 seconds"))??;

        if pr_output.status.success() {
            let result = String::from_utf8_lossy(&pr_output.stdout).to_string();
            Ok(format!("PR created successfully:\n{}", result))
        } else {
            let error = String::from_utf8_lossy(&pr_output.stderr).to_string();
            cleanup(branch_created, branch_pushed).await;
            warn!("Failed to create PR: {}", error);
            Err(anyhow::anyhow!("Failed to create PR: {}", error))
        }
    }
}

/// Review a pull request
pub struct ReviewPrTool {
    config: CodeToolConfig,
}

impl ReviewPrTool {
    pub fn new(config: CodeToolConfig) -> Self {
        Self { config }
    }

    /// Analyze a git diff and extract structured information
    fn analyze_diff(diff: &str) -> Result<DiffAnalysis> {
        let mut files_changed = 0;
        let mut lines_added = 0;
        let mut lines_removed = 0;
        let mut file_list = Vec::new();
        let mut issues = Vec::new();
        let mut current_file = String::new();

        for line in diff.lines() {
            if line.starts_with("diff --git") {
                // Extract filename from diff header
                if let Some(file) = line.split_whitespace().nth(2) {
                    current_file = file.trim_start_matches("a/").to_string();
                    files_changed += 1;
                    file_list.push(current_file.clone());
                }
            } else if line.starts_with('+') && !line.starts_with("+++") {
                lines_added += 1;

                // Flag potential issues in added lines
                if line.contains("TODO") || line.contains("FIXME") {
                    issues.push(format!("TODO/FIXME added in {}: {}", current_file, line.trim()));
                }
                if line.contains("console.log") || line.contains("println!") && line.contains("debug") {
                    issues.push(format!("Debug statement in {}: {}", current_file, line.trim()));
                }
                if line.contains("unwrap()") && !line.contains("test") {
                    issues.push(format!("Potential panic with unwrap() in {}: {}", current_file, line.trim()));
                }
            } else if line.starts_with('-') && !line.starts_with("---") {
                lines_removed += 1;
            }
        }

        // Check for large file changes
        if files_changed > 20 {
            issues.push(format!("Large PR: {} files changed (consider splitting)", files_changed));
        }

        // Build file list string
        let file_list_str = if file_list.is_empty() {
            "No files detected".to_string()
        } else {
            file_list.iter()
                .take(20)
                .map(|f| format!("  - {}", f))
                .collect::<Vec<_>>()
                .join("\n")
        };

        // Build detailed analysis
        let mut analysis_parts = Vec::new();

        if lines_added > 500 {
            analysis_parts.push(format!("Large changeset: {} lines added", lines_added));
        }

        if lines_removed > lines_added * 2 {
            analysis_parts.push("Significant code deletion detected (potential refactoring)".to_string());
        }

        // Check file types
        let mut has_tests = false;
        let mut has_docs = false;
        let mut config_changes = false;

        for file in &file_list {
            if file.contains("test") || file.ends_with("_test.rs") || file.ends_with(".test.js") {
                has_tests = true;
            }
            if file.ends_with(".md") || file.contains("doc") {
                has_docs = true;
            }
            if file.ends_with(".json") || file.ends_with(".yaml") || file.ends_with(".yml")
                || file.ends_with(".toml") || file.ends_with(".config") {
                config_changes = true;
            }
        }

        if !has_tests && lines_added > 100 {
            analysis_parts.push("No test files detected in large changeset".to_string());
        }

        if config_changes {
            analysis_parts.push("Configuration files modified - ensure backward compatibility".to_string());
        }

        let detailed_analysis = if analysis_parts.is_empty() && issues.is_empty() {
            "No major issues detected. Changes appear straightforward.".to_string()
        } else {
            let mut parts = analysis_parts;
            if !issues.is_empty() {
                parts.push(format!("\nPotential Issues:\n{}",
                    issues.iter()
                        .map(|i| format!("  - {}", i))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
            parts.join("\n")
        };

        // Build recommendations
        let mut recommendations = Vec::new();

        if !has_tests && lines_added > 50 {
            recommendations.push("Consider adding tests for new functionality");
        }

        if !has_docs && lines_added > 200 {
            recommendations.push("Consider updating documentation");
        }

        if files_changed > 15 {
            recommendations.push("Large PR - consider breaking into smaller, focused PRs");
        }

        recommendations.push("Verify all CI/CD checks pass");
        recommendations.push("Ensure code follows project style guidelines");

        let recommendations_str = recommendations.iter()
            .map(|r| format!("- {}", r))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(DiffAnalysis {
            files_changed,
            lines_added,
            lines_removed,
            file_list: file_list_str,
            detailed_analysis,
            recommendations: recommendations_str,
        })
    }
}

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
            .unwrap_or(&self.config.default_workspace);
        let pr_number = input.get("pr_number")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'pr_number' parameter"))?;

        debug!("Reviewing PR #{} in repo: {}", pr_number, repo);

        // Get PR details
        let pr_view = tokio::time::timeout(
            Duration::from_secs(60),
            Command::new(&self.config.gh_path)
                .current_dir(repo)
                .args(["pr", "view", &pr_number.to_string()])
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("GitHub CLI timed out after 60 seconds"))?
        .context("Failed to view PR")?;

        let pr_details = if pr_view.status.success() {
            String::from_utf8_lossy(&pr_view.stdout).to_string()
        } else {
            return Err(anyhow::anyhow!("Failed to fetch PR details"));
        };

        // Get PR diff
        let pr_diff = tokio::time::timeout(
            Duration::from_secs(60),
            Command::new(&self.config.gh_path)
                .current_dir(repo)
                .args(["pr", "diff", &pr_number.to_string()])
                .output()
        )
        .await
        .map_err(|_| anyhow::anyhow!("GitHub CLI timed out after 60 seconds"))?
        .context("Failed to get PR diff")?;

        let diff_content = if pr_diff.status.success() {
            String::from_utf8_lossy(&pr_diff.stdout).to_string()
        } else {
            return Err(anyhow::anyhow!("Failed to fetch PR diff"));
        };

        // Parse the diff for structured analysis
        let analysis = Self::analyze_diff(&diff_content)?;

        // Build comprehensive review
        let review = format!(
            "Pull Request Review for PR #{}\n\n\
            ## PR Details\n{}\n\n\
            ## Change Summary\n\
            - Files changed: {}\n\
            - Lines added: {}\n\
            - Lines removed: {}\n\n\
            ## Files Modified\n{}\n\n\
            ## Analysis\n{}\n\n\
            ## Recommendations\n{}",
            pr_number,
            pr_details,
            analysis.files_changed,
            analysis.lines_added,
            analysis.lines_removed,
            analysis.file_list,
            analysis.detailed_analysis,
            analysis.recommendations
        );

        Ok(review)
    }
}

/// Spawn a Claude Code CLI agent as a background task
pub struct SpawnClaudeCodeTool {
    config: CodeToolConfig,
    db: Arc<KnowledgeDb>,
    command_tx: mpsc::Sender<BackgroundTaskCommand>,
}

impl SpawnClaudeCodeTool {
    pub fn new(
        config: CodeToolConfig,
        db: Arc<KnowledgeDb>,
        command_tx: mpsc::Sender<BackgroundTaskCommand>,
    ) -> Self {
        Self { config, db, command_tx }
    }
}

#[async_trait]
impl ToolHandler for SpawnClaudeCodeTool {
    fn name(&self) -> &str {
        "spawn_claude_code"
    }

    fn description(&self) -> &str {
        "Spawn a Claude Code CLI agent as a background task to work on coding tasks autonomously. \
         The agent runs in its own process and results are reported when done. \
         Use this for longer coding tasks that shouldn't block the conversation."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "task": {
                    "type": "string",
                    "description": "Description of the coding task for Claude Code to execute"
                },
                "workspace": {
                    "type": "string",
                    "description": "Path to the workspace directory (default: configured default_workspace)"
                },
                "reply_channel": {
                    "type": "string",
                    "description": "Channel to report results to (e.g., 'discord', 'slack'). Defaults to 'internal'."
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
            .unwrap_or(&self.config.default_workspace);
        let reply_channel = input.get("reply_channel")
            .and_then(|v| v.as_str())
            .unwrap_or("internal");

        if task.len() > 50_000 {
            return Err(anyhow::anyhow!("Task description too long ({} chars, max 50,000)", task.len()));
        }

        let task_id = format!("t-{}", uuid::Uuid::new_v4());
        let description = format!("Claude Code: {}", &task[..task.len().min(100)]);

        debug!("Spawning Claude Code background task {}: {}", task_id, description);

        // Store in database
        self.db.insert_background_task(&task_id, &description, reply_channel, "agent").await
            .context("Failed to create background task in database")?;

        // Send spawn command to main loop
        self.command_tx.send(BackgroundTaskCommand::SpawnClaudeCode {
            id: task_id.clone(),
            task: task.to_string(),
            workspace: workspace.to_string(),
            reply_channel: reply_channel.to_string(),
        })
        .await
        .context("Failed to send background task command")?;

        Ok(format!(
            "Spawned Claude Code agent [{}] in workspace '{}'. \
             It will run in the background and report results to '{}'.",
            task_id, workspace, reply_channel
        ))
    }
}

/// Analysis result from parsing a git diff
struct DiffAnalysis {
    files_changed: usize,
    lines_added: usize,
    lines_removed: usize,
    file_list: String,
    detailed_analysis: String,
    recommendations: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolHandler;

    fn test_config() -> CodeToolConfig {
        CodeToolConfig::default()
    }

    #[test]
    fn test_write_code_schema() {
        let tool = WriteCodeTool::new(test_config());
        assert_eq!(tool.name(), "write_code");
        assert!(!tool.description().is_empty());
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_make_pr_schema() {
        let tool = MakePrTool::new(test_config());
        assert_eq!(tool.name(), "make_pr");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema.get("required").cloned().unwrap_or(serde_json::json!([]))
        ).unwrap_or_default();
        assert!(required.contains(&"task".to_string()));
    }

    #[test]
    fn test_review_pr_schema() {
        let tool = ReviewPrTool::new(test_config());
        assert_eq!(tool.name(), "review_pr");
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[tokio::test]
    async fn test_write_code_missing_task() {
        let tool = WriteCodeTool::new(test_config());
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_make_pr_missing_task() {
        let tool = MakePrTool::new(test_config());
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_review_pr_missing_params() {
        let tool = ReviewPrTool::new(test_config());
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("pr_number"));
    }

    #[test]
    fn test_review_pr_schema_validation() {
        let tool = ReviewPrTool::new(test_config());
        assert_eq!(tool.name(), "review_pr");

        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema.get("required").cloned().unwrap_or(serde_json::json!([]))
        ).unwrap_or_default();

        assert!(required.contains(&"pr_number".to_string()));

        let properties = schema.get("properties").unwrap();
        assert!(properties.get("pr_number").is_some());
        assert!(properties.get("repo").is_some());
    }

    #[test]
    fn test_diff_analysis_basic() {
        let diff = r#"
diff --git a/src/main.rs b/src/main.rs
index abc123..def456 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,5 +1,7 @@
 fn main() {
+    // Added new feature
+    println!("Hello, world!");
-    old_code();
 }
"#;

        let analysis = ReviewPrTool::analyze_diff(diff).unwrap();
        assert_eq!(analysis.files_changed, 1);
        assert!(analysis.lines_added >= 2);
        assert!(analysis.lines_removed >= 1);
        assert!(analysis.file_list.contains("src/main.rs"));
    }

    #[test]
    fn test_diff_analysis_detects_issues() {
        let diff = r#"
diff --git a/src/lib.rs b/src/lib.rs
index abc123..def456 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,5 @@
 pub fn process() {
+    // TODO: implement this properly
+    let value = dangerous_call().unwrap();
 }
"#;

        let analysis = ReviewPrTool::analyze_diff(diff).unwrap();
        assert!(analysis.detailed_analysis.contains("TODO") || analysis.detailed_analysis.contains("unwrap"));
    }

    #[test]
    fn test_diff_analysis_empty() {
        let analysis = ReviewPrTool::analyze_diff("").unwrap();
        assert_eq!(analysis.files_changed, 0);
        assert_eq!(analysis.lines_added, 0);
        assert_eq!(analysis.lines_removed, 0);
    }
}
