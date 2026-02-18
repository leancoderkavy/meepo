# Meepo iOS App

SwiftUI companion app for the [Meepo](https://github.com/kavyrattana/meepo) AI agent. Connects to your running Meepo daemon via the Gateway WebSocket protocol, letting you chat with your agent from your iPhone or iPad.

## Features

- **Real-time chat** — Send messages and receive responses via WebSocket
- **Session management** — Create, switch between, and browse sessions
- **Live status** — Typing indicators, tool execution feedback, connection state
- **Secure auth** — Bearer token authentication matching your gateway config
- **Settings** — Configure host, port, TLS, and auth token from the app

## Requirements

- iOS 17.0+
- Xcode 15.0+
- A running Meepo daemon with the gateway enabled

## Setup

### 1. Enable the Meepo Gateway

In your `config/default.toml` (or `~/.config/meepo/config.toml`):

```toml
[gateway]
enabled = true
bind = "0.0.0.0"    # Use 0.0.0.0 for LAN access from your phone
port = 18789
auth_token = "${MEEPO_GATEWAY_TOKEN}"
```

Set the environment variable:

```bash
export MEEPO_GATEWAY_TOKEN="your-secret-token"
```

Then start the daemon:

```bash
meepo start
```

### 2. Open the Xcode Project

```bash
cd MeepoApp
open MeepoApp.xcodeproj
```

If you need to regenerate the project (e.g. after adding files):

```bash
brew install xcodegen  # one-time
xcodegen generate
```

### 3. Configure the App

In the **Settings** tab:

- **Host** — Your Mac's local IP (e.g. `192.168.1.100`), or `127.0.0.1` for simulator
- **Port** — `18789` (default)
- **Gateway Token** — Same value as `MEEPO_GATEWAY_TOKEN`

Use **Test Connection** to verify connectivity.

### 4. Build & Run

Select your device or simulator in Xcode and hit ⌘R.

## Architecture

```
MeepoApp/
├── Sources/
│   ├── MeepoApp.swift              # @main entry point
│   ├── Models/
│   │   ├── GatewayProtocol.swift   # JSON-RPC protocol types
│   │   ├── ChatMessage.swift       # Chat message model
│   │   └── Session.swift           # Session + status models
│   ├── Networking/
│   │   ├── MeepoClient.swift       # WebSocket + REST client
│   │   └── SettingsStore.swift     # @AppStorage preferences
│   ├── ViewModels/
│   │   └── ChatViewModel.swift     # Chat state management
│   └── Views/
│       ├── ContentView.swift       # Tab navigation root
│       ├── ChatView.swift          # Chat UI (bubbles, input, typing)
│       ├── SessionsView.swift      # Session list + creation
│       └── SettingsView.swift      # Connection & preferences
├── Resources/
│   ├── Info.plist
│   └── Assets.xcassets/
└── project.yml                     # XcodeGen spec
```

## Gateway Protocol

The app communicates using the Meepo Gateway WebSocket protocol:

| Method | Description |
|--------|-------------|
| `message.send` | Send a message to the agent |
| `session.list` | List all sessions |
| `session.new` | Create a new session |
| `session.history` | Get session message history |
| `status.get` | Get daemon status |

Events received from the server:

| Event | Description |
|-------|-------------|
| `message.received` | Agent response message |
| `typing.start` / `typing.stop` | Typing indicators |
| `tool.executing` | Tool currently being run |
| `session.created` | New session was created |

## Networking Notes

- The app uses native `URLSessionWebSocketTask` — no third-party dependencies
- iOS doesn't send `Origin` headers on WebSocket connections, so the gateway's origin check is bypassed naturally
- `NSAllowsLocalNetworking` is enabled in Info.plist for HTTP connections to local network addresses
- Auto-reconnect with 3-second backoff on connection loss
- 30-second ping keepalive loop
