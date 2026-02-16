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
    #[serde(default)]
    pub notifications: NotificationsConfig,
    #[serde(default)]
    pub usage: UsageCliConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(default)]
    pub sandbox: SandboxCliConfig,
    #[serde(default)]
    pub secrets: SecretsCliConfig,
    #[serde(default)]
    pub guardrails: GuardrailsCliConfig,
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
    pub openai: Option<OpenAiProviderConfig>,
    #[serde(default)]
    pub google: Option<GoogleProviderConfig>,
    #[serde(default)]
    pub openai_compat: Option<OpenAiCompatProviderConfig>,
    #[serde(default)]
    pub ollama: Option<OllamaConfig>,
    #[serde(default)]
    pub tavily: Option<TavilyConfig>,
    #[serde(default)]
    pub failover_order: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AnthropicConfig {
    pub api_key: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
}

impl std::fmt::Debug for AnthropicConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicConfig")
            .field("api_key", &mask_secret(&self.api_key))
            .field("base_url", &self.base_url)
            .finish()
    }
}

fn default_base_url() -> String {
    "https://api.anthropic.com".to_string()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct OpenAiProviderConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_openai_base_url")]
    pub base_url: String,
    #[serde(default = "default_openai_model")]
    pub model: String,
    #[serde(default = "default_openai_max_tokens")]
    pub max_tokens: u32,
}

impl std::fmt::Debug for OpenAiProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiProviderConfig")
            .field("api_key", &mask_secret(&self.api_key))
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

fn default_openai_base_url() -> String {
    "https://api.openai.com".to_string()
}
fn default_openai_model() -> String {
    "gpt-4o".to_string()
}
fn default_openai_max_tokens() -> u32 {
    4096
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GoogleProviderConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_google_model")]
    pub model: String,
    #[serde(default = "default_google_max_tokens")]
    pub max_tokens: u32,
}

impl std::fmt::Debug for GoogleProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleProviderConfig")
            .field("api_key", &mask_secret(&self.api_key))
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

fn default_google_model() -> String {
    "gemini-2.0-flash".to_string()
}
fn default_google_max_tokens() -> u32 {
    4096
}

#[derive(Clone, Serialize, Deserialize)]
pub struct OpenAiCompatProviderConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    #[serde(default = "default_compat_max_tokens")]
    pub max_tokens: u32,
}

impl std::fmt::Debug for OpenAiCompatProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatProviderConfig")
            .field("name", &self.name)
            .field("api_key", &mask_secret(&self.api_key))
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

fn default_compat_max_tokens() -> u32 {
    4096
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_base_url")]
    pub base_url: String,
    #[serde(default = "default_ollama_model")]
    pub model: String,
    #[serde(default = "default_ollama_max_tokens")]
    pub max_tokens: u32,
}

fn default_ollama_base_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_ollama_model() -> String {
    "llama3.2".to_string()
}

fn default_ollama_max_tokens() -> u32 {
    4096
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TavilyConfig {
    #[serde(default)]
    pub api_key: String,
}

impl std::fmt::Debug for TavilyConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TavilyConfig")
            .field("api_key", &mask_secret(&self.api_key))
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelsConfig {
    pub discord: DiscordConfig,
    pub slack: SlackConfig,
    pub imessage: IMessageConfig,
    #[serde(default)]
    pub email: EmailConfig,
    #[serde(default)]
    pub alexa: AlexaConfig,
    #[serde(default)]
    pub reminders: RemindersConfig,
    #[serde(default)]
    pub notes: NotesConfig,
    #[serde(default)]
    pub contacts: ContactsConfig,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub allowed_users: Vec<String>,
}

impl std::fmt::Debug for DiscordConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscordConfig")
            .field("enabled", &self.enabled)
            .field("token", &mask_secret(&self.token))
            .field("allowed_users", &self.allowed_users)
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default = "default_slack_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default)]
    pub allowed_users: Vec<String>,
}

