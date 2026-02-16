# Meepo Codebase Analysis + OpenClaw Issue Audit

Generated: Feb 15, 2026

---

## Part 1: Meepo Codebase Deep Analysis

### Overview

| Metric | Value |
|--------|-------|
| **Crates** | 8 (`meepo-cli`, `meepo-core`, `meepo-channels`, `meepo-knowledge`, `meepo-scheduler`, `meepo-mcp`, `meepo-a2a`, `meepo-gateway`) |
| **Rust source files** | 101 |
| **Total lines (Rust)** | 41,288 |
| **Tests** | 490 passing |
| **Clippy warnings** | 18 (all in `meepo-core`, mostly style) |

### Crate Architecture

```
meepo-cli (3,334 lines)
├── main.rs — daemon startup, tool registration, event loop, CLI commands
├── config.rs — all config structs, env var expansion, TOML loading
└── template.rs — config template generation

meepo-core (20,000+ lines) — the heart
├── agent.rs — Agent struct, handle_message, context loading
├── api.rs — ApiClient wrapping ModelRouter, run_tool_loop
├── providers/ — LlmProvider trait + Anthropic/OpenAI/Google/OpenAI-compat/Router
├── tools/ — 75+ tools across 15 modules
│   ├── canvas.rs (NEW) — 4 canvas tools
│   ├── browser.rs — 11 Safari/Chrome automation tools
│   ├── code.rs — write_code, make_pr, review_pr, spawn_claude_code
│   ├── filesystem.rs — list_directory, search_files
│   ├── lifestyle/ — email, calendar, research, sms, tasks, news, finance, health, travel, social
│   ├── macos.rs — email, calendar, reminders, notes, notifications, music, contacts, clipboard
│   ├── memory.rs — remember, recall, search_knowledge, link_entities
│   ├── rag.rs — smart_recall, ingest_document
│   ├── search.rs — web_search (Tavily)
│   ├── system.rs — run_command, read_file, write_file, browse_url
│   ├── watchers.rs — create/list/cancel watchers
│   └── autonomous.rs — spawn_background_task, agent_status, stop_task
├── autonomy/ — autonomous loop, goals, action log, planner, user model
├── platform/ — macOS/Windows abstraction (AppleScript, etc.)
├── skills/ — SKILL.md parser + skill_tool
├── orchestrator.rs — sub-agent orchestration
├── middleware.rs — request/response middleware chain
├── usage.rs — token usage tracking + cost calculation
├── notifications.rs — notification service
├── context.rs — context loading
├── query_router.rs — query routing
├── tool_selector.rs — tool selection
├── summarization.rs — conversation summarization
├── corrective_rag.rs — corrective RAG pipeline
└── tavily.rs — Tavily API client

meepo-channels (3,500+ lines)
├── bus.rs — MessageBus (incoming/outgoing routing)
├── discord.rs — Discord bot via serenity
├── slack.rs — Slack bot
├── imessage.rs — iMessage via AppleScript
├── email.rs — Email via AppleScript
├── alexa.rs — Alexa skill
├── contacts.rs — Contact management
├── notes.rs — Notes integration
├── reminders.rs — Reminders integration
└── rate_limit.rs — Per-channel rate limiting

meepo-knowledge (4,000+ lines)
├── sqlite.rs — SQLite schema, CRUD, full-text search
├── graph.rs — Knowledge graph (entities + relations)
├── graph_rag.rs — GraphRAG expansion
├── tantivy.rs — Tantivy full-text search index
├── embeddings.rs — Local embedding index (cosine similarity)
├── chunking.rs — Document chunking
└── memory_sync.rs — Memory file sync

meepo-gateway (1,500+ lines) — NEW
├── server.rs — Axum WebSocket + REST server
├── protocol.rs — JSON-RPC message types
├── session.rs — Session manager
├── events.rs — Broadcast event bus
├── auth.rs — Bearer token auth
├── webchat.rs — Embedded React SPA serving
└── ui/ — React 19 + Vite + Tailwind WebChat SPA

meepo-scheduler (1,500+ lines)
├── runner.rs — Watcher runner (cron + interval)
├── watcher.rs — Watcher types
└── persistence.rs — SQLite persistence

meepo-mcp (800+ lines) — MCP server/client (STDIO JSON-RPC)
meepo-a2a (800+ lines) — A2A protocol server/client (HTTP)
```

