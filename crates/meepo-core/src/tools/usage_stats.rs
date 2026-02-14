//! Usage statistics tool — lets the agent inspect its own API costs

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

use super::{ToolHandler, json_schema};
use crate::usage::UsageTracker;

/// Tool that lets the agent query its own usage and cost data
pub struct GetUsageStatsTool {
    tracker: Arc<UsageTracker>,
}

impl GetUsageStatsTool {
    pub fn new(tracker: Arc<UsageTracker>) -> Self {
        Self { tracker }
    }
}

#[async_trait]
impl ToolHandler for GetUsageStatsTool {
    fn name(&self) -> &str {
        "get_usage_stats"
    }

    fn description(&self) -> &str {
        "Get AI usage statistics and cost data. Returns token counts, estimated costs, \
         and budget status for today, this month, or a custom date range."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "period": {
                    "type": "string",
                    "description": "Time period: 'today', 'month', or a date range like '2025-01-01:2025-01-31'",
                    "enum": ["today", "month"]
                }
            }),
            vec!["period"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let period = input
            .get("period")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'period' parameter"))?;

        debug!("Getting usage stats for period: {}", period);

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

        let (start, end) = match period {
            "today" => (today.clone(), today),
            "month" => {
                let now = chrono::Utc::now();
                let first_of_month = format!(
                    "{}-{:02}-01",
                    now.format("%Y"),
                    now.format("%m")
                );
                let today = now.format("%Y-%m-%d").to_string();
                (first_of_month, today)
            }
            other if other.contains(':') => {
                let parts: Vec<&str> = other.splitn(2, ':').collect();
                if parts.len() != 2 {
                    return Err(anyhow::anyhow!(
                        "Invalid date range format. Use 'YYYY-MM-DD:YYYY-MM-DD'"
                    ));
                }
                (parts[0].to_string(), parts[1].to_string())
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Invalid period '{}'. Use 'today', 'month', or 'YYYY-MM-DD:YYYY-MM-DD'",
                    period
                ));
            }
        };

        let summary = self.tracker.get_summary(&start, &end).await?;

        // Format as readable text
        let mut output = format!("## Usage Summary ({})\n\n", summary.period);
        output.push_str(&format!(
            "- **Total API calls:** {}\n",
            summary.total_api_calls
        ));
        output.push_str(&format!(
            "- **Input tokens:** {}\n",
            summary.total_input_tokens
        ));
        output.push_str(&format!(
            "- **Output tokens:** {}\n",
            summary.total_output_tokens
        ));
        output.push_str(&format!(
            "- **Tool calls:** {}\n",
            summary.total_tool_calls
        ));
        output.push_str(&format!(
            "- **Estimated cost:** ${:.4}\n",
            summary.estimated_cost_usd
        ));

        // Budget status
        match self.tracker.check_budget().await {
            Ok(status) => {
                output.push_str(&format!("\n**Budget status:** {}\n", status));
            }
            Err(e) => {
                debug!("Budget check failed: {}", e);
            }
        }

        // Breakdown by source
        if !summary.by_source.is_empty() {
            output.push_str("\n### By Source\n");
            for (source, usage) in &summary.by_source {
                output.push_str(&format!(
                    "- **{}:** {} calls, {} in / {} out tokens, ${:.4}\n",
                    source, usage.api_calls, usage.input_tokens, usage.output_tokens,
                    usage.estimated_cost_usd
                ));
            }
        }

        // Breakdown by model
        if !summary.by_model.is_empty() {
            output.push_str("\n### By Model\n");
            for (model, usage) in &summary.by_model {
                output.push_str(&format!(
                    "- **{}:** {} calls, {} in / {} out tokens, ${:.4}\n",
                    model, usage.api_calls, usage.input_tokens, usage.output_tokens,
                    usage.estimated_cost_usd
                ));
            }
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_usage_stats_schema() {
        use meepo_knowledge::KnowledgeDb;
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let db = Arc::new(KnowledgeDb::new(&temp.path().join("test.db")).unwrap());
        let config = crate::usage::UsageConfig::default();
        let tracker = Arc::new(UsageTracker::new(db, config));
        let tool = GetUsageStatsTool::new(tracker);

        assert_eq!(tool.name(), "get_usage_stats");
        assert!(!tool.description().is_empty());
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
        assert!(schema.get("required").is_some());
    }
}
