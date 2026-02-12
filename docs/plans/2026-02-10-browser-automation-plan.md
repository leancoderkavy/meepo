# Browser Automation (Safari + Chrome) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add full browser automation (tab management, page interaction, navigation, cookies) with Safari via AppleScript and headless Chrome via CDP.

**Architecture:** New `BrowserProvider` trait in `platform/mod.rs` with `MacOsSafariBrowser` in `platform/macos.rs` (AppleScript) and `HeadlessChromeBrowser` in `platform/headless.rs` (chromiumoxide CDP). Tools in `tools/browser.rs` delegate to `Box<dyn BrowserProvider>`. Registration in `main.rs` gated with `#[cfg(target_os = "macos")]` for Safari.

**Tech Stack:** AppleScript (Safari), `chromiumoxide` crate (headless Chrome/Chromium via CDP), existing `async_trait`, `serde_json`, `anyhow`, `tokio`

---

### Task 1: Add BrowserProvider trait to platform/mod.rs

**Files:**
- Modify: `crates/meepo-core/src/platform/mod.rs`

**Step 1: Add the BrowserProvider trait and data structs**

Add after the `ContactsProvider` trait (line ~85) and before the factory functions (line ~88):

```rust
/// Browser tab metadata
#[derive(Debug, Clone, serde::Serialize)]
pub struct BrowserTab {
    pub id: String,
    pub title: String,
    pub url: String,
    pub is_active: bool,
    pub window_index: u32,
}

/// Page content with both text and HTML
#[derive(Debug, Clone)]
pub struct PageContent {
    pub text: String,
    pub html: String,
    pub url: String,
    pub title: String,
}

/// Browser cookie
#[derive(Debug, Clone, serde::Serialize)]
pub struct BrowserCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
}

/// Browser automation provider
#[async_trait]
pub trait BrowserProvider: Send + Sync {
    // Tab management
    async fn list_tabs(&self) -> Result<Vec<BrowserTab>>;
    async fn open_tab(&self, url: &str) -> Result<BrowserTab>;
    async fn close_tab(&self, tab_id: &str) -> Result<()>;
    async fn switch_tab(&self, tab_id: &str) -> Result<()>;

    // Page interaction
    async fn get_page_content(&self, tab_id: Option<&str>) -> Result<PageContent>;
    async fn execute_javascript(&self, tab_id: Option<&str>, script: &str) -> Result<String>;
    async fn click_element(&self, tab_id: Option<&str>, selector: &str) -> Result<()>;
    async fn fill_form(&self, tab_id: Option<&str>, selector: &str, value: &str) -> Result<()>;
    async fn screenshot_page(&self, tab_id: Option<&str>, path: Option<&str>) -> Result<String>;

    // Navigation
    async fn go_back(&self, tab_id: Option<&str>) -> Result<()>;
    async fn go_forward(&self, tab_id: Option<&str>) -> Result<()>;
    async fn reload(&self, tab_id: Option<&str>) -> Result<()>;

    // Cookies & URL
    async fn get_cookies(&self, tab_id: Option<&str>) -> Result<Vec<BrowserCookie>>;
    async fn get_page_url(&self, tab_id: Option<&str>) -> Result<String>;
}
```

**Step 2: Add factory function**

Add after the `create_contacts_provider()` function:

```rust
/// Create platform browser provider (macOS: Safari via AppleScript)
pub fn create_browser_provider() -> Box<dyn BrowserProvider> {
    #[cfg(target_os = "macos")]
    { Box::new(macos::MacOsSafariBrowser) }
    #[cfg(not(target_os = "macos"))]
    { panic!("Browser provider not yet available on this platform (use headless Chrome)") }
}
```

**Step 3: Add test**

Add to the `#[cfg(target_os = "macos")] #[test] fn test_macos_providers_create()` block:

```rust
let _browser = create_browser_provider();
```

**Step 4: Run test to verify it compiles (will fail — no MacOsSafariBrowser yet)**

Run: `cargo check -p meepo-core 2>&1 | head -20`
Expected: Error about `MacOsSafariBrowser` not found in `macos`

**Step 5: Commit**

```bash
git add crates/meepo-core/src/platform/mod.rs
git commit -m "feat: add BrowserProvider trait with tab, page, navigation, and cookie methods"
```

