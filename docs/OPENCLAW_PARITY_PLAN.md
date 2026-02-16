# OpenClaw Feature Parity Plan

Comprehensive plan to implement OpenClaw-equivalent features in Meepo (excluding channel support).
Organized into 11 phases, ordered by dependency and impact.

---

## Phase 1: Multi-Model Support + Failover (HIGH PRIORITY)

**Goal**: Support Anthropic, OpenAI, Google, and arbitrary OpenAI-compatible endpoints with automatic failover.

Currently `ApiClient` in `meepo-core/src/api.rs` is hardcoded to Anthropic's `/v1/messages` endpoint with `x-api-key` auth. This phase abstracts the LLM layer.

### 1.1 — Provider Trait (`meepo-core/src/providers/mod.rs`)

Create a new `providers` module with a trait that all backends implement:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn model(&self) -> &str;
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        system: &str,
    ) -> Result<ChatResponse>;
}
```

- `ChatMessage` / `ChatResponse` are provider-agnostic types (mapped from/to each provider's wire format)
- Each provider converts internally to its own API format

### 1.2 — Provider Implementations

| File | Provider | Auth | Wire Format |
|------|----------|------|-------------|
| `providers/anthropic.rs` | Anthropic (Claude) | `x-api-key` header | `/v1/messages` (existing logic extracted) |
| `providers/openai.rs` | OpenAI (GPT-4o, o3, etc.) | `Bearer` token | `/v1/chat/completions` with `tool_choice` |
| `providers/google.rs` | Google (Gemini) | `Bearer` token | Gemini API format |
| `providers/openai_compat.rs` | Any OpenAI-compatible (Ollama, Together, Groq, etc.) | Configurable | Same as OpenAI with custom `base_url` |

Each implementation:
- Maps `ChatMessage` ↔ provider-specific message format
- Maps `ToolDefinition` ↔ provider-specific tool/function format
- Maps response back to `ChatResponse` (text blocks + tool calls)
- Handles provider-specific errors and rate limits

### 1.3 — Model Router (`providers/router.rs`)

```rust
pub struct ModelRouter {
    providers: Vec<Box<dyn LlmProvider>>,
    fallback_order: Vec<String>,  // e.g. ["anthropic/claude-opus-4-6", "openai/gpt-4o"]
    retry_config: RetryConfig,
}
```

- On failure (429, 500, 503, timeout), automatically tries the next provider in `fallback_order`
- Exponential backoff per provider
- Logs failover events via `tracing`

### 1.4 — Refactor `ApiClient` → Use `ModelRouter`

- `ApiClient::chat()` delegates to `ModelRouter` instead of making direct HTTP calls
- `run_tool_loop()` stays the same — it already works with generic messages/tools
- `Agent` and `AutonomousLoop` are unchanged (they only see `ApiClient`)

### 1.5 — Config Changes (`config.rs` + `default.toml`)

```toml
[providers]
default_provider = "anthropic"
fallback_order = ["anthropic/claude-opus-4-6", "openai/gpt-4o"]

[providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"
base_url = "https://api.anthropic.com"

[providers.openai]
api_key = "${OPENAI_API_KEY}"
base_url = "https://api.openai.com"

[providers.google]
api_key = "${GOOGLE_AI_API_KEY}"

[providers.openai_compat]
api_key = "${CUSTOM_API_KEY}"
base_url = "http://localhost:11434/v1"  # e.g. Ollama
model = "llama3"
```

### 1.6 — Tests

- Unit tests for each provider's message/tool format mapping
- Integration test for `ModelRouter` failover (mock HTTP responses)
- Test that existing Anthropic-only config still works (backward compat)

### Files to create/modify:
- **Create**: `meepo-core/src/providers/mod.rs`, `anthropic.rs`, `openai.rs`, `google.rs`, `openai_compat.rs`, `router.rs`, `types.rs`
- **Modify**: `meepo-core/src/api.rs` (delegate to router), `meepo-core/src/lib.rs` (add `pub mod providers`), `config.rs`, `default.toml`, `main.rs` (construct router from config)

---

## Phase 2: Remote Gateway — WebSocket Control Plane (HIGH PRIORITY)

**Goal**: Let Meepo run headless on a server with clients connecting over WebSocket. This is the foundation for WebChat, Canvas, companion apps, and multi-agent routing.

### 2.1 — New Crate: `meepo-gateway`

Add `crates/meepo-gateway` to the workspace. This crate owns the WS server and session management.

```
meepo-gateway/
├── src/
│   ├── lib.rs
│   ├── server.rs       # Axum + tokio-tungstenite WS server
│   ├── session.rs      # Session model (id, channel, agent state, history)
│   ├── protocol.rs     # JSON-RPC-like message protocol
│   ├── auth.rs         # Token auth + optional Tailscale identity
│   └── events.rs       # Event bus (broadcast channel for all connected clients)
```

### 2.2 — Protocol Design (`protocol.rs`)

JSON messages over WebSocket:

```json
// Client → Gateway
{"method": "message.send", "params": {"content": "Hello", "session_id": "main"}}
{"method": "session.list", "params": {}}
{"method": "session.new", "params": {"name": "research"}}
{"method": "session.history", "params": {"session_id": "main", "limit": 50}}
{"method": "status.get", "params": {}}

// Gateway → Client
{"event": "message.received", "data": {"content": "...", "session_id": "main"}}
{"event": "typing.start", "data": {"session_id": "main"}}
{"event": "typing.stop", "data": {"session_id": "main"}}
{"event": "tool.executing", "data": {"tool": "web_search", "session_id": "main"}}
{"event": "status.update", "data": {"sessions": [...], "uptime": 3600}}
```

### 2.3 — Server (`server.rs`)

- Axum HTTP server on configurable `bind` address (default `127.0.0.1:18789`)
- WebSocket upgrade at `/ws`
- Static file serving at `/` (for WebChat UI later)
- REST endpoints: `GET /api/status`, `GET /api/sessions`
- Auth: bearer token from config, validated on WS upgrade

### 2.4 — Session Model (`session.rs`)

```rust
pub struct Session {
    pub id: String,
    pub name: String,
    pub channel: ChannelType,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub message_count: u64,
}

pub struct SessionManager {
    sessions: DashMap<String, Session>,
    agent: Arc<Agent>,
}
```

- Each session maintains its own conversation history (stored in KnowledgeDb)
- `main` session is the default (like OpenClaw)
- Sessions can be created/destroyed via WS protocol

### 2.5 — Integration with Existing Architecture

- `cmd_start()` in `main.rs` optionally starts the Gateway alongside the existing channel listeners
- Channel adapters (Discord, Slack, etc.) route messages through the Gateway's session manager
- The autonomous loop feeds events into the Gateway's event bus
- CLI `meepo message` command can connect via WS instead of direct function call

### 2.6 — Config

```toml
[gateway]
enabled = true
bind = "127.0.0.1:18789"
auth_token = "${MEEPO_GATEWAY_TOKEN}"

[gateway.tailscale]
mode = "off"  # off | serve | funnel
```

### Files to create/modify:
- **Create**: entire `crates/meepo-gateway/` crate
- **Modify**: `Cargo.toml` (workspace member), `main.rs` (start gateway), `config.rs` (gateway config), `default.toml`

---

## Phase 3: WebChat UI (MEDIUM PRIORITY)

**Goal**: Built-in web chat interface served from the Gateway, like OpenClaw's WebChat.

*Depends on: Phase 2 (Gateway)*

### 3.1 — Frontend (`meepo-gateway/ui/`)

A lightweight React (or Preact for size) SPA:

```
ui/
├── src/
│   ├── App.tsx          # Main chat interface
│   ├── components/
│   │   ├── ChatMessage.tsx
│   │   ├── ChatInput.tsx
│   │   ├── SessionSidebar.tsx
│   │   ├── StatusBar.tsx
│   │   └── ToolIndicator.tsx
│   ├── hooks/
│   │   └── useWebSocket.ts
│   └── styles/
│       └── globals.css   # Tailwind
├── index.html
├── package.json
└── vite.config.ts
```

Features:
- Real-time chat via WebSocket (reuses Gateway protocol)
- Session switching sidebar
- Typing/tool-execution indicators
- Markdown rendering for responses
- Mobile-responsive
- Dark/light theme

### 3.2 — Static Serving

- Build step: `pnpm build` produces `dist/` with static assets
- Gateway's Axum server serves `dist/` at `/` (embedded via `include_dir` or served from filesystem)
- Option to embed at compile time (via `rust-embed`) for single-binary distribution

### 3.3 — Build Integration

- `scripts/build-ui.sh` — builds the UI and copies to a known location
- `cargo build --features webchat` — embeds the UI into the binary
- Without the feature flag, serves from `~/.meepo/ui/` if present

### Files to create:
- **Create**: `crates/meepo-gateway/ui/` (entire frontend)
- **Modify**: `meepo-gateway/src/server.rs` (static file serving)

---

## Phase 4: Live Canvas / A2UI (MEDIUM PRIORITY)

**Goal**: Agent-driven visual workspace — the agent can push HTML/React content to a canvas that renders in the WebChat or companion app.

*Depends on: Phase 2 (Gateway), Phase 3 (WebChat)*

### 4.1 — Canvas Protocol

Extend the Gateway WS protocol:

```json
// Gateway → Client
{"event": "canvas.push", "data": {"html": "<div>...</div>", "session_id": "main"}}
{"event": "canvas.reset", "data": {"session_id": "main"}}
{"event": "canvas.eval", "data": {"js": "document.getElementById('chart').update(data)", "session_id": "main"}}
{"event": "canvas.snapshot", "data": {"session_id": "main"}}

// Client → Gateway (snapshot response)
{"method": "canvas.snapshot.result", "params": {"image_base64": "...", "session_id": "main"}}
```

### 4.2 — Canvas Tools (`meepo-core/src/tools/canvas.rs`)

```rust
// Tools the agent can call:
- canvas_push    — Push HTML/Markdown/React content to the canvas
- canvas_reset   — Clear the canvas
- canvas_eval    — Execute JS in the canvas context
- canvas_snapshot — Request a screenshot of the current canvas state
```

### 4.3 — WebChat Canvas Component

- Sandboxed iframe in the WebChat UI that renders pushed content
- Supports HTML, Markdown, Mermaid diagrams, charts (Chart.js), code blocks
- Agent can iteratively update the canvas (like a whiteboard)

### Files to create/modify:
- **Create**: `meepo-core/src/tools/canvas.rs`, WebChat canvas components
- **Modify**: `meepo-gateway/src/protocol.rs`, `meepo-core/src/tools/mod.rs`, `main.rs` (register tools)

---

## Phase 5: Voice / Talk Mode (MEDIUM PRIORITY)

**Goal**: Speech input (STT) and output (TTS) for hands-free interaction.

### 5.1 — Audio Pipeline (`meepo-core/src/audio/`)

```
audio/
├── mod.rs
├── stt.rs      # Speech-to-text (Whisper API or local whisper.cpp)
├── tts.rs      # Text-to-speech (ElevenLabs API or local TTS)
├── vad.rs      # Voice activity detection (silero-vad or simple energy-based)
└── stream.rs   # Audio capture/playback via cpal crate
```

### 5.2 — STT Options

| Backend | Latency | Quality | Offline |
|---------|---------|---------|---------|
| OpenAI Whisper API | ~1-2s | Excellent | No |
| Local whisper.cpp (via whisper-rs) | ~0.5-3s | Good | Yes |

- Default: Whisper API (requires OpenAI key or dedicated Whisper endpoint)
- Optional: local whisper.cpp for offline use

### 5.3 — TTS Options

| Backend | Latency | Quality | Offline |
|---------|---------|---------|---------|
| ElevenLabs API | ~0.5-1s | Excellent | No |
| macOS `say` command | Instant | Basic | Yes |
| OpenAI TTS API | ~1s | Good | No |

### 5.4 — Talk Mode

- `meepo talk` CLI command — enters continuous conversation mode
- Uses `cpal` crate for audio capture/playback
- VAD detects speech start/end → STT → Agent → TTS → playback
- Gateway protocol supports audio streaming for remote talk mode

### 5.5 — Voice Wake (macOS)

- Optional always-on listening for a wake word (e.g., "Hey Meepo")
- Uses a lightweight local model (Porcupine or custom keyword spotter)
- On detection, activates Talk Mode for one interaction

### 5.6 — Config

```toml
[voice]
enabled = false
stt_provider = "whisper_api"  # whisper_api | whisper_local
tts_provider = "elevenlabs"   # elevenlabs | macos_say | openai_tts
elevenlabs_api_key = "${ELEVENLABS_API_KEY}"
elevenlabs_voice_id = "default"
wake_word = "hey meepo"
wake_enabled = false
```

### Files to create/modify:
- **Create**: `meepo-core/src/audio/` module, `meepo-cli` talk command
- **Modify**: `config.rs`, `default.toml`, `main.rs`

---

## Phase 6: Multi-Agent Routing (MEDIUM PRIORITY)

**Goal**: Route inbound channels/accounts to isolated agents with per-agent sessions, workspaces, and tool sets.

*Depends on: Phase 2 (Gateway)*

### 6.1 — Agent Profiles (`meepo-core/src/agents/`)

```rust
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub model: String,
    pub soul_file: PathBuf,
    pub memory_file: PathBuf,
    pub workspace: PathBuf,
    pub tools: Vec<String>,       // allowlist (empty = all)
    pub denied_tools: Vec<String>, // denylist
    pub channels: Vec<ChannelRoute>,
}