### Strengths

1. **Massive tool surface** — 75+ tools covering email, calendar, browser, code, filesystem, research, finance, health, travel, news, tasks, social, watchers, autonomous agents
2. **Knowledge graph** — SQLite + Tantivy + local embeddings + GraphRAG — far more sophisticated than OpenClaw's memory system
3. **Multi-model support** — Anthropic, OpenAI, Google, OpenAI-compat with automatic failover (OpenClaw only recently added this)
4. **Native Rust** — single binary, no Node.js runtime, no npm dependency hell
5. **Platform integration** — deep macOS integration (AppleScript for email, calendar, reminders, notes, music, contacts, browser, screen capture, accessibility)
6. **Autonomous loop** — observe/think/act cycle with goal evaluation, action risk classification, confidence gating, user modeling, daily planning
7. **Budget enforcement** — usage tracking, daily/monthly budgets, cost per model
8. **WebSocket gateway** — real-time WebChat UI embedded in binary

### Issues Found in Meepo Codebase

#### Critical (must fix)

| # | Issue | Location | Description |
|---|-------|----------|-------------|
| 1 | **`unwrap()` in production code** | `embeddings.rs` (7 occurrences) | `Mutex::lock().unwrap()` can panic if mutex is poisoned. Should use `.lock().map_err()` or `.expect()` with context |
| 2 | **Gateway TODO: message routing** | `server.rs:305` | `message.send` handler echoes back placeholder instead of routing to Agent — gateway chat is non-functional |
| 3 | **Gateway TODO: session history** | `server.rs:260` | `session.history` returns empty array — no KnowledgeDb integration |
| 4 | **Canvas tools not registered** | `main.rs` | Canvas tools created but never registered in `cmd_start()` or `cmd_mcp_server()` |
| 5 | **No error propagation in WS handler** | `server.rs:182-194` | Response broadcast uses `serde_json::to_value().unwrap_or_default()` — silently drops errors |

#### High (should fix)

| # | Issue | Location | Description |
|---|-------|----------|-------------|
| 6 | **18 clippy warnings** | `meepo-core` | Collapsible if statements, missing Default impls, map_or simplifications |
| 7 | **No graceful shutdown for gateway** | `server.rs` | Gateway spawned with `tokio::spawn` but not tracked in the `tokio::join!` shutdown block |
| 8 | **Large `main.rs`** | `main.rs` (3,334 lines) | Single file handles all CLI commands, tool registration, event loop — should be split |
| 9 | **No connection-scoped WS sender** | `server.rs` | WebSocket responses are broadcast to ALL clients via event bus instead of sent to the requesting client |
| 10 | **Missing `cmd_mcp_server()` registration** | `main.rs` | New providers, canvas tools, and gateway config not wired into MCP server path |

#### Medium (nice to fix)

| # | Issue | Location | Description |
|---|-------|----------|-------------|
| 11 | **No rate limiting on gateway** | `server.rs` | No per-client rate limiting on WS messages — DoS vector |
| 12 | **No TLS support** | `server.rs` | Gateway only supports `ws://`, not `wss://` — insecure for remote access |
| 13 | **Hardcoded 256 event bus capacity** | `server.rs:44` | Should be configurable |
| 14 | **No session persistence** | `session.rs` | Sessions are in-memory only — lost on restart |
| 15 | **No conversation history in gateway** | `session.rs` | Messages not stored — can't reload on reconnect |
| 16 | **WebChat has no auth UI** | `ui/` | No token input — assumes open access or pre-configured |
| 17 | **No health check endpoint** | `server.rs` | `/api/status` exists but no `/health` for load balancers |
| 18 | **Missing `vite.config.ts` SVG favicon** | `ui/index.html` | References `/meepo.svg` but file doesn't exist |
| 19 | **No CORS origin restriction** | `server.rs` | `CorsLayer::permissive()` allows any origin |
| 20 | **Provider failover_order not used** | `main.rs` | Config has `failover_order` field but it's not read when building the ModelRouter |

---

