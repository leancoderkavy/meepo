# Agent Templates — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `meepo template use/list/info/reset/create/remove` commands that let users instantly configure Meepo for specific use cases (stock analyst, code reviewer, etc.) by overlaying template configs on their existing setup.

**Architecture:** Templates are directories with `template.toml` (metadata + partial config overlay) plus optional SOUL.md/MEMORY.md. A new `template.rs` module in meepo-cli handles parsing, deep-merge, activation, and reset. Goals table gets a `source` column for clean rollback. 4 built-in templates compiled into the binary.

**Tech Stack:** Rust, clap (subcommands), toml (parsing + value-level merge), serde, meepo-knowledge (goals DB)

---

### Task 1: Add `source` Column to Goals Table

The goals table needs a `source` column to distinguish user-created goals from template-injected ones. This enables clean rollback on `meepo template reset`.

**Files:**
- Modify: `crates/meepo-knowledge/src/sqlite.rs`

**Step 1: Add `source` field to Goal struct (after `source_channel` field, line ~72)**

Add this field to the `Goal` struct:

```rust
pub struct Goal {
    pub id: String,
    pub description: String,
    pub status: String,          // active|paused|completed|failed
    pub priority: i32,           // 1 (low) to 5 (critical)
    pub success_criteria: Option<String>,
    pub strategy: Option<String>,
    pub check_interval_secs: i64,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub source_channel: Option<String>,
    pub source: String,          // "user" or "template:<name>"
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

**Step 2: Add `source` column to CREATE TABLE (line ~219)**

Change the goals CREATE TABLE to include the source column:

```sql
CREATE TABLE IF NOT EXISTS goals (
    id TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    priority INTEGER NOT NULL DEFAULT 3,
    success_criteria TEXT,
    strategy TEXT,
    check_interval_secs INTEGER NOT NULL DEFAULT 1800,
    last_checked_at TEXT,
    source_channel TEXT,
    source TEXT NOT NULL DEFAULT 'user',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
)
```

Also add an ALTER TABLE migration right after the CREATE TABLE + index to handle existing databases:

```rust
// Migration: add source column to existing goals tables
let _ = conn.execute("ALTER TABLE goals ADD COLUMN source TEXT NOT NULL DEFAULT 'user'", []);
```

**Step 3: Update `insert_goal` to accept `source` parameter (line ~829)**

Change the signature to:

```rust
pub async fn insert_goal(
    &self,
    description: &str,
    priority: i32,
    check_interval_secs: i64,
    success_criteria: Option<&str>,
    source_channel: Option<&str>,
    source: &str,
) -> Result<String> {
```

Update the SQL to include source:

```rust
conn.execute(
    "INSERT INTO goals (id, description, status, priority, success_criteria, check_interval_secs, source_channel, source, created_at, updated_at)
     VALUES (?1, ?2, 'active', ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    params![&id, &description, priority, success_criteria, check_interval_secs, source_channel, &source, now.to_rfc3339(), now.to_rfc3339()],
)?;
```

**Step 4: Update `row_to_goal` to read `source` (line ~958)**

The SELECT statements return columns by index. Add `source` to all SELECT queries and update `row_to_goal`:

All SELECT queries for goals need `source` added after `source_channel`. The column list becomes:
```
id, description, status, priority, success_criteria, strategy,
check_interval_secs, last_checked_at, source_channel, source, created_at, updated_at
```

Update `row_to_goal`:
```rust
fn row_to_goal(row: &rusqlite::Row) -> rusqlite::Result<Goal> {
    Ok(Goal {
        id: row.get(0)?,
        description: row.get(1)?,
        status: row.get(2)?,
        priority: row.get(3)?,
        success_criteria: row.get(4)?,
        strategy: row.get(5)?,
        check_interval_secs: row.get(6)?,
        last_checked_at: row.get::<_, Option<String>>(7)?
            .and_then(|s| s.parse().ok()),
        source_channel: row.get(8)?,
        source: row.get(9)?,
        created_at: row.get::<_, String>(10)?.parse().unwrap_or_else(|_| Utc::now()),
        updated_at: row.get::<_, String>(11)?.parse().unwrap_or_else(|_| Utc::now()),
    })
}
```

**Step 5: Add `delete_goals_by_source` method**

```rust
/// Delete all goals with a given source prefix (e.g. "template:stock-analyst")
pub async fn delete_goals_by_source(&self, source: &str) -> Result<usize> {
    let conn = Arc::clone(&self.conn);
    let source = source.to_owned();

    tokio::task::spawn_blocking(move || {
        let conn = conn.lock().unwrap_or_else(|p| p.into_inner());
        let count = conn.execute(
            "DELETE FROM goals WHERE source = ?1",
            params![&source],
        )?;
        Ok(count)
    })
    .await
    .context("spawn_blocking task panicked")?
}
```

**Step 6: Update all callers of `insert_goal` to pass `source`**

Search for all calls to `insert_goal` and add `"user"` as the source parameter. The only callers should be in the test and possibly in tools.

**Step 7: Update test `test_goal_operations`**

Update the test to pass `"user"` as source and add a test for `delete_goals_by_source`:

```rust
// In existing test, update insert_goal call:
let id = db.insert_goal("Review PRs daily", 3, 3600, Some("All PRs reviewed"), Some("discord"), "user").await?;

// Add template goal test:
let template_id = db.insert_goal("Monitor stocks", 4, 900, None, None, "template:stock-analyst").await?;
let active = db.get_active_goals().await?;
assert_eq!(active.len(), 2); // user + template

// Test delete by source
let deleted = db.delete_goals_by_source("template:stock-analyst").await?;
assert_eq!(deleted, 1);
let active = db.get_active_goals().await?;
assert_eq!(active.len(), 1); // only user goal remains
```

**Step 8: Run tests**

Run: `cargo test -p meepo-knowledge test_goal_operations -- --nocapture`
Expected: PASS

Run: `cargo check`
Expected: compiles clean

**Step 9: Commit**

```bash
git add crates/meepo-knowledge/src/sqlite.rs
git commit -m "feat: add source column to goals table for template tracking"
```

---

### Task 2: Create Template Types and Parser

A new `template.rs` module in meepo-cli that handles template parsing, resolution, and the deep-merge logic.

**Files:**
- Create: `crates/meepo-cli/src/template.rs`
- Modify: `crates/meepo-cli/src/main.rs` (add `mod template;`)

**Step 1: Create `crates/meepo-cli/src/template.rs`**

```rust
//! Agent template system — parse, resolve, activate, and reset templates.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::info;

/// Metadata section from template.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateMetadata {
    pub name: String,
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_version() -> String { "0.1.0".to_string() }

/// A goal defined in template.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateGoal {
    pub description: String,
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(default = "default_check_interval")]
    pub check_interval_secs: i64,
    pub success_criteria: Option<String>,
}

