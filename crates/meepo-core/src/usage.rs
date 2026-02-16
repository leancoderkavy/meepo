//! AI usage tracking and cost estimation
//!
//! Tracks token usage per API call, estimates costs based on configurable
//! model pricing, enforces daily/monthly budgets, and provides query methods
//! for CLI reporting and agent self-inspection.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::{Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use meepo_knowledge::{KnowledgeDb, UsageSummary};

/// Source of an API call (who triggered it)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    User,
    Autonomous,
    SubAgent,
    Watcher,
    Summarization,
    Internal,
}

impl std::fmt::Display for UsageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Autonomous => write!(f, "autonomous"),
            Self::SubAgent => write!(f, "sub_agent"),
            Self::Watcher => write!(f, "watcher"),
            Self::Summarization => write!(f, "summarization"),
            Self::Internal => write!(f, "internal"),
        }
    }
}

impl UsageSource {
    pub fn parse(s: &str) -> Self {
        match s {
            "user" => Self::User,
            "autonomous" => Self::Autonomous,
            "sub_agent" => Self::SubAgent,
            "watcher" => Self::Watcher,
            "summarization" => Self::Summarization,
            _ => Self::Internal,
        }
    }
}

/// Accumulated usage from a single tool loop (may span multiple API calls)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccumulatedUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub api_calls: u32,
    pub tool_calls: Vec<String>,
}

impl AccumulatedUsage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add usage from a single API response
    pub fn add(&mut self, input_tokens: u32, output_tokens: u32) {
        self.input_tokens += input_tokens as u64;
        self.output_tokens += output_tokens as u64;
        self.api_calls += 1;
    }

    /// Record a tool call
    pub fn record_tool_call(&mut self, tool_name: &str) {
        self.tool_calls.push(tool_name.to_string());
    }

    /// Total tokens
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// Pricing for a specific model (per million tokens)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    #[serde(default)]
    pub cache_read_per_mtok: f64,
    #[serde(default)]
    pub cache_write_per_mtok: f64,
}

impl ModelPricing {
    /// Estimate cost in USD for given token counts
    pub fn estimate_cost(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
    ) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_per_mtok;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_per_mtok;
        let cache_read_cost = (cache_read_tokens as f64 / 1_000_000.0) * self.cache_read_per_mtok;
        let cache_write_cost =
            (cache_write_tokens as f64 / 1_000_000.0) * self.cache_write_per_mtok;
        input_cost + output_cost + cache_read_cost + cache_write_cost
    }
}

/// Configuration for usage tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageConfig {
    pub enabled: bool,
    pub daily_budget_usd: Option<f64>,
    pub monthly_budget_usd: Option<f64>,
    pub warn_at_percent: u32,
    pub model_prices: HashMap<String, ModelPricing>,
}

impl Default for UsageConfig {
    fn default() -> Self {
        let mut model_prices = HashMap::new();
        // Anthropic models
        model_prices.insert(
            "claude-sonnet-4-20250514".to_string(),
            ModelPricing {
                input_per_mtok: 3.0,
                output_per_mtok: 15.0,
                cache_read_per_mtok: 0.3,
                cache_write_per_mtok: 3.75,
            },
        );
        // OpenAI models
        model_prices.insert(
            "gpt-4o".to_string(),
            ModelPricing {
                input_per_mtok: 2.5,
                output_per_mtok: 10.0,
                cache_read_per_mtok: 0.0,
                cache_write_per_mtok: 0.0,
            },
        );

        Self {
            enabled: true,
            daily_budget_usd: None,
            monthly_budget_usd: None,
            warn_at_percent: 80,
            model_prices,
        }
    }
}

/// Budget check result
#[derive(Debug, Clone)]
pub enum BudgetStatus {
    /// Under budget, all clear
    Ok,
    /// Approaching budget limit
    Warning {
        period: String,
        spent: f64,
        budget: f64,
        percent: f64,
    },
    /// Over budget, should refuse requests
    Exceeded {
        period: String,
        spent: f64,
        budget: f64,
    },
}

impl BudgetStatus {
    pub fn is_exceeded(&self) -> bool {
        matches!(self, Self::Exceeded { .. })
    }