pub struct ChannelRoute {
    pub channel_type: ChannelType,
    pub filter: RouteFilter,  // e.g. specific Discord server, Slack workspace, sender allowlist
}
```

### 6.2 — Agent Manager

```rust
pub struct AgentManager {
    agents: HashMap<String, Arc<Agent>>,
    routes: Vec<(ChannelRoute, String)>,  // route → agent_id
    default_agent: String,
}
```

- Routes incoming messages to the correct agent based on channel + sender
- Each agent has its own session history, knowledge DB partition, and tool set
- Default agent handles unrouted messages

### 6.3 — Config

```toml
[agents.default]
model = "anthropic/claude-opus-4-6"
soul = "SOUL.md"

[agents.work]
model = "anthropic/claude-sonnet-4-20250514"
soul = "SOUL_WORK.md"
workspace = "~/.meepo/workspaces/work"
channels = [{ type = "slack", workspace = "mycompany" }]
tools = ["email", "calendar", "code", "filesystem"]

[agents.personal]
model = "anthropic/claude-opus-4-6"
soul = "SOUL_PERSONAL.md"
channels = [{ type = "discord" }, { type = "imessage" }]
```

### Files to create/modify:
- **Create**: `meepo-core/src/agents/mod.rs`, `manager.rs`, `profile.rs`
- **Modify**: `config.rs`, `default.toml`, `main.rs`, `meepo-gateway/src/session.rs`

---

## Phase 7: Docker Sandboxing (MEDIUM PRIORITY)

**Goal**: Run tool execution in per-session Docker containers for isolation, especially for group/channel sessions.

### 7.1 — Sandbox Module (`meepo-core/src/sandbox/`)

```rust
pub struct DockerSandbox {
    container_id: String,
    session_id: String,
    allowed_tools: HashSet<String>,
    denied_tools: HashSet<String>,
    workspace_mount: PathBuf,
}