## Part 2: Top OpenClaw Issues — Analysis & Meepo Relevance

I analyzed the top ~100 most-commented issues from `openclaw/openclaw`. Here's the categorized breakdown with relevance to Meepo.

### Category 1: Installation & Setup (25+ issues)

| OpenClaw Issue | Description | Meepo Status |
|----------------|-------------|--------------|
| #4855 Control UI assets not found on npm global install | npm path resolution fails | **N/A** — Meepo is a single Rust binary, no npm |
| #1818 Onboarding wizard fails on Ubuntu 22.04 | Systemd service not installed | **Gap** — Meepo has `meepo setup` but no systemd/launchd installer |
| #3917 Windows installer error | npm install fails on Windows | **N/A** — Meepo compiles natively on Windows |
| #4007 Install fails on macOS 15 at npm install | Node.js compatibility | **N/A** — no Node.js dependency |
| #861 `clawdbot: not found` | PATH not set after install | **N/A** — `cargo install` handles PATH |
| #3480 Docker EACCES permission denied | Container user permissions | **Gap** — no Docker support yet (Phase 7) |
| #2178 Improve Docker setup | Docker compose, volumes | **Gap** — Phase 7 planned |
| #3038 `zsh: command not found: moltbot` | Binary not in PATH | **N/A** |

**Meepo advantage**: Single binary distribution eliminates entire class of npm/Node.js install issues. ~30% of OpenClaw's top issues are install-related and don't apply to Meepo.

### Category 2: Provider & Model Issues (20+ issues)

| OpenClaw Issue | Description | Meepo Status |
|----------------|-------------|--------------|
| #3475 Kimi/Moonshot OpenAI-compat models fail silently | Hangs instead of erroring | **Addressed** — ModelRouter has timeout + failover |
| #2697 Claude Code CLI OAuth auth fails | Config mismatch | **N/A** — Meepo uses API keys, not OAuth |
| #9095 Anthropic OAuth 401 invalid bearer token | Token refresh issue | **N/A** — direct API key auth |
| #1402 Google Antigravity OAuth tokens not used | Token not passed to gateway | **Addressed** — Google provider uses API key directly |
| #14203 Google banning accounts for Gemini CLI | Rate limit/ToS issues | **Risk** — same risk with Google provider, should add rate limiting |
| #2280 Azure OpenAI as model provider | Azure endpoint support | **Gap** — OpenAI-compat provider could work but not tested |
| #10374 Support new models (Opus 4.6, Codex 5.3) | Model updates | **Addressed** — model is configurable in config.toml |
| #9811 Support for Anthropic Opus 4.6 | Specific model | **Addressed** — default model is configurable |
| #2425 Config for Ollama | Local model setup | **Addressed** — `openai_compat` provider supports Ollama |
| #5980 NVIDIA NIM provider hangs | Provider-specific issue | **Addressed** — timeout + failover in ModelRouter |

**Meepo advantage**: API key auth avoids OAuth complexity. ModelRouter failover handles provider outages gracefully.

### Category 3: Channel & Messaging Issues (15+ issues)

| OpenClaw Issue | Description | Meepo Status |
|----------------|-------------|--------------|
| #2203 Discord "Failed to resolve application id" | Discord bot setup | **Has** — Discord channel adapter exists |
| #4772 Discord fails in China | Network/firewall | **Same risk** — depends on network |
| #4515 Telegram DMs never arrive | Polling issue | **Gap** — no Telegram adapter |
| #834 WhatsApp mass-messaged contacts | Catastrophic bug | **N/A** — no WhatsApp adapter (good — avoids this risk) |
| #1649 iMessage self-chat echo loop | Message dedup | **Risk** — Meepo has iMessage adapter, should verify dedup |
| #8650 Feishu plugin issues | Chinese messaging app | **Gap** — no Feishu adapter |
| #2170 Add Feishu and WeChat support | Feature request | **Gap** — not planned |

### Category 4: Security Issues (10+ issues)