---

### Task 2: Implement MacOsSafariBrowser in platform/macos.rs

**Files:**
- Modify: `crates/meepo-core/src/platform/macos.rs`

Safari's AppleScript dictionary supports:
- `tell application "Safari"` → `windows`, `tabs`, `current tab`, `URL`, `name`, `source`
- `do JavaScript` for JS execution in a tab
- Tab addressing: `tab N of window M`
- Tab IDs will use format `"w{window_index}t{tab_index}"` (e.g., `"w1t3"`)

**Step 1: Add import for BrowserProvider**

Update the import line at the top to include the new trait and structs:

```rust
use super::{EmailProvider, CalendarProvider, UiAutomation, RemindersProvider, NotesProvider, NotificationProvider, ScreenCaptureProvider, MusicProvider, ContactsProvider, BrowserProvider, BrowserTab, PageContent, BrowserCookie};
```

**Step 2: Add MacOsSafariBrowser struct and helper**

Add after `MacOsContactsProvider` impl block:

```rust
pub struct MacOsSafariBrowser;

/// Parse a tab_id "w{window}t{tab}" into (window_index, tab_index). Both are 1-based.
fn parse_tab_id(tab_id: &str) -> Result<(u32, u32)> {
    let parts: Vec<&str> = tab_id.split('t').collect();
    if parts.len() != 2 {
        return Err(anyhow::anyhow!("Invalid tab_id format '{}', expected 'wNtM'", tab_id));
    }
    let window: u32 = parts[0].trim_start_matches('w').parse()
        .map_err(|_| anyhow::anyhow!("Invalid window index in tab_id '{}'", tab_id))?;
    let tab: u32 = parts[1].parse()
        .map_err(|_| anyhow::anyhow!("Invalid tab index in tab_id '{}'", tab_id))?;
    if window == 0 || tab == 0 {
        return Err(anyhow::anyhow!("Window and tab indices must be >= 1"));
    }
    Ok((window, tab))
}

/// Build AppleScript tab reference from optional tab_id, defaulting to current tab of window 1
fn tab_ref(tab_id: Option<&str>) -> Result<String> {
    match tab_id {
        Some(id) => {
            let (w, t) = parse_tab_id(id)?;
            Ok(format!("tab {} of window {}", t, w))
        }
        None => Ok("current tab of front window".to_string()),
    }
}
```

**Step 3: Implement the BrowserProvider trait**

