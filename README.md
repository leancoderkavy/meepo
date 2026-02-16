# Meepo

A local AI agent for macOS and Windows that connects Claude to your digital life through Discord, Slack, iMessage, email, and more. **Divided We Stand.**

Meepo runs as a daemon on your machine — a prime agent that splits into clones to be everywhere at once. Channel clones monitor Discord, Slack, iMessage, and email simultaneously. Task clones dig in parallel on complex requests. Watcher clones stand guard over your inbox, calendar, and GitHub repos around the clock. The prime Meepo coordinates them all through an autonomous observe/think/act loop, with access to 75+ tools spanning email, calendar, reminders, notes, browser automation, web search, files, code, music, contacts, and a persistent knowledge graph. It also speaks MCP and A2A protocols — exposing its tools to other AI agents and consuming tools from external MCP servers.

## Features

- **Multi-channel messaging** — Discord DMs, Slack DMs, iMessage (macOS), email (macOS), or CLI one-shots
- **75+ built-in tools** — Email, calendar, reminders, notes, contacts, browser automation, web search, file browsing, code PRs, music control, screen capture, lifestyle integrations (research, tasks, finance, health, travel, social), and more
- **Autonomous agent loop** — Observe/think/act cycle with goal tracking, proactive actions, and notification alerts
- **Multiple LLM providers** — Anthropic Claude (API), Ollama (local models), OpenAI, Google Gemini, or any OpenAI-compatible endpoint with automatic failover
- **Cross-platform** — macOS (AppleScript) and Windows (PowerShell/Outlook COM) with platform abstraction layer
- **MCP support** — Expose Meepo's tools as an MCP server (STDIO) for Claude Desktop, Cursor, etc. — and consume tools from external MCP servers
- **A2A protocol** — Google's Agent-to-Agent protocol for delegating tasks to/from peer AI agents over HTTP
- **Clone delegation** — Spawns focused Meepo clones that dig in parallel on complex tasks, or work in the background and report back when done
- **Browser automation** — Full Safari and Chrome control — tabs, navigation, JS execution, form filling, screenshots
- **Web search** — Search the web and extract clean content from URLs via Tavily
- **Knowledge graph** — Remembers entities, relationships, and conversations across sessions with Tantivy full-text search
- **Scheduled watchers** — Monitor email, calendar, GitHub events, files, or run cron tasks
- **Agent templates** — Swap personalities, goals, and config overlays with `meepo template use`
- **Skills system** — Import OpenClaw-compatible SKILL.md files as additional tools
- **Proactive notifications** — iMessage/Discord/Slack alerts for task progress, watcher triggers, and errors (with quiet hours)
- **Security hardened** — Command allowlists, path traversal protection, SSRF blocking, input sanitization, 30s execution timeouts

## Requirements

- macOS or Windows
- LLM provider: Either Anthropic API key **or** Ollama installed locally
  - **Anthropic Claude**: Requires API key from https://console.anthropic.com
  - **Ollama**: Free, runs locally — supports Llama, Mistral, CodeLlama, and more
- Optional: Tavily API key (enables web search)
- Optional: Discord bot token, Slack bot token
- Rust toolchain only needed when building from source

### Using Ollama (Local LLMs)