| OpenClaw Issue | Description | Meepo Status |
|----------------|-------------|--------------|
| #8776 `soul-evil` hook hijacks agent | Malicious bundled hook | **N/A** — Meepo doesn't have a hook/plugin system that auto-loads |
| #5585 Global killswitch request | Central authority to disable all instances | **N/A** — Meepo is local-only, no central authority |
| #5675 Malicious skills from author | Supply chain attack | **Risk** — skills system exists, needs validation |
| #4311 Config self-mutation bug | Config file modified at runtime | **Addressed** — Meepo config is read-only after load |

**Meepo advantage**: Local-first architecture, no central server dependency, no auto-loading plugins from untrusted sources.

### Category 5: UI & WebChat Issues (10+ issues)

| OpenClaw Issue | Description | Meepo Status |
|----------------|-------------|--------------|
| #4418 Errors show blank in TUI/webchat | Error display | **Risk** — WebChat is new, needs error handling |
| #5030 No output | Agent produces nothing | **Risk** — same potential issue |
| #7189 Commands not working in Control UI | UI-backend disconnect | **Risk** — WebChat is new |
| #2254 Large session files | Unbounded growth | **Risk** — no session size limits |

### Category 6: Performance & Resource Issues (5+ issues)

| OpenClaw Issue | Description | Meepo Status |
|----------------|-------------|--------------|
| #1594 Tokens burned by huge context | Context window management | **Risk** — no automatic context truncation |
| #8786 QMD memory backend too slow on CPU | Embedding computation | **Addressed** — local embeddings are lightweight (no GPU needed) |
| #2596 Read tool validation fails | Parameter name mismatch | **N/A** — Meepo tools have strict schema validation |

### Category 7: Stability & Architecture (5+ issues)

| OpenClaw Issue | Description | Meepo Status |
|----------------|-------------|--------------|
| #5799 Stabilisation mode | Feature freeze for stability | **Relevant** — Meepo is adding features fast, should stabilize |
| #6535 Plugin hooks never called | Dead code | **N/A** — no plugin hook system |
| #2532 `spawn EBADF` on exec tool calls | File descriptor leak | **Risk** — `run_command` tool spawns processes |
| #2687 WebSocket disconnected (1006) | WS stability | **Risk** — gateway WS needs reconnection handling |

---

## Part 3: Priority Action Items for Meepo

Based on the codebase analysis and OpenClaw issue patterns, here are the highest-impact improvements:

### Immediate (before next release)

1. **Register canvas tools** in `cmd_start()` and `cmd_mcp_server()`
2. **Wire gateway message routing** to Agent (currently echoes placeholder)
3. **Fix `unwrap()` in embeddings.rs** — replace with proper error handling
4. **Add gateway to shutdown join** — track the spawned task
5. **Fix per-client WS responses** — don't broadcast responses to all clients

### Short-term (next sprint)

6. **Add context window management** — auto-truncate/summarize when approaching limit (OpenClaw's #1 token waste issue)
7. **Add session persistence** — store sessions in KnowledgeDb
8. **Add gateway rate limiting** — per-client message throttling
9. **Use `failover_order` from config** — currently ignored
10. **Add iMessage dedup** — prevent echo loops (OpenClaw #1649)

### Medium-term

11. **Split `main.rs`** — extract tool registration, event loop, CLI commands into separate modules
12. **Add TLS support** to gateway (or document Tailscale/reverse proxy)
13. **Add Docker support** (Phase 7) — many OpenClaw users want containerized deployment
14. **Add Telegram adapter** — high demand in OpenClaw community
15. **Add `meepo doctor` command** — diagnose common setup issues

### Meepo's Structural Advantages Over OpenClaw

| Area | OpenClaw | Meepo |
|------|----------|-------|
| **Language** | TypeScript/Node.js | Rust (single binary) |
| **Install issues** | ~30% of top bugs | Zero npm issues |
| **Auth** | OAuth (complex, fragile) | API keys (simple, reliable) |
| **Memory** | Plugin-based (QMD slow) | SQLite + Tantivy + local embeddings |
| **Architecture** | Monolithic Node.js | 8-crate workspace |
| **Provider support** | Many, but buggy | 4 providers with robust failover |
| **Security** | Plugin supply chain risk | Local-only, no auto-loading |
| **Performance** | Node.js GC pauses | Native Rust, zero-copy where possible |
| **Binary size** | ~200MB+ with node_modules | ~30MB single binary |
