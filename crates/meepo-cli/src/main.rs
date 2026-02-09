use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, error};
use tracing_subscriber::EnvFilter;

mod config;

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
    println!("Edit {} to configure your API keys and channels.", config_path.display());
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
    let api_ok = match api.chat(
        &[meepo_core::api::ApiMessage {
            role: "user".to_string(),
            content: meepo_core::api::MessageContent::Text("Say 'hello' in one word.".to_string()),
        }],
        &[],
        "You are a helpful assistant.",
    ).await {
        Ok(response) => {
            let text: String = response.content.iter()
                .filter_map(|b| if let meepo_core::api::ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                .collect();
            println!("  Response: {}", text.trim());
            println!("  API connection works!\n");
            true
        }
        Err(e) => {
            eprintln!("  API test failed: {}", e);
            eprintln!("  Check your API key and try again.\n");
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
    registry.register(Arc::new(meepo_core::tools::code::WriteCodeTool));
    registry.register(Arc::new(meepo_core::tools::code::MakePrTool));
    registry.register(Arc::new(meepo_core::tools::code::ReviewPrTool));
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

    // Initialize agent
    let registry = Arc::new(registry);
    assert!(registry_slot.set(registry.clone()).is_ok(), "registry slot already set");

    let agent = Arc::new(meepo_core::agent::Agent::new(
        api,
        registry,
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
            cfg.channels.imessage.trigger_prefix.clone(),
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

    // Semaphore to limit concurrent message processing
    let semaphore = Arc::new(Semaphore::new(10));

    // Main event loop
    let agent_clone = agent.clone();
    let cancel_clone = cancel.clone();
    let watcher_runner_clone = watcher_runner.clone();
    let main_loop = tokio::spawn(async move {
        let mut join_set = JoinSet::new();

        loop {
            tokio::select! {
                _ = cancel_clone.cancelled() => {
                    info!("Agent loop shutting down");
                    break;
                }
                msg = incoming_rx.recv() => {
                    match msg {
                        Some(incoming) => {
                            info!("Message from {} via {}: {}",
                                incoming.sender,
                                incoming.channel,
                                &incoming.content[..incoming.content.len().min(100)]);
                            let agent = agent_clone.clone();
                            let sender = bus_sender.clone();
                            let permit = semaphore.clone().acquire_owned().await.expect("semaphore closed");
                            join_set.spawn(async move {
                                let _permit = permit;
                                match agent.handle_message(incoming).await {
                                    Ok(response) => {
                                        info!("Response generated ({} chars), routing to {}", response.content.len(), response.channel);
                                        if let Err(e) = sender.send(response).await {
                                            error!("Failed to route response: {}", e);
                                        }
                                    }
                                    Err(e) => error!("Agent error: {}", e),
                                }
                            });
                        }
                        None => {
                            info!("Message bus closed");
                            break;
                        }
                    }
                }
                cmd = watcher_command_rx.recv() => {
                    if let Some(command) = cmd {
                        let runner = watcher_runner_clone.clone();
                        let sched_db = sched_db.clone();
                        tokio::spawn(async move {
                            use meepo_core::tools::watchers::WatcherCommand;
                            match command {
                                WatcherCommand::Create { id, kind: _, config, action, reply_channel } => {
                                    let watcher_kind = match serde_json::from_value(config) {
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

                                    // Persist to scheduler DB for restart recovery
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
                                    // Deactivate in scheduler DB
                                    if let Ok(conn) = sched_db.lock() {
                                        if let Err(e) = meepo_scheduler::persistence::deactivate_watcher(&conn, &id) {
                                            error!("Failed to deactivate watcher {} in scheduler DB: {}", id, e);
                                        }
                                    }

                                    if let Err(e) = runner.lock().await.stop_watcher(&id).await {
                                        error!("Failed to stop watcher {}: {}", id, e);
                                    }
                                }
                                WatcherCommand::List => {
                                    // List command is handled synchronously by the tool via DB query
                                }
                            }
                        });
                    }
                }
                progress = progress_rx.recv() => {
                    if let Some(msg) = progress {
                        info!("Sub-agent progress for {}: {}",
                            msg.channel,
                            &msg.content[..msg.content.len().min(100)]);
                        let sender = bus_sender.clone();
                        tokio::spawn(async move {
                            if let Err(e) = sender.send(msg).await {
                                error!("Failed to send progress message: {}", e);
                            }
                        });
                    }
                }
                event = watcher_event_rx.recv() => {
                    if let Some(event) = event {
                        info!("Watcher event: {} from {}", event.kind, event.watcher_id);
                        let agent = agent_clone.clone();
                        let permit = semaphore.clone().acquire_owned().await.expect("semaphore closed");
                        join_set.spawn(async move {
                            let _permit = permit;
                            let msg = meepo_core::types::IncomingMessage {
                                id: uuid::Uuid::new_v4().to_string(),
                                sender: "watcher".to_string(),
                                content: format!("Watcher {} triggered: {}", event.watcher_id, event.payload),
                                channel: meepo_core::types::ChannelType::Internal,
                                timestamp: chrono::Utc::now(),
                            };
                            match agent.handle_message(msg).await {
                                Ok(response) => {
                                    // Log the response instead of trying to send through bus
                                    // since Internal channel has no handler and watcher events
                                    // are informational notifications
                                    info!("Watcher {} response: {}", event.watcher_id, response.content);
                                }
                                Err(e) => error!("Failed to handle watcher event: {}", e),
                            }
                        });
                    }
                }
            }
        }

        // Drain remaining tasks for graceful shutdown
        while join_set.join_next().await.is_some() {}
    });

    // Wait for shutdown signal
    signal::ctrl_c().await?;
    info!("Received Ctrl+C, shutting down...");
    cancel.cancel();

    // Wait for main loop to finish
    let _ = main_loop.await;

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