impl std::fmt::Debug for SlackConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlackConfig")
            .field("enabled", &self.enabled)
            .field("bot_token", &mask_secret(&self.bot_token))
            .field("poll_interval_secs", &self.poll_interval_secs)
            .field("allowed_users", &self.allowed_users)
            .finish()
    }
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

#[derive(Clone, Serialize, Deserialize)]
pub struct AlexaConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub skill_id: String,
    #[serde(default = "default_alexa_poll_interval")]
    pub poll_interval_secs: u64,
}

impl std::fmt::Debug for AlexaConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlexaConfig")
            .field("enabled", &self.enabled)
            .field("skill_id", &self.skill_id)
            .field("poll_interval_secs", &self.poll_interval_secs)
            .finish()
    }
}

impl Default for AlexaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            skill_id: String::new(),
            poll_interval_secs: default_alexa_poll_interval(),
        }
    }
}

fn default_alexa_poll_interval() -> u64 {
    3
}

fn default_slack_poll_interval() -> u64 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemindersConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_reminders_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_reminders_list_name")]
    pub list_name: String,
}

fn default_reminders_poll_interval() -> u64 {
    10
}

fn default_reminders_list_name() -> String {
    "Meepo".to_string()
}

impl Default for RemindersConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: default_reminders_poll_interval(),
            list_name: default_reminders_list_name(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotesConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_notes_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_notes_folder_name")]
    pub folder_name: String,
    #[serde(default = "default_notes_tag_prefix")]
    pub tag_prefix: String,
}

fn default_notes_poll_interval() -> u64 {
    10
}

fn default_notes_folder_name() -> String {
    "Meepo".to_string()
}

fn default_notes_tag_prefix() -> String {
    "#meepo ".to_string()
}

impl Default for NotesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: default_notes_poll_interval(),
            folder_name: default_notes_folder_name(),
            tag_prefix: default_notes_tag_prefix(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_contacts_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_contacts_group_name")]
    pub group_name: String,
}

fn default_contacts_poll_interval() -> u64 {
    10
}

fn default_contacts_group_name() -> String {
    "Meepo".to_string()
}

impl Default for ContactsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: default_contacts_poll_interval(),
            group_name: default_contacts_group_name(),
        }
    }
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

fn default_max_concurrent_subtasks() -> usize {
    5
}
fn default_max_subtasks_per_request() -> usize {
    10
}
fn default_parallel_timeout_secs() -> u64 {
    120
}
fn default_background_timeout_secs() -> u64 {
    600
}
fn default_max_background_groups() -> usize {
    3
}

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
    #[serde(default = "default_daily_plan_hour")]
    pub daily_plan_hour: u32,
    #[serde(default = "default_max_calls_per_minute")]
    pub max_calls_per_minute: u32,
}

fn default_autonomy_enabled() -> bool {
    true
}
fn default_tick_interval() -> u64 {
    30
}
fn default_max_goals() -> usize {
    50
}
fn default_preference_decay_days() -> u32 {
    30
}
fn default_min_confidence() -> f64 {
    0.5
}
fn default_max_tokens_per_tick() -> u32 {
    4096
}
fn default_send_acknowledgments() -> bool {
    true
}
fn default_daily_plan_hour() -> u32 {
    7
}
fn default_max_calls_per_minute() -> u32 {
    10
}

fn default_autonomy_config() -> AutonomyConfig {
    AutonomyConfig {
        enabled: default_autonomy_enabled(),
        tick_interval_secs: default_tick_interval(),
        max_goals: default_max_goals(),
        preference_decay_days: default_preference_decay_days(),
        min_confidence_to_act: default_min_confidence(),
        max_tokens_per_tick: default_max_tokens_per_tick(),
        send_acknowledgments: default_send_acknowledgments(),
        daily_plan_hour: default_daily_plan_hour(),
        max_calls_per_minute: default_max_calls_per_minute(),
    }
}

