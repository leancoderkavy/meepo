# Meepo

A local AI agent for macOS and Windows that connects Claude to your digital life through Discord, Slack, and more.

Meepo runs as a daemon on your machine, monitoring your configured channels for messages. When you message it, it processes your request using Claude's API with access to 25 tools spanning email, calendar, web search, files, code, and a persistent knowledge graph.

## Features

- **Multi-channel messaging** — Discord DMs, Slack DMs, iMessage (macOS), or CLI one-shots
- **25 built-in tools** — Read/send emails, manage calendar events, search the web, run commands, browse URLs, read/write files, manage code PRs, and more
- **Cross-platform** — macOS (AppleScript) and Windows (PowerShell/Outlook COM) with platform abstraction layer
- **Sub-agent delegation** — Breaks complex tasks into parallel sub-tasks or fires off background work you can check on later
- **Web search** — Search the web and extract clean content from URLs via Tavily
- **Knowledge graph** — Remembers entities, relationships, and conversations across sessions with Tantivy full-text search
- **Scheduled watchers** — Monitor email, calendar, GitHub events, files, or run cron tasks
- **Security hardened** — Command allowlists, path traversal protection, SSRF blocking, input sanitization, 30s execution timeouts

## Requirements

- macOS or Windows
- Anthropic API key (required)
- Optional: Tavily API key (enables web search)
- Optional: Discord bot token, Slack bot token
- Rust toolchain only needed when building from source

### Platform Notes

| Feature | macOS | Windows |
|---------|-------|---------|
| Email (tool) | Mail.app via AppleScript | Outlook via PowerShell COM |
| Calendar (tool) | Calendar.app via AppleScript | Outlook via PowerShell COM |
| Clipboard | `arboard` crate (cross-platform) | `arboard` crate (cross-platform) |
| App launching | `open` crate (cross-platform) | `open` crate (cross-platform) |
| UI automation | System Events (AppleScript) | System.Windows.Automation (PowerShell) |
| Browser automation | Safari + Chrome (AppleScript) | Not yet available |
| iMessage channel | Messages.app (SQLite + AppleScript) | Not available |
| Email channel | Mail.app polling | Not available (use email tools instead) |
| Background service | `launchd` agent | Windows Task Scheduler |

## Install

**macOS / Linux (curl):**
```bash
curl -sSL https://raw.githubusercontent.com/kavymi/meepo/main/install.sh | bash
```

**macOS (Homebrew):**
```bash
brew install kavymi/tap/meepo
meepo setup
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/kavymi/meepo/main/install.ps1 | iex
```

**From source (macOS/Linux):**
```bash
git clone https://github.com/kavymi/meepo.git && cd meepo
cargo build --release && ./target/release/meepo setup
```

**From source (Windows PowerShell):**
```powershell
git clone https://github.com/kavymi/meepo.git; cd meepo
cargo build --release; .\target\release\meepo.exe setup
```

All methods run `meepo setup` — an interactive wizard that configures your API keys and tests the connection.

## Manual Setup

### 1. Build

```bash
git clone https://github.com/kavymi/meepo.git
cd meepo
cargo build --release
```

The binary is at `target/release/meepo` (macOS/Linux) or `target\release\meepo.exe` (Windows). First build takes ~5 minutes.

### 2. Initialize

```bash
meepo init
```

This creates `~/.meepo/` with:
- `config.toml` — Main configuration
- `workspace/SOUL.md` — Agent personality (editable)
- `workspace/MEMORY.md` — Persistent memory (auto-updated)

### 3. Configure API Keys

**Anthropic (required):**

```bash
# macOS/Linux
export ANTHROPIC_API_KEY="sk-ant-..."

# Windows PowerShell
$env:ANTHROPIC_API_KEY = "sk-ant-..."
# To persist across sessions:
[Environment]::SetEnvironmentVariable("ANTHROPIC_API_KEY", "sk-ant-...", "User")
```