#[async_trait]
impl ToolExecutor for DockerSandbox {
    async fn execute(&self, tool_name: &str, input: Value) -> Result<String> {
        if self.denied_tools.contains(tool_name) {
            return Err(anyhow!("Tool '{}' is denied in this sandbox", tool_name));
        }
        // Execute via docker exec or a thin RPC agent inside the container
    }
}
```

### 7.2 — Sandbox Modes

| Mode | Description |
|------|-------------|
| `off` | No sandboxing (current behavior) |
| `non-main` | Sandbox all sessions except `main` (like OpenClaw) |
| `all` | Sandbox everything including main |
| `per-agent` | Sandbox based on agent profile config |

### 7.3 — Container Lifecycle

- On session creation: `docker run` a lightweight container with Meepo's tool executor
- Mount workspace directory read-only (or read-write per config)
- Container has network access disabled by default
- On session end: `docker stop && docker rm`
- Reuse containers for the same session (keep warm)

### 7.4 — Config

```toml
[sandbox]
mode = "off"  # off | non-main | all | per-agent
image = "meepo-sandbox:latest"
network = false
workspace_readonly = true
allowed_tools = ["read", "write", "edit", "search", "filesystem"]
denied_tools = ["browser", "system_command"]
```

### Files to create/modify:
- **Create**: `meepo-core/src/sandbox/mod.rs`, `docker.rs`, `Dockerfile.sandbox`
- **Modify**: `config.rs`, `default.toml`, `main.rs`, `meepo-gateway/src/session.rs`

---

## Phase 8: Companion macOS Menu Bar App (LOW PRIORITY)

**Goal**: Native macOS menu bar app for quick access, status, Voice Wake, and Talk Mode.

*Depends on: Phase 2 (Gateway), Phase 5 (Voice)*

### 8.1 — Separate Swift Project

```
apps/macos/
├── MeepoApp/
│   ├── MeepoApp.swift         # @main App with MenuBarExtra
│   ├── GatewayClient.swift    # WebSocket client to Gateway
│   ├── StatusView.swift       # Menu bar popover (sessions, usage, status)
│   ├── QuickChatView.swift    # Inline chat input
│   ├── VoiceWakeManager.swift # Always-on wake word detection
│   ├── TalkModeView.swift     # Overlay for Talk Mode
│   └── Preferences.swift      # Settings (gateway URL, wake word, etc.)
├── MeepoApp.xcodeproj
└── README.md
```

### 8.2 — Features

- **Menu bar icon**: Shows agent status (idle/thinking/error)
- **Quick chat**: Type a message without opening a full window
- **Session list**: Switch between sessions
- **Usage display**: Token count, cost for current period
- **Voice Wake**: Background audio monitoring for wake word
- **Talk Mode**: Floating overlay for continuous speech conversation
- **Notifications**: Native macOS notifications for agent alerts

### 8.3 — Gateway Connection

- Connects to `ws://127.0.0.1:18789/ws` (or remote via Tailscale)
- Uses the same WS protocol as WebChat
- Handles reconnection, auth token from Keychain

