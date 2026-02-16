//! LLM Tool Selector
//!
//! Uses an LLM call to dynamically select the most relevant tools for a
//! given query before passing them to the main agent call. Reduces token
//! usage and improves accuracy when many tools are available.
//! Inspired by LangChain v1's LLMToolSelectorMiddleware.

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::api::{ApiClient, ApiMessage, ContentBlock, MessageContent, ToolDefinition};

/// Configuration for the tool selector
#[derive(Debug, Clone)]
pub struct ToolSelectorConfig {
    /// Whether the tool selector is enabled
    pub enabled: bool,
    /// Maximum number of tools to select per query
    pub max_tools: usize,
    /// Tools that are always included regardless of selection
    pub always_include: Vec<String>,
    /// Minimum number of registered tools before selector activates
    /// (no point selecting from a small set)
    pub activation_threshold: usize,
}

impl Default for ToolSelectorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_tools: 15,
            always_include: vec![
                "remember".to_string(),
                "recall".to_string(),
                "search_knowledge".to_string(),
                "delegate_tasks".to_string(),
                "agent_status".to_string(),
            ],
            activation_threshold: 20,
        }
    }
}

/// Select the most relevant tools for a query.
///
/// Returns a filtered list of tool definitions. If the selector is disabled
/// or the tool count is below the activation threshold, returns all tools.
pub async fn select_tools(
    api: &ApiClient,
    query: &str,
    all_tools: &[ToolDefinition],
    config: &ToolSelectorConfig,
) -> Result<Vec<ToolDefinition>> {
    // Skip selection if disabled or too few tools
    if !config.enabled || all_tools.len() <= config.activation_threshold {
        debug!(
            "Tool selection skipped (enabled={}, tool_count={})",
            config.enabled,
            all_tools.len()
        );
        return Ok(all_tools.to_vec());
    }

    // Try heuristic selection first (fast, free)
    let heuristic_result = select_heuristic(query, all_tools, config);

    // If heuristic is confident (found specific tool categories), use it
    if heuristic_result.len() >= 3 && heuristic_result.len() <= config.max_tools {
        debug!(
            "Heuristic selected {} tools for query",
            heuristic_result.len()
        );
        return Ok(heuristic_result);
    }

    // Fall back to LLM selection
    match select_with_llm(api, query, all_tools, config).await {
        Ok(selected) => {
            info!("LLM selected {} tools for query", selected.len());
            Ok(selected)
        }
        Err(e) => {
            warn!("LLM tool selection failed, using all tools: {}", e);
            Ok(all_tools.to_vec())
        }
    }
}