```rust
#[async_trait]
impl BrowserProvider for MacOsSafariBrowser {
    async fn list_tabs(&self) -> Result<Vec<BrowserTab>> {
        debug!("Listing Safari tabs");
        let script = r#"
tell application "Safari"
    set output to ""
    set winCount to count of windows
    repeat with w from 1 to winCount
        set theWin to window w
        set tabCount to count of tabs of theWin
        set activeIdx to 0
        try
            set activeIdx to index of current tab of theWin
        end try
        repeat with t from 1 to tabCount
            set theTab to tab t of theWin
            set tabUrl to URL of theTab
            if tabUrl is missing value then set tabUrl to ""
            set tabName to name of theTab
            if tabName is missing value then set tabName to ""
            set isActive to "false"
            if t = activeIdx then set isActive to "true"
            set output to output & "w" & w & "t" & t & "\t" & tabName & "\t" & tabUrl & "\t" & isActive & "\n"
        end repeat
    end repeat
    return output
end tell
"#;
        let raw = run_applescript(script).await?;
        let mut tabs = Vec::new();
        for line in raw.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 4 {
                let id = parts[0].trim().to_string();
                let window_index: u32 = id.split('t').next()
                    .unwrap_or("w1").trim_start_matches('w').parse().unwrap_or(1);
                tabs.push(BrowserTab {
                    id,
                    title: parts[1].to_string(),
                    url: parts[2].to_string(),
                    is_active: parts[3].trim() == "true",
                    window_index,
                });
            }
        }
        Ok(tabs)
    }

    async fn open_tab(&self, url: &str) -> Result<BrowserTab> {
        let safe_url = sanitize_applescript_string(url);
        debug!("Opening Safari tab: {}", url);
        let script = format!(r#"
tell application "Safari"
    activate
    tell front window
        set newTab to make new tab with properties {{URL:"{}"}}
        set current tab to newTab
        set t to index of newTab
        set w to index of front window
        return "w" & w & "t" & t
    end tell
end tell
"#, safe_url);
        let result = run_applescript(&script).await?;
        let id = result.trim().to_string();
        Ok(BrowserTab {
            id,
            title: String::new(),
            url: url.to_string(),
            is_active: true,
            window_index: 1,
        })
    }

    async fn close_tab(&self, tab_id: &str) -> Result<()> {
        let (w, t) = parse_tab_id(tab_id)?;
        debug!("Closing Safari tab {}", tab_id);
        let script = format!(r#"
tell application "Safari"
    close tab {} of window {}
end tell
"#, t, w);
        run_applescript(&script).await?;
        Ok(())
    }

    async fn switch_tab(&self, tab_id: &str) -> Result<()> {
        let (w, t) = parse_tab_id(tab_id)?;
        debug!("Switching to Safari tab {}", tab_id);
        let script = format!(r#"
tell application "Safari"
    set current tab of window {} to tab {} of window {}
end tell
"#, w, t, w);
        run_applescript(&script).await?;
        Ok(())
    }

    async fn get_page_content(&self, tab_id: Option<&str>) -> Result<PageContent> {
        let tref = tab_ref(tab_id)?;
        debug!("Getting page content from Safari ({})", tref);
        let script = format!(r#"
tell application "Safari"
    set theTab to {}
    set pageUrl to URL of theTab
    if pageUrl is missing value then set pageUrl to ""
    set pageName to name of theTab
    if pageName is missing value then set pageName to ""
    set pageSource to source of theTab
    if pageSource is missing value then set pageSource to ""
    set pageText to do JavaScript "document.body.innerText.substring(0, 50000)" in theTab
    if pageText is missing value then set pageText to ""
    return pageUrl & "\n---SPLIT---\n" & pageName & "\n---SPLIT---\n" & pageText & "\n---SPLIT---\n" & pageSource
end tell
"#, tref);
        let raw = run_applescript(&script).await?;
        let parts: Vec<&str> = raw.splitn(4, "\n---SPLIT---\n").collect();
        Ok(PageContent {
            url: parts.first().unwrap_or(&"").to_string(),
            title: parts.get(1).unwrap_or(&"").to_string(),
            text: parts.get(2).unwrap_or(&"").to_string(),
            html: parts.get(3).unwrap_or(&"").to_string(),
        })
    }

    async fn execute_javascript(&self, tab_id: Option<&str>, script: &str) -> Result<String> {
        let tref = tab_ref(tab_id)?;
        // For JS, we need to escape for AppleScript string embedding
        let safe_script = script.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        debug!("Executing JavaScript in Safari ({})", tref);
        let applescript = format!(r#"
tell application "Safari"
    set result to do JavaScript "{}" in {}
    if result is missing value then return ""
    return result as text
end tell
"#, safe_script, tref);
        run_applescript(&applescript).await
    }

    async fn click_element(&self, tab_id: Option<&str>, selector: &str) -> Result<()> {
        let safe_selector = selector.replace('\\', "\\\\").replace('"', "\\\"");
        let js = format!("document.querySelector(\\\"{}\\\").click()", safe_selector);
        self.execute_javascript(tab_id, &format!("document.querySelector(\"{}\").click()", selector.replace('"', "\\\""))).await?;
        Ok(())
    }

    async fn fill_form(&self, tab_id: Option<&str>, selector: &str, value: &str) -> Result<()> {
        let safe_value = value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        let js = format!(
            "var el = document.querySelector(\"{}\"); el.value = \"{}\"; el.dispatchEvent(new Event('input', {{bubbles: true}}));",
            selector.replace('"', "\\\""),
            safe_value,
        );
        self.execute_javascript(tab_id, &js).await?;
        Ok(())
    }

    async fn screenshot_page(&self, tab_id: Option<&str>, path: Option<&str>) -> Result<String> {
        // Safari doesn't have a direct screenshot API via AppleScript.
        // We use screencapture focused on Safari's window.
        if tab_id.is_some() {
            self.switch_tab(tab_id.unwrap()).await?;
        }
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let output_path = path
            .map(|p| p.to_string())
            .unwrap_or_else(|| format!("/tmp/meepo-browser-screenshot-{}.png", timestamp));

        // Bring Safari to front, then capture its window
        let script = r#"
tell application "Safari" to activate
delay 0.3
tell application "System Events"
    set frontProc to first application process whose frontmost is true
    set winPos to position of front window of frontProc
    set winSize to size of front window of frontProc
end tell
return (item 1 of winPos as text) & "," & (item 2 of winPos as text) & "," & (item 1 of winSize as text) & "," & (item 2 of winSize as text)
"#;
        let bounds = run_applescript(script).await?;
        let nums: Vec<i32> = bounds.trim().split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if nums.len() == 4 {
            let rect = format!("{},{},{},{}", nums[0], nums[1], nums[2], nums[3]);
            let output = tokio::process::Command::new("screencapture")
                .args(["-x", "-R", &rect, &output_path])
                .output()
                .await
                .context("Failed to run screencapture")?;
            if output.status.success() {
                return Ok(format!("Screenshot saved to {}", output_path));
            }
        }
        // Fallback: capture entire screen
        let output = tokio::process::Command::new("screencapture")
            .args(["-x", &output_path])
            .output()
            .await
            .context("Failed to run screencapture")?;
        if output.status.success() {
            Ok(format!("Screenshot saved to {}", output_path))
        } else {
            Err(anyhow::anyhow!("screencapture failed"))
        }
    }

    async fn go_back(&self, tab_id: Option<&str>) -> Result<()> {
        self.execute_javascript(tab_id, "history.back()").await?;
        Ok(())
    }

    async fn go_forward(&self, tab_id: Option<&str>) -> Result<()> {
        self.execute_javascript(tab_id, "history.forward()").await?;
        Ok(())
    }

    async fn reload(&self, tab_id: Option<&str>) -> Result<()> {
        self.execute_javascript(tab_id, "location.reload()").await?;
        Ok(())
    }

    async fn get_cookies(&self, tab_id: Option<&str>) -> Result<Vec<BrowserCookie>> {
        let raw = self.execute_javascript(tab_id, "document.cookie").await?;
        let cookies = raw.split(';')
            .filter_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                let name = parts.next()?.trim().to_string();
                let value = parts.next().unwrap_or("").trim().to_string();
                if name.is_empty() { return None; }
                Some(BrowserCookie {
                    name,
                    value,
                    domain: String::new(), // JS cookie API doesn't expose domain
                    path: String::new(),
                })
            })
            .collect();
        Ok(cookies)
    }

    async fn get_page_url(&self, tab_id: Option<&str>) -> Result<String> {
        let tref = tab_ref(tab_id)?;
        let script = format!(r#"
tell application "Safari"
    set tabUrl to URL of {}
    if tabUrl is missing value then return ""
    return tabUrl
end tell
"#, tref);
        let url = run_applescript(&script).await?;
        Ok(url.trim().to_string())
    }
}
```