---

## Phase 9: Onboarding Wizard (LOW PRIORITY)

**Goal**: Interactive `meepo onboard` command that guides through complete setup.

### 9.1 — Wizard Flow

The existing `meepo setup` covers API keys and macOS permissions. Extend it to a full onboarding:

```
meepo onboard
  1. Welcome + platform detection
  2. Provider setup (Anthropic key, optional OpenAI/Google keys)
  3. Model selection (with test call)
  4. Channel setup wizard
     - Discord: bot token, server selection, DM policy
     - Slack: bot token, workspace
     - iMessage: permission check
     - Email: account selection
  5. Gateway setup
     - Enable/disable
     - Port selection
     - Auth token generation
     - Optional Tailscale configuration
  6. Voice setup (optional)
     - STT/TTS provider selection
     - Microphone test
     - Wake word configuration
  7. Security review
     - Allowed directories
     - Sandbox mode
     - DM policy review
  8. Install daemon (launchd/systemd)
  9. Verification (send test message through each channel)
 10. Summary + next steps
```

### 9.2 — Implementation

- Extend `cmd_setup()` in `main.rs` or add new `cmd_onboard()`
- Use `dialoguer` crate for interactive prompts (already used in setup)
- Each step is idempotent — can re-run to fix individual sections
- `meepo doctor` command to diagnose issues post-setup (like `openclaw doctor`)

### Files to modify:
- **Modify**: `main.rs` (add `onboard` and `doctor` subcommands)

---

## Phase 10: Skills Registry / MeepoHub (LOW PRIORITY)

**Goal**: Searchable skill registry for discovering, installing, and managing community skills.

### 10.1 — Registry API

A simple HTTP API (could be hosted on GitHub Pages or a small server):

```
GET  /api/skills                    # List all skills
GET  /api/skills?q=<query>          # Search skills
GET  /api/skills/:id                # Get skill details
GET  /api/skills/:id/download       # Download SKILL.md + assets
POST /api/skills                    # Publish a skill (authenticated)
```

### 10.2 — CLI Commands

```bash
meepo skills search "github automation"
meepo skills install <skill-name>
meepo skills update
meepo skills list
meepo skills remove <skill-name>
meepo skills publish <path>
```

### 10.3 — Skill Format

Compatible with OpenClaw's SKILL.md format (already partially supported):

```
~/.meepo/skills/
├── github-pr-review/
│   ├── SKILL.md          # Skill definition (prompt + tool descriptions)
│   ├── skill.toml        # Metadata (name, version, author, dependencies)
│   └── assets/           # Optional supporting files
├── daily-standup/
│   ├── SKILL.md
│   └── skill.toml
```

### 10.4 — Auto-Discovery

Like OpenClaw's ClawHub: when the agent encounters a task it can't handle, it can search the registry and suggest installing a relevant skill.

### Files to create/modify:
- **Create**: `meepo-core/src/skills/registry.rs`, CLI commands
- **Modify**: `main.rs`, existing skills module

---

## Phase 11: Companion iOS/Android Nodes (LOW PRIORITY)

**Goal**: Mobile device nodes that pair with the Gateway for camera, location, notifications, and Talk Mode.