// ── MCP Config ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    #[serde(default)]
    pub server: McpServerConfig,
    #[serde(default)]
    pub clients: Vec<McpClientEntry>,
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

fn default_true() -> bool {
    true
}

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

#[derive(Clone, Serialize, Deserialize)]
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

impl std::fmt::Debug for A2aConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("A2aConfig")
            .field("enabled", &self.enabled)
            .field("port", &self.port)
            .field("auth_token", &mask_secret(&self.auth_token))
            .field("allowed_tools", &self.allowed_tools)
            .field("agents", &self.agents)
            .finish()
    }
}

fn default_a2a_port() -> u16 {
    8081
}

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

#[derive(Clone, Serialize, Deserialize)]
pub struct A2aAgentEntry {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub token: String,
}

impl std::fmt::Debug for A2aAgentEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("A2aAgentEntry")
            .field("name", &self.name)
            .field("url", &self.url)
            .field("token", &mask_secret(&self.token))
            .finish()
    }
}

// ── Skills Config ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_skills_dir")]
    pub dir: String,
}

fn default_skills_dir() -> String {
    "~/.meepo/skills".to_string()
}

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

fn default_browser_enabled() -> bool {
    true
}
fn default_browser_name() -> String {
    "safari".to_string()
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enabled: default_browser_enabled(),
            default_browser: default_browser_name(),
        }
    }
}

// ── Gateway Config ──────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_gateway_bind")]
    pub bind: String,
    #[serde(default = "default_gateway_port")]
    pub port: u16,
    #[serde(default)]
    pub auth_token: String,
}

impl std::fmt::Debug for GatewayConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayConfig")
            .field("enabled", &self.enabled)
            .field("bind", &self.bind)
            .field("port", &self.port)
            .field("auth_token", &mask_secret(&self.auth_token))
            .finish()
    }
}

fn default_gateway_bind() -> String {
    "127.0.0.1".to_string()
}

fn default_gateway_port() -> u16 {
    18789
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: default_gateway_bind(),
            port: default_gateway_port(),
            auth_token: String::new(),
        }
    }
}

// ── Voice / Audio Config ────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_stt_provider")]
    pub stt_provider: String,
    #[serde(default = "default_tts_provider")]
    pub tts_provider: String,
    #[serde(default)]
    pub elevenlabs_api_key: String,
    #[serde(default = "default_elevenlabs_voice_id")]
    pub elevenlabs_voice_id: String,
    #[serde(default)]
    pub wake_word: String,
    #[serde(default)]
    pub wake_enabled: bool,
}

impl std::fmt::Debug for VoiceConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoiceConfig")
            .field("enabled", &self.enabled)
            .field("stt_provider", &self.stt_provider)
            .field("tts_provider", &self.tts_provider)
            .field("elevenlabs_api_key", &mask_secret(&self.elevenlabs_api_key))
            .field("elevenlabs_voice_id", &self.elevenlabs_voice_id)
            .field("wake_word", &self.wake_word)
            .field("wake_enabled", &self.wake_enabled)
            .finish()
    }
}

fn default_stt_provider() -> String {
    "whisper_api".to_string()
}

fn default_tts_provider() -> String {
    "macos_say".to_string()
}

fn default_elevenlabs_voice_id() -> String {
    "default".to_string()
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            stt_provider: default_stt_provider(),
            tts_provider: default_tts_provider(),
            elevenlabs_api_key: String::new(),
            elevenlabs_voice_id: default_elevenlabs_voice_id(),
            wake_word: "hey meepo".to_string(),
            wake_enabled: false,
        }
    }
}

// ── Docker Sandbox Config ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxCliConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_docker_socket")]
    pub docker_socket: String,
    #[serde(default = "default_sandbox_memory_mb")]
    pub memory_mb: u64,
    #[serde(default = "default_sandbox_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub network_enabled: bool,
}

fn default_docker_socket() -> String {
    "/var/run/docker.sock".to_string()
}

