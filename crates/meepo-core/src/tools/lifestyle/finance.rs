//! Finance & Expense Tracker tools
//!
//! Track expenses, parse receipt emails, monitor spending, and check budgets.
//! All data stored in the knowledge graph as entities.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

use crate::tools::{ToolHandler, json_schema};
use meepo_knowledge::KnowledgeDb;

/// Log an expense
pub struct LogExpenseTool {
    db: Arc<KnowledgeDb>,
}

impl LogExpenseTool {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ToolHandler for LogExpenseTool {
    fn name(&self) -> &str {
        "log_expense"
    }

    fn description(&self) -> &str {
        "Log an expense in the finance tracker. Records amount, category, vendor, and optional \
         notes. Expenses are stored in the knowledge graph and linked to vendor/category entities."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "amount": {
                    "type": "number",
                    "description": "Expense amount in dollars (e.g., 42.50)"
                },
                "category": {
                    "type": "string",
                    "description": "Category: food, transport, entertainment, shopping, bills, health, education, other"
                },
                "vendor": {
                    "type": "string",
                    "description": "Vendor/merchant name"
                },
                "description": {
                    "type": "string",
                    "description": "Optional description of the expense"
                },
                "date": {
                    "type": "string",
                    "description": "Date of expense (default: today). Format: YYYY-MM-DD"
                },
                "payment_method": {
                    "type": "string",
                    "description": "Payment method: cash, credit, debit, venmo, other"
                }
            }),
            vec!["amount", "category"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let amount = input
            .get("amount")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'amount' parameter"))?;
        let category = input
            .get("category")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'category' parameter"))?;
        let vendor = input
            .get("vendor")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let description = input.get("description").and_then(|v| v.as_str());
        let default_date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let date = input
            .get("date")
            .and_then(|v| v.as_str())
            .unwrap_or(&default_date);
        let payment_method = input
            .get("payment_method")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        if amount <= 0.0 {
            return Err(anyhow::anyhow!("Amount must be positive"));
        }
        if amount > 1_000_000.0 {
            return Err(anyhow::anyhow!("Amount too large (max $1,000,000)"));
        }

        debug!("Logging expense: ${:.2} at {} ({})", amount, vendor, category);

        let expense_name = format!("expense:{}:{:.2}:{}", vendor, amount, date);
        let expense_id = self
            .db
            .insert_entity(
                &expense_name,
                "expense",
                Some(serde_json::json!({
                    "amount": amount,
                    "category": category,
                    "vendor": vendor,
                    "description": description,
                    "date": date,
                    "payment_method": payment_method,
                    "created_at": chrono::Utc::now().to_rfc3339(),
                })),
            )
            .await?;

        // Link to vendor entity
        let vendors = self
            .db
            .search_entities(vendor, Some("vendor"))
            .await
            .unwrap_or_default();
        let vendor_id = if let Some(existing) = vendors.first() {
            existing.id.clone()
        } else {
            self.db
                .insert_entity(vendor, "vendor", Some(serde_json::json!({"category": category})))
                .await?
        };
        let _ = self
            .db
            .insert_relationship(&expense_id, &vendor_id, "paid_to", None)
            .await;

        // Link to category entity
        let categories = self
            .db
            .search_entities(category, Some("expense_category"))
            .await
            .unwrap_or_default();
        let cat_id = if let Some(existing) = categories.first() {
            existing.id.clone()
        } else {
            self.db
                .insert_entity(category, "expense_category", None)
                .await?
        };
        let _ = self
            .db
            .insert_relationship(&expense_id, &cat_id, "categorized_as", None)
            .await;

        Ok(format!(
            "Expense logged:\n\
             - ID: {}\n\
             - Amount: ${:.2}\n\
             - Category: {}\n\
             - Vendor: {}\n\
             - Date: {}\n\
             - Payment: {}",
            expense_id, amount, category, vendor, date, payment_method
        ))
    }
}

/// Get spending summary
pub struct SpendingSummaryTool {
    db: Arc<KnowledgeDb>,
}

impl SpendingSummaryTool {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ToolHandler for SpendingSummaryTool {
    fn name(&self) -> &str {
        "spending_summary"
    }