/// Heuristic tool selection based on keyword matching
fn select_heuristic(
    query: &str,
    all_tools: &[ToolDefinition],
    config: &ToolSelectorConfig,
) -> Vec<ToolDefinition> {
    let lower = query.to_lowercase();

    // Map query keywords to tool categories
    let mut relevant_prefixes: Vec<&str> = Vec::new();

    if lower.contains("email") || lower.contains("mail") {
        relevant_prefixes.push("read_email");
        relevant_prefixes.push("send_email");
    }
    if lower.contains("calendar") || lower.contains("schedule") || lower.contains("meeting") {
        relevant_prefixes.push("read_calendar");
        relevant_prefixes.push("create_calendar");
    }
    if lower.contains("remind") {
        relevant_prefixes.push("list_reminder");
        relevant_prefixes.push("create_reminder");
    }
    if lower.contains("note") {
        relevant_prefixes.push("list_note");
        relevant_prefixes.push("create_note");
    }
    if lower.contains("browser")
        || lower.contains("web page")
        || lower.contains("website")
        || lower.contains("tab")
    {
        relevant_prefixes.push("browser_");
    }
    if lower.contains("search") || lower.contains("look up") || lower.contains("find") {
        relevant_prefixes.push("web_search");
        relevant_prefixes.push("browse_url");
        relevant_prefixes.push("search_");
    }
    if lower.contains("code")
        || lower.contains("pr ")
        || lower.contains("pull request")
        || lower.contains("github")
    {
        relevant_prefixes.push("write_code");
        relevant_prefixes.push("make_pr");
        relevant_prefixes.push("review_pr");
        relevant_prefixes.push("spawn_coding");
    }
    if lower.contains("file") || lower.contains("directory") || lower.contains("folder") {
        relevant_prefixes.push("read_file");
        relevant_prefixes.push("write_file");
        relevant_prefixes.push("list_directory");
        relevant_prefixes.push("search_files");
    }
    if lower.contains("music") || lower.contains("song") || lower.contains("play") {
        relevant_prefixes.push("get_current_track");
        relevant_prefixes.push("music_control");
    }
    if lower.contains("watch") || lower.contains("monitor") || lower.contains("cron") {
        relevant_prefixes.push("create_watcher");
        relevant_prefixes.push("list_watcher");
        relevant_prefixes.push("cancel_watcher");
    }
    if lower.contains("command")
        || lower.contains("terminal")
        || lower.contains("shell")
        || lower.contains("run ")
    {
        relevant_prefixes.push("run_command");
    }
    if lower.contains("screen") || lower.contains("click") || lower.contains("type") {
        relevant_prefixes.push("screen_capture");
        relevant_prefixes.push("read_screen");
        relevant_prefixes.push("click_element");
        relevant_prefixes.push("type_text");
    }
    if lower.contains("contact") {
        relevant_prefixes.push("search_contacts");
    }
    if lower.contains("clipboard") || lower.contains("paste") || lower.contains("copy") {
        relevant_prefixes.push("get_clipboard");
    }
    if lower.contains("notification") || lower.contains("notify") || lower.contains("alert") {
        relevant_prefixes.push("send_notification");
    }
    if lower.contains("background") || lower.contains("task") || lower.contains("status") {
        relevant_prefixes.push("spawn_background");
        relevant_prefixes.push("agent_status");
        relevant_prefixes.push("stop_task");
    }
    if lower.contains("remember")
        || lower.contains("memory")
        || lower.contains("knowledge")
        || lower.contains("know")
    {
        relevant_prefixes.push("remember");
        relevant_prefixes.push("recall");
        relevant_prefixes.push("search_knowledge");
        relevant_prefixes.push("link_entities");
    }
    if lower.contains("ingest") || lower.contains("index") || lower.contains("document") {
        relevant_prefixes.push("ingest_");
    }

    // Collect matching tools + always-include tools
    let mut selected: Vec<ToolDefinition> = all_tools
        .iter()
        .filter(|tool| {
            config.always_include.contains(&tool.name)
                || relevant_prefixes
                    .iter()
                    .any(|prefix| tool.name.starts_with(prefix))
        })
        .cloned()
        .collect();

    // Deduplicate
    let mut seen = std::collections::HashSet::new();
    selected.retain(|t| seen.insert(t.name.clone()));

    // Cap at max_tools
    selected.truncate(config.max_tools);

    selected
}

