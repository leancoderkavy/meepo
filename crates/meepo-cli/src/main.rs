use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, error};
use tracing_subscriber::EnvFilter;

mod config;
mod template;

use config::MeepoConfig;

#[derive(Parser)]
#[command(name = "meepo")]
#[command(version)]
#[command(about = "Meepo — a local AI agent")]
struct Cli {
    /// Path to config file
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Enable debug logging
    #[arg(short, long, global = true)]
    debug: bool,

    #[command(subcommand)]
    command: Commands,
}

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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging
    let filter = if cli.debug {
        "debug"
    } else {
        "info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(filter))
        .init();

    match cli.command {
        Commands::Init => cmd_init().await,
        Commands::Setup => cmd_setup().await,
        Commands::Config => cmd_config(&cli.config).await,
        Commands::Start => cmd_start(&cli.config).await,
        Commands::Stop => cmd_stop().await,
        Commands::Ask { message } => cmd_ask(&cli.config, &message).await,
        Commands::McpServer => cmd_mcp_server(&cli.config).await,
        Commands::Template { action } => cmd_template(action).await,
    }
}

async fn cmd_init() -> Result<()> {
    let config_dir = config::config_dir();
    tokio::fs::create_dir_all(&config_dir)
        .await
        .with_context(|| format!("Failed to create config dir: {}", config_dir.display()))?;

    let config_path = config_dir.join("config.toml");
    if config_path.exists() {
        warn!("Config already exists at {}", config_path.display());
    } else {
        let default_config = include_str!("../../../config/default.toml");
        tokio::fs::write(&config_path, default_config).await?;
        info!("Created default config at {}", config_path.display());
    }

    // Create workspace directory
    let workspace = config_dir.join("workspace");
    tokio::fs::create_dir_all(&workspace).await?;

    // Copy SOUL.md and MEMORY.md templates if not present
    let soul_path = workspace.join("SOUL.md");
    if !soul_path.exists() {
        tokio::fs::write(&soul_path, include_str!("../../../SOUL.md")).await?;
        info!("Created SOUL.md at {}", soul_path.display());
    }

    let memory_path = workspace.join("MEMORY.md");
    if !memory_path.exists() {
        tokio::fs::write(&memory_path, include_str!("../../../MEMORY.md")).await?;
        info!("Created MEMORY.md at {}", memory_path.display());
    }

    println!("Meepo initialized at {}", config_dir.display());
    println!();
    println!("Next steps:");
    println!("  meepo setup              # recommended — interactive wizard");
    println!("  nano {}  # or configure manually", config_path.display());
    Ok(())
}

async fn cmd_setup() -> Result<()> {
    use std::io::{self, Write, BufRead};

    println!("\n  Meepo Setup\n  ───────────\n");

    // Step 1: Init config
    cmd_init().await?;
    let config_dir = config::config_dir();
    let config_path = config_dir.join("config.toml");

    // Step 2: Anthropic API key
    println!("\n  Anthropic API Key (required)");
    println!("  Get one at: https://console.anthropic.com/settings/keys\n");

    let api_key = if let Ok(existing) = std::env::var("ANTHROPIC_API_KEY") {
        if !existing.is_empty() && existing.starts_with("sk-ant-") {
            println!("  Found ANTHROPIC_API_KEY in environment.");
            existing
        } else {
            prompt_api_key()?
        }
    } else {
        prompt_api_key()?
    };

    // Step 3: Write API key to shell RC
    let shell_rc = detect_shell_rc();
    if let Some(rc_path) = &shell_rc {
        let rc_content = std::fs::read_to_string(rc_path).unwrap_or_default();
        if !rc_content.contains("ANTHROPIC_API_KEY") {
            let mut file = std::fs::OpenOptions::new().append(true).open(rc_path)?;
            writeln!(file, "\nexport ANTHROPIC_API_KEY=\"{}\"", api_key)?;
            println!("  Saved to {}", rc_path.display());
        }
    } else {
        println!("  Could not detect shell (SHELL={:?}).", std::env::var("SHELL").unwrap_or_default());
        println!("  Add this to your shell profile manually:");
        println!("    export ANTHROPIC_API_KEY=\"{}\"", api_key);
    }

    // Step 4: Optional Tavily key
    println!("\n  Tavily API Key (optional — enables web search)");
    println!("  Get one at: https://app.tavily.com/home");
    println!("  Press Enter to skip.\n");

    print!("  API key: ");
    io::stdout().flush()?;
    let mut tavily_key = String::new();
    io::stdin().lock().read_line(&mut tavily_key)?;
    let tavily_key = tavily_key.trim().to_string();

    if !tavily_key.is_empty() {
        if let Some(rc_path) = &shell_rc {
            let rc_content = std::fs::read_to_string(rc_path).unwrap_or_default();
            if !rc_content.contains("TAVILY_API_KEY") {
                let mut file = std::fs::OpenOptions::new().append(true).open(rc_path)?;
                writeln!(file, "export TAVILY_API_KEY=\"{}\"", tavily_key)?;
            }
        }
        println!("  Saved.");
    } else {
        println!("  Skipped — web_search tool won't be available.");
    }

    // Step 5: Verify
    println!("\n  Verifying API connection...");
    let cfg = MeepoConfig::load(&None)?;
    let api = meepo_core::api::ApiClient::new(
        api_key,
        Some(cfg.agent.default_model.clone()),
    );
    let api_ok = match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        api.chat(
            &[meepo_core::api::ApiMessage {
                role: "user".to_string(),
                content: meepo_core::api::MessageContent::Text("Say 'hello' in one word.".to_string()),
            }],
            &[],
            "You are a helpful assistant.",
        ),
    ).await {
        Ok(Ok(response)) => {
            let text: String = response.content.iter()
                .filter_map(|b| if let meepo_core::api::ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                .collect();
            println!("  Response: {}", text.trim());
            println!("  API connection works!\n");
            true
        }
        Ok(Err(e)) => {
            let err_str = e.to_string();
            eprintln!("  API test failed: {}", err_str);
            if err_str.contains("401") || err_str.contains("auth") || err_str.contains("invalid") {
                eprintln!("  Your API key may be incorrect or expired.");
            }
            eprintln!("  Check your key and try again.\n");
            false
        }
        Err(_) => {
            eprintln!("  API test timed out (>15s).");
            eprintln!("  Check your internet connection.\n");
            false
        }
    };

    // Summary
    if api_ok {
        println!("  Setup complete!");
    } else {
        println!("  Setup complete (API verification failed — check your key).");
    }
    println!("  ─────────────");
    println!("  Config:  {}", config_path.display());
    println!("  Soul:    {}", config_dir.join("workspace/SOUL.md").display());
    println!("  Memory:  {}", config_dir.join("workspace/MEMORY.md").display());
    println!();
    println!("  Next steps:");
    println!("    meepo start          # start the daemon");
    println!("    meepo ask \"Hello\"    # one-shot question");
    println!("    nano {}  # enable channels", config_path.display());
    println!();

    Ok(())
}

