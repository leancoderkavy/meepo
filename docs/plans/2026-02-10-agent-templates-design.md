# Agent Templates — Design

**Goal:** Let users run `meepo template use stock-analyst` to instantly configure Meepo for a specific use case — personality, goals, watchers, tools, and autonomy settings — without losing their existing API keys and channels.

**Architecture:** Templates are directories with a `template.toml` manifest (same schema as config.toml, partial overlay) plus optional SOUL.md, MEMORY.md, and bundled skills. Three sources: built-in (compiled into binary), local (`~/.meepo/templates/`), and GitHub (`gh:user/repo/path`). Activation deep-merges the template into the user's existing config, injects goals into the database, and records a backup for clean rollback.

---

## Template Format

A template is a directory:

```
stock-analyst/
├── template.toml       # manifest: metadata + config overlays
├── SOUL.md             # personality override
├── MEMORY.md           # optional seed knowledge
└── skills/             # optional bundled skills
    └── market-scan/
        └── SKILL.md
```

### template.toml

Uses the same schema as `config.toml` for overlay sections. Only includes fields the template wants to override — everything else stays as-is.

```toml
[template]
name = "stock-analyst"
description = "Financial markets analyst — monitors stocks, summarizes earnings, alerts on price movements"
version = "1.0.0"
author = "kavymi"
tags = ["finance", "stocks", "analysis"]

# Goals injected into the database on activation
[[goals]]
description = "Monitor watchlist stocks for significant price movements (>3%)"
priority = 4
check_interval_secs = 900

[[goals]]
description = "Summarize market open conditions every trading day at 9:35 AM ET"
priority = 3
check_interval_secs = 3600

# Config overlays — merged into user's config.toml
[autonomy]
tick_interval_secs = 15

[[mcp.clients]]
name = "yahoo-finance"
command = "npx"
args = ["-y", "mcp-yahoo-finance"]
```

---

## CLI Commands

```
meepo template list                    # built-in + installed templates
meepo template use <name|path|url>     # activate a template (overlay)
meepo template info <name>             # preview what will change
meepo template create <name>           # scaffold from current config
meepo template remove <name>           # uninstall a community template
meepo template reset                   # remove overlay, restore base config
```

### Resolution Order

1. **Built-in** — compiled via `include_str!()`. Always available, zero network.
2. **Local** — `~/.meepo/templates/<name>/` or explicit path (`./my-template/`).
3. **GitHub** — `gh:user/repo/path` fetches via GitHub API, cached locally after first fetch.

---

## Activation Flow (`meepo template use`)

```
1. Resolve template source → parse template.toml
2. Backup current state:
   - config.toml → config.toml.bak
   - SOUL.md → SOUL.md.bak
   - Record which goals are user-created (not template)
3. Apply overlay:
   - Deep-merge template.toml config sections into config.toml
   - Copy SOUL.md → workspace/SOUL.md
   - Copy MEMORY.md → workspace/MEMORY.md (if present)
   - Insert [[goals]] into DB with source = "template:<name>"
   - Copy skills/ → ~/.meepo/skills/
4. Write ~/.meepo/.active-template with template name + timestamp
5. Print summary: what changed, new goals, new tools
6. Advise restart if daemon is running
```

### Reset Flow (`meepo template reset`)

```
1. Restore config.toml.bak → config.toml
2. Restore SOUL.md.bak → SOUL.md
3. Delete goals WHERE source LIKE 'template:%'
4. Remove template-installed skills
5. Delete .active-template
```

---

## Deep Merge Rules

- **Scalar values** (strings, numbers, bools): template overrides user
- **Arrays** (e.g. `allowed_directories`, `mcp.clients`): template values are *appended*, not replaced
- **Tables** (e.g. `[autonomy]`): recursively merged (template fields override, user fields preserved)
- **Sections not in template**: left untouched

This means `[providers.anthropic]` is never touched unless the template explicitly includes it.

---

## Goal Source Tracking

Goals table gets a `source` column:

- `"user"` — created by the user or agent during normal operation
- `"template:stock-analyst"` — injected by a template

This enables clean rollback: `reset` deletes template goals without touching user goals.

---

## Built-in Templates

### stock-analyst
Financial markets analyst. Monitors stocks, summarizes earnings, alerts on price movements. Adds yahoo-finance MCP client, fast tick interval (15s), market-hours active window.

### code-reviewer
GitHub PR reviewer. Daily PR triage, automated review comments. Adds GitHub MCP client, repository file watchers.

### personal-assistant
General daily assistant. Morning briefing, calendar/email integration, reminder management. Conservative autonomy settings.

### research-agent
Deep research on topics. Web-search focused, saves findings to knowledge graph, synthesizes long-form reports. Higher token budget per tick.

---

## Template Creation (`meepo template create`)

Scaffolds a new template from the current running config:

1. Export current SOUL.md, MEMORY.md
2. Export active goals as `[[goals]]` entries
3. Export non-default config values as overlay sections
4. Write to `~/.meepo/templates/<name>/`
5. User edits and optionally publishes via git

---

## Implementation Order

1. **Template format + parser** — `template.toml` deserialization, config deep-merge logic, goal source tracking
2. **CLI subcommands** — `template list/use/info/reset/create/remove` via clap subcommand group
3. **Built-in templates** — 4 templates compiled into binary with `include_str!()`
4. **GitHub fetcher** — `gh:` URL resolution, download, local caching
5. **Template creation** — export current config state as a reusable template