Meepo supports [Ollama](https://ollama.ai) for running local LLMs without requiring an API key:

1. **Install Ollama:**
   ```bash
   curl -fsSL https://ollama.ai/install.sh | sh
   ```

2. **Pull a model:**
   ```bash
   ollama pull llama3.2        # or mistral, codellama, phi3, etc.
   ```

3. **Configure Meepo** to use Ollama in `~/.meepo/config.toml`:
   ```toml
   [agent]
   default_model = "ollama"

   [providers.ollama]
   base_url = "http://localhost:11434"
   model = "llama3.2"
   ```

4. **Start Meepo:**
   ```bash
   meepo start
   ```

Ollama runs entirely on your machine — no API key needed, no data sent to external servers.

### Platform Notes

| Feature | macOS | Windows |
|---------|-------|---------|
| Email (tool) | Mail.app via AppleScript | Outlook via PowerShell COM |
| Calendar (tool) | Calendar.app via AppleScript | Outlook via PowerShell COM |
| Reminders (tool) | Reminders.app via AppleScript | Not available |
| Notes (tool) | Notes.app via AppleScript | Not available |
| Contacts (tool) | Contacts.app via AppleScript | Not available |
| Music (tool) | Apple Music via AppleScript | Not available |
| Screen capture | `screencapture` CLI | Not available |
| Notifications | `osascript` display notification | Not available |
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

All methods run `meepo setup` — an interactive wizard that walks you through API keys, macOS permissions (Accessibility, Full Disk Access, Automation, Screen Recording), feature selection, and connection verification. It opens System Settings panes for you and detects your terminal app automatically.

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

### 4. macOS Permissions

Meepo's tools require several macOS permissions. The `meepo setup` wizard handles all of these automatically — it detects what's missing, opens the correct System Settings pane, and tells you exactly what to click. You can also grant them manually:

| Permission | Required For | System Settings Path |
|------------|-------------|---------------------|
| **Accessibility** | `read_screen`, `click_element`, `type_text` (UI automation) | Privacy & Security → Accessibility |
| **Full Disk Access** | iMessage channel (reads `~/Library/Messages/chat.db`) | Privacy & Security → Full Disk Access |
| **Automation** | Email, Calendar, Reminders, Notes, Messages, Music tools | Privacy & Security → Automation |
| **Screen Recording** | `screen_capture` tool | Privacy & Security → Screen Recording |

Grant each permission to your terminal app (Terminal, iTerm, Warp, Ghostty, VS Code, etc.). The setup wizard detects which terminal you're using.

> **Tip:** If a tool fails with a permission error after setup, re-run `meepo setup` — it will check and guide you through any missing permissions.

### 5. Enable Channels

The setup wizard lets you enable channels interactively. You can also edit `~/.meepo/config.toml` manually:

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

#### Safari Browser Automation

If you enabled browser automation with Safari, one extra setting is needed:

1. Open Safari
2. Safari → Settings → Advanced
3. Check "Show features for web developers"
4. Close Settings
5. Develop menu → Allow JavaScript from Apple Events (check it)

Chrome requires no extra setup.

### 6. Run

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
| `meepo setup` | Interactive setup wizard (API keys, macOS permissions, feature selection, connection test) |
| `meepo init` | Create `~/.meepo/` with default config |
| `meepo start` | Start the agent daemon |
| `meepo stop` | Stop a running daemon |
| `meepo ask "..."` | One-shot question (no daemon needed) |
| `meepo config` | Show loaded configuration |
| `meepo mcp-server` | Run as an MCP server over STDIO |
| `meepo template list` | List available agent templates |
| `meepo template use <name>` | Activate a template (overlay on current config) |
| `meepo template info <name>` | Show what a template will change |
| `meepo template reset` | Remove active template, restore previous config |
| `meepo template create <name>` | Create a new template from current config |
| `meepo template remove <name>` | Remove an installed template |
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
trigger_prefix = "/d"                  # Optional prefix filter

[channels.email]
enabled = false                        # macOS only — poll Mail.app
poll_interval_secs = 10
subject_prefix = "[meepo]"            # Only process emails with this prefix

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

[filesystem]
allowed_directories = ["~/Coding"]     # Directories the agent can browse/search

[browser]
enabled = true                         # Enable browser automation tools
default_browser = "safari"             # "safari" or "chrome"

[autonomy]
enabled = true                         # Autonomous observe/think/act loop
tick_interval_secs = 30                # Idle tick rate
max_goals = 50                         # Prevent runaway goal creation
send_acknowledgments = true            # Typing indicators before processing

[notifications]
enabled = false                        # Proactive alerts via iMessage/Discord/Slack
channel = "imessage"                   # "imessage", "discord", "slack", "email"
on_task_start = true
on_task_complete = true
on_task_fail = true
on_watcher_triggered = true
on_autonomous_action = true
on_error = true
# [notifications.quiet_hours]
# start = "23:00"
# end = "08:00"

[mcp.server]
enabled = true                         # Expose tools via MCP (STDIO)
exposed_tools = []                     # Empty = all tools (except delegate_tasks)

# [[mcp.clients]]                      # Connect to external MCP servers
# name = "github"
# command = "npx"
# args = ["-y", "@modelcontextprotocol/server-github"]
# env = [["GITHUB_TOKEN", "${GITHUB_TOKEN}"]]

[a2a]
enabled = false                        # A2A protocol (HTTP)
port = 8081
auth_token = "${A2A_AUTH_TOKEN}"
allowed_tools = []                     # Tools available to incoming A2A tasks

[skills]
enabled = false                        # Import SKILL.md files as tools
dir = "~/.meepo/skills"
```

Environment variables are expanded with `${VAR_NAME}` syntax. Paths support `~/` expansion.

## Tools

Meepo registers 75+ tools that Claude can use during conversations:

| Category | Tools |
|----------|-------|
| **Email & Calendar** | `read_emails`, `send_email`, `read_calendar`, `create_calendar_event` |
| **Reminders & Notes** | `list_reminders`, `create_reminder`, `list_notes`, `create_note` |
| **System Apps** | `open_app`, `get_clipboard`, `send_notification`, `screen_capture`, `search_contacts` |
| **Music** | `get_current_track`, `music_control` |
| **UI Automation** | `read_screen`, `click_element`, `type_text` |
| **Browser** | `browser_list_tabs`, `browser_open_tab`, `browser_close_tab`, `browser_switch_tab`, `browser_get_page_content`, `browser_execute_js`, `browser_click`, `browser_fill_form`, `browser_navigate`, `browser_get_url`, `browser_screenshot` |
| **Code** | `write_code`, `make_pr`, `review_pr`, `spawn_claude_code` |
| **Web** | `web_search`, `browse_url` |
| **Memory** | `remember`, `recall`, `search_knowledge`, `link_entities` |
| **System** | `run_command`, `read_file`, `write_file` |
| **Filesystem** | `list_directory`, `search_files` |
| **Watchers** | `create_watcher`, `list_watchers`, `cancel_watcher` |
| **Autonomous** | `spawn_background_task`, `agent_status`, `stop_task` |
| **Delegation** | `delegate_tasks` |
| **Email Intelligence** | `email_triage`, `email_draft_reply`, `email_summarize_thread`, `email_unsubscribe` |
| **Smart Calendar** | `find_free_time`, `schedule_meeting`, `reschedule_event`, `daily_briefing`, `weekly_review` |
| **Deep Research** | `research_topic`, `compile_report`, `track_topic`, `fact_check` |
| **SMS Autopilot** | `send_sms`, `set_auto_reply`, `message_summary` |
| **Task Manager** | `create_task`, `list_tasks`, `update_task`, `complete_task`, `project_status` |
| **News Curator** | `track_feed`, `untrack_feed`, `summarize_article`, `content_digest` |
| **Finance Tracker** | `log_expense`, `spending_summary`, `budget_check`, `parse_receipt` |
| **Health & Habits** | `log_habit`, `habit_streak`, `habit_report` |
| **Travel Assistant** | `get_weather`, `get_directions`, `flight_status`, `packing_list` |
| **Social Manager** | `relationship_summary`, `suggest_followups` |

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

## Running in a Tart VM

[Tart](https://tart.run) is a virtualization toolset for macOS VMs on Apple Silicon. Running Meepo inside a Tart VM is useful for sandboxed testing, CI, or isolating the agent from your host machine.

### Quick Start

```bash
# On the host — create and boot a macOS VM
tart create meepo-vm --from-oci ghcr.io/cirruslabs/macos-sequoia-base:latest
tart run meepo-vm

# Inside the VM — install and set up Meepo
curl -sSL https://raw.githubusercontent.com/kavymi/meepo/main/install.sh | bash
export ANTHROPIC_API_KEY="sk-ant-..."
meepo setup
meepo start
```

### What works in a Tart VM

| Feature | GUI mode (`tart run`) | Headless (`tart run --no-graphics`) |
|---------|----------------------|-------------------------------------|
| CLI (`meepo ask`, `meepo start`) | ✓ | ✓ |
| Discord / Slack / Email channels | ✓ | ✓ |
| Knowledge graph, watchers, MCP, A2A | ✓ | ✓ |
| iMessage channel | ✓ (requires Apple ID sign-in) | ✓ (requires Apple ID sign-in) |
| Browser automation (Safari/Chrome) | ✓ | ✗ (no display session) |
| Screen capture, UI automation | ✓ | ✗ (no display session) |
| Music control | ✓ | ✗ |

### Permissions in a Tart VM

macOS permissions (Accessibility, Full Disk Access, Automation, Screen Recording) must be granted **inside the VM** just like on a physical Mac. Run `meepo setup` inside the VM — it detects the environment and walks you through each permission.

### Networking

Tart VMs use NAT by default. If you need the A2A server (port 8081) or MCP server accessible from the host:

```bash
# Use softnet for bridged networking
tart run meepo-vm --net-softnet

# Or use Tart's built-in port forwarding (Tart 2.0+)
tart run meepo-vm --rosetta --dir=share:~/shared
```

> **Tip:** The `meepo setup` wizard detects when running inside a VM and shows relevant guidance automatically.

## Troubleshooting

**"API key not set" or empty responses**
- Verify: `echo $ANTHROPIC_API_KEY` — should start with `sk-ant-`
- If using the launch agent, re-run `scripts/install.sh` after setting new env vars (the plist snapshots env vars at install time)

**iMessage not receiving messages**
- Run `meepo setup` — it checks Full Disk Access and opens System Settings for you
- Or manually: System Settings → Privacy & Security → Full Disk Access → add your terminal app
- Check `allowed_contacts` in config includes the sender's phone/email
- You may need to restart your terminal after granting Full Disk Access

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

**macOS permission errors ("not allowed assistive access", "not authorized")**
- Run `meepo setup` — it detects missing permissions and opens the correct System Settings pane
- Accessibility: System Settings → Privacy & Security → Accessibility → add your terminal
- Automation: System Settings → Privacy & Security → Automation → enable apps under your terminal
- Screen Recording: System Settings → Privacy & Security → Screen Recording → add your terminal
- Full Disk Access: System Settings → Privacy & Security → Full Disk Access → add your terminal
- After granting, you may need to restart your terminal

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
│   ├── meepo-core/       # Agent loop, API client, tool system, orchestrator, autonomy
│   ├── meepo-channels/   # Discord, Slack, iMessage, email adapters + message bus
│   ├── meepo-knowledge/  # SQLite + Tantivy knowledge graph
│   ├── meepo-scheduler/  # Watcher runner, persistence, polling
│   ├── meepo-mcp/        # MCP server (STDIO) and client for external MCP servers
│   ├── meepo-a2a/        # A2A (Agent-to-Agent) protocol server and client
│   └── meepo-cli/        # CLI binary, config loading, template system
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
├── CODE_OF_CONDUCT.md     # Community guidelines
├── SECURITY.md            # Vulnerability reporting policy
├── CHANGELOG.md           # Release history
├── LICENSE                # MIT license
├── SOUL.md               # Agent personality template
└── MEMORY.md             # Agent memory template
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, testing, and contribution guidelines.

Please read our [Code of Conduct](CODE_OF_CONDUCT.md) before participating.

## Security

To report a vulnerability, see [SECURITY.md](SECURITY.md). **Do not open a public issue for security vulnerabilities.**

## License

MIT — see [LICENSE](LICENSE) for details.