fn prompt_api_key() -> Result<String> {
    use std::io::{self, Write, BufRead};
    loop {
        print!("  API key (sk-ant-...): ");
        io::stdout().flush()?;
        let mut key = String::new();
        io::stdin().lock().read_line(&mut key)?;
        let key = key.trim().to_string();
        if key.starts_with("sk-ant-") {
            return Ok(key);
        }
        if key.is_empty() {
            anyhow::bail!("API key is required. Get one at https://console.anthropic.com/settings/keys");
        }
        println!("  Key should start with 'sk-ant-'. Try again.");
    }
}

fn detect_shell_rc() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.contains("zsh") {
        Some(home.join(".zshrc"))
    } else if shell.contains("bash") {
        let bashrc = home.join(".bashrc");
        let profile = home.join(".bash_profile");
        if profile.exists() { Some(profile) } else { Some(bashrc) }
    } else {
        None
    }
}

async fn cmd_config(config_path: &Option<PathBuf>) -> Result<()> {
    let cfg = MeepoConfig::load(config_path)?;
    println!("{}", toml::to_string_pretty(&cfg)?);
    Ok(())
}

async fn cmd_start(config_path: &Option<PathBuf>) -> Result<()> {
    let cfg = MeepoConfig::load(config_path)?;
    info!("Starting Meepo daemon...");

    let cancel = CancellationToken::new();

    // Initialize knowledge database and graph
    let db_path = shellexpand(&cfg.knowledge.db_path);
    let tantivy_path = shellexpand(&cfg.knowledge.tantivy_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&tantivy_path)?;

    // Create KnowledgeGraph which includes both DB and Tantivy index
    let knowledge_graph = Arc::new(
        meepo_knowledge::KnowledgeGraph::new(&db_path, &tantivy_path)
            .context("Failed to initialize knowledge graph")?,
    );

    // Use the graph's internal DB to avoid duplicate SQLite connections to the same file
    let db = knowledge_graph.db();
    info!("Knowledge database and Tantivy index initialized");

    // Load SOUL and MEMORY
    let workspace = shellexpand(&cfg.memory.workspace);
    let soul = meepo_knowledge::load_soul(&workspace.join(&cfg.agent.system_prompt_file))
        .unwrap_or_else(|_| "You are Meepo, a helpful AI assistant.".to_string());
    let memory = meepo_knowledge::load_memory(&workspace.join(&cfg.agent.memory_file))
        .unwrap_or_default();
    info!("Loaded SOUL ({} chars) and MEMORY ({} chars)", soul.len(), memory.len());

    // Initialize API client
    let api_key = shellexpand_str(&cfg.providers.anthropic.api_key);
    if api_key.is_empty() || api_key.contains("${") {
        anyhow::bail!(
            "ANTHROPIC_API_KEY is not set.\n\n\
             Fix it with:\n  \
             export ANTHROPIC_API_KEY=\"sk-ant-...\"\n\n\
             Or run the setup wizard:\n  \
             meepo setup\n\n\
             Get a key at: https://console.anthropic.com/settings/keys"
        );
    }
    let base_url = shellexpand_str(&cfg.providers.anthropic.base_url);
    let api = meepo_core::api::ApiClient::new(
        api_key,
        Some(cfg.agent.default_model.clone()),
    ).with_max_tokens(cfg.agent.max_tokens)
     .with_base_url(base_url);
    info!("Anthropic API client initialized (model: {})", cfg.agent.default_model);

    // Initialize Tavily client (optional — web search works only if API key is set)
    let tavily_client = cfg.providers.tavily
        .as_ref()
        .map(|t| shellexpand_str(&t.api_key))
        .filter(|key| !key.is_empty())
        .map(|key| Arc::new(meepo_core::tavily::TavilyClient::new(key)));

    if tavily_client.is_some() {
        info!("Tavily client initialized (web search enabled)");
    } else {
        info!("Tavily API key not set — web search disabled, browse_url uses raw fetch");
    }

    // Initialize watcher command channel (needed for tool registration)
    let (watcher_command_tx, mut watcher_command_rx) = tokio::sync::mpsc::channel::<meepo_core::tools::watchers::WatcherCommand>(100);

    // Initialize background task command channel
    let (bg_task_tx, mut bg_task_rx) = tokio::sync::mpsc::channel::<meepo_core::tools::autonomous::BackgroundTaskCommand>(100);

    // Build tool registry
    let mut registry = meepo_core::tools::ToolRegistry::new();
    // Email, calendar, and UI automation tools require macOS or Windows platform support
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        registry.register(Arc::new(meepo_core::tools::macos::ReadEmailsTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::ReadCalendarTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::SendEmailTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::CreateEventTool::new()));
        registry.register(Arc::new(meepo_core::tools::accessibility::ReadScreenTool::new()));
        registry.register(Arc::new(meepo_core::tools::accessibility::ClickElementTool::new()));
        registry.register(Arc::new(meepo_core::tools::accessibility::TypeTextTool::new()));
    }
    // Clipboard and app launcher are cross-platform (arboard + open crates)
    registry.register(Arc::new(meepo_core::tools::macos::OpenAppTool::new()));
    registry.register(Arc::new(meepo_core::tools::macos::GetClipboardTool::new()));
    // macOS-only tools: Reminders, Notes, Notifications, Screen Capture, Music, Contacts
    #[cfg(target_os = "macos")]
    {
        registry.register(Arc::new(meepo_core::tools::macos::ListRemindersTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::CreateReminderTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::ListNotesTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::CreateNoteTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::SendNotificationTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::ScreenCaptureTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::GetCurrentTrackTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::MusicControlTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::SearchContactsTool::new()));
    }
    // Browser automation tools (macOS: Safari/Chrome via AppleScript)
    #[cfg(target_os = "macos")]
    if cfg.browser.enabled {
        let browser = &cfg.browser.default_browser;
        registry.register(Arc::new(meepo_core::tools::browser::BrowserListTabsTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserOpenTabTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserCloseTabTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserSwitchTabTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserGetPageContentTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserExecuteJsTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserClickElementTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserFillFormTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserNavigateTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserGetUrlTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserScreenshotTool::new(browser)));
        info!("Registered browser tools (browser: {})", browser);
    }
    let code_config = meepo_core::tools::code::CodeToolConfig {
        claude_code_path: shellexpand_str(&cfg.code.claude_code_path),
        gh_path: shellexpand_str(&cfg.code.gh_path),
        default_workspace: shellexpand_str(&cfg.code.default_workspace),
    };
    registry.register(Arc::new(meepo_core::tools::code::WriteCodeTool::new(code_config.clone())));
    registry.register(Arc::new(meepo_core::tools::code::MakePrTool::new(code_config.clone())));
    registry.register(Arc::new(meepo_core::tools::code::ReviewPrTool::new(code_config.clone())));
    registry.register(Arc::new(meepo_core::tools::code::SpawnClaudeCodeTool::new(
        code_config.clone(), db.clone(), bg_task_tx.clone(),
    )));
    registry.register(Arc::new(meepo_core::tools::memory::RememberTool::new(db.clone())));
    registry.register(Arc::new(meepo_core::tools::memory::RecallTool::new(db.clone())));
    // Use KnowledgeGraph for SearchKnowledgeTool to enable Tantivy full-text search
    registry.register(Arc::new(meepo_core::tools::memory::SearchKnowledgeTool::with_graph(knowledge_graph.clone())));
    registry.register(Arc::new(meepo_core::tools::memory::LinkEntitiesTool::new(db.clone())));
    registry.register(Arc::new(meepo_core::tools::system::RunCommandTool));
    registry.register(Arc::new(meepo_core::tools::system::ReadFileTool));
    registry.register(Arc::new(meepo_core::tools::system::WriteFileTool));
    // Filesystem access tools — validate configured directories exist
    for dir in &cfg.filesystem.allowed_directories {
        let expanded = shellexpand(dir);
        if !expanded.exists() {
            warn!("Configured allowed directory does not exist: {} (expanded: {})", dir, expanded.display());
        }
    }
    registry.register(Arc::new(meepo_core::tools::filesystem::ListDirectoryTool::new(
        cfg.filesystem.allowed_directories.clone(),
    )));
    registry.register(Arc::new(meepo_core::tools::filesystem::SearchFilesTool::new(
        cfg.filesystem.allowed_directories.clone(),
    )));
    // BrowseUrlTool with optional Tavily extract
    if let Some(ref tavily) = tavily_client {
        registry.register(Arc::new(meepo_core::tools::system::BrowseUrlTool::with_tavily(tavily.clone())));
    } else {
        registry.register(Arc::new(meepo_core::tools::system::BrowseUrlTool::new()));
    }
    // Register web_search tool if Tavily is available
    if let Some(ref tavily) = tavily_client {
        registry.register(Arc::new(meepo_core::tools::search::WebSearchTool::new(tavily.clone())));
    }
    registry.register(Arc::new(meepo_core::tools::watchers::CreateWatcherTool::new(db.clone(), watcher_command_tx.clone())));
    registry.register(Arc::new(meepo_core::tools::watchers::ListWatchersTool::new(db.clone())));
    registry.register(Arc::new(meepo_core::tools::watchers::CancelWatcherTool::new(db.clone(), watcher_command_tx.clone())));
    // Autonomous agent management tools
    registry.register(Arc::new(meepo_core::tools::autonomous::SpawnBackgroundTaskTool::new(db.clone(), bg_task_tx.clone())));
    registry.register(Arc::new(meepo_core::tools::autonomous::AgentStatusTool::new(db.clone())));
    registry.register(Arc::new(meepo_core::tools::autonomous::StopTaskTool::new(db.clone(), watcher_command_tx.clone(), bg_task_tx.clone())));
    info!("Registered {} tools", registry.len());

    // Initialize progress channel for sub-agent orchestrator
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::channel::<meepo_core::types::OutgoingMessage>(100);

    // Build orchestrator
    let orchestrator_config = meepo_core::orchestrator::OrchestratorConfig {
        max_concurrent_subtasks: cfg.orchestrator.max_concurrent_subtasks,
        max_subtasks_per_request: cfg.orchestrator.max_subtasks_per_request,
        parallel_timeout_secs: cfg.orchestrator.parallel_timeout_secs,
        background_timeout_secs: cfg.orchestrator.background_timeout_secs,
        max_background_groups: cfg.orchestrator.max_background_groups,
    };
    let orchestrator_api = api.clone();
    let orchestrator = Arc::new(meepo_core::orchestrator::TaskOrchestrator::new(
        orchestrator_api,
        progress_tx,
        orchestrator_config,
    ));

    // Register delegate_tasks tool with OnceLock for circular dependency
    let registry_slot = Arc::new(std::sync::OnceLock::new());
    registry.register(Arc::new(
        meepo_core::tools::delegate::DelegateTasksTool::new(
            orchestrator.clone(),
            registry_slot.clone(),
        )
    ));
    info!("Registered delegate_tasks tool (total: {} tools)", registry.len());

    // ── Phase 2: MCP Clients — connect to external MCP servers ──
    for client_cfg in &cfg.mcp.clients {
        let mcp_config = meepo_mcp::McpClientConfig {
            name: client_cfg.name.clone(),
            command: shellexpand_str(&client_cfg.command),
            args: client_cfg.args.iter().map(|a| shellexpand_str(a)).collect(),
            env: client_cfg.env.iter().map(|(k, v)| (k.clone(), shellexpand_str(v))).collect(),
        };

        match meepo_mcp::McpClient::connect(mcp_config).await {
            Ok(client) => {
                match client.discover_tools().await {
                    Ok(tools) => {
                        let count = tools.len();
                        for tool in tools {
                            registry.register(tool);
                        }
                        info!("MCP client '{}': registered {} tools", client_cfg.name, count);
                    }
                    Err(e) => warn!("MCP client '{}': failed to discover tools: {}", client_cfg.name, e),
                }
            }
            Err(e) => warn!("MCP client '{}': failed to connect: {}", client_cfg.name, e),
        }
    }

    // ── Phase 3: A2A — register delegate_to_agent tool ──────────
    if cfg.a2a.enabled {
        let peers: Vec<meepo_a2a::PeerAgentConfig> = cfg.a2a.agents.iter().map(|a| {
            meepo_a2a::PeerAgentConfig {
                name: a.name.clone(),
                url: shellexpand_str(&a.url),
                token: if a.token.is_empty() { None } else { Some(shellexpand_str(&a.token)) },
            }
        }).collect();

        registry.register(Arc::new(meepo_a2a::DelegateToAgentTool::new(peers)));
        info!("A2A: registered delegate_to_agent tool ({} peer agents)", cfg.a2a.agents.len());
    }

    // ── Phase 4: Skills — load SKILL.md files as tools ──────────
    if cfg.skills.enabled {
        let skills_dir = shellexpand(&cfg.skills.dir);
        match meepo_core::skills::load_skills(&skills_dir) {
            Ok(skill_tools) => {
                let count = skill_tools.len();
                for tool in skill_tools {
                    registry.register(tool);
                }
                info!("Skills: loaded {} tools from {}", count, skills_dir.display());
            }
            Err(e) => warn!("Skills: failed to load from {}: {}", skills_dir.display(), e),
        }
    }

    info!("Total tools registered: {}", registry.len());

    // Initialize agent
    let registry = Arc::new(registry);
    assert!(registry_slot.set(registry.clone()).is_ok(), "registry slot already set");

    let agent = Arc::new(meepo_core::agent::Agent::new(
        api,
        registry.clone(),
        soul,
        memory,
        db.clone(),
    ));

    // Initialize watcher scheduler
    let (watcher_event_tx, mut watcher_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let watcher_runner = Arc::new(tokio::sync::Mutex::new(
        meepo_scheduler::runner::WatcherRunner::new(watcher_event_tx),
    ));

    // Initialize scheduler database (kept alive for runtime persistence)
    let sched_db = Arc::new(std::sync::Mutex::new(rusqlite::Connection::open(&db_path)?));
    {
        let conn = sched_db.lock().unwrap();
        meepo_scheduler::persistence::init_watcher_tables(&conn)?;
        let watchers = meepo_scheduler::persistence::get_active_watchers(&conn)?;
        drop(conn); // release lock before async runner.lock()
        let runner = watcher_runner.lock().await;
        for w in watchers {
            if let Err(e) = runner.start_watcher(w.clone()).await {
                warn!("Failed to start watcher {}: {}", w.id, e);
            }
        }
    }
    info!("Watcher scheduler initialized");

    // Initialize message bus
    let mut bus = meepo_channels::bus::MessageBus::new(256);

    // Register Discord channel if enabled
    if cfg.channels.discord.enabled {
        let discord = meepo_channels::discord::DiscordChannel::new(
            shellexpand_str(&cfg.channels.discord.token),
            cfg.channels.discord.allowed_users.clone(),
        );
        bus.register(Box::new(discord));
        info!("Discord channel registered");
    }

    // Register iMessage channel if enabled (macOS only)
    #[cfg(target_os = "macos")]
    if cfg.channels.imessage.enabled {
        let imessage = meepo_channels::imessage::IMessageChannel::new(
            std::time::Duration::from_secs(cfg.channels.imessage.poll_interval_secs),
            cfg.channels.imessage.allowed_contacts.clone(),
            None,
        );
        bus.register(Box::new(imessage));
        info!("iMessage channel registered");
    }
    #[cfg(not(target_os = "macos"))]
    if cfg.channels.imessage.enabled {
        warn!("iMessage channel is only available on macOS — ignoring");
    }

    // Register Slack channel if enabled
    if cfg.channels.slack.enabled {
        let slack = meepo_channels::slack::SlackChannel::new(
            shellexpand_str(&cfg.channels.slack.bot_token),
            std::time::Duration::from_secs(cfg.channels.slack.poll_interval_secs),
        );
        bus.register(Box::new(slack));
        info!("Slack channel registered");
    }

    // Register Email channel if enabled (macOS only — uses Mail.app)
    #[cfg(target_os = "macos")]
    if cfg.channels.email.enabled {
        let email = meepo_channels::email::EmailChannel::new(
            std::time::Duration::from_secs(cfg.channels.email.poll_interval_secs),
            cfg.channels.email.subject_prefix.clone(),
        );
        bus.register(Box::new(email));
        info!("Email channel registered");
    }
    #[cfg(not(target_os = "macos"))]
    if cfg.channels.email.enabled {
        warn!("Email channel (Mail.app) is only available on macOS — use the read_emails/send_email tools for Outlook on Windows");
    }

    // Start all channels
    bus.start_all().await?;
    info!("All message channels started");

    println!("Meepo is running. Press Ctrl+C to stop.");

    // Split bus into receiver + sender for concurrent use
    let (mut incoming_rx, bus_sender) = bus.split();
    let bus_sender = Arc::new(bus_sender);

    // ── Autonomous Loop ─────────────────────────────────────────
    let bus_sender_for_progress = bus_sender.clone();

    let (loop_msg_tx, loop_msg_rx) = tokio::sync::mpsc::channel::<meepo_core::types::IncomingMessage>(256);
    let (loop_resp_tx, mut loop_resp_rx) = tokio::sync::mpsc::channel::<meepo_core::types::OutgoingMessage>(256);
    let wake = meepo_core::autonomy::AutonomousLoop::create_wake_handle();

    // Forward incoming bus messages to the autonomous loop
    let wake_clone = wake.clone();
    let cancel_clone = cancel.clone();
    let bus_to_loop = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel_clone.cancelled() => break,
                msg = incoming_rx.recv() => {
                    match msg {
                        Some(incoming) => {
                            info!("Message from {} via {}: {}",
                                incoming.sender,
                                incoming.channel,
                                &incoming.content[..incoming.content.len().min(100)]);
                            if loop_msg_tx.send(incoming).await.is_err() {
                                break;
                            }
                            wake_clone.notify_one();
                        }
                        None => break,
                    }
                }
            }
        }
    });

    // Forward watcher events to the autonomous loop
    let (loop_watcher_tx, loop_watcher_rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel_clone2 = cancel.clone();
    let wake_clone2 = wake.clone();
    let watcher_to_loop = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel_clone2.cancelled() => break,
                event = watcher_event_rx.recv() => {
                    match event {
                        Some(ev) => {
                            info!("Watcher event: {} from {}", ev.kind, ev.watcher_id);
                            let _ = loop_watcher_tx.send(ev);
                            wake_clone2.notify_one();
                        }
                        None => break,
                    }
                }
            }
        }
    });

    // Clone bus_sender for background task handler before it moves into resp_to_bus
    let bus_sender_for_bg = bus_sender.clone();

    // Forward loop responses to the bus sender
    let cancel_clone3 = cancel.clone();
    let resp_to_bus = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel_clone3.cancelled() => break,
                resp = loop_resp_rx.recv() => {
                    match resp {
                        Some(msg) => {
                            let channel = msg.channel.clone();
                            if let Err(e) = bus_sender.send(msg).await {
                                // Internal channel has no handler — this is expected
                                if channel != meepo_core::types::ChannelType::Internal {
                                    error!("Failed to route response to {}: {}", channel, e);
                                }
                            }
                        }
                        None => break,
                    }
                }
            }
        }
    });

    // Handle watcher commands (independent of the loop)
    let cancel_clone4 = cancel.clone();
    let watcher_runner_clone = watcher_runner.clone();
    let watcher_cmd_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel_clone4.cancelled() => break,
                cmd = watcher_command_rx.recv() => {
                    if let Some(command) = cmd {
                        let runner = watcher_runner_clone.clone();
                        let sched_db = sched_db.clone();
                        tokio::spawn(async move {
                            use meepo_core::tools::watchers::WatcherCommand;
                            match command {
                                WatcherCommand::Create { id, kind, config, action, reply_channel } => {
                                    // Map the tool's kind string to WatcherKind's serde tag variant name
                                    let type_tag = match kind.as_str() {
                                        "email" => "EmailWatch",
                                        "calendar" => "CalendarWatch",
                                        "github" => "GitHubWatch",
                                        "file" => "FileWatch",
                                        "message" => "MessageWatch",
                                        "scheduled" | "time" => "Scheduled",
                                        "oneshot" => "OneShot",
                                        other => {
                                            error!("Unknown watcher kind: {}", other);
                                            return;
                                        }
                                    };
                                    // Inject the "type" tag into config for serde deserialization
                                    let config_with_type = match config {
                                        serde_json::Value::Object(mut map) => {
                                            map.insert("type".to_string(), serde_json::Value::String(type_tag.to_string()));
                                            serde_json::Value::Object(map)
                                        }
                                        _ => {
                                            error!("Watcher config is not a JSON object");
                                            return;
                                        }
                                    };
                                    let watcher_kind: meepo_scheduler::watcher::WatcherKind = match serde_json::from_value(config_with_type) {
                                        Ok(k) => k,
                                        Err(e) => {
                                            error!("Failed to deserialize watcher kind: {}", e);
                                            return;
                                        }
                                    };
                                    let watcher = meepo_scheduler::watcher::Watcher {
                                        id,
                                        kind: watcher_kind,
                                        action,
                                        reply_channel,
                                        active: true,
                                        created_at: chrono::Utc::now(),
                                    };
                                    if let Ok(conn) = sched_db.lock() {
                                        if let Err(e) = meepo_scheduler::persistence::save_watcher(&conn, &watcher) {
                                            error!("Failed to persist watcher {}: {}", watcher.id, e);
                                        }
                                    }
                                    if let Err(e) = runner.lock().await.start_watcher(watcher).await {
                                        error!("Failed to start watcher: {}", e);
                                    }
                                }
                                WatcherCommand::Cancel { id } => {
                                    if let Ok(conn) = sched_db.lock() {
                                        if let Err(e) = meepo_scheduler::persistence::deactivate_watcher(&conn, &id) {
                                            error!("Failed to deactivate watcher {} in scheduler DB: {}", id, e);
                                        }
                                    }
                                    if let Err(e) = runner.lock().await.stop_watcher(&id).await {
                                        error!("Failed to stop watcher {}: {}", id, e);
                                    }
                                }
                                WatcherCommand::List => {}
                            }
                        });
                    }
                }
            }
        }
    });

    // Handle background task commands
    let cancel_clone_bg = cancel.clone();
    let agent_bg = agent.clone();
    let db_bg = db.clone();
    let bus_sender_bg = bus_sender_for_bg;
    let code_config_bg = meepo_core::tools::code::CodeToolConfig {
        claude_code_path: shellexpand_str(&cfg.code.claude_code_path),
        gh_path: shellexpand_str(&cfg.code.gh_path),
        default_workspace: shellexpand_str(&cfg.code.default_workspace),
    };
    let bg_task_handler = tokio::spawn(async move {
        // Track cancellation tokens for background tasks
        let task_cancels = Arc::new(tokio::sync::Mutex::new(
            std::collections::HashMap::<String, tokio_util::sync::CancellationToken>::new()
        ));

        loop {
            tokio::select! {
                _ = cancel_clone_bg.cancelled() => break,
                cmd = bg_task_rx.recv() => {
                    match cmd {
                        Some(meepo_core::tools::autonomous::BackgroundTaskCommand::Spawn { id, description, reply_channel }) => {
                            info!("Spawning background task [{}]: {}", id, description);
                            let task_cancel = tokio_util::sync::CancellationToken::new();
                            task_cancels.lock().await.insert(id.clone(), task_cancel.clone());

                            let agent = agent_bg.clone();
                            let db = db_bg.clone();
                            let bus = bus_sender_bg.clone();
                            let task_cancels = task_cancels.clone();
                            let id_clone = id.clone();
                            let reply_channel_clone = reply_channel.clone();

                            tokio::spawn(async move {
                                // Update status to running
                                if let Err(e) = db.update_background_task(&id_clone, "running", None).await {
                                    error!("Failed to update task {} to running: {}", id_clone, e);
                                }

                                // Run the task as a message through the agent
                                let msg = meepo_core::types::IncomingMessage {
                                    id: id_clone.clone(),
                                    sender: "background_task".to_string(),
                                    content: description.clone(),
                                    channel: meepo_core::types::ChannelType::from_string(&reply_channel_clone),
                                    timestamp: chrono::Utc::now(),
                                };

                                let result = tokio::select! {
                                    _ = task_cancel.cancelled() => {
                                        Err(anyhow::anyhow!("Task cancelled"))
                                    }
                                    result = agent.handle_message(msg) => result
                                };

                                match result {
                                    Ok(response) => {
                                        if let Err(e) = db.update_background_task(&id_clone, "completed", Some(&response.content)).await {
                                            error!("Failed to update task {} to completed: {}", id_clone, e);
                                        }
                                        // Notify user on reply_channel
                                        let notify_msg = meepo_core::types::OutgoingMessage {
                                            content: format!("Background task [{}] completed:\n{}", id_clone, response.content),
                                            channel: meepo_core::types::ChannelType::from_string(&reply_channel_clone),
                                            reply_to: None,
                                            kind: meepo_core::types::MessageKind::Response,
                                        };
                                        let _ = bus.send(notify_msg).await;
                                    }
                                    Err(e) => {
                                        let err_msg = e.to_string();
                                        let status = if err_msg.contains("cancelled") { "cancelled" } else { "failed" };
                                        if let Err(e) = db.update_background_task(&id_clone, status, Some(&err_msg)).await {
                                            error!("Failed to update task {} to {}: {}", id_clone, status, e);
                                        }
                                        if status == "failed" {
                                            let notify_msg = meepo_core::types::OutgoingMessage {
                                                content: format!("Background task [{}] failed: {}", id_clone, err_msg),
                                                channel: meepo_core::types::ChannelType::from_string(&reply_channel_clone),
                                                reply_to: None,
                                                kind: meepo_core::types::MessageKind::Response,
                                            };
                                            let _ = bus.send(notify_msg).await;
                                        }
                                    }
                                }

                                // Clean up cancellation token
                                task_cancels.lock().await.remove(&id_clone);
                            });
                        }
                        Some(meepo_core::tools::autonomous::BackgroundTaskCommand::SpawnClaudeCode { id, task, workspace, reply_channel }) => {
                            info!("Spawning Claude Code agent [{}] in {}", id, workspace);
                            let task_cancel = tokio_util::sync::CancellationToken::new();
                            task_cancels.lock().await.insert(id.clone(), task_cancel.clone());

                            let db = db_bg.clone();
                            let bus = bus_sender_bg.clone();
                            let task_cancels = task_cancels.clone();
                            let claude_path = code_config_bg.claude_code_path.clone();

                            tokio::spawn(async move {
                                // Update status to running
                                if let Err(e) = db.update_background_task(&id, "running", None).await {
                                    error!("Failed to update task {} to running: {}", id, e);
                                }

                                // Spawn Claude Code CLI as a child process
                                let mut child = match tokio::process::Command::new(&claude_path)
                                    .arg("--print")
                                    .arg("--dangerously-skip-permissions")
                                    .arg(&task)
                                    .current_dir(&workspace)
                                    .stdout(std::process::Stdio::piped())
                                    .stderr(std::process::Stdio::piped())
                                    .spawn()
                                {
                                    Ok(child) => child,
                                    Err(e) => {
                                        let err_msg = format!("Failed to spawn Claude Code CLI: {}", e);
                                        error!("{}", err_msg);
                                        let _ = db.update_background_task(&id, "failed", Some(&err_msg)).await;
                                        let notify = meepo_core::types::OutgoingMessage {
                                            content: format!("Claude Code task [{}] failed: {}", id, err_msg),
                                            channel: meepo_core::types::ChannelType::from_string(&reply_channel),
                                            reply_to: None,
                                            kind: meepo_core::types::MessageKind::Response,
                                        };
                                        let _ = bus.send(notify).await;
                                        task_cancels.lock().await.remove(&id);
                                        return;
                                    }
                                };

                                // Wait for child with cancellation support
                                let result = tokio::select! {
                                    _ = task_cancel.cancelled() => {
                                        let _ = child.kill().await;
                                        Err(anyhow::anyhow!("Task cancelled"))
                                    }
                                    status = child.wait() => {
                                        match status {
                                            Ok(exit) if exit.success() => {
                                                // Read stdout after process exits
                                                let mut stdout_buf = Vec::new();
                                                if let Some(mut stdout) = child.stdout.take() {
                                                    use tokio::io::AsyncReadExt;
                                                    let _ = stdout.read_to_end(&mut stdout_buf).await;
                                                }
                                                let stdout = String::from_utf8_lossy(&stdout_buf);
                                                // Truncate to 10K chars for DB storage
                                                let result = if stdout.len() > 10_000 {
                                                    format!("{}...\n[truncated, {} total chars]", &stdout[..10_000], stdout.len())
                                                } else {
                                                    stdout.to_string()
                                                };
                                                Ok(result)
                                            }
                                            Ok(exit) => {
                                                let mut stderr_buf = Vec::new();
                                                if let Some(mut stderr) = child.stderr.take() {
                                                    use tokio::io::AsyncReadExt;
                                                    let _ = stderr.read_to_end(&mut stderr_buf).await;
                                                }
                                                let stderr = String::from_utf8_lossy(&stderr_buf);
                                                Err(anyhow::anyhow!("Claude Code exited with {}: {}", exit, stderr))
                                            }
                                            Err(e) => Err(anyhow::anyhow!("Failed to wait for Claude Code: {}", e)),
                                        }
                                    }
                                };

                                match result {
                                    Ok(output) => {
                                        if let Err(e) = db.update_background_task(&id, "completed", Some(&output)).await {
                                            error!("Failed to update task {} to completed: {}", id, e);
                                        }
                                        let notify = meepo_core::types::OutgoingMessage {
                                            content: format!("Claude Code task [{}] completed:\n{}", id, output),
                                            channel: meepo_core::types::ChannelType::from_string(&reply_channel),
                                            reply_to: None,
                                            kind: meepo_core::types::MessageKind::Response,
                                        };
                                        let _ = bus.send(notify).await;
                                    }
                                    Err(e) => {
                                        let err_msg = e.to_string();
                                        let status = if err_msg.contains("cancelled") { "cancelled" } else { "failed" };
                                        if let Err(e) = db.update_background_task(&id, status, Some(&err_msg)).await {
                                            error!("Failed to update task {} to {}: {}", id, status, e);
                                        }
                                        if status == "failed" {
                                            let notify = meepo_core::types::OutgoingMessage {
                                                content: format!("Claude Code task [{}] failed: {}", id, err_msg),
                                                channel: meepo_core::types::ChannelType::from_string(&reply_channel),
                                                reply_to: None,
                                                kind: meepo_core::types::MessageKind::Response,
                                            };
                                            let _ = bus.send(notify).await;
                                        }
                                    }
                                }

                                // Clean up cancellation token
                                task_cancels.lock().await.remove(&id);
                            });
                        }
                        Some(meepo_core::tools::autonomous::BackgroundTaskCommand::Cancel { id }) => {
                            info!("Cancelling background task [{}]", id);
                            if let Some(token) = task_cancels.lock().await.get(&id) {
                                token.cancel();
                            }
                        }
                        None => break,
                    }
                }
            }
        }
    });

    // Handle sub-agent progress
    let cancel_clone5 = cancel.clone();
    let progress_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel_clone5.cancelled() => break,
                progress = progress_rx.recv() => {
                    if let Some(msg) = progress {
                        info!("Sub-agent progress for {}: {}", msg.channel, &msg.content[..msg.content.len().min(100)]);
                        let _ = bus_sender_for_progress.send(msg).await;
                    }
                }
            }
        }
    });

    // Start the autonomous loop
    let autonomy_config = meepo_core::autonomy::AutonomyConfig {
        enabled: cfg.autonomy.enabled,
        tick_interval_secs: cfg.autonomy.tick_interval_secs,
        max_goals: cfg.autonomy.max_goals,
        send_acknowledgments: cfg.autonomy.send_acknowledgments,
    };

    let auto_loop = meepo_core::autonomy::AutonomousLoop::new(
        agent.clone(),
        db.clone(),
        autonomy_config,
        loop_msg_rx,
        loop_watcher_rx,
        loop_resp_tx,
        wake,
    );

    let cancel_clone6 = cancel.clone();
    let loop_task = tokio::spawn(async move {
        auto_loop.run(cancel_clone6).await;
    });

    // ── Phase 3: A2A Server ─────────────────────────────────────
    if cfg.a2a.enabled {
        let a2a_card = meepo_a2a::AgentCard {
            name: "meepo".to_string(),
            description: "Personal AI agent with macOS integration, code tools, and web search".to_string(),
            url: format!("http://localhost:{}", cfg.a2a.port),
            capabilities: vec![
                "file_operations".to_string(),
                "web_research".to_string(),
                "email".to_string(),
                "calendar".to_string(),
                "code_review".to_string(),
            ],
            authentication: meepo_a2a::AuthConfig {
                schemes: if cfg.a2a.auth_token.is_empty() { vec![] } else { vec!["bearer".to_string()] },
            },
        };

        let auth_token = {
            let t = shellexpand_str(&cfg.a2a.auth_token);
            if t.is_empty() { None } else { Some(t) }
        };

        let a2a_server = Arc::new(meepo_a2a::A2aServer::new(
            agent.clone(),
            registry.clone(),
            a2a_card,
            auth_token,
            cfg.a2a.allowed_tools.clone(),
        ));

        let a2a_port = cfg.a2a.port;
        tokio::spawn(async move {
            if let Err(e) = a2a_server.serve(a2a_port).await {
                error!("A2A server error: {}", e);
            }
        });
        info!("A2A server started on port {}", cfg.a2a.port);
    }

    // Wait for shutdown signal
    signal::ctrl_c().await?;
    info!("Received Ctrl+C, shutting down...");
    cancel.cancel();

    // Wait for all tasks
    let _ = tokio::join!(loop_task, bus_to_loop, watcher_to_loop, resp_to_bus, watcher_cmd_task, progress_task, bg_task_handler);

    // Stop all watchers
    watcher_runner.lock().await.stop_all().await;

    println!("Meepo stopped.");
    Ok(())
}