    pub fn is_warning(&self) -> bool {
        matches!(self, Self::Warning { .. })
    }
}

impl std::fmt::Display for BudgetStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "OK — within budget"),
            Self::Warning {
                period,
                spent,
                budget,
                percent,
            } => {
                write!(
                    f,
                    "Warning — {} budget at {:.0}% (${:.2} of ${:.2})",
                    period, percent, spent, budget
                )
            }
            Self::Exceeded {
                period,
                spent,
                budget,
            } => {
                write!(
                    f,
                    "EXCEEDED — {} budget (${:.2} of ${:.2})",
                    period, spent, budget
                )
            }
        }
    }
}

/// The usage tracker — records API calls and enforces budgets
pub struct UsageTracker {
    db: Arc<KnowledgeDb>,
    config: UsageConfig,
    session_id: String,
}

impl UsageTracker {
    pub fn new(db: Arc<KnowledgeDb>, config: UsageConfig) -> Self {
        let session_id = uuid::Uuid::new_v4().to_string();
        info!("Usage tracker initialized (session: {})", session_id);
        Self {
            db,
            config,
            session_id,
        }
    }

    /// Record an API call's usage
    pub async fn record(
        &self,
        model: &str,
        usage: &AccumulatedUsage,
        source: &UsageSource,
        channel: Option<&str>,
    ) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let cost = self.estimate_cost(model, usage);

        let tool_names_json = serde_json::to_string(&usage.tool_calls).unwrap_or_default();

        self.db
            .insert_usage_log(
                model,
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_write_tokens,
                cost,
                &source.to_string(),
                channel,
                usage.tool_calls.len() as u32,
                &tool_names_json,
                &self.session_id,
            )
            .await?;

        debug!(
            "Recorded usage: {} in={} out={} cost=${:.4} source={}",
            model, usage.input_tokens, usage.output_tokens, cost, source
        );

        Ok(())
    }

    /// Estimate cost for accumulated usage
    pub fn estimate_cost(&self, model: &str, usage: &AccumulatedUsage) -> f64 {
        if let Some(pricing) = self.config.model_prices.get(model) {
            pricing.estimate_cost(
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_write_tokens,
            )
        } else {
            // Fallback: use moderate pricing as a conservative estimate
            let fallback = ModelPricing {
                input_per_mtok: 3.0,
                output_per_mtok: 15.0,
                cache_read_per_mtok: 0.3,
                cache_write_per_mtok: 3.75,
            };
            fallback.estimate_cost(
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_write_tokens,
            )
        }
    }

    /// Check if we're within budget
    pub async fn check_budget(&self) -> Result<BudgetStatus> {
        if !self.config.enabled {
            return Ok(BudgetStatus::Ok);
        }

        // Check daily budget
        if let Some(daily_budget) = self.config.daily_budget_usd {
            let today = Utc::now().format("%Y-%m-%d").to_string();
            let daily_cost = self.db.get_usage_cost_for_date(&today).await?;

            if daily_cost >= daily_budget {
                return Ok(BudgetStatus::Exceeded {
                    period: "daily".to_string(),
                    spent: daily_cost,
                    budget: daily_budget,
                });
            }

            let percent = (daily_cost / daily_budget) * 100.0;
            if percent >= self.config.warn_at_percent as f64 {
                return Ok(BudgetStatus::Warning {
                    period: "daily".to_string(),
                    spent: daily_cost,
                    budget: daily_budget,
                    percent,
                });
            }
        }

        // Check monthly budget
        if let Some(monthly_budget) = self.config.monthly_budget_usd {
            let now = Utc::now();
            let month_start = NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
                .unwrap_or_else(|| now.date_naive());
            let month_start_str = month_start.format("%Y-%m-%d").to_string();
            let today = now.format("%Y-%m-%d").to_string();

            let monthly_cost = self
                .db
                .get_usage_cost_for_range(&month_start_str, &today)
                .await?;

            if monthly_cost >= monthly_budget {
                return Ok(BudgetStatus::Exceeded {
                    period: "monthly".to_string(),
                    spent: monthly_cost,
                    budget: monthly_budget,
                });
            }

            let percent = (monthly_cost / monthly_budget) * 100.0;
            if percent >= self.config.warn_at_percent as f64 {
                return Ok(BudgetStatus::Warning {
                    period: "monthly".to_string(),
                    spent: monthly_cost,
                    budget: monthly_budget,
                    percent,
                });
            }
        }

        Ok(BudgetStatus::Ok)
    }

    /// Get usage summary for today
    pub async fn get_daily_summary(&self) -> Result<UsageSummary> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        self.db.get_usage_summary(&today, &today).await
    }

    /// Get usage summary for the current month
    pub async fn get_monthly_summary(&self) -> Result<UsageSummary> {
        let now = Utc::now();
        let month_start =
            NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap_or_else(|| now.date_naive());
        let month_start_str = month_start.format("%Y-%m-%d").to_string();
        let today = now.format("%Y-%m-%d").to_string();
        self.db.get_usage_summary(&month_start_str, &today).await
    }

    /// Get usage summary for a custom date range
    pub async fn get_range_summary(&self, start: &str, end: &str) -> Result<UsageSummary> {
        self.db.get_usage_summary(start, end).await
    }

    /// Get usage summary (convenience alias used by tools)
    pub async fn get_summary(&self, start: &str, end: &str) -> Result<UsageSummary> {
        self.db.get_usage_summary(start, end).await
    }

    /// Export usage data as CSV
    pub async fn export_csv(&self, start: &str, end: &str) -> Result<String> {
        self.db.export_usage_csv(start, end).await
    }

    /// Get the current config
    pub fn config(&self) -> &UsageConfig {
        &self.config
    }

    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