fn default_priority() -> i32 { 3 }
fn default_check_interval() -> i64 { 1800 }

/// Parsed template.toml — metadata + goals + raw TOML overlay
#[derive(Debug, Clone)]
pub struct Template {
    pub metadata: TemplateMetadata,
    pub goals: Vec<TemplateGoal>,
    /// The raw TOML table for config overlay (everything except [template] and [[goals]])
    pub config_overlay: toml::Value,
    /// Directory the template was loaded from
    pub dir: PathBuf,
}

/// Where a template was resolved from
#[derive(Debug)]
pub enum TemplateSource {
    BuiltIn(String),
    Local(PathBuf),
}

/// Active template state stored in .active-template
#[derive(Debug, Serialize, Deserialize)]
pub struct ActiveTemplate {
    pub name: String,
    pub source: String,
    pub activated_at: String,
}

// ── Built-in templates ──────────────────────────────────────────

struct BuiltInTemplate {
    name: &'static str,
    template_toml: &'static str,
    soul_md: &'static str,
}

const BUILT_IN_TEMPLATES: &[BuiltInTemplate] = &[
    BuiltInTemplate {
        name: "stock-analyst",
        template_toml: include_str!("../templates/stock-analyst/template.toml"),
        soul_md: include_str!("../templates/stock-analyst/SOUL.md"),
    },
    BuiltInTemplate {
        name: "code-reviewer",
        template_toml: include_str!("../templates/code-reviewer/template.toml"),
        soul_md: include_str!("../templates/code-reviewer/SOUL.md"),
    },
    BuiltInTemplate {
        name: "personal-assistant",
        template_toml: include_str!("../templates/personal-assistant/template.toml"),
        soul_md: include_str!("../templates/personal-assistant/SOUL.md"),
    },
    BuiltInTemplate {
        name: "research-agent",
        template_toml: include_str!("../templates/research-agent/template.toml"),
        soul_md: include_str!("../templates/research-agent/SOUL.md"),
    },
];

// ── Parsing ─────────────────────────────────────────────────────

impl Template {
    /// Parse a template.toml string into a Template
    pub fn parse(content: &str, dir: PathBuf) -> Result<Self> {
        let raw: toml::Value = toml::from_str(content)
            .context("Failed to parse template.toml")?;

        let table = raw.as_table()
            .context("template.toml must be a TOML table")?;

        // Extract [template] section
        let template_section = table.get("template")
            .context("template.toml must have a [template] section")?;
        let metadata: TemplateMetadata = template_section.clone().try_into()
            .context("Invalid [template] section")?;

        // Extract [[goals]] array
        let goals: Vec<TemplateGoal> = if let Some(goals_val) = table.get("goals") {
            goals_val.clone().try_into()
                .context("Invalid [[goals]] array")?
        } else {
            vec![]
        };

        // Everything else is config overlay
        let mut overlay = toml::map::Map::new();
        for (key, value) in table {
            if key != "template" && key != "goals" {
                overlay.insert(key.clone(), value.clone());
            }
        }

        Ok(Template {
            metadata,
            goals,
            config_overlay: toml::Value::Table(overlay),
            dir,
        })
    }
}