async fn cmd_stop() -> Result<()> {
    #[cfg(target_os = "macos")]
    let output = tokio::process::Command::new("pkill")
        .args(["-f", "meepo start"])
        .output()
        .await?;

    #[cfg(target_os = "windows")]
    let output = tokio::process::Command::new("taskkill")
        .args(["/IM", "meepo.exe", "/F"])
        .output()
        .await?;

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let output = tokio::process::Command::new("pkill")
        .args(["-f", "meepo start"])
        .output()
        .await?;

    if output.status.success() {
        println!("Meepo daemon stopped.");
    } else {
        println!("No running Meepo daemon found.");
    }
    Ok(())
}

async fn cmd_ask(config_path: &Option<PathBuf>, message: &str) -> Result<()> {
    let cfg = MeepoConfig::load(config_path)?;

    let api_key = shellexpand_str(&cfg.providers.anthropic.api_key);
    if api_key.is_empty() || api_key.contains("${") {
        anyhow::bail!(
            "ANTHROPIC_API_KEY is not set.\n\n\
             Fix it with:\n  \
             export ANTHROPIC_API_KEY=\"sk-ant-...\"\n\n\
             Or run the setup wizard:\n  \
             meepo setup\n\n\
             Get a key at: https://console.anthropic.com/settings/keys"
        );
    }
    let base_url = shellexpand_str(&cfg.providers.anthropic.base_url);
    let api = meepo_core::api::ApiClient::new(
        api_key,
        Some(cfg.agent.default_model.clone()),
    ).with_max_tokens(cfg.agent.max_tokens)
     .with_base_url(base_url);

    // Load context
    let workspace = shellexpand(&cfg.memory.workspace);
    let soul = meepo_knowledge::load_soul(&workspace.join(&cfg.agent.system_prompt_file))
        .unwrap_or_else(|_| "You are Meepo, a helpful AI assistant.".to_string());
    let memory = meepo_knowledge::load_memory(&workspace.join(&cfg.agent.memory_file))
        .unwrap_or_default();

    let system = format!("{}\n\n## Current Memory\n{}", soul, memory);

    let response = api
        .chat(
            &[meepo_core::api::ApiMessage {
                role: "user".to_string(),
                content: meepo_core::api::MessageContent::Text(message.to_string()),
            }],
            &[],
            &system,
        )
        .await?;

    for block in &response.content {
        if let meepo_core::api::ContentBlock::Text { text } = block {
            println!("{}", text);
        }
    }

    Ok(())
}

