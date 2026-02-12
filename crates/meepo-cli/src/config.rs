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
    #[serde(default)]
    pub filesystem: FilesystemConfig,
    #[serde(default = "default_orchestrator_config")]
    pub orchestrator: OrchestratorConfig,
    #[serde(default = "default_autonomy_config")]
    pub autonomy: AutonomyConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub a2a: A2aConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub browser: BrowserConfig,
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
    #[serde(default)]
    pub tavily: Option<TavilyConfig>,
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
pub struct TavilyConfig {
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelsConfig {
    pub discord: DiscordConfig,
    pub slack: SlackConfig,
    pub imessage: IMessageConfig,
    #[serde(default)]
    pub email: EmailConfig,
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
    #[serde(default)]
    pub allowed_contacts: Vec<String>,
}

fn default_poll_interval() -> u64 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_email_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_subject_prefix")]
    pub subject_prefix: String,
}

fn default_email_poll_interval() -> u64 {
    10
}

fn default_subject_prefix() -> String {
    "[meepo]".to_string()
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: default_email_poll_interval(),
            subject_prefix: default_subject_prefix(),
        }
    }
}

fn default_slack_poll_interval() -> u64 {
    3
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemConfig {
    #[serde(default = "default_allowed_directories")]
    pub allowed_directories: Vec<String>,
}

fn default_allowed_directories() -> Vec<String> {
    vec!["~/Coding".to_string()]
}

impl Default for FilesystemConfig {
    fn default() -> Self {
        Self {
            allowed_directories: default_allowed_directories(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    #[serde(default = "default_max_concurrent_subtasks")]
    pub max_concurrent_subtasks: usize,
    #[serde(default = "default_max_subtasks_per_request")]
    pub max_subtasks_per_request: usize,
    #[serde(default = "default_parallel_timeout_secs")]
    pub parallel_timeout_secs: u64,
    #[serde(default = "default_background_timeout_secs")]
    pub background_timeout_secs: u64,
    #[serde(default = "default_max_background_groups")]
    pub max_background_groups: usize,
}

fn default_max_concurrent_subtasks() -> usize { 5 }
fn default_max_subtasks_per_request() -> usize { 10 }
fn default_parallel_timeout_secs() -> u64 { 120 }
fn default_background_timeout_secs() -> u64 { 600 }
fn default_max_background_groups() -> usize { 3 }

fn default_orchestrator_config() -> OrchestratorConfig {
    OrchestratorConfig {
        max_concurrent_subtasks: default_max_concurrent_subtasks(),
        max_subtasks_per_request: default_max_subtasks_per_request(),
        parallel_timeout_secs: default_parallel_timeout_secs(),
        background_timeout_secs: default_background_timeout_secs(),
        max_background_groups: default_max_background_groups(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyConfig {
    #[serde(default = "default_autonomy_enabled")]
    pub enabled: bool,
    #[serde(default = "default_tick_interval")]
    pub tick_interval_secs: u64,
    #[serde(default = "default_max_goals")]
    pub max_goals: usize,
    #[serde(default = "default_preference_decay_days")]
    pub preference_decay_days: u32,
    #[serde(default = "default_min_confidence")]
    pub min_confidence_to_act: f64,
    #[serde(default = "default_max_tokens_per_tick")]
    pub max_tokens_per_tick: u32,
    #[serde(default = "default_send_acknowledgments")]
    pub send_acknowledgments: bool,
}

fn default_autonomy_enabled() -> bool { true }
fn default_tick_interval() -> u64 { 30 }
fn default_max_goals() -> usize { 50 }
fn default_preference_decay_days() -> u32 { 30 }
fn default_min_confidence() -> f64 { 0.5 }
fn default_max_tokens_per_tick() -> u32 { 4096 }
fn default_send_acknowledgments() -> bool { true }

fn default_autonomy_config() -> AutonomyConfig {
    AutonomyConfig {
        enabled: default_autonomy_enabled(),
        tick_interval_secs: default_tick_interval(),
        max_goals: default_max_goals(),
        preference_decay_days: default_preference_decay_days(),
        min_confidence_to_act: default_min_confidence(),
        max_tokens_per_tick: default_max_tokens_per_tick(),
        send_acknowledgments: default_send_acknowledgments(),
    }
}

// ── MCP Config ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub server: McpServerConfig,
    #[serde(default)]
    pub clients: Vec<McpClientEntry>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            server: McpServerConfig::default(),
            clients: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub exposed_tools: Vec<String>,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            exposed_tools: vec![],
        }
    }
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpClientEntry {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

// ── A2A Config ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_a2a_port")]
    pub port: u16,
    #[serde(default)]
    pub auth_token: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub agents: Vec<A2aAgentEntry>,
}

fn default_a2a_port() -> u16 { 8081 }

impl Default for A2aConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: default_a2a_port(),
            auth_token: String::new(),
            allowed_tools: vec![],
            agents: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aAgentEntry {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub token: String,
}

// ── Skills Config ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_skills_dir")]
    pub dir: String,
}

fn default_skills_dir() -> String { "~/.meepo/skills".to_string() }

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dir: default_skills_dir(),
        }
    }
}

// ── Browser Config ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConfig {
    #[serde(default = "default_browser_enabled")]
    pub enabled: bool,
    #[serde(default = "default_browser_name")]
    pub default_browser: String,
}

fn default_browser_enabled() -> bool { true }
fn default_browser_name() -> String { "safari".to_string() }

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enabled: default_browser_enabled(),
            default_browser: default_browser_name(),
        }
    }
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