// ── Resolution ──────────────────────────────────────────────────

/// Resolve a template name/path to a Template
pub fn resolve_template(name_or_path: &str) -> Result<Template> {
    // 1. Check if it's a local path
    let path = PathBuf::from(name_or_path);
    if path.exists() && path.join("template.toml").exists() {
        let content = std::fs::read_to_string(path.join("template.toml"))
            .context("Failed to read template.toml")?;
        return Template::parse(&content, path);
    }

    // 2. Check built-in templates
    if let Some(built_in) = BUILT_IN_TEMPLATES.iter().find(|t| t.name == name_or_path) {
        // Built-in templates use a synthetic dir
        let dir = crate::config::config_dir().join("templates").join(built_in.name);
        let mut template = Template::parse(built_in.template_toml, dir)?;
        // Store soul content for later use
        return Ok(template);
    }

    // 3. Check ~/.meepo/templates/<name>/
    let local_dir = crate::config::config_dir().join("templates").join(name_or_path);
    if local_dir.join("template.toml").exists() {
        let content = std::fs::read_to_string(local_dir.join("template.toml"))
            .context("Failed to read template.toml")?;
        return Template::parse(&content, local_dir);
    }

    // 4. GitHub (gh:user/repo/path) — deferred to future task
    if name_or_path.starts_with("gh:") {
        bail!("GitHub template fetching not yet implemented. Download the template locally and use the path instead.");
    }

    bail!(
        "Template '{}' not found.\n\n\
         Available built-in templates: {}\n\
         Or provide a path to a template directory.",
        name_or_path,
        BUILT_IN_TEMPLATES.iter().map(|t| t.name).collect::<Vec<_>>().join(", ")
    );
}

/// List all available templates (built-in + local)
pub fn list_templates() -> Vec<(String, String, String)> {
    let mut templates = Vec::new();

    // Built-in
    for built_in in BUILT_IN_TEMPLATES {
        if let Ok(t) = Template::parse(built_in.template_toml, PathBuf::new()) {
            templates.push((
                t.metadata.name,
                t.metadata.description,
                "built-in".to_string(),
            ));
        }
    }

    // Local
    let templates_dir = crate::config::config_dir().join("templates");
    if let Ok(entries) = std::fs::read_dir(&templates_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join("template.toml").exists() {
                if let Ok(content) = std::fs::read_to_string(path.join("template.toml")) {
                    if let Ok(t) = Template::parse(&content, path) {
                        templates.push((
                            t.metadata.name,
                            t.metadata.description,
                            "local".to_string(),
                        ));
                    }
                }
            }
        }
    }

    templates
}

// ── Deep Merge ──────────────────────────────────────────────────

/// Deep merge template overlay into user config TOML value.
/// Rules: scalars override, arrays append, tables recurse.
pub fn deep_merge(base: &mut toml::Value, overlay: &toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base_table), toml::Value::Table(overlay_table)) => {
            for (key, overlay_val) in overlay_table {
                if let Some(base_val) = base_table.get_mut(key) {
                    deep_merge(base_val, overlay_val);
                } else {
                    base_table.insert(key.clone(), overlay_val.clone());
                }
            }
        }
        (toml::Value::Array(base_arr), toml::Value::Array(overlay_arr)) => {
            base_arr.extend(overlay_arr.iter().cloned());
        }
        (base, overlay) => {
            *base = overlay.clone();
        }
    }
}

// ── Activation ──────────────────────────────────────────────────

/// Get the SOUL.md content for a template.
/// For built-in templates, returns the compiled-in content.
/// For local templates, reads from the template directory.
pub fn get_template_soul(template: &Template) -> Result<Option<String>> {
    // Check built-in first
    if let Some(built_in) = BUILT_IN_TEMPLATES.iter().find(|t| t.name == template.metadata.name) {
        return Ok(Some(built_in.soul_md.to_string()));
    }

    // Check template directory
    let soul_path = template.dir.join("SOUL.md");
    if soul_path.exists() {
        let content = std::fs::read_to_string(&soul_path)
            .context("Failed to read template SOUL.md")?;
        return Ok(Some(content));
    }

    Ok(None)
}