Get yours at [console.anthropic.com/settings/keys](https://console.anthropic.com/settings/keys).

**Tavily (optional — enables web search):**

```bash
export TAVILY_API_KEY="tvly-..."
```

Get yours at [tavily.com](https://tavily.com). Without this key, Meepo still works — the `web_search` tool just won't be available, and `browse_url` will fall back to raw HTML fetching.

### 4. Enable Channels

Edit `~/.meepo/config.toml` to enable the channels you want:

#### Discord

```toml
[channels.discord]
enabled = true
token = "${DISCORD_BOT_TOKEN}"
allowed_users = ["123456789012345678"]  # Your Discord user ID
```

Requires a Discord bot with `MESSAGE_CONTENT` and `DIRECT_MESSAGES` intents enabled. Create one at the [Discord Developer Portal](https://discord.com/developers/applications).

#### Slack

```toml
[channels.slack]
enabled = true
bot_token = "${SLACK_BOT_TOKEN}"
poll_interval_secs = 3
```

Requires a Slack app with `chat:write`, `channels:read`, and `im:history` scopes. Create one at [api.slack.com/apps](https://api.slack.com/apps).

#### iMessage (macOS only)

```toml
[channels.imessage]
enabled = true
allowed_contacts = ["+15551234567", "user@icloud.com"]
poll_interval_secs = 3
```

No API key needed. Requires macOS with **Full Disk Access** granted to your terminal (System Settings > Privacy & Security > Full Disk Access). Not available on Windows.

All messages from allowed contacts are processed. Example: text "What's on my calendar?" to get a response.

### 5. Run

```bash
# Start the daemon (Ctrl+C to stop)
meepo start

# With debug logging
meepo --debug start

# Stop a backgrounded daemon
meepo stop
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `meepo setup` | Interactive setup wizard (API keys, connection test) |
| `meepo init` | Create `~/.meepo/` with default config |
| `meepo start` | Start the agent daemon |
| `meepo stop` | Stop a running daemon |
| `meepo ask "..."` | One-shot question (no daemon needed) |
| `meepo config` | Show loaded configuration |
| `meepo --debug <cmd>` | Enable debug logging |
| `meepo --config <path> <cmd>` | Use custom config file |

## Configuration Reference

Full config file: `~/.meepo/config.toml`

```toml
[agent]
default_model = "claude-opus-4-6"     # Claude model to use
max_tokens = 8192                      # Max response tokens

[providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"       # Required
base_url = "https://api.anthropic.com" # API endpoint

[providers.tavily]
api_key = "${TAVILY_API_KEY}"          # Optional — enables web_search tool

[channels.discord]
enabled = false
token = "${DISCORD_BOT_TOKEN}"
allowed_users = []                     # Discord user IDs (strings)

[channels.slack]
enabled = false
bot_token = "${SLACK_BOT_TOKEN}"
poll_interval_secs = 3                 # How often to check for messages

[channels.imessage]
enabled = false
allowed_contacts = []                  # Phone numbers or emails
poll_interval_secs = 3

[knowledge]
db_path = "~/.meepo/knowledge.db"
tantivy_path = "~/.meepo/tantivy_index"

[watchers]
max_concurrent = 50
min_poll_interval_secs = 30
active_hours = { start = "08:00", end = "23:00" }

[orchestrator]
max_concurrent_subtasks = 5            # Parallel sub-tasks per delegation
max_subtasks_per_request = 10          # Max sub-tasks per delegate call
parallel_timeout_secs = 120            # Timeout per parallel sub-task
background_timeout_secs = 600          # Timeout per background sub-task
max_background_groups = 3              # Concurrent background groups

[code]
claude_code_path = "claude"            # Path to Claude CLI
gh_path = "gh"                         # Path to GitHub CLI
default_workspace = "~/Coding"

[memory]
workspace = "~/.meepo/workspace"       # Contains SOUL.md and MEMORY.md

[browser]
enabled = true                         # Enable browser automation tools
default_browser = "safari"             # "safari" or "chrome"
```

Environment variables are expanded with `${VAR_NAME}` syntax. Paths support `~/` expansion.

## Tools

Meepo registers 25 tools that Claude can use during conversations:

| Category | Tools |
|----------|-------|
| **Email & Calendar** | `read_emails`, `send_email`, `read_calendar`, `create_calendar_event`, `open_app`, `get_clipboard` |
| **UI Automation** | `read_screen`, `click_element`, `type_text` |
| **Browser** | `browser_list_tabs`, `browser_open_tab`, `browser_close_tab`, `browser_switch_tab`, `browser_get_page_content`, `browser_execute_js`, `browser_click`, `browser_fill_form`, `browser_navigate`, `browser_get_url`, `browser_screenshot` |
| **Code** | `write_code`, `make_pr`, `review_pr` |
| **Web** | `web_search`, `browse_url` |
| **Memory** | `remember`, `recall`, `search_knowledge`, `link_entities` |
| **System** | `run_command`, `read_file`, `write_file` |
| **Watchers** | `create_watcher`, `list_watchers`, `cancel_watcher` |
| **Delegation** | `delegate_tasks` |

## Architecture

See [docs/architecture.md](docs/architecture.md) for detailed architecture documentation with Mermaid diagrams.

## Running as a Background Service

**macOS** — Install as a launchd agent (starts on login, auto-restarts):

```bash
scripts/install.sh     # Install and start
scripts/uninstall.sh   # Remove
```

Logs are at `~/.meepo/logs/meepo.out.log`.

**Windows** — Install as a scheduled task (starts on login, auto-restarts):

```powershell
scripts\install.ps1     # Install and start (requires Administrator)
scripts\uninstall.ps1   # Remove
```

## Troubleshooting

**"API key not set" or empty responses**
- Verify: `echo $ANTHROPIC_API_KEY` — should start with `sk-ant-`
- If using the launch agent, re-run `scripts/install.sh` after setting new env vars (the plist snapshots env vars at install time)

**iMessage not receiving messages**
- Grant Full Disk Access to your terminal: System Settings > Privacy & Security > Full Disk Access
- Check `allowed_contacts` in config includes the sender's phone/email

**`web_search` tool not available**
- Set `TAVILY_API_KEY` env var — Meepo logs a warning at startup if it's missing
- The tool is only registered when a valid Tavily API key is configured

**Discord bot not responding**
- Enable `MESSAGE CONTENT INTENT` in the Discord Developer Portal (Bot > Privileged Gateway Intents)
- Verify `allowed_users` contains your Discord user ID (right-click your name > Copy User ID)

**Build failures**
- Ensure Rust is up to date: `rustup update`
- Clean build: `cargo clean && cargo build --release`
- Windows: Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with "Desktop development with C++"

**"Permission denied" running scripts**
- macOS/Linux: `chmod +x scripts/*.sh`
- Windows: `Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser`

**Windows: Email/Calendar tools not working**
- Outlook must be installed (uses COM automation)
- Verify with PowerShell: `New-Object -ComObject Outlook.Application`
- If using Windows Mail instead of Outlook, these tools won't work

**Windows: API key not persisting across sessions**
- PowerShell `$env:` variables are session-only. To persist:
  ```powershell
  [Environment]::SetEnvironmentVariable("ANTHROPIC_API_KEY", "sk-ant-...", "User")
  ```
- Then restart your terminal

## Project Structure

```
meepo/
├── crates/
│   ├── meepo-core/       # Agent loop, API client, tool system, orchestrator
│   ├── meepo-channels/   # Discord, Slack, iMessage adapters + message bus
│   ├── meepo-knowledge/  # SQLite + Tantivy knowledge graph
│   ├── meepo-scheduler/  # Watcher runner, persistence, polling
│   └── meepo-cli/        # CLI binary, config loading
├── config/
│   └── default.toml      # Default configuration template (heavily commented)
├── scripts/
│   ├── setup.sh          # Interactive first-time setup (macOS)
│   ├── setup.ps1         # Interactive first-time setup (Windows)
│   ├── install.sh        # Install as macOS launch agent
│   ├── install.ps1       # Install as Windows scheduled task
│   ├── uninstall.sh      # Remove macOS launch agent
│   ├── uninstall.ps1     # Remove Windows scheduled task
│   ├── run.sh            # Quick build-and-start (macOS)
│   └── run.ps1           # Quick build-and-start (Windows)
├── docs/
│   └── architecture.md   # Detailed architecture with Mermaid diagrams
├── CONTRIBUTING.md        # Developer setup and contribution guide
├── SOUL.md               # Agent personality template
└── MEMORY.md             # Agent memory template
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, testing, and contribution guidelines.

## License

MIT