**Step 4: Add unit tests for parse_tab_id**

```rust
#[test]
fn test_parse_tab_id() {
    let (w, t) = parse_tab_id("w1t3").unwrap();
    assert_eq!(w, 1);
    assert_eq!(t, 3);
    assert!(parse_tab_id("invalid").is_err());
    assert!(parse_tab_id("w0t1").is_err());
}
```

**Step 5: Run check**

Run: `cargo check -p meepo-core 2>&1 | head -20`
Expected: Compiles successfully

**Step 6: Run tests**

Run: `cargo test -p meepo-core -- platform 2>&1 | tail -20`
Expected: All tests pass

**Step 7: Commit**

```bash
git add crates/meepo-core/src/platform/macos.rs crates/meepo-core/src/platform/mod.rs
git commit -m "feat: implement Safari browser automation via AppleScript"
```

---

### Task 3: Create browser tools in tools/browser.rs

**Files:**
- Create: `crates/meepo-core/src/tools/browser.rs`
- Modify: `crates/meepo-core/src/tools/mod.rs` (add `pub mod browser;`)

This creates 14 tool structs, each holding `Box<dyn BrowserProvider>` and delegating to the provider. Follow the exact same pattern as `tools/macos.rs`.

**Step 1: Add module declaration**

In `tools/mod.rs`, add after `pub mod autonomous;`:

```rust
pub mod browser;
```

