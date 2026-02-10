# Autonomous Agent — Watch, Act, and Manage Design

## Goal

Let users tell the agent to watch for things and have it work autonomously. Users can see everything the agent is managing and stop anything at any time. The agent can also spin up its own sub-agents to accomplish tasks.

## Architecture

Three types of autonomous work, unified under one status/control surface:

1. **Watchers** (existing) — monitor for events. Now route notifications to user AND auto-act.
2. **Background Tasks** (new) — agent-spawned sub-agents for autonomous work.
3. **Unified Control** — `agent_status` shows everything, `stop_task` cancels anything.

## Changes

### A. Fix Watcher Event Routing (main.rs)

**Problem**: Watcher events use `ChannelType::Internal` — responses are logged, never reach user.

**Fix**: In the `watcher_event_rx` handler, look up the watcher's `reply_channel` from the DB, parse it into a `ChannelType`, and route the agent's response through the bus.

Also include the watcher's `action` in the prompt so the agent knows what to do:
`"Watcher {id} triggered: {payload}. Your requested action: {action}"`

### B. Background Tasks Table (meepo-knowledge)

New `background_tasks` table:
- `id` TEXT PRIMARY KEY (prefixed `t-`)
- `description` TEXT
- `status` TEXT — pending, running, completed, failed, cancelled
- `reply_channel` TEXT
- `spawned_by` TEXT — watcher ID or "agent"
- `created_at` TEXT
- `updated_at` TEXT
- `result` TEXT — output when done

KnowledgeDb methods:
- `insert_background_task()`
- `update_background_task(id, status, result)`
- `get_active_background_tasks()`
- `get_recent_background_tasks(limit)`

### C. `spawn_background_task` Tool

Agent calls this to fire off autonomous sub-agents. Parameters:
- `description` (string, required) — what to accomplish
- `reply_channel` (string, optional) — where to report results (default: current channel)

Flow:
1. Insert task record (status: pending)
2. Send BackgroundTaskCommand::Spawn to main loop
3. Main loop spawns sub-agent via existing TaskOrchestrator
4. On completion: update DB, notify user on reply_channel
5. On failure: update DB, notify user with error

Sub-agents get same tools minus `delegate_tasks` and `spawn_background_task` (no recursion).

### D. `agent_status` Tool

Single tool showing everything. No parameters. Returns:

```
## Active Watchers (3)
- [w-abc123] Email: urgent emails from boss → slack (2h ago)
- [w-def456] File: ~/Coding/myapp/src → discord (1d ago)
- [w-ghi789] Cron: daily 9am summary → slack (3d ago)

## Running Tasks (1)
- [t-jkl012] Researching competitor pricing → slack (5m ago)

## Recently Completed (2)
- [t-mno345] Drafted reply to boss → completed 10m ago
- [t-pqr678] Reviewed PR #42 → completed 1h ago
```

### E. `stop_task` Tool

Parameters:
- `task_id` (string, required) — watcher or task ID

Flow:
- If starts with `w-`: cancel watcher (existing logic)
- If starts with `t-`: cancel background task (cancel token, update DB)

### F. BackgroundTaskCommand Channel

```rust
pub enum BackgroundTaskCommand {
    Spawn { id: String, description: String, reply_channel: String },
    Cancel { id: String },
}
```

Handled in main.rs event loop alongside existing WatcherCommand.

### G. ID Convention

- Watchers: `w-{uuid}` prefix (change existing create_watcher)
- Background tasks: `t-{uuid}` prefix
- Makes status output and stop commands unambiguous

## Existing Tools

Keep `create_watcher`, `list_watchers`, `cancel_watcher` working (backward compat). The `agent_status` and `stop_task` are the unified superset.
