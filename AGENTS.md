# Meepo — Agent Guidelines

Instructions for AI coding agents (Windsurf Cascade, Claude Code, Cursor, Copilot, etc.) working on this codebase.

## Build & Verify

```bash
cargo build                    # Must pass before committing
cargo test --workspace         # 167+ tests across 5 crates
cargo clippy --workspace       # No warnings allowed
```

Always run `cargo check` after edits to catch compile errors early.

## Project Overview

Meepo is a local AI agent (Rust, macOS/Windows) that connects Claude to your digital life via Discord, Slack, iMessage, email, and more. It runs as a daemon with an autonomous observe/think/act loop, 75+ tools, MCP/A2A protocol support, and a persistent knowledge graph.

**Workspace:** 7 crates in `crates/` — see `Cargo.toml` for the full list.

| Crate | Role |
|-------|------|
| `meepo-cli` | Binary, CLI commands, daemon startup, config, templates |
| `meepo-core` | Agent loop, API client, tools, orchestrator, autonomy, platform abstraction |
| `meepo-channels` | Message bus + channel adapters (Discord, Slack, iMessage, email, Alexa, etc.) |
| `meepo-knowledge` | SQLite + Tantivy knowledge graph |
| `meepo-scheduler` | Watcher runner and persistence |
| `meepo-mcp` | MCP server/client (STDIO JSON-RPC) |
| `meepo-a2a` | A2A protocol server/client (HTTP) |

## Key Files

- **`crates/meepo-cli/src/main.rs`** — Daemon startup, tool registration, event loop (~3000 lines). Tools are registered in both `cmd_start()` and `cmd_mcp_server()`.
- **`crates/meepo-cli/src/config.rs`** — All config structs, env var expansion, TOML loading.
- **`crates/meepo-core/src/tools/mod.rs`** — `ToolHandler` trait, `ToolRegistry`, `json_schema()` helper.
- **`crates/meepo-core/src/platform/mod.rs`** — OS abstraction traits and factory functions.
- **`crates/meepo-core/src/autonomy/mod.rs`** — `AutonomousLoop` (observe/think/act cycle).
- **`config/default.toml`** — Default config template with extensive comments.

## Conventions

### Rust
- **Edition:** 2024 — use `let` chains, `if let` chains
- **Errors:** `anyhow::Result` in application code, `thiserror` for library types
- **Logging:** `tracing` macros (`debug!`, `info!`, `warn!`, `error!`) — never `println!` in library code
- **Async:** `tokio` runtime, `async_trait` for trait methods
- **Config:** `serde` with `#[serde(default = "fn_name")]` for optional fields

### Tool Pattern
```rust
pub struct MyTool { /* dependencies */ }

impl MyTool {
    pub fn new(/* deps */) -> Self { Self { /* ... */ } }
}

#[async_trait]
impl ToolHandler for MyTool {
    fn name(&self) -> &str { "my_tool" }
    fn description(&self) -> &str { "What this tool does" }
    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "param": { "type": "string", "description": "..." }
            }),
            vec!["param"],
        )
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let param = input.get("param").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'param'"))?;
        // validate inputs, do work, return string result
        Ok("result".to_string())
    }
}
```

### Security Rules
- **No hardcoded secrets** — use `${ENV_VAR}` in config, custom `Debug` impls for secret-holding structs
- **Validate all inputs** — string lengths, path traversal, branch name characters, command injection
- **Timeout external commands** — `tokio::time::timeout()` (30-300s)
- **Sandbox file access** — validate against `allowed_directories` config

### Platform Code
- OS-specific code behind traits in `meepo-core::platform`
- Gate with `#[cfg(target_os = "macos")]` / `#[cfg(target_os = "windows")]`
- Factory functions return `Box<dyn Trait>`

### Testing
- Tests in same file: `#[cfg(test)] mod tests { ... }`
- Test schemas, required fields, error cases, basic execution
- `#[tokio::test]` for async tests
- Temp dirs for database tests

## Common Workflows

### Add a tool
1. Implement `ToolHandler` in `crates/meepo-core/src/tools/`
2. Register in `main.rs` → `cmd_start()` **and** `cmd_mcp_server()`
3. Add tests
4. Run `cargo test -p meepo-core`

### Add a channel adapter
1. Implement `MessageChannel` in `crates/meepo-channels/src/`
2. Add config struct in `crates/meepo-cli/src/config.rs`
3. Register in `main.rs` → `cmd_start()`
4. Add to `config/default.toml`

### Add a config option
1. Add field with `#[serde(default = "default_fn")]` in `config.rs`
2. Write the default function
3. Document in `config/default.toml`

## Do NOT

- Add `#[allow(dead_code)]` — remove unused code instead
- Use `unwrap()` outside tests — use `?` or `.ok_or_else()`
- Import in the middle of a file — imports go at the top
- Create files outside `crates/`, `config/`, `scripts/`, `docs/` without asking
- Modify `Cargo.lock` directly — let Cargo manage it