*Depends on: Phase 2 (Gateway), Phase 5 (Voice)*

### 11.1 — Node Protocol

Extend the Gateway WS protocol for device capabilities:

```json
// Node → Gateway (registration)
{"method": "node.register", "params": {
  "platform": "ios",
  "capabilities": ["camera", "location", "notifications", "talk", "screen_record"],
  "permissions": {"camera": "granted", "location": "granted", "notifications": "denied"}
}}

// Gateway → Node (invoke action)
{"method": "node.invoke", "params": {
  "action": "camera.snap",
  "args": {"quality": "high"}
}}

// Node → Gateway (result)
{"event": "node.result", "data": {
  "action": "camera.snap",
  "result": {"image_base64": "..."}
}}
```

### 11.2 — iOS App (Swift)

```
apps/ios/
├── MeepoNode/
│   ├── MeepoNodeApp.swift
│   ├── GatewayClient.swift     # WS connection + Bonjour discovery
│   ├── CameraManager.swift     # Photo/video capture
│   ├── LocationManager.swift   # GPS location
│   ├── NotificationManager.swift
│   ├── TalkModeView.swift      # Voice conversation UI
│   └── CanvasView.swift        # Render agent-pushed content
```

### 11.3 — Android App (Kotlin)

```
apps/android/
├── app/src/main/java/ai/meepo/node/
│   ├── MainActivity.kt
│   ├── GatewayClient.kt
│   ├── CameraManager.kt
│   ├── LocationManager.kt
│   ├── NotificationManager.kt
│   ├── TalkModeActivity.kt
│   └── CanvasActivity.kt
```

### 11.4 — Node Tools

When a node is connected, the Gateway registers device-specific tools:

```rust
// Auto-registered when an iOS/Android node connects:
- node_camera_snap     — Take a photo
- node_camera_clip     — Record a short video
- node_location_get    — Get current GPS coordinates
- node_notify          — Send a push notification to the device
- node_screen_record   — Record the device screen
```

---

## Dependency Graph

```
Phase 1: Multi-Model (standalone)
Phase 2: Gateway (standalone)
  ├── Phase 3: WebChat (needs Gateway)
  │   └── Phase 4: Canvas (needs WebChat)
  ├── Phase 6: Multi-Agent (needs Gateway)
  ├── Phase 7: Docker Sandbox (needs Gateway sessions)
  ├── Phase 8: macOS App (needs Gateway + Voice)
  └── Phase 11: Mobile Nodes (needs Gateway + Voice)
Phase 5: Voice (standalone, but enhanced by Gateway)
Phase 9: Onboarding Wizard (standalone, but covers all features)
Phase 10: Skills Registry (standalone)
```

## Recommended Execution Order

1. **Phase 1**: Multi-Model — unblocks OpenAI/Gemini users, biggest reach impact
2. **Phase 2**: Gateway — foundation for everything UI/remote
3. **Phase 5**: Voice — standalone, high user value
4. **Phase 3**: WebChat — first visible UI on top of Gateway
5. **Phase 4**: Canvas — extends WebChat
6. **Phase 9**: Onboarding — improves setup experience
7. **Phase 6**: Multi-Agent — power user feature
8. **Phase 7**: Docker Sandbox — security for multi-user
9. **Phase 10**: Skills Registry — community growth
10. **Phase 8**: macOS App — polish
11. **Phase 11**: Mobile Nodes — last mile

## Estimated Effort

| Phase | Effort | New Code (est.) |
|-------|--------|-----------------|
| 1. Multi-Model | 2-3 days | ~2,000 lines |
| 2. Gateway | 3-4 days | ~2,500 lines |
| 3. WebChat | 2-3 days | ~1,500 lines (TS/TSX) + ~200 lines (Rust) |
| 4. Canvas | 1-2 days | ~800 lines |
| 5. Voice | 2-3 days | ~1,500 lines |
| 6. Multi-Agent | 2-3 days | ~1,200 lines |
| 7. Docker Sandbox | 1-2 days | ~800 lines |
| 8. macOS App | 3-5 days | ~2,000 lines (Swift) |
| 9. Onboarding | 1-2 days | ~600 lines |
| 10. Skills Registry | 2-3 days | ~1,000 lines |
| 11. Mobile Nodes | 5-7 days | ~3,000 lines (Swift + Kotlin) |
| **Total** | **~25-37 days** | **~17,000 lines** |