/// LLM-based tool selection for when heuristics are insufficient
async fn select_with_llm(
    api: &ApiClient,
    query: &str,
    all_tools: &[ToolDefinition],
    config: &ToolSelectorConfig,
) -> Result<Vec<ToolDefinition>> {
    // Build tool list summary
    let tool_list: String = all_tools
        .iter()
        .map(|t| {
            format!(
                "- {}: {}",
                t.name,
                t.description.chars().take(80).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Given this user query, select the most relevant tools (up to {max}).\n\
         Respond with ONLY a comma-separated list of tool names, nothing else.\n\n\
         Query: {query}\n\n\
         Available tools:\n{tools}\n\n\
         Selected tools:",
        max = config.max_tools,
        query = query,
        tools = tool_list,
    );

    let messages = vec![ApiMessage {
        role: "user".to_string(),
        content: MessageContent::Text(prompt),
    }];

    let response = api
        .chat(
            &messages,
            &[],
            "You are a tool selector. Output only comma-separated tool names.",
        )
        .await
        .context("Failed to select tools via LLM")?;

    let text = response
        .content
        .iter()
        .filter_map(|b| {
            if let ContentBlock::Text { text } = b {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<String>();

    // Parse tool names from response
    let selected_names: Vec<String> = text
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Build tool list: selected + always-include
    let mut result: Vec<ToolDefinition> = all_tools
        .iter()
        .filter(|t| selected_names.contains(&t.name) || config.always_include.contains(&t.name))
        .cloned()
        .collect();

    // Deduplicate
    let mut seen = std::collections::HashSet::new();
    result.retain(|t| seen.insert(t.name.clone()));

    result.truncate(config.max_tools);

    // Ensure we have at least the always-include tools
    if result.is_empty() {
        return Ok(all_tools.to_vec());
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool(name: &str, desc: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: desc.to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn sample_tools() -> Vec<ToolDefinition> {
        vec![
            make_tool("read_emails", "Read recent emails"),
            make_tool("send_email", "Send an email"),
            make_tool("read_calendar", "Read calendar events"),
            make_tool("create_calendar_event", "Create calendar event"),
            make_tool("browser_open_tab", "Open browser tab"),
            make_tool("browser_get_page_content", "Get page content"),
            make_tool("web_search", "Search the web"),
            make_tool("remember", "Store in knowledge graph"),
            make_tool("recall", "Search knowledge graph"),
            make_tool("search_knowledge", "Full-text search"),
            make_tool("run_command", "Execute shell command"),
            make_tool("read_file", "Read a file"),
            make_tool("write_file", "Write a file"),
            make_tool("delegate_tasks", "Delegate to sub-agents"),
            make_tool("agent_status", "Show agent status"),
            make_tool("music_control", "Control music playback"),
            make_tool("get_current_track", "Get current track"),
            make_tool("create_reminder", "Create a reminder"),
            make_tool("list_reminders", "List reminders"),
            make_tool("write_code", "Write code via Claude CLI"),
            make_tool("make_pr", "Create a pull request"),
        ]
    }

    #[test]
    fn test_heuristic_email_query() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        let selected = select_heuristic("read my latest emails", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"read_emails"));
        assert!(names.contains(&"remember")); // always included
    }

    #[test]
    fn test_heuristic_code_query() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        let selected = select_heuristic("write code for a new feature", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"write_code"));
    }

    #[test]
    fn test_heuristic_music_query() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        let selected = select_heuristic("play some music", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"music_control"));
    }

    #[test]
    fn test_always_include() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        let selected = select_heuristic("play music", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        // Always-include tools should be present
        assert!(names.contains(&"remember"));
        assert!(names.contains(&"recall"));
        assert!(names.contains(&"search_knowledge"));
    }

    #[tokio::test]
    async fn test_select_tools_below_threshold() {
        let config = ToolSelectorConfig {
            activation_threshold: 100, // higher than our tool count
            ..Default::default()
        };
        let tools = sample_tools();
        let api = ApiClient::new("test-key".to_string(), None);

        let selected = select_tools(&api, "hello", &tools, &config).await.unwrap();
        assert_eq!(selected.len(), tools.len()); // all tools returned
    }

    #[tokio::test]
    async fn test_select_tools_disabled() {
        let config = ToolSelectorConfig {
            enabled: false,
            ..Default::default()
        };
        let tools = sample_tools();
        let api = ApiClient::new("test-key".to_string(), None);

        let selected = select_tools(&api, "hello", &tools, &config).await.unwrap();
        assert_eq!(selected.len(), tools.len());
    }

    #[test]
    fn test_heuristic_calendar_query() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        let selected = select_heuristic("check my calendar for meetings", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"read_calendar"));
        assert!(names.contains(&"create_calendar_event"));
    }

    #[test]
    fn test_heuristic_reminder_query() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        let selected = select_heuristic("remind me to buy groceries", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"create_reminder"));
    }

    #[test]
    fn test_heuristic_file_query() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        let selected = select_heuristic("read the file at /tmp/test.txt", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
    }

    #[test]
    fn test_heuristic_browser_query() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        let selected = select_heuristic("open a browser tab to google.com", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"browser_open_tab"));
    }

    #[test]
    fn test_heuristic_command_query() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        let selected = select_heuristic("run a command in the terminal", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"run_command"));
    }

    #[test]
    fn test_heuristic_watcher_query() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        let selected = select_heuristic("monitor this cron job", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        // always_include tools should be present
        assert!(names.contains(&"remember"));
    }

    #[test]
    fn test_heuristic_knowledge_query() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        let selected = select_heuristic("what do you know about Rust?", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"remember"));
        assert!(names.contains(&"recall"));
        assert!(names.contains(&"search_knowledge"));
    }

    #[test]
    fn test_heuristic_deduplicates() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        // "remember" is both always-include and matched by "memory" keyword
        let selected = select_heuristic("search my memory and knowledge", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        let remember_count = names.iter().filter(|&&n| n == "remember").count();
        assert_eq!(remember_count, 1);
    }

    #[test]
    fn test_heuristic_respects_max_tools() {
        let config = ToolSelectorConfig {
            max_tools: 3,
            ..Default::default()
        };
        let tools = sample_tools();
        let selected =
            select_heuristic("search for code in files and run command", &tools, &config);
        assert!(selected.len() <= 3);
    }

    #[test]
    fn test_tool_selector_config_default() {
        let config = ToolSelectorConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_tools, 15);
        assert_eq!(config.activation_threshold, 20);
        assert!(config.always_include.contains(&"remember".to_string()));
        assert!(config.always_include.contains(&"recall".to_string()));
    }

    #[test]
    fn test_heuristic_pr_query() {
        let config = ToolSelectorConfig::default();
        let tools = sample_tools();
        let selected = select_heuristic("create a pull request for this feature", &tools, &config);

        let names: Vec<&str> = selected.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"make_pr"));
    }
}
