# Contributing to Meepo

## Prerequisites

### macOS
- **Rust toolchain** — Install via [rustup](https://rustup.rs/):
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **Optional CLIs:**
  - `gh` — GitHub CLI (`brew install gh`)
  - `claude` — Claude Code CLI (`npm install -g @anthropic-ai/claude-code`)

### Windows
- **Rust toolchain** — Install via [rustup](https://rustup.rs/). Download and run `rustup-init.exe`.
- **Visual Studio Build Tools** — Required for C compilation (SQLite, etc.):
  - Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  - Select "Desktop development with C++" workload
- **Microsoft Outlook** — Required for email/calendar tools (uses COM automation)
- **Optional CLIs:**
  - `gh` — GitHub CLI (`winget install GitHub.cli`)
  - `claude` — Claude Code CLI (`npm install -g @anthropic-ai/claude-code`)

## Getting Started

```bash
git clone https://github.com/kavymi/meepo.git
cd meepo

# Build the workspace
cargo build

# Run all tests (167 tests across 5 crates)
cargo test --workspace

# Run tests for a single crate
cargo test -p meepo-core

# Build release binary
cargo build --release
```

## Workspace Layout

Meepo is a Cargo workspace with 7 crates. Dependencies flow downward — `meepo-cli` depends on everything, leaf crates have no internal dependencies.

```
crates/
├── meepo-cli/          # Binary entry point
│   └── src/
│       ├── main.rs     # CLI commands, daemon startup, event loop
│       ├── config.rs   # Config loading, env var expansion
│       └── template.rs # Agent template system (parse, activate, reset)
│
├── meepo-core/         # Core agent logic (largest crate)
│   └── src/
│       ├── lib.rs      # Public API exports
│       ├── agent.rs    # Agent struct, message handling, conversation history
│       ├── api.rs      # Anthropic API client, tool loop
│       ├── tavily.rs   # Tavily Search/Extract client
│       ├── orchestrator.rs  # Sub-agent task orchestrator
│       ├── context.rs  # System prompt builder (SOUL + MEMORY)
│       ├── notifications.rs # Proactive notification service
│       ├── types.rs    # Shared types (IncomingMessage, OutgoingMessage, ChannelType)
│       ├── autonomy/   # Autonomous agent loop
│       │   ├── mod.rs          # AutonomousLoop (observe/think/act cycle)
│       │   ├── goals.rs        # Goal tracking and evaluation
│       │   ├── planner.rs      # Action planning
│       │   ├── user_model.rs   # User preference learning
│       │   └── action_log.rs   # Action history
│       ├── platform/   # OS abstraction layer
│       │   ├── mod.rs          # Trait definitions + factory functions
│       │   ├── macos.rs        # macOS implementations (AppleScript)
│       │   └── windows.rs      # Windows implementations (PowerShell/COM)
│       ├── skills/     # Skill system (OpenClaw-compatible SKILL.md)
│       │   ├── mod.rs          # Skill loader
│       │   ├── parser.rs       # YAML frontmatter parser
│       │   └── skill_tool.rs   # SkillToolHandler wrapper
│       └── tools/      # All 40+ tool implementations
│           ├── mod.rs          # ToolHandler trait, ToolRegistry
│           ├── macos.rs        # Email, Calendar, Reminders, Notes, Contacts, Music, etc.
│           ├── accessibility.rs # Screen reader, click, type
│           ├── browser.rs      # Browser automation (11 tools)
│           ├── code.rs         # write_code, make_pr, review_pr, spawn_claude_code
│           ├── search.rs       # web_search via Tavily
│           ├── memory.rs       # Knowledge graph tools (4 tools)
│           ├── system.rs       # Commands, files, browse_url
│           ├── filesystem.rs   # list_directory, search_files (sandboxed)
│           ├── watchers.rs     # Watcher management (3 tools)
│           ├── autonomous.rs   # spawn_background_task, agent_status, stop_task
│           └── delegate.rs     # Sub-agent delegation
│
├── meepo-channels/     # Messaging adapters
│   └── src/
│       ├── lib.rs      # MessageBus, BusSender, MessageChannel trait
│       ├── discord.rs  # Discord via Serenity WebSocket
│       ├── slack.rs    # Slack via HTTP polling
│       ├── imessage.rs # iMessage via SQLite + AppleScript (macOS only)
│       └── email.rs    # Email via Mail.app polling (macOS only)
│
├── meepo-knowledge/    # Persistence layer
│   └── src/
│       ├── lib.rs      # KnowledgeGraph (combines SQLite + Tantivy)
│       ├── db.rs       # KnowledgeDb (SQLite operations)
│       └── search.rs   # TantivyIndex (full-text search)
│
├── meepo-scheduler/    # Background watchers
│   └── src/
│       ├── lib.rs      # WatcherRunner, task management
│       ├── watchers.rs # 7 watcher types (email, calendar, file, etc.)
│       └── persistence.rs # Watcher state in SQLite
│
├── meepo-mcp/          # MCP server and client
│   └── src/
│       ├── lib.rs      # Public exports
│       ├── server.rs   # MCP server over STDIO (JSON-RPC)
│       ├── client.rs   # MCP client (spawn external servers)
│       ├── adapter.rs  # McpToolAdapter (ToolRegistry → MCP format)
│       └── protocol.rs # MCP protocol types
│
└── meepo-a2a/          # A2A (Agent-to-Agent) protocol
    └── src/
        ├── lib.rs      # Public exports
        ├── server.rs   # A2A HTTP server (task submission, polling)
        ├── client.rs   # A2A client (discover + delegate to peers)
        ├── tool.rs     # DelegateToAgentTool
        └── protocol.rs # AgentCard, TaskRequest, TaskResponse types
```

## Key Patterns

**Tool system:** All tools implement the `ToolHandler` trait (`name()`, `description()`, `input_schema()`, `execute()`). They're registered in a `ToolRegistry` (HashMap-backed) at daemon startup. The API client runs a tool loop until Claude returns a final text response or hits the 10-iteration limit.

**Channel adapters:** Channels implement `MessageChannel` trait (`start()`, `send()`, `channel_type()`). The `MessageBus` splits into a receiver and an `Arc<BusSender>` for concurrent send/receive.

**Secrets in config:** API keys use `${ENV_VAR}` syntax in TOML, expanded at load time. Never hardcode secrets. Structs holding secrets get custom `Debug` impls (not `#[derive(Debug)]`).

**Platform abstraction:** OS-specific code lives behind traits in `meepo-core::platform` (`EmailProvider`, `CalendarProvider`, `BrowserProvider`, etc.). Implementations are selected at compile time via `#[cfg(target_os)]`. Factory functions return `Box<dyn Trait>`.

**Optional providers:** Use `Option<Config>` with `#[serde(default)]` for optional features like Tavily. Construct the client only if the key is non-empty. Conditionally register tools.

**Concurrency:** Use `tokio::sync::Semaphore` for concurrency limits. Use CAS loops (`compare_exchange`) for atomic counters under contention, not load-then-increment.

**Autonomous loop:** The `AutonomousLoop` drives the agent with a tick-based observe/think/act cycle. It drains inputs (user messages + watcher events), checks due goals, and processes everything. A `Notify` handle wakes the loop immediately when new inputs arrive.

**MCP/A2A interop:** `meepo-mcp` exposes tools via STDIO JSON-RPC. `meepo-a2a` exposes an HTTP server for peer agents. Both depend only on `meepo-core`.

## Running Locally

**First-time setup (recommended):**
```bash
# Build and run the interactive setup wizard
cargo run -- setup
```
This walks you through API keys, macOS permissions (Accessibility, Full Disk Access, Automation, Screen Recording), feature selection, and verifies the API connection. It opens System Settings panes for you and detects your terminal app automatically.

**macOS/Linux (manual):**
```bash
# Initialize config (creates ~/.meepo/)
cargo run -- init

# Set API key (for Anthropic)
export ANTHROPIC_API_KEY="sk-ant-..."

# OR use Ollama (local models, no API key needed)
# 1. Install Ollama: curl -fsSL https://ollama.ai/install.sh | sh
# 2. Pull a model: ollama pull llama3.2
# 3. Edit ~/.meepo/config.toml:
#    [agent]
#    default_model = "ollama"
#    [providers.ollama]
#    base_url = "http://localhost:11434"
#    model = "llama3.2"

# Start daemon in debug mode
cargo run -- --debug start

# One-shot test (doesn't need daemon running)
cargo run -- ask "Hello, what tools do you have?"
```

**Windows (PowerShell):**
```powershell
# Initialize config (creates %USERPROFILE%\.meepo\)
cargo run -- init

# Set API key (session only)
$env:ANTHROPIC_API_KEY = "sk-ant-..."

# Or use the run script (auto-builds, checks config)
.\scripts\run.ps1

# One-shot test
cargo run -- ask "Hello, what tools do you have?"
```

**Windows interactive setup (recommended for first time):**
```powershell
.\scripts\setup.ps1
```
This builds the binary, initializes config, walks through API keys, and tests the connection.

## Development in a Tart VM

[Tart](https://tart.run) lets you run macOS VMs on Apple Silicon. This is useful for testing Meepo in a clean environment or running CI locally.

```bash
# Create a macOS VM
tart create meepo-dev --from-oci ghcr.io/cirruslabs/macos-sequoia-xcode:latest

# Boot it (GUI mode — needed for browser/UI tools)
tart run meepo-dev

# Or headless (CLI, Discord, Slack, MCP, A2A still work)
tart run meepo-dev --no-graphics
```

Inside the VM:
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone and build
git clone https://github.com/kavymi/meepo.git && cd meepo
cargo build

# Run tests
cargo test --workspace

# Set up and run
export ANTHROPIC_API_KEY="sk-ant-..."
cargo run -- setup
```

The setup wizard (`meepo setup` / `cargo run -- setup`) auto-detects Tart VMs and shows which features work in the current environment. You can also force detection with `TART_VM=1`.

**Networking:** Tart uses NAT by default. For A2A testing from the host, use `tart run meepo-dev --net-softnet`.

## Adding a New Tool

1. Create a struct in the appropriate file under `crates/meepo-core/src/tools/`
2. Implement `ToolHandler` trait:
   ```rust
   #[async_trait]
   impl ToolHandler for MyTool {
       fn name(&self) -> &str { "my_tool" }
       fn description(&self) -> &str { "Does something useful" }
       fn input_schema(&self) -> serde_json::Value {
           serde_json::json!({
               "type": "object",
               "properties": { ... },
               "required": [...]
           })
       }
       async fn execute(&self, input: serde_json::Value) -> anyhow::Result<String> {
           // Implementation
       }
   }
   ```
3. Register it in `crates/meepo-cli/src/main.rs` during daemon startup
4. Add tests

## Pull Request Workflow

1. Create a feature branch: `git checkout -b feature/my-feature`
2. Make changes
3. Run tests: `cargo test --workspace`
4. Run clippy: `cargo clippy --workspace`
5. Open a PR against `main`

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
