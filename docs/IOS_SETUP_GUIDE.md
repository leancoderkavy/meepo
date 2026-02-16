# Meepo iOS Companion App — Setup Guide

The Meepo iOS app gives you mobile access to your local AI agent. It connects to the Meepo daemon running on your Mac via the Gateway WebSocket protocol.

## Prerequisites

- **Meepo daemon running on your Mac** with the Gateway enabled
- **Xcode 15+** installed on your Mac
- **XcodeGen** (`brew install xcodegen`)
- iPhone running **iOS 17.0+** (physical device or Simulator)

## Step 1: Enable the Gateway on Your Mac

The iOS app communicates with Meepo through the Gateway server. Edit `~/.meepo/config.toml`:

```toml
[gateway]
enabled = true
bind = "0.0.0.0"       # Use 0.0.0.0 for LAN access from a physical iPhone
port = 18789
auth_token = "${MEEPO_GATEWAY_TOKEN}"
```

Set the auth token in your environment:

```bash
export MEEPO_GATEWAY_TOKEN="your-secret-token-here"
```

Restart Meepo:

```bash
meepo stop && meepo start
```

> **Tip:** If you only use the iOS Simulator, `bind = "127.0.0.1"` is fine — the Simulator shares your Mac's network.

## Step 2: Build the iOS App

### Option A: Build from Source (Recommended)

```bash
# Clone the repo (if you haven't already)
git clone https://github.com/leancoderkavy/meepo.git
cd meepo

# Install XcodeGen
brew install xcodegen

# Generate the Xcode project
cd MeepoApp
xcodegen generate

# Open in Xcode
open MeepoApp.xcodeproj
```

In Xcode:
1. Select your **Team** under Signing & Capabilities
2. Choose your target device (iPhone Simulator or your physical iPhone)
3. Press **⌘R** to build and run

### Option B: Build via Command Line

```bash
cd MeepoApp
xcodegen generate

# Build for Simulator
xcodebuild -project MeepoApp.xcodeproj -scheme MeepoApp \
  -destination 'platform=iOS Simulator,name=iPhone 16,OS=latest' build

# Build for physical device (requires signing)
xcodebuild -project MeepoApp.xcodeproj -scheme MeepoApp \
  -destination 'generic/platform=iOS' \
  -allowProvisioningUpdates build
```

### Option C: Install via TestFlight (Coming Soon)

We plan to distribute the iOS app via TestFlight in the future. Watch the [releases page](https://github.com/leancoderkavy/meepo/releases) for updates.

## Step 3: Configure the App

Launch the Meepo app on your iPhone and go to the **Settings** tab (gear icon):

| Setting | Value | Notes |
|---------|-------|-------|
| **Host** | Your Mac's IP address | Use `127.0.0.1` for Simulator, or your Mac's LAN IP (e.g. `192.168.1.x`) for a physical device |
| **Port** | `18789` | Must match your `config.toml` gateway port |
| **Auth Token** | Your secret token | Must match `MEEPO_GATEWAY_TOKEN` |
| **Use TLS** | Off | Unless you've set up TLS termination |

### Finding Your Mac's IP Address

```bash
# On your Mac
ipconfig getifaddr en0    # Wi-Fi
# or
ipconfig getifaddr en1    # Ethernet
```

Or: **System Settings → Wi-Fi → Details → IP Address**

## Step 4: Connect

1. Tap the **Chat** tab
2. The app will automatically connect to the Gateway
3. You should see a green "Connected" indicator
4. Send a message — Meepo will respond in real-time!

## Networking Tips

### Simulator
- Uses `127.0.0.1` — no special setup needed
- Gateway `bind` can be `127.0.0.1`

### Physical iPhone (Same Wi-Fi)
- Both devices must be on the **same Wi-Fi network**
- Gateway `bind` must be `0.0.0.0`
- Use your Mac's **LAN IP** in the app settings
- Ensure **port 18789** isn't blocked by your firewall

### Physical iPhone (Remote / Different Network)
For access outside your local network, you'll need one of:
- **Tailscale** or **ZeroTier** (recommended) — creates a private mesh VPN
- **Cloudflare Tunnel** — exposes the gateway securely without port forwarding
- **SSH tunnel** — `ssh -L 18789:localhost:18789 your-mac` from a jump host
- **Port forwarding** on your router (not recommended for security reasons)

### Firewall
If you're on macOS and the connection fails:

```bash
# Check if the port is listening
lsof -i :18789

# Temporarily allow connections (if using pf firewall)
sudo pfctl -d  # Disable packet filter (re-enables on reboot)
```

## Troubleshooting

### "Connection Failed" or "Disconnected"

1. **Is Meepo running?** Check with `meepo ask "hello"` on your Mac
2. **Is the Gateway enabled?** Look for `Gateway server listening on ...` in Meepo's startup logs
3. **Correct IP?** Verify with `ipconfig getifaddr en0` on your Mac
4. **Same network?** Both devices must be on the same Wi-Fi for LAN access
5. **Firewall?** Try `lsof -i :18789` to confirm the port is open

### "Unauthorized" Error

- Your auth token in the app doesn't match `MEEPO_GATEWAY_TOKEN`
- Re-check both values — they must be identical

### App Crashes on Launch

- Ensure you're running iOS 17.0 or later
- Try deleting and reinstalling the app
- Check Xcode console for crash logs

### Slow Responses

- The LLM response time depends on your provider (Anthropic, OpenAI, Ollama, etc.)
- Ollama running locally is fastest for simple queries
- Check your Mac's CPU/memory usage — Meepo + LLM can be resource-intensive

## App Features

| Feature | Description |
|---------|-------------|
| **Chat** | Real-time messaging with your Meepo agent |
| **Sessions** | Create and switch between conversation sessions |
| **Typing Indicators** | See when Meepo is thinking or executing tools |
| **Tool Badges** | Visual indicators when tools are being used |
| **Auto-Reconnect** | Automatically reconnects with exponential backoff |
| **Meepo Theme** | Earthy cave aesthetic inspired by the Dota 2 Geomancer |

## Architecture

```
┌──────────────┐         WebSocket          ┌──────────────────┐
│   iPhone     │ ◄──────────────────────►   │   Mac (Meepo)    │
│  MeepoApp    │    JSON-RPC over WS        │   Gateway :18789 │
│  (SwiftUI)   │                            │   Agent Loop     │
└──────────────┘                            │   75+ Tools      │
                                            └──────────────────┘
```

The app communicates exclusively through the Gateway's WebSocket endpoint (`/ws`). All messages use the JSON-RPC protocol defined in the Gateway module.

## Updating the App

When you pull new changes from the repo:

```bash
cd MeepoApp
xcodegen generate   # Regenerate project if project.yml changed
# Then build and run in Xcode (⌘R)
```

## Uninstalling

- **Delete from iPhone:** Long-press the app icon → Remove App
- **Delete Xcode project:** `rm -rf MeepoApp/MeepoApp.xcodeproj MeepoApp/build`