**Step 2: Create tools/browser.rs**

Each tool follows the pattern:
- Struct with `provider: Box<dyn BrowserProvider>`
- `new()` calls `crate::platform::create_browser_provider()`
- `ToolHandler` impl with name, description, input_schema, execute

Create the file with these 14 tools:

1. **`ListTabsTool`** — name: `"list_browser_tabs"`, no required params
2. **`OpenTabTool`** — name: `"open_browser_tab"`, required: `["url"]`
3. **`CloseTabTool`** — name: `"close_browser_tab"`, required: `["tab_id"]`
4. **`SwitchTabTool`** — name: `"switch_browser_tab"`, required: `["tab_id"]`
5. **`GetPageContentTool`** — name: `"get_page_content"`, optional: `tab_id`
6. **`ExecuteJavaScriptTool`** — name: `"execute_javascript"`, required: `["script"]`, optional: `tab_id`
7. **`ClickBrowserElementTool`** — name: `"click_browser_element"`, required: `["selector"]`, optional: `tab_id`
8. **`FillFormTool`** — name: `"fill_form"`, required: `["selector", "value"]`, optional: `tab_id`
9. **`BrowserScreenshotTool`** — name: `"browser_screenshot"`, optional: `tab_id`, `path`
10. **`GoBackTool`** — name: `"browser_go_back"`, optional: `tab_id`
11. **`GoForwardTool`** — name: `"browser_go_forward"`, optional: `tab_id`
12. **`ReloadPageTool`** — name: `"reload_page"`, optional: `tab_id`
13. **`GetCookiesTool`** — name: `"get_browser_cookies"`, optional: `tab_id`
14. **`GetPageUrlTool`** — name: `"get_page_url"`, optional: `tab_id`

Full implementation:

```rust
//! Browser automation tools — Safari (macOS) and Chrome (headless)

use async_trait::async_trait;
use serde_json::Value;
use anyhow::Result;
use tracing::debug;

use super::{ToolHandler, json_schema};
use crate::platform::BrowserProvider;

// ── Tab Management ──

pub struct ListTabsTool {
    provider: Box<dyn BrowserProvider>,
}

impl ListTabsTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for ListTabsTool {
    fn name(&self) -> &str { "list_browser_tabs" }
    fn description(&self) -> &str {
        "List all open browser tabs across all windows. Returns tab ID, title, URL, and active status."
    }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({}), vec![])
    }
    async fn execute(&self, _input: Value) -> Result<String> {
        debug!("Listing browser tabs");
        let tabs = self.provider.list_tabs().await?;
        if tabs.is_empty() {
            return Ok("No browser tabs open".to_string());
        }
        Ok(serde_json::to_string_pretty(&tabs)?)
    }
}

pub struct OpenTabTool {
    provider: Box<dyn BrowserProvider>,
}

impl OpenTabTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for OpenTabTool {
    fn name(&self) -> &str { "open_browser_tab" }
    fn description(&self) -> &str {
        "Open a URL in a new browser tab and make it active."
    }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "url": { "type": "string", "description": "URL to open" }
        }), vec!["url"])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let url = input.get("url").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?;
        if url.len() > 2048 {
            return Err(anyhow::anyhow!("URL too long (max 2048 characters)"));
        }
        debug!("Opening browser tab: {}", url);
        let tab = self.provider.open_tab(url).await?;
        Ok(format!("Opened tab {} at {}", tab.id, url))
    }
}

pub struct CloseTabTool {
    provider: Box<dyn BrowserProvider>,
}

impl CloseTabTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for CloseTabTool {
    fn name(&self) -> &str { "close_browser_tab" }
    fn description(&self) -> &str {
        "Close a browser tab by its ID (e.g., 'w1t3' for window 1, tab 3)."
    }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "tab_id": { "type": "string", "description": "Tab ID from list_browser_tabs (e.g., 'w1t3')" }
        }), vec!["tab_id"])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'tab_id' parameter"))?;
        debug!("Closing browser tab: {}", tab_id);
        self.provider.close_tab(tab_id).await?;
        Ok(format!("Closed tab {}", tab_id))
    }
}

pub struct SwitchTabTool {
    provider: Box<dyn BrowserProvider>,
}

impl SwitchTabTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for SwitchTabTool {
    fn name(&self) -> &str { "switch_browser_tab" }
    fn description(&self) -> &str {
        "Switch to a specific browser tab by its ID."
    }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "tab_id": { "type": "string", "description": "Tab ID to switch to (e.g., 'w1t3')" }
        }), vec!["tab_id"])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'tab_id' parameter"))?;
        debug!("Switching to browser tab: {}", tab_id);
        self.provider.switch_tab(tab_id).await?;
        Ok(format!("Switched to tab {}", tab_id))
    }
}

// ── Page Interaction ──

pub struct GetPageContentTool {
    provider: Box<dyn BrowserProvider>,
}

impl GetPageContentTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for GetPageContentTool {
    fn name(&self) -> &str { "get_page_content" }
    fn description(&self) -> &str {
        "Get the text content, HTML source, URL, and title of a browser page. Defaults to the active tab."
    }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "tab_id": { "type": "string", "description": "Optional tab ID (defaults to active tab)" },
            "include_html": { "type": "boolean", "description": "Include raw HTML source (default: false, returns text only)" }
        }), vec![])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());
        let include_html = input.get("include_html").and_then(|v| v.as_bool()).unwrap_or(false);
        debug!("Getting page content");
        let content = self.provider.get_page_content(tab_id).await?;
        let mut output = format!("URL: {}\nTitle: {}\n\n{}", content.url, content.title, content.text);
        if include_html {
            // Truncate HTML to 50k chars
            let html = if content.html.len() > 50_000 {
                &content.html[..50_000]
            } else {
                &content.html
            };
            output.push_str(&format!("\n\n--- HTML Source ---\n{}", html));
        }
        Ok(output)
    }
}

pub struct ExecuteJavaScriptTool {
    provider: Box<dyn BrowserProvider>,
}

impl ExecuteJavaScriptTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for ExecuteJavaScriptTool {
    fn name(&self) -> &str { "execute_javascript" }
    fn description(&self) -> &str {
        "Execute JavaScript code in a browser tab and return the result."
    }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "script": { "type": "string", "description": "JavaScript code to execute" },
            "tab_id": { "type": "string", "description": "Optional tab ID (defaults to active tab)" }
        }), vec!["script"])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let script = input.get("script").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'script' parameter"))?;
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());
        if script.len() > 100_000 {
            return Err(anyhow::anyhow!("Script too long (max 100,000 characters)"));
        }
        debug!("Executing JavaScript ({} chars)", script.len());
        self.provider.execute_javascript(tab_id, script).await
    }
}

pub struct ClickBrowserElementTool {
    provider: Box<dyn BrowserProvider>,
}

impl ClickBrowserElementTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for ClickBrowserElementTool {
    fn name(&self) -> &str { "click_browser_element" }
    fn description(&self) -> &str {
        "Click an element on the page identified by a CSS selector."
    }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "selector": { "type": "string", "description": "CSS selector for the element to click (e.g., '#submit-btn', '.login-link')" },
            "tab_id": { "type": "string", "description": "Optional tab ID (defaults to active tab)" }
        }), vec!["selector"])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let selector = input.get("selector").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'selector' parameter"))?;
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());
        if selector.len() > 1000 {
            return Err(anyhow::anyhow!("Selector too long (max 1000 characters)"));
        }
        debug!("Clicking browser element: {}", selector);
        self.provider.click_element(tab_id, selector).await?;
        Ok(format!("Clicked element matching '{}'", selector))
    }
}

pub struct FillFormTool {
    provider: Box<dyn BrowserProvider>,
}

impl FillFormTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for FillFormTool {
    fn name(&self) -> &str { "fill_form" }
    fn description(&self) -> &str {
        "Fill a form field identified by CSS selector with a value. Triggers input events."
    }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "selector": { "type": "string", "description": "CSS selector for the input element (e.g., '#email', 'input[name=username]')" },
            "value": { "type": "string", "description": "Value to fill into the field" },
            "tab_id": { "type": "string", "description": "Optional tab ID (defaults to active tab)" }
        }), vec!["selector", "value"])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let selector = input.get("selector").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'selector' parameter"))?;
        let value = input.get("value").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'value' parameter"))?;
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());
        if value.len() > 50_000 {
            return Err(anyhow::anyhow!("Value too long (max 50,000 characters)"));
        }
        debug!("Filling form field: {}", selector);
        self.provider.fill_form(tab_id, selector, value).await?;
        Ok(format!("Filled '{}' with {} chars", selector, value.len()))
    }
}

pub struct BrowserScreenshotTool {
    provider: Box<dyn BrowserProvider>,
}

impl BrowserScreenshotTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for BrowserScreenshotTool {
    fn name(&self) -> &str { "browser_screenshot" }
    fn description(&self) -> &str {
        "Take a screenshot of the current browser page."
    }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "tab_id": { "type": "string", "description": "Optional tab ID to screenshot" },
            "path": { "type": "string", "description": "Output file path (default: /tmp/meepo-browser-screenshot-{timestamp}.png)" }
        }), vec![])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());
        let path = input.get("path").and_then(|v| v.as_str());
        if let Some(p) = path {
            if !p.ends_with(".png") && !p.ends_with(".jpg") {
                return Err(anyhow::anyhow!("Output path must end with .png or .jpg"));
            }
        }
        debug!("Taking browser screenshot");
        self.provider.screenshot_page(tab_id, path).await
    }
}

// ── Navigation ──

pub struct GoBackTool {
    provider: Box<dyn BrowserProvider>,
}

impl GoBackTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for GoBackTool {
    fn name(&self) -> &str { "browser_go_back" }
    fn description(&self) -> &str { "Navigate back in browser history." }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "tab_id": { "type": "string", "description": "Optional tab ID" }
        }), vec![])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());
        self.provider.go_back(tab_id).await?;
        Ok("Navigated back".to_string())
    }
}

pub struct GoForwardTool {
    provider: Box<dyn BrowserProvider>,
}

impl GoForwardTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for GoForwardTool {
    fn name(&self) -> &str { "browser_go_forward" }
    fn description(&self) -> &str { "Navigate forward in browser history." }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "tab_id": { "type": "string", "description": "Optional tab ID" }
        }), vec![])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());
        self.provider.go_forward(tab_id).await?;
        Ok("Navigated forward".to_string())
    }
}

pub struct ReloadPageTool {
    provider: Box<dyn BrowserProvider>,
}

impl ReloadPageTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for ReloadPageTool {
    fn name(&self) -> &str { "reload_page" }
    fn description(&self) -> &str { "Reload the current browser page." }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "tab_id": { "type": "string", "description": "Optional tab ID" }
        }), vec![])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());
        self.provider.reload(tab_id).await?;
        Ok("Page reloaded".to_string())
    }
}

// ── Cookies & URL ──

pub struct GetCookiesTool {
    provider: Box<dyn BrowserProvider>,
}

impl GetCookiesTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for GetCookiesTool {
    fn name(&self) -> &str { "get_browser_cookies" }
    fn description(&self) -> &str {
        "Get cookies from the current browser page (JavaScript-accessible cookies only, not HttpOnly)."
    }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "tab_id": { "type": "string", "description": "Optional tab ID" }
        }), vec![])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());
        debug!("Getting browser cookies");
        let cookies = self.provider.get_cookies(tab_id).await?;
        if cookies.is_empty() {
            return Ok("No cookies found".to_string());
        }
        Ok(serde_json::to_string_pretty(&cookies)?)
    }
}

pub struct GetPageUrlTool {
    provider: Box<dyn BrowserProvider>,
}

impl GetPageUrlTool {
    pub fn new() -> Self {
        Self { provider: crate::platform::create_browser_provider() }
    }
}

#[async_trait]
impl ToolHandler for GetPageUrlTool {
    fn name(&self) -> &str { "get_page_url" }
    fn description(&self) -> &str { "Get the URL of the current browser page." }
    fn input_schema(&self) -> Value {
        json_schema(serde_json::json!({
            "tab_id": { "type": "string", "description": "Optional tab ID" }
        }), vec![])
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let tab_id = input.get("tab_id").and_then(|v| v.as_str());
        self.provider.get_page_url(tab_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn test_browser_tools_create() {
        let _ = ListTabsTool::new();
        let _ = OpenTabTool::new();
        let _ = CloseTabTool::new();
        let _ = SwitchTabTool::new();
        let _ = GetPageContentTool::new();
        let _ = ExecuteJavaScriptTool::new();
        let _ = ClickBrowserElementTool::new();
        let _ = FillFormTool::new();
        let _ = BrowserScreenshotTool::new();
        let _ = GoBackTool::new();
        let _ = GoForwardTool::new();
        let _ = ReloadPageTool::new();
        let _ = GetCookiesTool::new();
        let _ = GetPageUrlTool::new();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_tool_names_unique() {
        let tools: Vec<Box<dyn ToolHandler>> = vec![
            Box::new(ListTabsTool::new()),
            Box::new(OpenTabTool::new()),
            Box::new(CloseTabTool::new()),
            Box::new(SwitchTabTool::new()),
            Box::new(GetPageContentTool::new()),
            Box::new(ExecuteJavaScriptTool::new()),
            Box::new(ClickBrowserElementTool::new()),
            Box::new(FillFormTool::new()),
            Box::new(BrowserScreenshotTool::new()),
            Box::new(GoBackTool::new()),
            Box::new(GoForwardTool::new()),
            Box::new(ReloadPageTool::new()),
            Box::new(GetCookiesTool::new()),
            Box::new(GetPageUrlTool::new()),
        ];
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        let unique: std::collections::HashSet<&&str> = names.iter().collect();
        assert_eq!(names.len(), unique.len(), "Duplicate tool names found");
    }

    #[tokio::test]
    async fn test_open_tab_missing_url() {
        // This test validates parameter checking without needing Safari
        // We can't construct the tool on non-macOS, so gate it
        #[cfg(target_os = "macos")]
        {
            let tool = OpenTabTool::new();
            let result = tool.execute(serde_json::json!({})).await;
            assert!(result.is_err());
        }
    }

    #[tokio::test]
    async fn test_execute_js_too_long() {
        #[cfg(target_os = "macos")]
        {
            let tool = ExecuteJavaScriptTool::new();
            let long_script = "x".repeat(100_001);
            let result = tool.execute(serde_json::json!({"script": long_script})).await;
            assert!(result.is_err());
        }
    }
}
```

