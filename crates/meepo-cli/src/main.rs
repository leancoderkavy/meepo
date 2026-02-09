use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, error};
use tracing_subscriber::EnvFilter;

mod config;

use config::MeepoConfig;

#[derive(Parser)]
#[command(name = "meepo")]
#[command(version)]
#[command(about = "Meepo — a local AI agent for macOS")]
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
        Commands::Config => cmd_config(&cli.config).await,
        Commands::Start => cmd_start(&cli.config).await,
        Commands::Stop => cmd_stop().await,
        Commands::Ask { message } => cmd_ask(&cli.config, &message).await,
    }
}

async fn cmd_init() -> Result<()> {
    let config_dir = config::config_dir();
    std::fs::create_dir_all(&config_dir)
        .with_context(|| format!("Failed to create config dir: {}", config_dir.display()))?;

    let config_path = config_dir.join("config.toml");
    if config_path.exists() {
        warn!("Config already exists at {}", config_path.display());
    } else {
        let default_config = include_str!("../../../config/default.toml");
        std::fs::write(&config_path, default_config)?;
        info!("Created default config at {}", config_path.display());
    }

    // Create workspace directory
    let workspace = config_dir.join("workspace");
    std::fs::create_dir_all(&workspace)?;

    // Copy SOUL.md and MEMORY.md templates if not present
    let soul_path = workspace.join("SOUL.md");
    if !soul_path.exists() {
        std::fs::write(&soul_path, include_str!("../../../SOUL.md"))?;
        info!("Created SOUL.md at {}", soul_path.display());
    }

    let memory_path = workspace.join("MEMORY.md");
    if !memory_path.exists() {
        std::fs::write(&memory_path, include_str!("../../../MEMORY.md"))?;
        info!("Created MEMORY.md at {}", memory_path.display());
    }

    println!("Meepo initialized at {}", config_dir.display());
    println!("Edit {} to configure your API keys and channels.", config_path.display());
    Ok(())
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
    let soul = meepo_knowledge::load_soul(&workspace.join("SOUL.md"))
        .unwrap_or_else(|_| "You are Meepo, a helpful AI assistant.".to_string());
    let memory = meepo_knowledge::load_memory(&workspace.join("MEMORY.md"))
        .unwrap_or_default();
    info!("Loaded SOUL ({} chars) and MEMORY ({} chars)", soul.len(), memory.len());

    // Initialize API client
    let api_key = shellexpand_str(&cfg.providers.anthropic.api_key);
    let api = meepo_core::api::ApiClient::new(
        api_key,
        Some(cfg.agent.default_model.clone()),
    ).with_max_tokens(cfg.agent.max_tokens);
    info!("Anthropic API client initialized (model: {})", cfg.agent.default_model);

    // Initialize watcher command channel (needed for tool registration)
    let (watcher_command_tx, mut watcher_command_rx) = tokio::sync::mpsc::channel::<meepo_core::tools::watchers::WatcherCommand>(100);

    // Build tool registry
    let mut registry = meepo_core::tools::ToolRegistry::new();
    registry.register(Arc::new(meepo_core::tools::macos::ReadEmailsTool));
    registry.register(Arc::new(meepo_core::tools::macos::ReadCalendarTool));
    registry.register(Arc::new(meepo_core::tools::macos::SendEmailTool));
    registry.register(Arc::new(meepo_core::tools::macos::CreateEventTool));
    registry.register(Arc::new(meepo_core::tools::macos::OpenAppTool));
    registry.register(Arc::new(meepo_core::tools::macos::GetClipboardTool));
    registry.register(Arc::new(meepo_core::tools::accessibility::ReadScreenTool));
    registry.register(Arc::new(meepo_core::tools::accessibility::ClickElementTool));
    registry.register(Arc::new(meepo_core::tools::accessibility::TypeTextTool));
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
    registry.register(Arc::new(meepo_core::tools::system::BrowseUrlTool));
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
    let orchestrator_api = meepo_core::api::ApiClient::new(
        shellexpand_str(&cfg.providers.anthropic.api_key),
        Some(cfg.agent.default_model.clone()),
    ).with_max_tokens(cfg.agent.max_tokens);
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

    // Load persisted watchers
    {
        let sched_db = rusqlite::Connection::open(&db_path)?;
        meepo_scheduler::persistence::init_watcher_tables(&sched_db)?;
        let watchers = meepo_scheduler::persistence::get_active_watchers(&sched_db)?;
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

    // Register iMessage channel if enabled
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

    // Register Slack channel if enabled
    if cfg.channels.slack.enabled {
        let slack = meepo_channels::slack::SlackChannel::new(
            shellexpand_str(&cfg.channels.slack.bot_token),
            std::time::Duration::from_secs(cfg.channels.slack.poll_interval_secs),
        );
        bus.register(Box::new(slack));
        info!("Slack channel registered");
    }

    // Start all channels
    bus.start_all().await?;
    info!("All message channels started");

    println!("Meepo is running. Press Ctrl+C to stop.");

    // Split bus into receiver + sender for concurrent use
    let (mut incoming_rx, bus_sender) = bus.split();
    let bus_sender = Arc::new(bus_sender);

    // Main event loop
    let agent_clone = agent.clone();
    let cancel_clone = cancel.clone();
    let watcher_runner_clone = watcher_runner.clone();
    let main_loop = tokio::spawn(async move {
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
                            tokio::spawn(async move {
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
                        tokio::spawn(async move {
                            use meepo_core::tools::watchers::WatcherCommand;
                            match command {
                                WatcherCommand::Create { kind: _, config, action, reply_channel } => {
                                    let watcher = meepo_scheduler::watcher::Watcher::new(
                                        match serde_json::from_value(config) {
                                            Ok(k) => k,
                                            Err(e) => {
                                                error!("Failed to deserialize watcher kind: {}", e);
                                                return;
                                            }
                                        },
                                        action,
                                        reply_channel,
                                    );
                                    if let Err(e) = runner.lock().await.start_watcher(watcher).await {
                                        error!("Failed to start watcher: {}", e);
                                    }
                                }
                                WatcherCommand::Cancel { id } => {
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
                        tokio::spawn(async move {
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
    let api = meepo_core::api::ApiClient::new(
        api_key,
        Some(cfg.agent.default_model.clone()),
    ).with_max_tokens(cfg.agent.max_tokens);

    // Load context
    let workspace = shellexpand(&cfg.memory.workspace);
    let soul = meepo_knowledge::load_soul(&workspace.join("SOUL.md"))
        .unwrap_or_else(|_| "You are Meepo, a helpful AI assistant.".to_string());
    let memory = meepo_knowledge::load_memory(&workspace.join("MEMORY.md"))
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