fn default_sandbox_memory_mb() -> u64 {
    256
}

fn default_sandbox_timeout() -> u64 {
    30
}

impl Default for SandboxCliConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            docker_socket: default_docker_socket(),
            memory_mb: default_sandbox_memory_mb(),
            timeout_secs: default_sandbox_timeout(),
            network_enabled: false,
        }
    }
}

// ── Secrets Manager Config ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsCliConfig {
    #[serde(default = "default_secrets_provider")]
    pub provider: String,
    #[serde(default)]
    pub secrets_dir: Option<String>,
}

fn default_secrets_provider() -> String {
    "env".to_string()
}

impl Default for SecretsCliConfig {
    fn default() -> Self {
        Self {
            provider: default_secrets_provider(),
            secrets_dir: None,
        }
    }
}

// ── Guardrails Config ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailsCliConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_block_severity")]
    pub block_severity: String,
    #[serde(default = "default_max_input_length")]
    pub max_input_length: usize,
}

fn default_block_severity() -> String {
    "high".to_string()
}

fn default_max_input_length() -> usize {
    100_000
}

impl Default for GuardrailsCliConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            block_severity: default_block_severity(),
            max_input_length: default_max_input_length(),
        }
    }
}

// ── Usage & Cost Tracking Config ────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageCliConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub daily_budget_usd: Option<f64>,
    #[serde(default)]
    pub monthly_budget_usd: Option<f64>,
    #[serde(default = "default_warn_at_percent")]
    pub warn_at_percent: f64,
    #[serde(default)]
    pub model_prices: std::collections::HashMap<String, ModelPriceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPriceConfig {
    #[serde(default)]
    pub input_per_mtok: f64,
    #[serde(default)]
    pub output_per_mtok: f64,
    #[serde(default)]
    pub cache_read_per_mtok: f64,
    #[serde(default)]
    pub cache_write_per_mtok: f64,
}

fn default_warn_at_percent() -> f64 {
    80.0
}

impl Default for UsageCliConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            daily_budget_usd: None,
            monthly_budget_usd: None,
            warn_at_percent: default_warn_at_percent(),
            model_prices: std::collections::HashMap::new(),
        }
    }
}

// ── Notifications Config ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationsConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Channel to send notifications to (e.g., "imessage", "discord", "slack")
    #[serde(default = "default_notify_channel")]
    pub channel: String,
    /// Notify when a background task starts
    #[serde(default = "default_true")]
    pub on_task_start: bool,
    /// Notify when a background task completes
    #[serde(default = "default_true")]
    pub on_task_complete: bool,
    /// Notify when a background task fails
    #[serde(default = "default_true")]
    pub on_task_fail: bool,
    /// Notify when a watcher triggers and the agent takes action
    #[serde(default = "default_true")]
    pub on_watcher_triggered: bool,
    /// Notify when the agent takes an autonomous/proactive action (goal evaluation, etc.)
    #[serde(default = "default_true")]
    pub on_autonomous_action: bool,
    /// Notify on errors (agent failures, channel errors, etc.)
    #[serde(default = "default_true")]
    pub on_error: bool,
    /// Daily digest configuration
    #[serde(default)]
    pub digest: DigestConfig,
    /// Quiet hours — suppress notifications during this window (except errors)
    #[serde(default)]
    pub quiet_hours: Option<QuietHoursConfig>,
}

fn default_notify_channel() -> String {
    "imessage".to_string()
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            channel: default_notify_channel(),
            on_task_start: true,
            on_task_complete: true,
            on_task_fail: true,
            on_watcher_triggered: true,
            on_autonomous_action: true,
            on_error: true,
            digest: DigestConfig::default(),
            quiet_hours: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Cron expression for morning briefing (default: 9am daily)
    #[serde(default = "default_morning_cron")]
    pub morning_cron: String,
    /// Cron expression for end-of-day recap (default: 6pm daily)
    #[serde(default = "default_evening_cron")]
    pub evening_cron: String,
}