/// Format a UsageSummary as a human-readable string
pub fn format_usage_summary(summary: &UsageSummary) -> String {
    let mut out = String::new();

    out.push_str(&format!("## Usage Summary ({})\n\n", summary.period));
    out.push_str(&format!(
        "**Total Cost:** ${:.4}\n",
        summary.estimated_cost_usd
    ));
    out.push_str(&format!(
        "**Total Tokens:** {} (in: {}, out: {})\n",
        summary.total_input_tokens + summary.total_output_tokens,
        summary.total_input_tokens,
        summary.total_output_tokens
    ));
    out.push_str(&format!("**API Calls:** {}\n", summary.total_api_calls));
    out.push_str(&format!("**Tool Calls:** {}\n\n", summary.total_tool_calls));

    if !summary.by_source.is_empty() {
        out.push_str("### By Source\n\n");
        out.push_str("| Source | Cost | Tokens | Calls |\n");
        out.push_str("|--------|------|--------|-------|\n");
        let mut sources: Vec<_> = summary.by_source.iter().collect();
        sources.sort_by(|a, b| {
            b.1.estimated_cost_usd
                .partial_cmp(&a.1.estimated_cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (source, usage) in sources {
            out.push_str(&format!(
                "| {} | ${:.4} | {} | {} |\n",
                source,
                usage.estimated_cost_usd,
                usage.input_tokens + usage.output_tokens,
                usage.api_calls
            ));
        }
        out.push('\n');
    }

    if !summary.by_model.is_empty() {
        out.push_str("### By Model\n\n");
        out.push_str("| Model | Cost | Tokens | Calls |\n");
        out.push_str("|-------|------|--------|-------|\n");
        let mut models: Vec<_> = summary.by_model.iter().collect();
        models.sort_by(|a, b| {
            b.1.estimated_cost_usd
                .partial_cmp(&a.1.estimated_cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (model, usage) in models {
            out.push_str(&format!(
                "| {} | ${:.4} | {} | {} |\n",
                model,
                usage.estimated_cost_usd,
                usage.input_tokens + usage.output_tokens,
                usage.api_calls
            ));
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_pricing() {
        let pricing = ModelPricing {
            input_per_mtok: 15.0,
            output_per_mtok: 75.0,
            cache_read_per_mtok: 1.5,
            cache_write_per_mtok: 18.75,
        };

        // 1000 input + 500 output tokens
        let cost = pricing.estimate_cost(1000, 500, 0, 0);
        assert!((cost - 0.0525).abs() < 0.0001);
    }

    #[test]
    fn test_accumulated_usage() {
        let mut usage = AccumulatedUsage::new();
        usage.add(100, 50);
        usage.add(200, 100);
        usage.record_tool_call("web_search");

        assert_eq!(usage.input_tokens, 300);
        assert_eq!(usage.output_tokens, 150);
        assert_eq!(usage.api_calls, 2);
        assert_eq!(usage.tool_calls.len(), 1);
        assert_eq!(usage.total_tokens(), 450);
    }

    #[test]
    fn test_usage_source_display() {
        assert_eq!(UsageSource::User.to_string(), "user");
        assert_eq!(UsageSource::Autonomous.to_string(), "autonomous");
        assert_eq!(UsageSource::SubAgent.to_string(), "sub_agent");
    }

    #[test]
    fn test_default_config() {
        let config = UsageConfig::default();
        assert!(config.enabled);
        assert!(config.model_prices.contains_key("claude-sonnet-4-20250514"));
        assert!(config.model_prices.contains_key("gpt-4o"));
        assert_eq!(config.warn_at_percent, 80);
    }

    #[test]
    fn test_format_usage_summary() {
        let summary = UsageSummary {
            period: "2026-02-14".to_string(),
            total_input_tokens: 10000,
            total_output_tokens: 5000,
            total_api_calls: 10,
            total_tool_calls: 25,
            estimated_cost_usd: 0.525,
            by_source: HashMap::new(),
            by_model: HashMap::new(),
        };
        let formatted = format_usage_summary(&summary);
        assert!(formatted.contains("$0.5250"));
        assert!(formatted.contains("15000"));
    }

    // ── UsageSource::parse ──────────────────────────────────────

    #[test]
    fn test_usage_source_parse_all_variants() {
        assert_eq!(UsageSource::parse("user"), UsageSource::User);
        assert_eq!(UsageSource::parse("autonomous"), UsageSource::Autonomous);
        assert_eq!(UsageSource::parse("sub_agent"), UsageSource::SubAgent);
        assert_eq!(UsageSource::parse("watcher"), UsageSource::Watcher);
        assert_eq!(UsageSource::parse("summarization"), UsageSource::Summarization);
        assert_eq!(UsageSource::parse("internal"), UsageSource::Internal);
    }

    #[test]
    fn test_usage_source_parse_unknown_defaults_internal() {
        assert_eq!(UsageSource::parse("unknown"), UsageSource::Internal);
        assert_eq!(UsageSource::parse(""), UsageSource::Internal);
        assert_eq!(UsageSource::parse("something_else"), UsageSource::Internal);
    }

    #[test]
    fn test_usage_source_display_all() {
        assert_eq!(UsageSource::User.to_string(), "user");
        assert_eq!(UsageSource::Autonomous.to_string(), "autonomous");
        assert_eq!(UsageSource::SubAgent.to_string(), "sub_agent");
        assert_eq!(UsageSource::Watcher.to_string(), "watcher");
        assert_eq!(UsageSource::Summarization.to_string(), "summarization");
        assert_eq!(UsageSource::Internal.to_string(), "internal");
    }

    #[test]
    fn test_usage_source_serde_roundtrip() {
        let source = UsageSource::Autonomous;
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(json, "\"autonomous\"");
        let parsed: UsageSource = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, source);
    }

    // ── AccumulatedUsage ────────────────────────────────────────

    #[test]
    fn test_accumulated_usage_default() {
        let u = AccumulatedUsage::new();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
        assert_eq!(u.cache_read_tokens, 0);
        assert_eq!(u.cache_write_tokens, 0);
        assert_eq!(u.api_calls, 0);
        assert!(u.tool_calls.is_empty());
        assert_eq!(u.total_tokens(), 0);
    }

    #[test]
    fn test_accumulated_usage_multiple_tools() {
        let mut u = AccumulatedUsage::new();
        u.record_tool_call("read_file");
        u.record_tool_call("write_file");
        u.record_tool_call("search");
        assert_eq!(u.tool_calls.len(), 3);
        assert_eq!(u.tool_calls[0], "read_file");
        assert_eq!(u.tool_calls[2], "search");
    }

    // ── ModelPricing ────────────────────────────────────────────

    #[test]
    fn test_model_pricing_with_cache() {
        let pricing = ModelPricing {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.3,
            cache_write_per_mtok: 3.75,
        };
        let cost = pricing.estimate_cost(1_000_000, 500_000, 200_000, 100_000);
        // 3.0 + 7.5 + 0.06 + 0.375 = 10.935
        assert!((cost - 10.935).abs() < 0.001);
    }

    #[test]
    fn test_model_pricing_zero_tokens() {
        let pricing = ModelPricing {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.3,
            cache_write_per_mtok: 3.75,
        };
        let cost = pricing.estimate_cost(0, 0, 0, 0);
        assert!((cost - 0.0).abs() < f64::EPSILON);
    }

    // ── BudgetStatus ────────────────────────────────────────────

    #[test]
    fn test_budget_status_ok() {
        let status = BudgetStatus::Ok;
        assert!(!status.is_exceeded());
        assert!(!status.is_warning());
        assert_eq!(status.to_string(), "OK — within budget");
    }

    #[test]
    fn test_budget_status_warning() {
        let status = BudgetStatus::Warning {
            period: "daily".to_string(),
            spent: 8.0,
            budget: 10.0,
            percent: 80.0,
        };
        assert!(!status.is_exceeded());
        assert!(status.is_warning());
        let display = status.to_string();
        assert!(display.contains("Warning"));
        assert!(display.contains("daily"));
        assert!(display.contains("80"));
    }

    #[test]
    fn test_budget_status_exceeded() {
        let status = BudgetStatus::Exceeded {
            period: "monthly".to_string(),
            spent: 110.0,
            budget: 100.0,
        };
        assert!(status.is_exceeded());
        assert!(!status.is_warning());
        let display = status.to_string();
        assert!(display.contains("EXCEEDED"));
        assert!(display.contains("monthly"));
    }

    // ── UsageConfig ─────────────────────────────────────────────

    #[test]
    fn test_usage_config_default_prices() {
        let config = UsageConfig::default();
        let sonnet = config.model_prices.get("claude-sonnet-4-20250514").unwrap();
        assert!((sonnet.input_per_mtok - 3.0).abs() < f64::EPSILON);
        assert!((sonnet.output_per_mtok - 15.0).abs() < f64::EPSILON);
        let gpt4o = config.model_prices.get("gpt-4o").unwrap();
        assert!((gpt4o.input_per_mtok - 2.5).abs() < f64::EPSILON);
    }

    // ── UsageTracker (with real DB) ─────────────────────────────

    #[tokio::test]
    async fn test_usage_tracker_record_and_budget() {
        let temp_path =
            std::env::temp_dir().join(format!("test_tracker_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&temp_path);
        let db = Arc::new(KnowledgeDb::new(&temp_path).unwrap());

        let config = UsageConfig {
            enabled: true,
            daily_budget_usd: Some(1.0),
            monthly_budget_usd: Some(10.0),
            warn_at_percent: 80,
            model_prices: UsageConfig::default().model_prices,
        };
        let tracker = UsageTracker::new(db.clone(), config);

        // Session ID is set
        assert!(!tracker.session_id().is_empty());

        // Check budget — should be OK initially
        let status = tracker.check_budget().await.unwrap();
        assert!(!status.is_exceeded());
        assert!(!status.is_warning());

        // Record some usage
        let mut usage = AccumulatedUsage::new();
        usage.add(1000, 500);
        usage.record_tool_call("test_tool");
        tracker
            .record("claude-sonnet-4-20250514", &usage, &UsageSource::User, Some("discord"))
            .await
            .unwrap();

        // Estimate cost
        let cost = tracker.estimate_cost("claude-sonnet-4-20250514", &usage);
        assert!(cost > 0.0);

        // Get daily summary
        let summary = tracker.get_daily_summary().await.unwrap();
        assert_eq!(summary.total_api_calls, 1);

        // Get monthly summary
        let monthly = tracker.get_monthly_summary().await.unwrap();
        assert!(monthly.total_api_calls >= 1);

        let _ = std::fs::remove_file(&temp_path);
    }

    #[tokio::test]
    async fn test_usage_tracker_disabled() {
        let temp_path =
            std::env::temp_dir().join(format!("test_tracker_off_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&temp_path);
        let db = Arc::new(KnowledgeDb::new(&temp_path).unwrap());

        let config = UsageConfig {
            enabled: false,
            daily_budget_usd: None,
            monthly_budget_usd: None,
            warn_at_percent: 80,
            model_prices: HashMap::new(),
        };
        let tracker = UsageTracker::new(db.clone(), config);

        // Record should be a no-op when disabled
        let usage = AccumulatedUsage::new();
        tracker
            .record("test-model", &usage, &UsageSource::Internal, None)
            .await
            .unwrap();

        // Budget check returns Ok when disabled
        let status = tracker.check_budget().await.unwrap();
        assert!(!status.is_exceeded());

        let _ = std::fs::remove_file(&temp_path);
    }

    #[tokio::test]
    async fn test_usage_tracker_fallback_pricing() {
        let temp_path =
            std::env::temp_dir().join(format!("test_tracker_fb_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&temp_path);
        let db = Arc::new(KnowledgeDb::new(&temp_path).unwrap());

        let config = UsageConfig {
            enabled: true,
            daily_budget_usd: None,
            monthly_budget_usd: None,
            warn_at_percent: 80,
            model_prices: HashMap::new(), // no prices — will use fallback
        };
        let tracker = UsageTracker::new(db.clone(), config);

        let mut usage = AccumulatedUsage::new();
        usage.add(1_000_000, 500_000);
        let cost = tracker.estimate_cost("unknown-model", &usage);
        // Fallback: 3.0 * 1 + 15.0 * 0.5 = 10.5
        assert!((cost - 10.5).abs() < 0.01);

        let _ = std::fs::remove_file(&temp_path);
    }

    // ── format_usage_summary with data ──────────────────────────

    #[test]
    fn test_format_usage_summary_with_sources_and_models() {
        use meepo_knowledge::{ModelUsage, SourceUsage};

        let mut by_source = HashMap::new();
        by_source.insert("user".to_string(), SourceUsage {
            input_tokens: 5000,
            output_tokens: 2500,
            api_calls: 5,
            estimated_cost_usd: 0.3,
        });
        by_source.insert("watcher".to_string(), SourceUsage {
            input_tokens: 3000,
            output_tokens: 1500,
            api_calls: 3,
            estimated_cost_usd: 0.2,
        });

        let mut by_model = HashMap::new();
        by_model.insert("claude-sonnet-4-20250514".to_string(), ModelUsage {
            input_tokens: 8000,
            output_tokens: 4000,
            api_calls: 8,
            estimated_cost_usd: 0.5,
        });

        let summary = UsageSummary {
            period: "2026-02-16".to_string(),
            total_input_tokens: 8000,
            total_output_tokens: 4000,
            total_api_calls: 8,
            total_tool_calls: 15,
            estimated_cost_usd: 0.5,
            by_source,
            by_model,
        };

        let formatted = format_usage_summary(&summary);
        assert!(formatted.contains("By Source"));
        assert!(formatted.contains("user"));
        assert!(formatted.contains("watcher"));
        assert!(formatted.contains("By Model"));
        assert!(formatted.contains("claude-sonnet-4-20250514"));
        assert!(formatted.contains("$0.5000"));
    }

    // ── UsageTracker range/export ───────────────────────────────

    #[tokio::test]
    async fn test_usage_tracker_range_and_export() {
        let temp_path =
            std::env::temp_dir().join(format!("test_tracker_range_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&temp_path);
        let db = Arc::new(KnowledgeDb::new(&temp_path).unwrap());

        let tracker = UsageTracker::new(db.clone(), UsageConfig::default());

        let mut usage = AccumulatedUsage::new();
        usage.add(500, 200);
        tracker
            .record("gpt-4o", &usage, &UsageSource::User, None)
            .await
            .unwrap();

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let range = tracker.get_range_summary(&today, &today).await.unwrap();
        assert_eq!(range.total_api_calls, 1);

        let summary = tracker.get_summary(&today, &today).await.unwrap();
        assert_eq!(summary.total_api_calls, 1);

        let csv = tracker.export_csv(&today, &today).await.unwrap();
        assert!(csv.contains("gpt-4o"));

        assert!(tracker.config().enabled);

        let _ = std::fs::remove_file(&temp_path);
    }
}