**Step 3: Run check**

Run: `cargo check -p meepo-core 2>&1 | head -30`
Expected: Compiles

**Step 4: Run tests**

Run: `cargo test -p meepo-core -- browser 2>&1 | tail -20`
Expected: All tests pass

**Step 5: Commit**

```bash
git add crates/meepo-core/src/tools/browser.rs crates/meepo-core/src/tools/mod.rs
git commit -m "feat: add 14 browser automation tools (tab, page, navigation, cookies)"
```

---

### Task 4: Register browser tools in main.rs

**Files:**
- Modify: `crates/meepo-cli/src/main.rs`

**Step 1: Add browser tool registration**

Add after the `#[cfg(target_os = "macos")]` block that registers `SearchContactsTool` (around line ~427), add:

```rust
    // Browser automation tools (macOS: Safari via AppleScript)
    #[cfg(target_os = "macos")]
    {
        registry.register(Arc::new(meepo_core::tools::browser::ListTabsTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::OpenTabTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::CloseTabTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::SwitchTabTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::GetPageContentTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::ExecuteJavaScriptTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::ClickBrowserElementTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::FillFormTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::BrowserScreenshotTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::GoBackTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::GoForwardTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::ReloadPageTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::GetCookiesTool::new()));
        registry.register(Arc::new(meepo_core::tools::browser::GetPageUrlTool::new()));
    }
```

Also add the same block in `cmd_chat` (the second tool registration location, around line ~1270) if it exists.

**Step 2: Run check**

Run: `cargo check -p meepo-cli 2>&1 | head -20`
Expected: Compiles

**Step 3: Commit**

```bash
git add crates/meepo-cli/src/main.rs
git commit -m "feat: register 14 browser automation tools in CLI"
```

---

### Task 5: Build and verify

**Step 1: Full build**

Run: `cargo build -p meepo-cli 2>&1 | tail -10`
Expected: Compiles successfully

**Step 2: Run all tests**

Run: `cargo test -p meepo-core 2>&1 | tail -20`
Expected: All tests pass

**Step 3: Commit all**

If any remaining uncommitted changes:
```bash
git add -A && git commit -m "feat: browser automation — Safari AppleScript + 14 tools"
```