/// Get optional MEMORY.md content from the template directory.
pub fn get_template_memory(template: &Template) -> Result<Option<String>> {
    let memory_path = template.dir.join("MEMORY.md");
    if memory_path.exists() {
        let content = std::fs::read_to_string(&memory_path)
            .context("Failed to read template MEMORY.md")?;
        return Ok(Some(content));
    }
    Ok(None)
}

/// Read the active template state
pub fn get_active_template() -> Option<ActiveTemplate> {
    let path = crate::config::config_dir().join(".active-template");
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

/// Write the active template state
pub fn set_active_template(name: &str, source: &str) -> Result<()> {
    let state = ActiveTemplate {
        name: name.to_string(),
        source: source.to_string(),
        activated_at: chrono::Utc::now().to_rfc3339(),
    };
    let path = crate::config::config_dir().join(".active-template");
    let content = toml::to_string_pretty(&state)?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// Clear the active template state
pub fn clear_active_template() -> Result<()> {
    let path = crate::config::config_dir().join(".active-template");
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_template() {
        let toml = r#"
[template]
name = "test-agent"
description = "A test template"

[[goals]]
description = "Do something"
priority = 4
check_interval_secs = 900

[autonomy]
tick_interval_secs = 10
"#;
        let t = Template::parse(toml, PathBuf::from("/tmp/test")).unwrap();
        assert_eq!(t.metadata.name, "test-agent");
        assert_eq!(t.goals.len(), 1);
        assert_eq!(t.goals[0].priority, 4);

        // Config overlay should have autonomy but not template or goals
        let overlay = t.config_overlay.as_table().unwrap();
        assert!(overlay.contains_key("autonomy"));
        assert!(!overlay.contains_key("template"));
        assert!(!overlay.contains_key("goals"));
    }

    #[test]
    fn test_deep_merge_scalars() {
        let mut base: toml::Value = toml::from_str(r#"
[autonomy]
tick_interval_secs = 30
max_goals = 50
"#).unwrap();
        let overlay: toml::Value = toml::from_str(r#"
[autonomy]
tick_interval_secs = 10
"#).unwrap();
        deep_merge(&mut base, &overlay);
        let autonomy = base.get("autonomy").unwrap().as_table().unwrap();
        assert_eq!(autonomy["tick_interval_secs"].as_integer(), Some(10));
        assert_eq!(autonomy["max_goals"].as_integer(), Some(50)); // preserved
    }

    #[test]
    fn test_deep_merge_arrays_append() {
        let mut base: toml::Value = toml::from_str(r#"
[filesystem]
allowed_directories = ["~/Coding"]
"#).unwrap();
        let overlay: toml::Value = toml::from_str(r#"
[filesystem]
allowed_directories = ["~/Projects"]
"#).unwrap();
        deep_merge(&mut base, &overlay);
        let dirs = base["filesystem"]["allowed_directories"].as_array().unwrap();
        assert_eq!(dirs.len(), 2);
    }

    #[test]
    fn test_list_built_in_templates() {
        let templates = list_templates();
        assert!(templates.len() >= 4); // at least the 4 built-in ones
        let names: Vec<&str> = templates.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(names.contains(&"stock-analyst"));
        assert!(names.contains(&"code-reviewer"));
    }

    #[test]
    fn test_resolve_built_in() {
        let t = resolve_template("stock-analyst").unwrap();
        assert_eq!(t.metadata.name, "stock-analyst");
        assert!(!t.goals.is_empty());
    }
}
```

**Step 2: Add `mod template;` to main.rs (after `mod config;`, line 12)**

```rust
mod config;
mod template;
```

**Step 3: Run tests**

Run: `cargo test -p meepo-cli template -- --nocapture`
Expected: PASS (after templates are created in Task 3)

Run: `cargo check`
Expected: compile error about missing template files — that's OK, Task 3 creates them

**Step 4: Commit (after Task 3 creates the template files)**

This task and Task 3 commit together since the template module depends on the built-in template files.

---

### Task 3: Create Built-in Template Files

Create the 4 template directories with template.toml and SOUL.md for each.

**Files:**
- Create: `crates/meepo-cli/templates/stock-analyst/template.toml`
- Create: `crates/meepo-cli/templates/stock-analyst/SOUL.md`
- Create: `crates/meepo-cli/templates/code-reviewer/template.toml`
- Create: `crates/meepo-cli/templates/code-reviewer/SOUL.md`
- Create: `crates/meepo-cli/templates/personal-assistant/template.toml`
- Create: `crates/meepo-cli/templates/personal-assistant/SOUL.md`
- Create: `crates/meepo-cli/templates/research-agent/template.toml`
- Create: `crates/meepo-cli/templates/research-agent/SOUL.md`

**Step 1: stock-analyst/template.toml**

```toml
[template]
name = "stock-analyst"
description = "Financial markets analyst — monitors stocks, summarizes earnings, alerts on price movements"
version = "1.0.0"
author = "meepo"
tags = ["finance", "stocks", "analysis"]

[[goals]]
description = "Monitor watchlist stocks for significant price movements (>3% intraday change)"
priority = 4
check_interval_secs = 900
success_criteria = "Alert sent for every >3% move within 5 minutes of detection"

[[goals]]
description = "Summarize market conditions at market open (9:35 AM ET) on trading days"
priority = 3
check_interval_secs = 3600
success_criteria = "Summary includes major indices, top movers, and relevant news"

[[goals]]
description = "Track earnings calendar and summarize results for watchlist companies"
priority = 3
check_interval_secs = 7200

[autonomy]
tick_interval_secs = 15
max_tokens_per_tick = 8192
```

**Step 2: stock-analyst/SOUL.md**

```markdown
# Meepo — Stock Analyst

You are Meepo configured as a financial markets analyst. You monitor stocks, track earnings, and alert on significant price movements.

## Personality
- Data-driven and precise — always cite numbers and sources
- Proactive — alert on significant moves before being asked
- Concise — lead with the key number, then context

## Capabilities
- Monitor stock prices and alert on >3% intraday moves
- Summarize market conditions at market open
- Track earnings calendar and summarize results
- Research companies using web search
- Maintain a watchlist of stocks the user cares about

## Rules
- Always include ticker symbols and percentage changes
- Use web search to verify current prices — never hallucinate numbers
- For earnings: report EPS vs estimate, revenue vs estimate, and guidance
- Active during US market hours (9:30 AM - 4:00 PM ET) by default
- Save important findings to knowledge graph for future reference
```

**Step 3: code-reviewer/template.toml**

```toml
[template]
name = "code-reviewer"
description = "GitHub PR reviewer — daily triage, automated review comments, code quality analysis"
version = "1.0.0"
author = "meepo"
tags = ["code", "github", "review"]

[[goals]]
description = "Triage open PRs daily — summarize what needs review and flag stale PRs"
priority = 4
check_interval_secs = 3600
success_criteria = "Daily summary of open PRs with age, size, and review status"

[[goals]]
description = "Review new PRs for code quality, security, and test coverage"
priority = 4
check_interval_secs = 1800

[autonomy]
tick_interval_secs = 30
max_tokens_per_tick = 8192
```

**Step 4: code-reviewer/SOUL.md**

```markdown
# Meepo — Code Reviewer

You are Meepo configured as a code review assistant. You triage PRs, review code, and help maintain code quality.

## Personality
- Thorough but constructive — find issues, suggest improvements
- Prioritize: security > correctness > performance > style
- Concise review comments — one issue per comment, with fix suggestion

## Capabilities
- Daily PR triage across configured repositories
- Automated code review for new PRs
- Security vulnerability detection
- Test coverage analysis
- Code quality and style feedback

## Rules
- Never approve PRs automatically — always flag for human decision
- Prioritize security issues and mark them as blocking
- Include code suggestions (diff format) in review comments
- Track review turnaround time and flag stale PRs (>48 hours)
- Delegate actual code writing to Claude Code CLI
```

**Step 5: personal-assistant/template.toml**

```toml
[template]
name = "personal-assistant"
description = "Daily assistant — morning briefings, calendar management, reminders, email triage"
version = "1.0.0"
author = "meepo"
tags = ["assistant", "calendar", "email", "productivity"]

[[goals]]
description = "Prepare a morning briefing with calendar, unread emails, and pending reminders"
priority = 3
check_interval_secs = 3600
success_criteria = "Briefing sent by 8:30 AM on weekdays"

[[goals]]
description = "Monitor incoming emails and flag urgent ones that need attention"
priority = 4
check_interval_secs = 600

[autonomy]
tick_interval_secs = 30
min_confidence_to_act = 0.6
```

**Step 6: personal-assistant/SOUL.md**

```markdown
# Meepo — Personal Assistant

You are Meepo configured as a daily personal assistant. You manage calendar, email, reminders, and help stay organized.

## Personality
- Friendly and efficient — like a great executive assistant
- Proactive about scheduling conflicts and deadlines
- Respectful of focus time — batch non-urgent notifications

## Capabilities
- Morning briefings with calendar, emails, and reminders
- Email triage — flag urgent, summarize the rest
- Calendar conflict detection and scheduling help
- Reminder management and follow-up tracking
- Note-taking and information lookup

## Rules
- Morning briefing by 8:30 AM on weekdays
- Don't interrupt during calendar events unless urgent
- Batch non-urgent notifications into periodic digests
- Always confirm before sending emails or creating events on behalf of user
- Respect active hours — no notifications before 8 AM or after 10 PM
```

**Step 7: research-agent/template.toml**

```toml
[template]
name = "research-agent"
description = "Deep research assistant — web search, knowledge synthesis, long-form reports"
version = "1.0.0"
author = "meepo"
tags = ["research", "analysis", "writing"]

[[goals]]
description = "Research assigned topics thoroughly using web search and save findings to knowledge graph"
priority = 4
check_interval_secs = 1800

[[goals]]
description = "Synthesize research findings into concise reports when enough data is collected"
priority = 3
check_interval_secs = 3600

[autonomy]
tick_interval_secs = 30
max_tokens_per_tick = 16384
```

**Step 8: research-agent/SOUL.md**

```markdown
# Meepo — Research Agent

You are Meepo configured as a deep research assistant. You investigate topics thoroughly, synthesize findings, and produce well-sourced reports.

## Personality
- Thorough and methodical — follow threads to primary sources
- Skeptical — verify claims, cross-reference sources, note conflicts
- Clear writer — synthesize complex topics into accessible summaries

## Capabilities
- Deep web research with source verification
- Knowledge graph for connecting related findings
- Long-form report synthesis with citations
- Comparison analysis (pros/cons, alternatives)
- Ongoing monitoring of research topics

## Rules
- Always cite sources with URLs
- Distinguish facts from opinions and estimates
- Note when information is uncertain or conflicting
- Save all significant findings to the knowledge graph
- Present multiple perspectives on controversial topics
- Use web search aggressively — don't rely on training data for current info
```

**Step 9: Run tests and compile**

Run: `cargo test -p meepo-cli template -- --nocapture`
Expected: PASS

Run: `cargo check`
Expected: compiles clean

**Step 10: Commit Tasks 2 + 3 together**

```bash
git add crates/meepo-cli/src/template.rs crates/meepo-cli/src/main.rs crates/meepo-cli/templates/
git commit -m "feat: add template parser, resolver, deep-merge, and 4 built-in templates"
```

---

### Task 4: Add CLI Subcommands

Wire the template commands into the clap CLI.

**Files:**
- Modify: `crates/meepo-cli/src/main.rs`

**Step 1: Add Template subcommand group to the Commands enum (line ~31)**

```rust
#[derive(Subcommand)]
enum Commands {
    /// Start the Meepo daemon
    Start,

    /// Stop a running Meepo daemon
    Stop,

    /// Send a one-shot message to the agent
    Ask {
        /// The message to send
        message: String,
    },

    /// Initialize config directory and default config
    Init,

    /// Interactive first-time setup wizard
    Setup,

    /// Show current configuration
    Config,

    /// Run as an MCP server (STDIO transport)
    McpServer,

    /// Manage agent templates
    Template {
        #[command(subcommand)]
        action: TemplateAction,
    },
}

#[derive(Subcommand)]
enum TemplateAction {
    /// List available templates (built-in + installed)
    List,

    /// Activate a template (overlay on current config)
    Use {
        /// Template name, path, or gh:user/repo/path
        name: String,
    },

    /// Show what a template will change
    Info {
        /// Template name or path
        name: String,
    },

    /// Remove active template and restore previous config
    Reset,

    /// Create a new template from current config
    Create {
        /// Name for the new template
        name: String,
    },

    /// Remove an installed template
    Remove {
        /// Template name to remove
        name: String,
    },
}
```

**Step 2: Add match arm in main() (after McpServer arm, line ~79)**

```rust
Commands::Template { action } => cmd_template(action).await,
```

**Step 3: Implement `cmd_template`**

```rust
async fn cmd_template(action: TemplateAction) -> Result<()> {
    match action {
        TemplateAction::List => {
            let templates = template::list_templates();
            if templates.is_empty() {
                println!("No templates available.");
                return Ok(());
            }
            println!("\n  Available Templates\n  ───────────────────\n");
            for (name, description, source) in &templates {
                println!("  {:20} ({}) — {}", name, source, description);
            }

            // Show active template if any
            if let Some(active) = template::get_active_template() {
                println!("\n  Active: {} (since {})", active.name, &active.activated_at[..10]);
            }
            println!();
            Ok(())
        }
        TemplateAction::Use { name } => {
            let t = template::resolve_template(&name)?;
            println!("\n  Activating template: {}", t.metadata.name);
            println!("  {}\n", t.metadata.description);

            let config_dir = config::config_dir();
            let config_path = config_dir.join("config.toml");
            let workspace = config_dir.join("workspace");

            // 1. Backup current config
            if config_path.exists() {
                std::fs::copy(&config_path, config_dir.join("config.toml.bak"))?;
                println!("  Backed up config.toml → config.toml.bak");
            }

            // 2. Backup and replace SOUL.md
            let soul_path = workspace.join("SOUL.md");
            if soul_path.exists() {
                std::fs::copy(&soul_path, workspace.join("SOUL.md.bak"))?;
            }
            if let Some(soul) = template::get_template_soul(&t)? {
                std::fs::create_dir_all(&workspace)?;
                std::fs::write(&soul_path, &soul)?;
                println!("  Installed SOUL.md ({} chars)", soul.len());
            }

            // 3. Replace MEMORY.md if template provides one
            if let Some(memory) = template::get_template_memory(&t)? {
                let memory_path = workspace.join("MEMORY.md");
                if memory_path.exists() {
                    std::fs::copy(&memory_path, workspace.join("MEMORY.md.bak"))?;
                }
                std::fs::write(&memory_path, &memory)?;
                println!("  Installed MEMORY.md ({} chars)", memory.len());
            }

            // 4. Deep-merge config overlay
            if config_path.exists() {
                let config_content = std::fs::read_to_string(&config_path)?;
                let mut config_val: toml::Value = toml::from_str(&config_content)
                    .context("Failed to parse current config.toml")?;
                template::deep_merge(&mut config_val, &t.config_overlay);
                let merged = toml::to_string_pretty(&config_val)?;
                std::fs::write(&config_path, &merged)?;
                println!("  Merged config overlay");
            }

            // 5. Insert goals into database
            if !t.goals.is_empty() {
                let db_path = config_dir.join("knowledge.db");
                if db_path.exists() {
                    let db = meepo_knowledge::KnowledgeDb::new(&db_path)?;
                    let source = format!("template:{}", t.metadata.name);
                    for goal in &t.goals {
                        db.insert_goal(
                            &goal.description,
                            goal.priority,
                            goal.check_interval_secs,
                            goal.success_criteria.as_deref(),
                            None,
                            &source,
                        ).await?;
                    }
                    println!("  Injected {} goals", t.goals.len());
                } else {
                    println!("  Warning: knowledge.db not found — goals not injected. Run `meepo start` first.");
                }
            }

            // 6. Copy skills if present
            let skills_src = t.dir.join("skills");
            if skills_src.exists() && skills_src.is_dir() {
                let skills_dst = config_dir.join("skills");
                std::fs::create_dir_all(&skills_dst)?;
                let mut count = 0;
                for entry in std::fs::read_dir(&skills_src)?.flatten() {
                    let dst = skills_dst.join(entry.file_name());
                    if entry.path().is_dir() {
                        copy_dir_recursive(&entry.path(), &dst)?;
                        count += 1;
                    }
                }
                if count > 0 {
                    println!("  Installed {} skills", count);
                }
            }

            // 7. Record active template
            template::set_active_template(&t.metadata.name, "local")?;

            println!("\n  Template '{}' activated!", t.metadata.name);
            println!("  Restart the daemon for changes to take effect: meepo stop && meepo start\n");
            Ok(())
        }
        TemplateAction::Info { name } => {
            let t = template::resolve_template(&name)?;
            println!("\n  Template: {}", t.metadata.name);
            println!("  Description: {}", t.metadata.description);
            println!("  Version: {}", t.metadata.version);
            if !t.metadata.author.is_empty() {
                println!("  Author: {}", t.metadata.author);
            }
            if !t.metadata.tags.is_empty() {
                println!("  Tags: {}", t.metadata.tags.join(", "));
            }
            println!("\n  Goals ({}):", t.goals.len());
            for goal in &t.goals {
                println!("    - [P{}] {} (every {}s)", goal.priority, goal.description, goal.check_interval_secs);
            }
            if let Some(overlay) = t.config_overlay.as_table() {
                if !overlay.is_empty() {
                    println!("\n  Config overlay:");
                    for key in overlay.keys() {
                        println!("    [{}]", key);
                    }
                }
            }
            if let Some(soul) = template::get_template_soul(&t)? {
                println!("\n  SOUL.md: {} chars", soul.len());
            }
            println!();
            Ok(())
        }
        TemplateAction::Reset => {
            let config_dir = config::config_dir();

            let active = template::get_active_template();
            if active.is_none() {
                println!("No active template to reset.");
                return Ok(());
            }
            let active = active.unwrap();
            println!("\n  Resetting template: {}", active.name);

            // 1. Restore config.toml
            let bak = config_dir.join("config.toml.bak");
            if bak.exists() {
                std::fs::copy(&bak, config_dir.join("config.toml"))?;
                std::fs::remove_file(&bak)?;
                println!("  Restored config.toml from backup");
            }

            // 2. Restore SOUL.md
            let workspace = config_dir.join("workspace");
            let soul_bak = workspace.join("SOUL.md.bak");
            if soul_bak.exists() {
                std::fs::copy(&soul_bak, workspace.join("SOUL.md"))?;
                std::fs::remove_file(&soul_bak)?;
                println!("  Restored SOUL.md from backup");
            }

            // 3. Restore MEMORY.md
            let memory_bak = workspace.join("MEMORY.md.bak");
            if memory_bak.exists() {
                std::fs::copy(&memory_bak, workspace.join("MEMORY.md"))?;
                std::fs::remove_file(&memory_bak)?;
                println!("  Restored MEMORY.md from backup");
            }

            // 4. Delete template goals
            let db_path = config_dir.join("knowledge.db");
            if db_path.exists() {
                let db = meepo_knowledge::KnowledgeDb::new(&db_path)?;
                let source = format!("template:{}", active.name);
                let deleted = db.delete_goals_by_source(&source).await?;
                println!("  Removed {} template goals", deleted);
            }

            // 5. Clear active template
            template::clear_active_template()?;

            println!("\n  Template reset complete!");
            println!("  Restart the daemon: meepo stop && meepo start\n");
            Ok(())
        }
        TemplateAction::Create { name } => {
            let config_dir = config::config_dir();
            let template_dir = config_dir.join("templates").join(&name);

            if template_dir.exists() {
                bail!("Template '{}' already exists at {}", name, template_dir.display());
            }

            std::fs::create_dir_all(&template_dir)?;

            // Copy current SOUL.md
            let workspace = config_dir.join("workspace");
            let soul_src = workspace.join("SOUL.md");
            if soul_src.exists() {
                std::fs::copy(&soul_src, template_dir.join("SOUL.md"))?;
            }

            // Create template.toml with metadata
            let template_toml = format!(
                r#"[template]
name = "{}"
description = "Custom agent template"
version = "0.1.0"
author = ""
tags = []

# Add goals below:
# [[goals]]
# description = "Your goal here"
# priority = 3
# check_interval_secs = 1800

# Add config overrides below (same format as config.toml):
# [autonomy]
# tick_interval_secs = 30
"#,
                name
            );
            std::fs::write(template_dir.join("template.toml"), template_toml)?;

            println!("\n  Created template scaffold at {}", template_dir.display());
            println!("  Edit template.toml and SOUL.md, then activate with:");
            println!("    meepo template use {}\n", name);
            Ok(())
        }
        TemplateAction::Remove { name } => {
            let template_dir = config::config_dir().join("templates").join(&name);
            if !template_dir.exists() {
                bail!("Template '{}' not found at {}", name, template_dir.display());
            }

            // Check if active
            if let Some(active) = template::get_active_template() {
                if active.name == name {
                    bail!("Template '{}' is currently active. Run `meepo template reset` first.", name);
                }
            }

            std::fs::remove_dir_all(&template_dir)?;
            println!("Removed template '{}'.", name);
            Ok(())
        }
    }
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)?.flatten() {
        let target = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
```

**Step 4: Add missing import to main.rs**

At the top of main.rs, add `use std::path::Path;` if not already present.
Also add `use anyhow::bail;` if not already imported.

**Step 5: Run checks**

Run: `cargo check`
Expected: compiles clean

Run: `cargo test -p meepo-cli -- --nocapture`
Expected: PASS

**Step 6: Commit**

```bash
git add crates/meepo-cli/src/main.rs
git commit -m "feat: add meepo template CLI commands (list/use/info/reset/create/remove)"
```

---

### Task 5: Fix Merge Conflicts in default.toml

The file `config/default.toml` has unresolved merge conflicts on lines 53-76 and 105-111. Fix them.

**Files:**
- Modify: `config/default.toml`

**Step 1: Resolve conflicts**

Keep the better (more detailed) version from the "Stashed changes" side for iMessage (the one with requirements and how-it-works), and the "Updated upstream" side for email (the one with `poll_interval_secs = 10` and `subject_prefix`).

**Step 2: Commit**

```bash
git add config/default.toml
git commit -m "fix: resolve merge conflicts in default.toml"
```

---

### Task 6: Integration Test and Verify

**Step 1: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests pass

**Step 2: Build release**

Run: `cargo build --release`
Expected: compiles successfully

**Step 3: Smoke test CLI commands**

```bash
./target/release/meepo template list
./target/release/meepo template info stock-analyst
./target/release/meepo template info code-reviewer
```

Expected: Lists 4 built-in templates, shows info for each.

**Step 4: Commit if any fixes were needed**

---

### Summary of Changes

| File | Change |
|------|--------|
| `crates/meepo-knowledge/src/sqlite.rs` | +`source` column on goals, +`delete_goals_by_source`, updated `insert_goal`/`row_to_goal`/queries |
| `crates/meepo-cli/src/template.rs` | NEW — template parser, resolver, deep-merge, activation/reset logic |
| `crates/meepo-cli/src/main.rs` | +`Template` subcommand group, +`cmd_template()` with 6 actions |
| `crates/meepo-cli/templates/*/template.toml` | 4 built-in template manifests |
| `crates/meepo-cli/templates/*/SOUL.md` | 4 built-in personality files |
| `config/default.toml` | Merge conflict resolution |