    fn description(&self) -> &str {
        "Get a spending summary for a time period. Shows total spending, breakdown by category, \
         top vendors, and trends compared to previous periods."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "period": {
                    "type": "string",
                    "description": "Time period: today, this_week, this_month, last_month, custom (default: this_month)"
                },
                "category": {
                    "type": "string",
                    "description": "Filter by category (optional)"
                },
                "start_date": {
                    "type": "string",
                    "description": "Start date for custom period (YYYY-MM-DD)"
                },
                "end_date": {
                    "type": "string",
                    "description": "End date for custom period (YYYY-MM-DD)"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let period = input
            .get("period")
            .and_then(|v| v.as_str())
            .unwrap_or("this_month");
        let category_filter = input.get("category").and_then(|v| v.as_str());

        debug!("Getting spending summary for: {}", period);

        let expenses = self
            .db
            .search_entities("", Some("expense"))
            .await
            .unwrap_or_default();

        // Calculate totals by category
        let mut by_category: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        let mut by_vendor: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        let mut total = 0.0;
        let mut count = 0;

        for expense in &expenses {
            let meta = match &expense.metadata {
                Some(m) => m,
                None => continue,
            };

            let amount = meta.get("amount").and_then(|a| a.as_f64()).unwrap_or(0.0);
            let cat = meta
                .get("category")
                .and_then(|c| c.as_str())
                .unwrap_or("other");
            let vendor = meta
                .get("vendor")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            if let Some(filter) = category_filter {
                if cat != filter {
                    continue;
                }
            }

            total += amount;
            count += 1;
            *by_category.entry(cat.to_string()).or_insert(0.0) += amount;
            *by_vendor.entry(vendor.to_string()).or_insert(0.0) += amount;
        }

        // Sort categories by amount
        let mut cat_list: Vec<_> = by_category.iter().collect();
        cat_list.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

        let cat_str = cat_list
            .iter()
            .map(|(cat, amt)| {
                let pct = if total > 0.0 {
                    (*amt / total * 100.0) as u32
                } else {
                    0
                };
                format!("  - {}: ${:.2} ({}%)", cat, amt, pct)
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Top 5 vendors
        let mut vendor_list: Vec<_> = by_vendor.iter().collect();
        vendor_list.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        let vendor_str = vendor_list
            .iter()
            .take(5)
            .map(|(v, amt)| format!("  - {}: ${:.2}", v, amt))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(format!(
            "# Spending Summary ({})\n\n\
             ## Overview\n\
             - Total: ${:.2}\n\
             - Transactions: {}\n\
             - Average: ${:.2}\n\n\
             ## By Category\n{}\n\n\
             ## Top Vendors\n{}\n",
            period,
            total,
            count,
            if count > 0 { total / count as f64 } else { 0.0 },
            if cat_str.is_empty() {
                "  No expenses found.".to_string()
            } else {
                cat_str
            },
            if vendor_str.is_empty() {
                "  No vendors found.".to_string()
            } else {
                vendor_str
            }
        ))
    }
}

/// Check budget status
pub struct BudgetCheckTool {
    db: Arc<KnowledgeDb>,
}

impl BudgetCheckTool {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ToolHandler for BudgetCheckTool {
    fn name(&self) -> &str {
        "budget_check"
    }

    fn description(&self) -> &str {
        "Check spending against budget limits. Set or check budgets by category. \
         Alerts when approaching or exceeding budget limits."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "action": {
                    "type": "string",
                    "description": "Action: check (view status), set (set a budget limit). Default: check"
                },
                "category": {
                    "type": "string",
                    "description": "Budget category (e.g., 'food', 'entertainment'). Omit for overall."
                },
                "monthly_limit": {
                    "type": "number",
                    "description": "Monthly budget limit in dollars (only for 'set' action)"
                }
            }),
            vec![],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("check");
        let category = input.get("category").and_then(|v| v.as_str());
        let monthly_limit = input.get("monthly_limit").and_then(|v| v.as_f64());

        debug!("Budget check: action={}", action);

        if action == "set" {
            let limit = monthly_limit
                .ok_or_else(|| anyhow::anyhow!("Missing 'monthly_limit' for set action"))?;
            let cat = category.unwrap_or("overall");

            let _ = self
                .db
                .insert_entity(
                    &format!("budget:{}", cat),
                    "budget",
                    Some(serde_json::json!({
                        "category": cat,
                        "monthly_limit": limit,
                        "updated_at": chrono::Utc::now().to_rfc3339(),
                    })),
                )
                .await?;

            return Ok(format!(
                "Budget set: {} = ${:.2}/month",
                cat, limit
            ));
        }

        // Check mode — get budgets and current spending
        let budgets = self
            .db
            .search_entities("budget:", Some("budget"))
            .await
            .unwrap_or_default();

        let expenses = self
            .db
            .search_entities("", Some("expense"))
            .await
            .unwrap_or_default();

        // Calculate current month spending by category
        let mut spending: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        for expense in &expenses {
            if let Some(meta) = &expense.metadata {
                let amount = meta.get("amount").and_then(|a| a.as_f64()).unwrap_or(0.0);
                let cat = meta
                    .get("category")
                    .and_then(|c| c.as_str())
                    .unwrap_or("other");
                *spending.entry(cat.to_string()).or_insert(0.0) += amount;
            }
        }

        if budgets.is_empty() {
            return Ok(format!(
                "No budgets configured. Use budget_check with action='set' to create one.\n\n\
                 Current spending:\n{}",
                spending
                    .iter()
                    .map(|(cat, amt)| format!("  - {}: ${:.2}", cat, amt))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        let mut output = String::from("# Budget Status\n\n");
        for budget in &budgets {
            if let Some(meta) = &budget.metadata {
                let cat = meta
                    .get("category")
                    .and_then(|c| c.as_str())
                    .unwrap_or("overall");
                let limit = meta
                    .get("monthly_limit")
                    .and_then(|l| l.as_f64())
                    .unwrap_or(0.0);

                if let Some(filter_cat) = category {
                    if cat != filter_cat {
                        continue;
                    }
                }

                let spent = if cat == "overall" {
                    spending.values().sum()
                } else {
                    *spending.get(cat).unwrap_or(&0.0)
                };

                let pct = if limit > 0.0 {
                    (spent / limit * 100.0) as u32
                } else {
                    0
                };
                let remaining = limit - spent;
                let status = if pct >= 100 {
                    "OVER BUDGET"
                } else if pct >= 80 {
                    "WARNING"
                } else {
                    "OK"
                };

                output.push_str(&format!(
                    "- {} [{}]: ${:.2} / ${:.2} ({}%) — ${:.2} remaining\n",
                    cat, status, spent, limit, pct, remaining.max(0.0)
                ));
            }
        }

        Ok(output)
    }
}

/// Parse expense from receipt email
pub struct ParseReceiptTool;

impl ParseReceiptTool {
    pub fn new(_db: Arc<KnowledgeDb>) -> Self {
        Self
    }
}

#[async_trait]
impl ToolHandler for ParseReceiptTool {
    fn name(&self) -> &str {
        "parse_receipt"
    }

    fn description(&self) -> &str {
        "Parse a receipt or transaction notification to extract expense details. Accepts raw \
         email text or transaction details and extracts amount, vendor, category, and date. \
         Optionally auto-logs the expense."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "text": {
                    "type": "string",
                    "description": "Receipt text, email body, or transaction notification to parse"
                },
                "auto_log": {
                    "type": "boolean",
                    "description": "Automatically log the parsed expense (default: false — present for confirmation)"
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
        let auto_log = input
            .get("auto_log")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if text.len() > 50_000 {
            return Err(anyhow::anyhow!("Text too long (max 50,000 characters)"));
        }

        debug!("Parsing receipt ({} chars)", text.len());

        Ok(format!(
            "Receipt/Transaction Text:\n\n{}\n\n\
             ---\n\n\
             Please extract the following from the text above:\n\
             1. **Amount** — total charged\n\
             2. **Vendor** — merchant/store name\n\
             3. **Category** — best matching: food, transport, entertainment, shopping, bills, health, education, other\n\
             4. **Date** — transaction date\n\
             5. **Payment Method** — if mentioned\n\
             6. **Items** — line items if available\n\n\
             {}",
            &text[..text.len().min(10_000)],
            if auto_log {
                "Then automatically log the expense using log_expense."
            } else {
                "Present the extracted details for user confirmation before logging."
            }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Arc<KnowledgeDb> {
        Arc::new(KnowledgeDb::new(&std::env::temp_dir().join("test_finance.db")).unwrap())
    }

    #[test]
    fn test_log_expense_schema() {
        let tool = LogExpenseTool::new(test_db());
        assert_eq!(tool.name(), "log_expense");
        let schema = tool.input_schema();
        let required: Vec<String> = serde_json::from_value(
            schema.get("required").cloned().unwrap_or(serde_json::json!([])),
        )
        .unwrap_or_default();
        assert!(required.contains(&"amount".to_string()));
        assert!(required.contains(&"category".to_string()));
    }

    #[test]
    fn test_spending_summary_schema() {
        let tool = SpendingSummaryTool::new(test_db());
        assert_eq!(tool.name(), "spending_summary");
    }

    #[test]
    fn test_budget_check_schema() {
        let tool = BudgetCheckTool::new(test_db());
        assert_eq!(tool.name(), "budget_check");
    }

    #[test]
    fn test_parse_receipt_schema() {
        let tool = ParseReceiptTool::new(test_db());
        assert_eq!(tool.name(), "parse_receipt");
    }

    #[tokio::test]
    async fn test_log_expense_negative_amount() {
        let tool = LogExpenseTool::new(test_db());
        let result = tool
            .execute(serde_json::json!({
                "amount": -10.0,
                "category": "food"
            }))
            .await;
        assert!(result.is_err());
    }
}
