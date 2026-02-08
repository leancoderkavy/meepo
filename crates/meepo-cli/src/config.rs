use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::warn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeepoConfig {
    pub agent: AgentConfig,
    pub providers: ProvidersConfig,
    pub channels: ChannelsConfig,
    pub knowledge: KnowledgeConfig,
    pub watchers: WatchersConfig,
    pub code: CodeConfig,
    pub memory: MemoryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub default_model: String,
    pub max_tokens: u32,
    #[serde(default = "default_system_prompt_file")]
    pub system_prompt_file: String,
    #[serde(default = "default_memory_file")]
    pub memory_file: String,
}

fn default_system_prompt_file() -> String {
    "SOUL.md".to_string()
}

fn default_memory_file() -> String {
    "MEMORY.md".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    pub anthropic: AnthropicConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicConfig {
    pub api_key: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
}

fn default_base_url() -> String {
    "https://api.anthropic.com".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelsConfig {
    pub discord: DiscordConfig,
    pub slack: SlackConfig,
    pub imessage: IMessageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub allowed_users: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub app_token: String,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default = "default_slack_poll_interval")]
    pub poll_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IMessageConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_trigger_prefix")]
    pub trigger_prefix: String,
    #[serde(default)]
    pub allowed_contacts: Vec<String>,
}

fn default_poll_interval() -> u64 {
    3
}

fn default_slack_poll_interval() -> u64 {
    3
}

fn default_trigger_prefix() -> String {
    "/d".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeConfig {
    pub db_path: String,
    pub tantivy_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchersConfig {
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    #[serde(default = "default_min_poll")]
    pub min_poll_interval_secs: u64,
    pub active_hours: ActiveHours,
}

fn default_max_concurrent() -> usize {
    50
}

fn default_min_poll() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveHours {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeConfig {
    #[serde(default = "default_claude_path")]
    pub claude_code_path: String,
    #[serde(default = "default_gh_path")]
    pub gh_path: String,
    #[serde(default = "default_workspace")]
    pub default_workspace: String,
}

fn default_claude_path() -> String {
    "claude".to_string()
}

fn default_gh_path() -> String {
    "gh".to_string()
}

fn default_workspace() -> String {
    "~/Coding".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub workspace: String,
}

pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".meepo")
}

impl MeepoConfig {
    pub fn load(custom_path: &Option<PathBuf>) -> Result<Self> {
        let path = custom_path
            .clone()
            .unwrap_or_else(|| config_dir().join("config.toml"));

        let content = std::fs::read_to_string(&path)
            .with_context(|| {
                format!(
                    "Failed to read config at {}. Run `meepo init` first.",
                    path.display()
                )
            })?;

        // Expand environment variables before parsing
        let expanded = expand_env_vars(&content);

        let config: Self = toml::from_str(&expanded)
            .with_context(|| format!("Failed to parse config at {}", path.display()))?;

        // Check for hardcoded API keys and tokens
        if config.providers.anthropic.api_key.starts_with("sk-ant-") {
            warn!("API key is hardcoded in config file. For security, use environment variables: api_key = \"${{ANTHROPIC_API_KEY}}\"");
        }

        if !config.channels.discord.token.is_empty() && !config.channels.discord.token.contains("${") {
            warn!("Discord token is hardcoded in config file. For security, use environment variables: token = \"${{DISCORD_TOKEN}}\"");
        }

        if !config.channels.slack.bot_token.is_empty() && !config.channels.slack.bot_token.contains("${") {
            warn!("Slack bot token is hardcoded in config file. For security, use environment variables: bot_token = \"${{SLACK_BOT_TOKEN}}\"");
        }

        if !config.channels.slack.app_token.is_empty() && !config.channels.slack.app_token.contains("${") {
            warn!("Slack app token is hardcoded in config file. For security, use environment variables: app_token = \"${{SLACK_APP_TOKEN}}\"");
        }

        Ok(config)
    }
}

fn expand_env_vars(s: &str) -> String {
    let mut result = s.to_string();
    let mut pos = 0;
    while pos < result.len() {
        if let Some(start) = result[pos..].find("${") {
            let abs_start = pos + start;
            if let Some(end) = result[abs_start..].find('}') {
                let var_name = result[abs_start + 2..abs_start + end].to_string();
                let value = std::env::var(&var_name).unwrap_or_default();
                let value_len = value.len();
                result = format!("{}{}{}", &result[..abs_start], value, &result[abs_start + end + 1..]);
                pos = abs_start + value_len; // Skip past the expanded value
            } else {
                break;
            }
        } else {
            break;
        }
    }
    result
}
