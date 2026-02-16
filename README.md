<div align="center">

# Meepo

**A local AI agent that connects LLMs to your digital life.**

[![CI](https://github.com/leancoderkavy/meepo/actions/workflows/ci.yml/badge.svg)](https://github.com/leancoderkavy/meepo/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2024_edition-orange.svg)](https://www.rust-lang.org/)
[![macOS](https://img.shields.io/badge/macOS-supported-brightgreen.svg)](#platform-support)
[![Windows](https://img.shields.io/badge/Windows-supported-brightgreen.svg)](#platform-support)

75+ tools · Discord · Slack · iMessage · Email · MCP · A2A · Autonomous Agent Loop

[Quick Start](#quick-start) · [Features](#features) · [Documentation](#configuration-reference) · [Contributing](CONTRIBUTING.md)

</div>

---

Meepo is an open-source, privacy-first AI agent written in Rust that runs as a daemon on your machine. It connects large language models (Claude, GPT-4, Gemini, Llama, Mistral) to your email, calendar, messages, browser, files, code, and more — all without sending your data to third-party agent platforms.

Think of it as a **local AI assistant that actually does things**: reads your email, schedules meetings, searches the web, automates your browser, monitors GitHub, manages reminders, and talks to you over Discord, Slack, or iMessage — all running on your own hardware.

### Why Meepo?

- **Runs locally** — Your data stays on your machine. No cloud agent platform required.
- **75+ tools out of the box** — Email, calendar, browser automation, web search, code PRs, knowledge graph, and more.
- **Works with any LLM** — Claude, GPT-4o, Gemini, Llama 3, Mistral, or any OpenAI-compatible API. Automatic failover between providers.
- **Always-on daemon** — Autonomous observe/think/act loop with scheduled watchers, proactive notifications, and goal tracking.
- **Interoperable** — Speaks [MCP](https://modelcontextprotocol.io/) and [A2A](https://google.github.io/A2A/) protocols. Plug into Claude Desktop, Cursor, or other AI agents.
- **Extensible** — Add custom tools, import SKILL.md files, swap agent personalities with templates, or connect external MCP servers.
- **Cross-platform** — macOS (AppleScript) and Windows (PowerShell/COM) with a clean platform abstraction layer.

---

## Quick Start

The fastest way to get running:

```bash
# macOS (Homebrew)
brew install leancoderkavy/tap/meepo
meepo setup    # Interactive wizard — walks you through API keys, permissions, and channels

# macOS / Linux (curl)
curl -sSL https://raw.githubusercontent.com/leancoderkavy/meepo/main/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/leancoderkavy/meepo/main/install.ps1 | iex
```

Then start the agent:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."   # or OPENAI_API_KEY, GOOGLE_AI_API_KEY
meepo start
```

> **No API key?** Use [Ollama](https://ollama.ai) for free local models — see [Using Ollama](#using-ollama-local-llms) below.

One-shot mode (no daemon needed):

```bash
meepo ask "What's on my calendar today?"
```

<details>
<summary><strong>More install options (DMG, from source)</strong></summary>

**macOS (DMG):**

Download the latest `.dmg` from the [Releases page](https://github.com/leancoderkavy/meepo/releases), open it, and drag **Meepo.app** to Applications. On first launch, the setup wizard opens automatically. Or build the DMG yourself:

```bash
git clone https://github.com/leancoderkavy/meepo.git && cd meepo
./installer/scripts/build-dmg.sh
open installer/dist/Meepo-*.dmg
```

See [installer/README.md](installer/README.md) for build options (universal binary, custom icon, etc.).

**From source (macOS/Linux):**
```bash
git clone https://github.com/leancoderkavy/meepo.git && cd meepo
cargo build --release && ./target/release/meepo setup
```

**From source (Windows PowerShell):**
```powershell
git clone https://github.com/leancoderkavy/meepo.git; cd meepo
cargo build --release; .\target\release\meepo.exe setup
```

</details>

## Features

| Category | Highlights |
|----------|-----------|
| **Messaging** | Discord, Slack, iMessage (macOS), email (macOS), CLI one-shots |
| **75+ Tools** | Email, calendar, reminders, notes, contacts, browser, web search, files, code PRs, music, screen capture, research, tasks, finance, health, travel, social |
| **Autonomous Loop** | Observe/think/act cycle, goal tracking, proactive notifications, quiet hours |
| **LLM Providers** | Anthropic Claude, OpenAI, Google Gemini, Ollama (local), any OpenAI-compatible endpoint — with automatic failover |
| **Browser Automation** | Safari + Chrome: tabs, navigation, JS execution, form filling, screenshots |
| **Knowledge Graph** | Persistent memory with SQLite + Tantivy full-text search across sessions |
| **Clone Delegation** | Spawn parallel sub-agents for complex tasks; background clones report back when done |
| **Watchers** | Monitor email, calendar, GitHub, files, or run cron tasks on a schedule |
| **MCP** | Expose tools as an MCP server (STDIO) for Claude Desktop / Cursor; consume external MCP servers |
| **A2A Protocol** | Google's Agent-to-Agent protocol for multi-agent task delegation over HTTP |
| **Remote Gateway** | WebSocket + REST server for mobile apps and external clients (Bearer auth, sessions) |
| **iOS App** | Native SwiftUI companion app — real-time chat, sessions, tool indicators |
| **Templates & Skills** | Swap agent personalities; import OpenClaw-compatible SKILL.md files as tools |
| **Security** | Command allowlists, path traversal protection, SSRF blocking, input sanitization, execution timeouts |

## Requirements

- **macOS** or **Windows**
- **At least one LLM provider:**

| Provider | How to get access |
|----------|-------------------|
| Anthropic Claude | API key from [console.anthropic.com](https://console.anthropic.com) |
| OpenAI | API key from [platform.openai.com](https://platform.openai.com/api-keys) |
| Google Gemini | API key from [aistudio.google.com](https://aistudio.google.com/apikey) |
| Ollama (local) | Free — [ollama.ai](https://ollama.ai). No API key needed. |
| OpenAI-compatible | Together, Groq, LM Studio, etc. |

- **Optional:** [Tavily](https://tavily.com) API key (enables `web_search` tool), Discord bot token, Slack bot token
- **Rust toolchain** only needed when building from source

### Using Ollama (Local LLMs)

Run models locally with zero API costs:

```bash
curl -fsSL https://ollama.ai/install.sh | sh   # Install Ollama
ollama pull llama3.2                             # Pull a model
```

Configure `~/.meepo/config.toml`:

```toml
[agent]
default_model = "ollama"

[providers.ollama]
base_url = "http://localhost:11434"
model = "llama3.2"
```

```bash
meepo start   # No API key needed — everything runs on your machine
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `meepo setup` | Interactive setup wizard (API keys, permissions, channels, connection test) |
| `meepo start` | Start the agent daemon |
| `meepo stop` | Stop a running daemon |
| `meepo ask "..."` | One-shot question (no daemon needed) |
| `meepo init` | Create `~/.meepo/` with default config |
| `meepo config` | Show loaded configuration |
| `meepo doctor` | Diagnose common issues |
| `meepo mcp-server` | Run as an MCP server over STDIO |
| `meepo template list\|use\|info\|reset\|create\|remove` | Manage agent templates |
| `meepo --debug <cmd>` | Enable debug logging |
| `meepo --config <path> <cmd>` | Use custom config file |

## Channels

Meepo monitors multiple messaging platforms simultaneously. Enable them in `~/.meepo/config.toml` or via `meepo setup`:

| Channel | Config Key | Requirements |
|---------|-----------|--------------|
| **Discord** | `[channels.discord]` | Bot token + `MESSAGE_CONTENT` intent ([Developer Portal](https://discord.com/developers/applications)) |
| **Slack** | `[channels.slack]` | Bot token with `chat:write`, `channels:read`, `im:history` ([api.slack.com](https://api.slack.com/apps)) |
| **iMessage** | `[channels.imessage]` | macOS only. Full Disk Access permission. No API key. |
| **Email** | `[channels.email]` | macOS only. Polls Mail.app with subject prefix filtering. |
| **CLI** | `meepo ask "..."` | Works everywhere, no setup needed. |

<details>
<summary><strong>Channel configuration examples</strong></summary>

```toml
[channels.discord]
enabled = true
token = "${DISCORD_BOT_TOKEN}"
allowed_users = ["123456789012345678"]

[channels.slack]
enabled = true
bot_token = "${SLACK_BOT_TOKEN}"
poll_interval_secs = 3

[channels.imessage]
enabled = true
allowed_contacts = ["+15551234567", "user@icloud.com"]
poll_interval_secs = 3

[channels.email]
enabled = true
poll_interval_secs = 10
subject_prefix = "[meepo]"
```

</details>

## Tools

Meepo ships with 75+ tools the LLM can invoke during conversations:

<details>
<summary><strong>Full tool list</strong></summary>

| Category | Tools |
|----------|-------|
| **Email & Calendar** | `read_emails`, `send_email`, `read_calendar`, `create_calendar_event` |
| **Reminders & Notes** | `list_reminders`, `create_reminder`, `list_notes`, `create_note` |
| **System Apps** | `open_app`, `get_clipboard`, `send_notification`, `screen_capture`, `search_contacts` |
| **Music** | `get_current_track`, `music_control` |
| **UI Automation** | `read_screen`, `click_element`, `type_text` |
| **Browser** | `browser_list_tabs`, `browser_open_tab`, `browser_close_tab`, `browser_switch_tab`, `browser_get_page_content`, `browser_execute_js`, `browser_click`, `browser_fill_form`, `browser_navigate`, `browser_get_url`, `browser_screenshot` |
| **Code** | `write_code`, `make_pr`, `review_pr`, `spawn_coding_agent` |
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

</details>

## Configuration Reference

<details>
<summary><strong>Full <code>~/.meepo/config.toml</code> reference</strong></summary>

```toml
[agent]
default_model = "claude-sonnet-4-20250514"  # or gpt-4o, gemini-2.0-flash, ollama
max_tokens = 8192                           # Max response tokens

[providers.anthropic]                       # Optional — Anthropic Claude
api_key = "${ANTHROPIC_API_KEY}"
base_url = "https://api.anthropic.com"

# [providers.openai]                        # Optional — OpenAI
# api_key = "${OPENAI_API_KEY}"
# model = "gpt-4o"

# [providers.ollama]                        # Optional — local Ollama
# base_url = "http://localhost:11434"
# model = "llama3.2"

[providers.tavily]
api_key = "${TAVILY_API_KEY}"               # Optional — enables web_search tool

[channels.discord]
enabled = false
token = "${DISCORD_BOT_TOKEN}"
allowed_users = []                     # Discord user IDs (strings)

[channels.slack]
enabled = false
bot_token = "${SLACK_BOT_TOKEN}"
poll_interval_secs = 3

[channels.imessage]
enabled = false
allowed_contacts = []                  # Phone numbers or emails
poll_interval_secs = 3
trigger_prefix = "/d"                  # Optional prefix filter

[channels.email]
enabled = false                        # macOS only — poll Mail.app
poll_interval_secs = 10
subject_prefix = "[meepo]"

[knowledge]
db_path = "~/.meepo/knowledge.db"
tantivy_path = "~/.meepo/tantivy_index"

[watchers]
max_concurrent = 50
min_poll_interval_secs = 30
active_hours = { start = "08:00", end = "23:00" }

[orchestrator]
max_concurrent_subtasks = 5
max_subtasks_per_request = 10
parallel_timeout_secs = 120
background_timeout_secs = 600
max_background_groups = 3

[code]
coding_agent_path = "claude"           # claude, aider, codex
gh_path = "gh"
default_workspace = "~/Coding"

[memory]
workspace = "~/.meepo/workspace"       # Contains SOUL.md and MEMORY.md

[filesystem]
allowed_directories = ["~/Coding"]     # Sandboxed file access

[browser]
enabled = true
default_browser = "safari"             # "safari" or "chrome"

[autonomy]
enabled = true
tick_interval_secs = 30
max_goals = 50
send_acknowledgments = true

[notifications]
enabled = false
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
enabled = true
exposed_tools = []                     # Empty = all tools (except delegate_tasks)

# [[mcp.clients]]                      # Connect to external MCP servers
# name = "github"
# command = "npx"
# args = ["-y", "@modelcontextprotocol/server-github"]
# env = [["GITHUB_TOKEN", "${GITHUB_TOKEN}"]]

[gateway]
enabled = false
bind = "127.0.0.1"                     # Use 0.0.0.0 for LAN access
port = 18789
auth_token = "${MEEPO_GATEWAY_TOKEN}"

[a2a]
enabled = false
port = 8081
auth_token = "${A2A_AUTH_TOKEN}"
allowed_tools = []

[skills]
enabled = false
dir = "~/.meepo/skills"
```

Environment variables are expanded with `${VAR_NAME}` syntax. Paths support `~/` expansion.

</details>

## Remote Gateway

The gateway is a WebSocket + REST server for remote access from mobile apps and external clients.

```toml
[gateway]
enabled = true
bind = "0.0.0.0"    # LAN access (use 127.0.0.1 for local only)
port = 18789
auth_token = "${MEEPO_GATEWAY_TOKEN}"
```

| Endpoint | Protocol | Description |
|----------|----------|-------------|
| `/ws` | WebSocket | Real-time chat, events, typing indicators (JSON-RPC) |
| `/api/status` | REST GET | Agent health check |
| `/api/sessions` | REST GET | List active sessions |

<details>
<summary><strong>WebSocket JSON-RPC methods & events</strong></summary>

**Methods:**

| Method | Description |
|--------|-------------|
| `message.send` | Send a chat message to the agent |
| `session.list` | List all sessions |
| `session.new` | Create a new session |
| `session.history` | Get message history for a session |
| `status.get` | Get agent status |

**Events (server → client):**

| Event | Description |
|-------|-------------|
| `message.received` | Agent response or incoming message |
| `typing.start` / `typing.stop` | Typing indicators |
| `tool.executing` | Tool execution in progress |
| `session.created` | New session created |

</details>

**Networking tips:**
- **iOS Simulator:** `127.0.0.1` works (shares Mac's network stack)
- **Physical iPhone:** Use `bind = "0.0.0.0"` and your Mac's LAN IP in the app
- **Firewall:** Ensure port 18789 is open for cross-device access

## iOS Companion App

A native SwiftUI app that connects to the gateway for mobile access to your agent.

```bash
brew install xcodegen
cd MeepoApp && xcodegen generate
open MeepoApp.xcodeproj
# Select your device → ⌘R
```

See the **[iOS Setup Guide](docs/IOS_SETUP_GUIDE.md)** for networking, troubleshooting, and physical device configuration.

## Platform Support

<details>
<summary><strong>macOS vs Windows feature matrix</strong></summary>

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
| Clipboard | `arboard` (cross-platform) | `arboard` (cross-platform) |
| App launching | `open` (cross-platform) | `open` (cross-platform) |
| UI automation | System Events (AppleScript) | System.Windows.Automation (PowerShell) |
| Browser automation | Safari + Chrome (AppleScript) | Not yet available |
| iMessage channel | Messages.app (SQLite + AppleScript) | Not available |
| Email channel | Mail.app polling | Not available |
| Background service | `launchd` agent | Windows Task Scheduler |

</details>

<details>
<summary><strong>macOS permissions</strong></summary>

The `meepo setup` wizard handles all of these automatically. You can also grant them manually:

| Permission | Required For | System Settings Path |
|------------|-------------|---------------------|
| **Accessibility** | UI automation (`read_screen`, `click_element`, `type_text`) | Privacy & Security → Accessibility |
| **Full Disk Access** | iMessage channel | Privacy & Security → Full Disk Access |
| **Automation** | Email, Calendar, Reminders, Notes, Messages, Music | Privacy & Security → Automation |
| **Screen Recording** | `screen_capture` tool | Privacy & Security → Screen Recording |

Grant each to your terminal app. Re-run `meepo setup` if a tool fails with a permission error.

</details>

## Running as a Background Service

```bash
# macOS (Homebrew — recommended)
brew services start meepo    # Start and enable on login
brew services stop meepo
brew services restart meepo
# Logs: $(brew --prefix)/var/log/meepo/meepo.log

# macOS (manual launchd)
scripts/install.sh           # Install and start
scripts/uninstall.sh         # Remove
# Logs: ~/.meepo/logs/meepo.out.log
```

```powershell
# Windows (scheduled task — starts on login, auto-restarts)
scripts\install.ps1          # Install (requires Administrator)
scripts\uninstall.ps1        # Remove
```

<details>
<summary><strong>Running in a Tart VM</strong></summary>

[Tart](https://tart.run) lets you run macOS VMs on Apple Silicon for sandboxed testing or CI.

```bash
tart create meepo-vm --from-oci ghcr.io/cirruslabs/macos-sequoia-base:latest
tart run meepo-vm

# Inside the VM:
curl -sSL https://raw.githubusercontent.com/leancoderkavy/meepo/main/install.sh | bash
export ANTHROPIC_API_KEY="sk-ant-..."
meepo setup && meepo start
```

| Feature | GUI mode | Headless |
|---------|----------|----------|
| CLI, Discord, Slack, Email, MCP, A2A, Knowledge graph | ✓ | ✓ |
| iMessage | ✓ (Apple ID required) | ✓ (Apple ID required) |
| Browser, Screen capture, UI automation, Music | ✓ | ✗ |

Use `tart run meepo-vm --net-softnet` for bridged networking (A2A/gateway access from host).

</details>

## Architecture

```
crates/
├── meepo-cli/        # Binary, CLI commands, daemon startup, config, templates
├── meepo-core/       # Agent loop, API client, 75+ tools, orchestrator, autonomy
├── meepo-channels/   # Discord, Slack, iMessage, email adapters + message bus
├── meepo-knowledge/  # SQLite + Tantivy knowledge graph
├── meepo-scheduler/  # Watcher runner, persistence, polling
├── meepo-mcp/        # MCP server (STDIO) + client for external MCP servers
├── meepo-a2a/        # A2A protocol server + client (HTTP)
└── meepo-gateway/    # Remote gateway (WebSocket + REST, Axum)
```

See [docs/architecture.md](docs/architecture.md) for detailed diagrams.

## Troubleshooting

<details>
<summary><strong>Common issues and solutions</strong></summary>

**"No LLM provider configured" or empty responses**
- Verify at least one provider key is set: `echo $ANTHROPIC_API_KEY` or `echo $OPENAI_API_KEY`
- If using the launch agent, re-run `scripts/install.sh` after setting new env vars

**iMessage not receiving messages**
- Run `meepo setup` — it checks Full Disk Access and opens System Settings for you
- Check `allowed_contacts` in config includes the sender's phone/email
- Restart your terminal after granting Full Disk Access

**`web_search` tool not available**
- Set `TAVILY_API_KEY` — the tool is only registered when a valid key is configured

**Discord bot not responding**
- Enable `MESSAGE CONTENT INTENT` in the Developer Portal (Bot → Privileged Gateway Intents)
- Verify `allowed_users` contains your Discord user ID

**Build failures**
- Update Rust: `rustup update`
- Clean build: `cargo clean && cargo build --release`
- Windows: Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with "Desktop development with C++"

**macOS permission errors**
- Run `meepo setup` — it detects missing permissions and opens the correct System Settings pane
- After granting permissions, restart your terminal

**Windows: API key not persisting**
- Use `[Environment]::SetEnvironmentVariable("ANTHROPIC_API_KEY", "sk-ant-...", "User")` to persist, then restart terminal

</details>

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, testing, and contribution guidelines.

```bash
git clone https://github.com/leancoderkavy/meepo.git && cd meepo
cargo build && cargo test --workspace && cargo clippy --workspace
```

Please read our [Code of Conduct](CODE_OF_CONDUCT.md) before participating.

## Security

To report a vulnerability, see [SECURITY.md](SECURITY.md). **Do not open a public issue for security vulnerabilities.**

## License

MIT — see [LICENSE](LICENSE) for details.