fn default_morning_cron() -> String {
    "0 9 * * *".to_string()
}
fn default_evening_cron() -> String {
    "0 18 * * *".to_string()
}

impl Default for DigestConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            morning_cron: default_morning_cron(),
            evening_cron: default_evening_cron(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuietHoursConfig {
    pub start: String,
    pub end: String,
}

/// Mask a secret string for safe display in Debug output / logs.
/// Shows first 3 and last 4 chars for keys longer than 7 chars, otherwise "***".
/// Uses char-boundary-safe slicing to avoid panics on multi-byte UTF-8 (L-1 fix).
fn mask_secret(s: &str) -> String {
    if s.is_empty() {
        return "(empty)".to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > 7 {
        let prefix: String = chars[..3].iter().collect();
        let suffix: String = chars[chars.len() - 4..].iter().collect();
        format!("{}...{}", prefix, suffix)
    } else {
        "***".to_string()
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

        // Enforce config file permissions (Unix only, I-2 fix)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(&path) {
                let mode = metadata.permissions().mode();
                // Refuse to start if group or other can read (mode & 0o077 != 0)
                if mode & 0o077 != 0 {
                    return Err(anyhow::anyhow!(
                        "Config file {:?} has overly permissive permissions ({:o}). \
                         It may contain secrets. Fix with: chmod 600 {:?}",
                        path,
                        mode & 0o777,
                        path
                    ));
                }
            }
        }

        let content = std::fs::read_to_string(&path).with_context(|| {
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
            warn!(
                "API key is hardcoded in config file. For security, use environment variables: api_key = \"${{ANTHROPIC_API_KEY}}\""
            );
        }

        if !config.channels.discord.token.is_empty()
            && !config.channels.discord.token.contains("${")
        {
            warn!(
                "Discord token is hardcoded in config file. For security, use environment variables: token = \"${{DISCORD_TOKEN}}\""
            );
        }

        if !config.channels.slack.bot_token.is_empty()
            && !config.channels.slack.bot_token.contains("${")
        {
            warn!(
                "Slack bot token is hardcoded in config file. For security, use environment variables: bot_token = \"${{SLACK_BOT_TOKEN}}\""
            );
        }

        Ok(config)
    }
}

/// Allowlist of environment variable names that may be expanded in config files.
/// This prevents an attacker who can modify the config from reading arbitrary env vars.
const ALLOWED_ENV_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "GOOGLE_AI_API_KEY",
    "CUSTOM_LLM_API_KEY",
    "TAVILY_API_KEY",
    "DISCORD_BOT_TOKEN",
    "SLACK_BOT_TOKEN",
    "A2A_AUTH_TOKEN",
    "OPENCLAW_A2A_TOKEN",
    "GITHUB_TOKEN",
    "MEEPO_GATEWAY_TOKEN",
    "ELEVENLABS_API_KEY",
    "HOME",
    "USER",
];

fn expand_env_vars(s: &str) -> String {
    let mut result = s.to_string();
    let mut pos = 0;
    while pos < result.len() {
        if let Some(start) = result[pos..].find("${") {
            let abs_start = pos + start;
            if let Some(end) = result[abs_start..].find('}') {
                let var_name = result[abs_start + 2..abs_start + end].to_string();

                // Only expand variables in the allowlist
                let value = if ALLOWED_ENV_VARS.contains(&var_name.as_str()) {
                    std::env::var(&var_name).unwrap_or_default()
                } else {
                    warn!(
                        "Skipping expansion of unrecognized env var '{}' in config (not in allowlist)",
                        var_name
                    );
                    // Leave the ${VAR} unexpanded so it's obvious
                    pos = abs_start + end + 1;
                    continue;
                };

                let value_len = value.len();
                result = format!(
                    "{}{}{}",
                    &result[..abs_start],
                    value,
                    &result[abs_start + end + 1..]
                );
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
