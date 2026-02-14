# Meepo — Development Guide

## Quick Reference

```bash
cargo build                    # Build all crates
cargo test --workspace         # Run all 167+ tests
cargo test -p meepo-core       # Test a single crate
cargo clippy --workspace       # Lint
cargo run -- ask "Hello"       # One-shot test (needs ANTHROPIC_API_KEY)
cargo run -- --debug start     # Start daemon with debug logging
```

## Architecture

Rust workspace with 7 crates. Dependencies flow downward — `meepo-cli` depends on everything, leaf crates have no internal dependencies.

| Crate | Purpose |
|-------|---------|
| `meepo-cli` | Binary entry point, CLI commands, daemon startup, event loop, config, templates |
| `meepo-core` | Agent loop, Anthropic API client, 75+ tools, orchestrator, autonomy, platform abstraction |
| `meepo-channels` | Message bus + channel adapters (Discord, Slack, iMessage, email, Alexa, etc.) |
| `meepo-knowledge` | SQLite + Tantivy knowledge graph (entities, relationships, full-text search) |
| `meepo-scheduler` | Watcher runner, 7 watcher types, persistence |
| `meepo-mcp` | MCP server (STDIO JSON-RPC) and client for external MCP servers |
| `meepo-a2a` | Google A2A protocol — HTTP server/client for agent-to-agent delegation |

## Key Patterns

**Tool system:** All tools implement `ToolHandler` trait (`name()`, `description()`, `input_schema()`, `execute()`). Registered in `ToolRegistry` at startup. API client runs a tool loop until Claude returns text or hits 10-iteration limit.

**Channel adapters:** Implement `MessageChannel` trait (`start()`, `send()`, `channel_type()`). `MessageBus` splits into receiver + `Arc<BusSender>` for concurrent use.

**Platform abstraction:** OS-specific code behind traits in `meepo-core::platform` (`EmailProvider`, `CalendarProvider`, `BrowserProvider`, etc.). Selected at compile time via `#[cfg(target_os)]`. Factory functions return `Box<dyn Trait>`.

**Secrets:** API keys use `${ENV_VAR}` syntax in TOML config, expanded at load time via an allowlist. Structs holding secrets get custom `Debug` impls (not `#[derive(Debug)]`). Never hardcode secrets.

**Concurrency:** Tokio async runtime. `CancellationToken` for graceful shutdown. `mpsc` channels between components. `Semaphore` for concurrency limits.

**Autonomous loop:** `AutonomousLoop` drives observe/think/act cycle. Drains inputs (user messages + watcher events), checks due goals, processes everything. `Notify` handle wakes the loop on new inputs.

## File Layout

```
crates/
├── meepo-cli/src/
│   ├── main.rs        # CLI commands, daemon startup, tool registration, event loop (~3000 lines)
│   ├── config.rs      # Config structs, env var expansion, TOML loading
│   └── template.rs    # Agent template system
├── meepo-core/src/
│   ├── agent.rs       # Agent struct, message handling
│   ├── api.rs         # Anthropic API client with tool loop
│   ├── tools/         # All tool implementations
│   │   ├── mod.rs     # ToolHandler trait, ToolRegistry, ToolExecutor
│   │   ├── macos.rs   # Email, Calendar, Reminders, Notes, Contacts, Music, etc.
│   │   ├── browser.rs # 11 browser automation tools
│   │   ├── code.rs    # write_code, make_pr, review_pr, spawn_claude_code
│   │   ├── lifestyle/ # Email intelligence, calendar, research, SMS, tasks, news, finance, health, travel, social
│   │   └── ...
│   ├── autonomy/      # Autonomous loop, goals, planner, user model
│   ├── platform/      # OS abstraction (mod.rs traits, macos.rs, windows.rs)
│   └── skills/        # OpenClaw SKILL.md loader
├── meepo-channels/src/  # Discord, Slack, iMessage, email, Alexa, Reminders, Notes, Contacts adapters
├── meepo-knowledge/src/ # KnowledgeGraph (SQLite + Tantivy), KnowledgeDb
├── meepo-scheduler/src/ # WatcherRunner, 7 watcher types, persistence
├── meepo-mcp/src/       # MCP server/client, protocol types
└── meepo-a2a/src/       # A2A server/client, AgentCard, protocol types
```

## Adding a New Tool

1. Create struct in `crates/meepo-core/src/tools/` (or appropriate submodule)
2. Implement `ToolHandler` trait with `name()`, `description()`, `input_schema()`, `execute()`
3. Register in `crates/meepo-cli/src/main.rs` in both `cmd_start()` and `cmd_mcp_server()`
4. Add tests in the same file
5. Use `json_schema()` helper for input schema, `anyhow::Result<String>` for return

## Code Style

- Rust 2024 edition
- `anyhow` for error handling in applications, `thiserror` for library error types
- `tracing` for logging (`debug!`, `info!`, `warn!`, `error!`)
- `async_trait` for async trait methods
- `serde` with `#[serde(default)]` for optional config fields
- Input validation: check lengths, sanitize paths, validate branch names
- Security: command allowlists, path traversal protection, SSRF blocking, 30s execution timeouts
- Platform-gated code uses `#[cfg(target_os = "macos")]` / `#[cfg(target_os = "windows")]`
- Tools that accept `Arc<KnowledgeDb>` store it for DB operations; tools that don't need it use unit structs

## Config

Main config: `~/.meepo/config.toml` (see `config/default.toml` for template)
Runtime data: `~/.meepo/` (knowledge.db, tantivy_index/, workspace/SOUL.md, workspace/MEMORY.md)
Env var allowlist for expansion: `ANTHROPIC_API_KEY`, `TAVILY_API_KEY`, `DISCORD_BOT_TOKEN`, `SLACK_BOT_TOKEN`, `A2A_AUTH_TOKEN`, `GITHUB_TOKEN`, `HOME`, `USER`