async fn cmd_mcp_server(config_path: &Option<PathBuf>) -> Result<()> {
    let cfg = MeepoConfig::load(config_path)?;

    // Build tool registry (same tools as cmd_start, minus channels/bus/orchestrator)
    let db_path = shellexpand(&cfg.knowledge.db_path);
    let tantivy_path = shellexpand(&cfg.knowledge.tantivy_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&tantivy_path)?;

    let knowledge_graph = Arc::new(
        meepo_knowledge::KnowledgeGraph::new(&db_path, &tantivy_path)
            .context("Failed to initialize knowledge graph")?,
    );
    let db = knowledge_graph.db();

    // Tavily client (optional)
    let tavily_client = cfg.providers.tavily
        .as_ref()
        .map(|t| shellexpand_str(&t.api_key))
        .filter(|key| !key.is_empty())
        .map(|key| Arc::new(meepo_core::tavily::TavilyClient::new(key)));

    // Watcher command channel (needed for tool registration even in MCP mode)
    let (watcher_command_tx, _watcher_command_rx) = tokio::sync::mpsc::channel::<meepo_core::tools::watchers::WatcherCommand>(100);

    let mut registry = meepo_core::tools::ToolRegistry::new();

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        registry.register(Arc::new(meepo_core::tools::macos::ReadEmailsTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::ReadCalendarTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::SendEmailTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::CreateEventTool::new()));
        registry.register(Arc::new(meepo_core::tools::accessibility::ReadScreenTool::new()));
        registry.register(Arc::new(meepo_core::tools::accessibility::ClickElementTool::new()));
        registry.register(Arc::new(meepo_core::tools::accessibility::TypeTextTool::new()));
    }
    registry.register(Arc::new(meepo_core::tools::macos::OpenAppTool::new()));
    registry.register(Arc::new(meepo_core::tools::macos::GetClipboardTool::new()));
    #[cfg(target_os = "macos")]
    {
        registry.register(Arc::new(meepo_core::tools::macos::ListRemindersTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::CreateReminderTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::ListNotesTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::CreateNoteTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::SendNotificationTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::ScreenCaptureTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::GetCurrentTrackTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::MusicControlTool::new()));
        registry.register(Arc::new(meepo_core::tools::macos::SearchContactsTool::new()));
    }
    // Browser automation tools for ask command
    #[cfg(target_os = "macos")]
    if cfg.browser.enabled {
        let browser = &cfg.browser.default_browser;
        registry.register(Arc::new(meepo_core::tools::browser::BrowserListTabsTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserOpenTabTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserCloseTabTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserSwitchTabTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserGetPageContentTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserExecuteJsTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserClickElementTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserFillFormTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserNavigateTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserGetUrlTool::new(browser)));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserScreenshotTool::new(browser)));
    }
    let code_config = meepo_core::tools::code::CodeToolConfig {
        claude_code_path: shellexpand_str(&cfg.code.claude_code_path),
        gh_path: shellexpand_str(&cfg.code.gh_path),
        default_workspace: shellexpand_str(&cfg.code.default_workspace),
    };
    registry.register(Arc::new(meepo_core::tools::code::WriteCodeTool::new(code_config.clone())));
    registry.register(Arc::new(meepo_core::tools::code::MakePrTool::new(code_config.clone())));
    registry.register(Arc::new(meepo_core::tools::code::ReviewPrTool::new(code_config)));
    registry.register(Arc::new(meepo_core::tools::memory::RememberTool::new(db.clone())));
    registry.register(Arc::new(meepo_core::tools::memory::RecallTool::new(db.clone())));
    registry.register(Arc::new(meepo_core::tools::memory::SearchKnowledgeTool::with_graph(knowledge_graph.clone())));
    registry.register(Arc::new(meepo_core::tools::memory::LinkEntitiesTool::new(db.clone())));
    registry.register(Arc::new(meepo_core::tools::system::RunCommandTool));
    registry.register(Arc::new(meepo_core::tools::system::ReadFileTool));
    registry.register(Arc::new(meepo_core::tools::system::WriteFileTool));
    registry.register(Arc::new(meepo_core::tools::filesystem::ListDirectoryTool::new(
        cfg.filesystem.allowed_directories.clone(),
    )));
    registry.register(Arc::new(meepo_core::tools::filesystem::SearchFilesTool::new(
        cfg.filesystem.allowed_directories.clone(),
    )));
    if let Some(ref tavily) = tavily_client {
        registry.register(Arc::new(meepo_core::tools::system::BrowseUrlTool::with_tavily(tavily.clone())));
        registry.register(Arc::new(meepo_core::tools::search::WebSearchTool::new(tavily.clone())));
    } else {
        registry.register(Arc::new(meepo_core::tools::system::BrowseUrlTool::new()));
    }
    registry.register(Arc::new(meepo_core::tools::watchers::CreateWatcherTool::new(db.clone(), watcher_command_tx.clone())));
    registry.register(Arc::new(meepo_core::tools::watchers::ListWatchersTool::new(db.clone())));
    registry.register(Arc::new(meepo_core::tools::watchers::CancelWatcherTool::new(db.clone(), watcher_command_tx.clone())));
    // Autonomous tools — agent_status works in MCP mode, spawn/stop won't have handlers
    registry.register(Arc::new(meepo_core::tools::autonomous::AgentStatusTool::new(db.clone())));

    // Load skills if enabled
    if cfg.skills.enabled {
        let skills_dir = shellexpand(&cfg.skills.dir);
        if let Ok(skill_tools) = meepo_core::skills::load_skills(&skills_dir) {
            for tool in skill_tools {
                registry.register(tool);
            }
        }
    }

    let registry = Arc::new(registry);
    info!("MCP server: {} tools available", registry.len());

    // Create MCP adapter and server
    let adapter = meepo_mcp::McpToolAdapter::new(registry);
    let server = meepo_mcp::McpServer::new(adapter);

    // Serve over STDIO
    server.serve_stdio().await
}

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
                    println!("  Note: knowledge.db not found — goals will be injected on first `meepo start`");
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

            // Create template.toml scaffold
            let template_toml = format!(
                r#"[template]
name = "{name}"
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
"#
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
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
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

// Utility: expand ~ and env vars in paths
fn shellexpand(s: &str) -> PathBuf {
    let expanded = shellexpand_str(s);
    PathBuf::from(expanded)
}

fn shellexpand_str(s: &str) -> String {
    let mut result = s.to_string();
    if result.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            result = format!("{}{}", home.display(), &result[1..]);
        }
    }
    // Expand ${VAR} patterns with position tracking to avoid infinite loops
    let mut pos = 0;
    while pos < result.len() {
        if let Some(start) = result[pos..].find("${") {
            let abs_start = pos + start;
            if let Some(end) = result[abs_start..].find('}') {
                let var_name = &result[abs_start + 2..abs_start + end];
                let value = std::env::var(var_name).unwrap_or_default();
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
