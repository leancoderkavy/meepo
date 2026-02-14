//! macOS platform implementations using AppleScript

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command;
use tracing::{debug, warn};

use super::{
    BrowserCookie, BrowserProvider, BrowserTab, CalendarProvider, ContactsProvider, EmailProvider,
    MusicProvider, NotesProvider, NotificationProvider, PageContent, RemindersProvider,
    ScreenCaptureProvider, UiAutomation,
};

/// Sanitize a string for safe use in AppleScript
fn sanitize_applescript_string(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
        .chars()
        .filter(|&c| c >= ' ' || c == '\t')
        .collect()
}

/// Validate screenshot output path to prevent writing to sensitive locations
fn validate_screenshot_path(path: &str) -> Result<()> {
    if path.contains("..") {
        return Err(anyhow::anyhow!(
            "Screenshot path contains '..' which is not allowed"
        ));
    }

    let path_buf = std::path::PathBuf::from(path);

    // Resolve parent directory to check location
    let check_path = if let Some(parent) = path_buf.parent() {
        if parent.as_os_str().is_empty() || !parent.exists() {
            path_buf.clone()
        } else {
            parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf())
                .join(path_buf.file_name().unwrap_or_default())
        }
    } else {
        path_buf.clone()
    };

    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    let temp_dir = std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir());

    let is_in_home = check_path.starts_with(&home_dir);
    let is_in_temp = check_path.starts_with(&temp_dir);

    if !is_in_home && !is_in_temp {
        return Err(anyhow::anyhow!(
            "Screenshot path '{}' must be within home or temp directory",
            path
        ));
    }

    // Block system directories even if under home
    let system_dirs = [
        "/etc",
        "/bin",
        "/sbin",
        "/usr/bin",
        "/usr/sbin",
        "/System",
        "/Library",
    ];
    for sys_dir in &system_dirs {
        if check_path.starts_with(sys_dir) {
            return Err(anyhow::anyhow!(
                "Screenshot path cannot target system directory '{}'",
                sys_dir
            ));
        }
    }

    Ok(())
}

/// Run an AppleScript with 30 second timeout
async fn run_applescript(script: &str) -> Result<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        Command::new("osascript").arg("-e").arg(script).output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("AppleScript execution timed out after 30 seconds"))?
    .context("Failed to execute osascript")?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let error = String::from_utf8_lossy(&output.stderr).to_string();
        warn!("AppleScript failed: {}", error);
        Err(anyhow::anyhow!("AppleScript failed: {}", error))
    }
}

pub struct MacOsEmailProvider;

#[async_trait]
impl EmailProvider for MacOsEmailProvider {
    async fn read_emails(&self, limit: u64, mailbox: &str, search: Option<&str>) -> Result<String> {
        let safe_mailbox = match mailbox.to_lowercase().as_str() {
            "inbox" => "inbox",
            "sent" => "sent mailbox",
            "drafts" => "drafts",
            "trash" => "trash",
            _ => "inbox",
        };
        let filter_clause = if let Some(term) = search {
            let safe_term = sanitize_applescript_string(term);
            format!(
                r#" whose (subject contains "{}" or sender contains "{}")"#,
                safe_term, safe_term
            )
        } else {
            String::new()
        };
        debug!("Reading {} emails from Mail.app ({})", limit, mailbox);
        let script = format!(
            r#"
tell application "Mail"
    try
        set msgs to (messages 1 thru {} of {}{})
        set output to ""
        repeat with m in msgs
            set msgBody to content of m
            if length of msgBody > 500 then
                set msgBody to text 1 thru 500 of msgBody
            end if
            set output to output & "From: " & (sender of m) & "\n"
            set output to output & "Subject: " & (subject of m) & "\n"
            set output to output & "Date: " & (date received of m as string) & "\n"
            set output to output & "Preview: " & msgBody & "\n"
            set output to output & "---\n"
        end repeat
        return output
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
            limit, safe_mailbox, filter_clause
        );
        run_applescript(&script).await
    }

    async fn send_email(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        cc: Option<&str>,
        in_reply_to: Option<&str>,
    ) -> Result<String> {
        let safe_to = sanitize_applescript_string(to);
        let safe_subject = sanitize_applescript_string(subject);
        let safe_body = sanitize_applescript_string(body);

        let script = if let Some(reply_subject) = in_reply_to {
            let safe_reply_subject = sanitize_applescript_string(reply_subject);
            debug!("Replying to email with subject: {}", reply_subject);
            format!(
                r#"
tell application "Mail"
    try
        set targetMsgs to (every message of inbox whose subject contains "{}")
        if (count of targetMsgs) > 0 then
            set originalMsg to item 1 of targetMsgs
            set replyMsg to reply originalMsg with opening window
            set content of replyMsg to "{}"
            send replyMsg
            return "Reply sent (threaded)"
        else
            set newMessage to make new outgoing message with properties {{subject:"{}", content:"{}", visible:true}}
            tell newMessage
                make new to recipient at end of to recipients with properties {{address:"{}"}}
                send
            end tell
            return "Email sent (no original found for threading)"
        end if
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
                safe_reply_subject, safe_body, safe_subject, safe_body, safe_to
            )
        } else {
            debug!("Sending new email to: {}", to);
            let cc_block = if let Some(cc_addr) = cc {
                let safe_cc = sanitize_applescript_string(cc_addr);
                format!(
                    r#"
                make new cc recipient at end of cc recipients with properties {{address:"{}"}}"#,
                    safe_cc
                )
            } else {
                String::new()
            };
            format!(
                r#"
tell application "Mail"
    try
        set newMessage to make new outgoing message with properties {{subject:"{}", content:"{}", visible:true}}
        tell newMessage
            make new to recipient at end of to recipients with properties {{address:"{}"}}{}
            send
        end tell
        return "Email sent successfully"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
                safe_subject, safe_body, safe_to, cc_block
            )
        };
        run_applescript(&script).await
    }
}

pub struct MacOsCalendarProvider;

#[async_trait]
impl CalendarProvider for MacOsCalendarProvider {
    async fn read_events(&self, days_ahead: u64) -> Result<String> {
        debug!("Reading calendar events for next {} days", days_ahead);
        let script = format!(
            r#"
tell application "Calendar"
    try
        set startDate to current date
        set endDate to (current date) + ({} * days)
        set output to ""
        repeat with cal in calendars
            set calName to name of cal
            set theEvents to (every event of cal whose start date is greater than or equal to startDate and start date is less than or equal to endDate)
            repeat with evt in theEvents
                set output to output & "Calendar: " & calName & "\n"
                set output to output & "Event: " & (summary of evt) & "\n"
                set output to output & "Start: " & (start date of evt as string) & "\n"
                set output to output & "End: " & (end date of evt as string) & "\n"
                set output to output & "---\n"
            end repeat
        end repeat
        return output
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
            days_ahead
        );
        run_applescript(&script).await
    }

    async fn create_event(
        &self,
        summary: &str,
        start_time: &str,
        duration_minutes: u64,
    ) -> Result<String> {
        debug!("Creating calendar event: {}", summary);
        let safe_summary = sanitize_applescript_string(summary);
        let safe_start_time = sanitize_applescript_string(start_time);
        let script = format!(
            r#"
tell application "Calendar"
    try
        set startDate to date "{}"
        set endDate to startDate + ({} * minutes)
        set targetCal to first calendar
        tell targetCal
            make new event with properties {{summary:"{}", start date:startDate, end date:endDate}}
        end tell
        return "Event created successfully in calendar: " & (name of targetCal)
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
            safe_start_time, duration_minutes, safe_summary
        );
        run_applescript(&script).await
    }
}

/// Allowlist of valid UI element types for macOS accessibility
const VALID_ELEMENT_TYPES: &[&str] = &[
    "button",
    "checkbox",
    "radio button",
    "text field",
    "text area",
    "pop up button",
    "menu item",
    "menu button",
    "slider",
    "tab group",
    "table",
    "outline",
    "list",
    "scroll area",
    "group",
    "window",
    "sheet",
    "toolbar",
    "static text",
    "image",
    "link",
    "cell",
    "row",
    "column",
    "combo box",
    "incrementor",
    "relevance indicator",
];

pub struct MacOsUiAutomation;

#[async_trait]
impl UiAutomation for MacOsUiAutomation {
    async fn read_screen(&self) -> Result<String> {
        debug!("Reading screen information");
        let script = r#"
tell application "System Events"
    try
        set frontApp to first application process whose frontmost is true
        set appName to name of frontApp
        try
            set windowTitle to name of front window of frontApp
            return "App: " & appName & "\nWindow: " & windowTitle
        on error
            return "App: " & appName & "\nWindow: (no window)"
        end try
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#;
        run_applescript(script).await
    }

    async fn click_element(&self, element_name: &str, element_type: &str) -> Result<String> {
        if !VALID_ELEMENT_TYPES
            .iter()
            .any(|&valid| valid.eq_ignore_ascii_case(element_type))
        {
            return Err(anyhow::anyhow!("Invalid element type: {}", element_type));
        }
        debug!("Clicking {} element: {}", element_type, element_name);
        let safe_element_name = sanitize_applescript_string(element_name);
        let script = format!(
            r#"
tell application "System Events"
    try
        set frontApp to first application process whose frontmost is true
        tell frontApp
            click {} "{}"
        end tell
        return "Clicked successfully"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
            element_type, safe_element_name
        );
        run_applescript(&script).await
    }

    async fn type_text(&self, text: &str) -> Result<String> {
        debug!("Typing text ({} chars)", text.len());
        let safe_text = sanitize_applescript_string(text);
        let script = format!(
            r#"
tell application "System Events"
    try
        keystroke "{}"
        return "Text typed successfully"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
            safe_text.replace('\n', "\" & return & \"")
        );
        run_applescript(&script).await
    }
}

pub struct MacOsRemindersProvider;

#[async_trait]
impl RemindersProvider for MacOsRemindersProvider {
    async fn list_reminders(&self, list_name: Option<&str>) -> Result<String> {
        let list_clause = if let Some(name) = list_name {
            let safe = sanitize_applescript_string(name);
            format!(r#"list "{}""#, safe)
        } else {
            "default list".to_string()
        };
        debug!("Listing reminders from {}", list_clause);
        let script = format!(
            r#"
tell application "Reminders"
    try
        set theList to {}
        set output to "List: " & (name of theList) & "\n---\n"
        set theReminders to (reminders of theList whose completed is false)
        repeat with r in theReminders
            set output to output & "- " & (name of r) & "\n"
            try
                set d to due date of r
                set output to output & "  Due: " & (d as string) & "\n"
            end try
            try
                set n to body of r
                if n is not missing value and n is not "" then
                    set output to output & "  Notes: " & n & "\n"
                end if
            end try
        end repeat
        if (count of theReminders) = 0 then
            set output to output & "(no incomplete reminders)\n"
        end if
        return output
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
            list_clause
        );
        run_applescript(&script).await
    }

    async fn create_reminder(
        &self,
        name: &str,
        list_name: Option<&str>,
        due_date: Option<&str>,
        notes: Option<&str>,
    ) -> Result<String> {
        let safe_name = sanitize_applescript_string(name);
        let list_clause = if let Some(ln) = list_name {
            let safe = sanitize_applescript_string(ln);
            format!(r#"list "{}""#, safe)
        } else {
            "default list".to_string()
        };
        let props = {
            let mut p = format!(r#"{{name:"{}""#, safe_name);
            if let Some(notes_text) = notes {
                let safe_notes = sanitize_applescript_string(notes_text);
                p.push_str(&format!(r#", body:"{}""#, safe_notes));
            }
            p.push('}');
            p
        };
        let due_clause = if let Some(due) = due_date {
            let safe_due = sanitize_applescript_string(due);
            format!(
                r#"
            set due date of newReminder to date "{}""#,
                safe_due
            )
        } else {
            String::new()
        };
        debug!("Creating reminder: {}", name);
        let script = format!(
            r#"
tell application "Reminders"
    try
        set newReminder to make new reminder at end of {} with properties {}{}
        return "Reminder created: " & (name of newReminder)
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
            list_clause, props, due_clause
        );
        run_applescript(&script).await
    }
}

pub struct MacOsNotesProvider;

#[async_trait]
impl NotesProvider for MacOsNotesProvider {
    async fn list_notes(&self, folder: Option<&str>, limit: u64) -> Result<String> {
        let limit = limit.min(50);
        let folder_clause = if let Some(f) = folder {
            let safe = sanitize_applescript_string(f);
            format!(r#"notes of folder "{}""#, safe)
        } else {
            "notes".to_string()
        };
        debug!("Listing {} notes", limit);
        let script = format!(
            r#"
tell application "Notes"
    try
        set allNotes to {}
        set maxCount to {}
        if (count of allNotes) < maxCount then
            set maxCount to (count of allNotes)
        end if
        set output to ""
        repeat with i from 1 to maxCount
            set n to item i of allNotes
            set output to output & "Title: " & (name of n) & "\n"
            set output to output & "Date: " & (modification date of n as string) & "\n"
            set noteBody to plaintext of n
            if length of noteBody > 200 then
                set noteBody to text 1 thru 200 of noteBody
            end if
            set output to output & "Preview: " & noteBody & "\n---\n"
        end repeat
        if maxCount = 0 then
            set output to "(no notes found)\n"
        end if
        return output
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
            folder_clause, limit
        );
        run_applescript(&script).await
    }

    async fn create_note(&self, title: &str, body: &str, folder: Option<&str>) -> Result<String> {
        let safe_title = sanitize_applescript_string(title);
        let safe_body = sanitize_applescript_string(body);
        let html_body = format!(
            "<h1>{}</h1><br>{}",
            safe_title,
            safe_body.replace('\n', "<br>")
        );
        let folder_clause = if let Some(f) = folder {
            let safe = sanitize_applescript_string(f);
            format!(r#" in folder "{}""#, safe)
        } else {
            String::new()
        };
        debug!("Creating note: {}", title);
        let script = format!(
            r#"
tell application "Notes"
    try
        make new note{} with properties {{name:"{}", body:"{}"}}
        return "Note created: {}"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
            folder_clause, safe_title, html_body, safe_title
        );
        run_applescript(&script).await
    }
}

pub struct MacOsNotificationProvider;

#[async_trait]
impl NotificationProvider for MacOsNotificationProvider {
    async fn send_notification(
        &self,
        title: &str,
        message: &str,
        sound: Option<&str>,
    ) -> Result<String> {
        let safe_title = sanitize_applescript_string(title);
        let safe_message = sanitize_applescript_string(message);
        let sound_clause = if let Some(s) = sound {
            let safe_sound = sanitize_applescript_string(s);
            format!(r#" sound name "{}""#, safe_sound)
        } else {
            r#" sound name "default""#.to_string()
        };
        debug!("Sending notification: {}", title);
        let script = format!(
            r#"display notification "{}" with title "{}"{}"#,
            safe_message, safe_title, sound_clause
        );
        run_applescript(&script).await?;
        Ok("Notification sent".to_string())
    }
}

pub struct MacOsScreenCaptureProvider;

#[async_trait]
impl ScreenCaptureProvider for MacOsScreenCaptureProvider {
    async fn capture_screen(&self, path: Option<&str>) -> Result<String> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let output_path = path
            .map(|p| p.to_string())
            .unwrap_or_else(|| format!("/tmp/meepo-screenshot-{}.png", timestamp));

        // Validate output path to prevent writing to sensitive locations
        validate_screenshot_path(&output_path)?;

        debug!("Capturing screen to {}", output_path);
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("screencapture")
                .arg("-x") // silent (no shutter sound)
                .arg(&output_path)
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Screen capture timed out"))?
        .context("Failed to run screencapture")?;

        if output.status.success() {
            Ok(format!("Screenshot saved to {}", output_path))
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("Screen capture failed: {}", error))
        }
    }
}

pub struct MacOsMusicProvider;

#[async_trait]
impl MusicProvider for MacOsMusicProvider {
    async fn get_current_track(&self) -> Result<String> {
        debug!("Getting current track from Music.app");
        let script = r#"
tell application "Music"
    try
        if player state is not stopped then
            set trackName to name of current track
            set trackArtist to artist of current track
            set trackAlbum to album of current track
            set trackDuration to duration of current track
            set playerPos to player position
            set output to "Track: " & trackName & "\n"
            set output to output & "Artist: " & trackArtist & "\n"
            set output to output & "Album: " & trackAlbum & "\n"
            set output to output & "State: " & (player state as string) & "\n"
            set output to output & "Position: " & (playerPos as integer) & "s / " & (trackDuration as integer) & "s"
            return output
        else
            return "No track playing"
        end if
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#;
        run_applescript(script).await
    }

    async fn control_playback(&self, action: &str) -> Result<String> {
        let command = match action.to_lowercase().as_str() {
            "play" => "play",
            "pause" => "pause",
            "stop" => "stop",
            "next" | "skip" => "next track",
            "previous" | "prev" | "back" => "previous track",
            _ => {
                return Err(anyhow::anyhow!(
                    "Invalid action: {}. Use: play, pause, stop, next, previous",
                    action
                ));
            }
        };
        debug!("Music control: {}", command);
        let script = format!(
            r#"
tell application "Music"
    try
        {}
        return "OK: {}"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
            command, command
        );
        run_applescript(&script).await
    }
}

pub struct MacOsContactsProvider;

#[async_trait]
impl ContactsProvider for MacOsContactsProvider {
    async fn search_contacts(&self, query: &str) -> Result<String> {
        let safe_query = sanitize_applescript_string(query);
        debug!("Searching contacts for: {}", query);
        let script = format!(
            r#"
tell application "Contacts"
    try
        set results to (every person whose name contains "{}")
        set output to ""
        set maxResults to 20
        if (count of results) < maxResults then
            set maxResults to (count of results)
        end if
        repeat with i from 1 to maxResults
            set p to item i of results
            set output to output & "Name: " & (name of p) & "\n"
            repeat with e in (emails of p)
                set output to output & "  Email: " & (value of e) & "\n"
            end repeat
            repeat with ph in (phones of p)
                set output to output & "  Phone: " & (value of ph) & "\n"
            end repeat
            set output to output & "---\n"
        end repeat
        if maxResults = 0 then
            return "No contacts found matching '{}'"
        end if
        return output
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#,
            safe_query, safe_query
        );
        run_applescript(&script).await
    }
}

/// Safari browser automation via AppleScript
pub struct MacOsSafariBrowser;

#[async_trait]
impl BrowserProvider for MacOsSafariBrowser {
    async fn list_tabs(&self) -> Result<Vec<BrowserTab>> {
        let script = r#"
tell application "Safari"
    set output to ""
    set winIdx to 1
    repeat with w in windows
        set tabIdx to 1
        repeat with t in tabs of w
            set isActive to (current tab of w is t)
            set activeStr to "false"
            if isActive then set activeStr to "true"
            set output to output & winIdx & "|" & tabIdx & "|" & (name of t) & "|" & (URL of t) & "|" & activeStr & "\n"
            set tabIdx to tabIdx + 1
        end repeat
        set winIdx to winIdx + 1
    end repeat
    return output
end tell
"#;
        let raw = run_applescript(script).await?;
        let mut tabs = Vec::new();
        for line in raw.lines() {
            let parts: Vec<&str> = line.splitn(5, '|').collect();
            if parts.len() >= 5 {
                let win_idx: u32 = parts[0].trim().parse().unwrap_or(1);
                let tab_idx = parts[1].trim();
                tabs.push(BrowserTab {
                    id: format!("safari:{}:{}", win_idx, tab_idx),
                    title: parts[2].to_string(),
                    url: parts[3].to_string(),
                    is_active: parts[4].trim() == "true",
                    window_index: win_idx,
                });
            }
        }
        Ok(tabs)
    }

    async fn open_tab(&self, url: &str) -> Result<BrowserTab> {
        let safe_url = sanitize_applescript_string(url);
        let script = format!(
            r#"
tell application "Safari"
    activate
    tell window 1
        set newTab to make new tab with properties {{URL:"{}"}}
        set current tab to newTab
        return (name of newTab) & "|" & (URL of newTab)
    end tell
end tell
"#,
            safe_url
        );
        let raw = run_applescript(&script).await?;
        let parts: Vec<&str> = raw.splitn(2, '|').collect();
        Ok(BrowserTab {
            id: "safari:1:new".to_string(),
            title: parts.first().unwrap_or(&"").to_string(),
            url: parts.get(1).unwrap_or(&url).trim().to_string(),
            is_active: true,
            window_index: 1,
        })
    }

    async fn close_tab(&self, tab_id: &str) -> Result<()> {
        let (win, tab) = parse_safari_tab_id(tab_id)?;
        let script = format!(
            r#"
tell application "Safari"
    close tab {} of window {}
end tell
"#,
            tab, win
        );
        run_applescript(&script).await?;
        Ok(())
    }

    async fn switch_tab(&self, tab_id: &str) -> Result<()> {
        let (win, tab) = parse_safari_tab_id(tab_id)?;
        let script = format!(
            r#"
tell application "Safari"
    set current tab of window {} to tab {} of window {}
end tell
"#,
            win, tab, win
        );
        run_applescript(&script).await?;
        Ok(())
    }

    async fn get_page_content(&self, tab_id: Option<&str>) -> Result<PageContent> {
        let tab_clause = safari_tab_clause(tab_id)?;
        let script = format!(
            r#"
tell application "Safari"
    set t to {}
    set pageTitle to name of t
    set pageUrl to URL of t
    set pageText to do JavaScript "document.body.innerText.substring(0, 50000)" in t
    set pageHtml to do JavaScript "document.documentElement.outerHTML.substring(0, 50000)" in t
    return pageTitle & "|||" & pageUrl & "|||" & pageText & "|||" & pageHtml
end tell
"#,
            tab_clause
        );
        let raw = run_applescript(&script).await?;
        let parts: Vec<&str> = raw.splitn(4, "|||").collect();
        Ok(PageContent {
            title: parts.first().unwrap_or(&"").to_string(),
            url: parts.get(1).unwrap_or(&"").to_string(),
            text: parts.get(2).unwrap_or(&"").to_string(),
            html: parts.get(3).unwrap_or(&"").to_string(),
        })
    }

    async fn execute_javascript(&self, tab_id: Option<&str>, script: &str) -> Result<String> {
        let tab_clause = safari_tab_clause(tab_id)?;
        let safe_script = sanitize_applescript_string(script);
        let applescript = format!(
            r#"
tell application "Safari"
    set t to {}
    do JavaScript "{}" in t
end tell
"#,
            tab_clause, safe_script
        );
        run_applescript(&applescript).await
    }

    async fn click_element(&self, tab_id: Option<&str>, selector: &str) -> Result<()> {
        let safe_selector = sanitize_applescript_string(selector);
        let js = format!("document.querySelector('{}').click()", safe_selector);
        self.execute_javascript(tab_id, &js).await?;
        Ok(())
    }

    async fn fill_form(&self, tab_id: Option<&str>, selector: &str, value: &str) -> Result<()> {
        let safe_selector = sanitize_applescript_string(selector);
        let safe_value = sanitize_applescript_string(value);
        let js = format!(
            "var el = document.querySelector('{}'); el.value = '{}'; el.dispatchEvent(new Event('input', {{bubbles: true}}))",
            safe_selector, safe_value
        );
        self.execute_javascript(tab_id, &js).await?;
        Ok(())
    }

    async fn screenshot_page(&self, _tab_id: Option<&str>, path: Option<&str>) -> Result<String> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let output_path = path
            .map(|p| p.to_string())
            .unwrap_or_else(|| format!("/tmp/meepo-browser-screenshot-{}.png", timestamp));

        // Validate path to prevent writing to sensitive locations (H-3 fix)
        validate_screenshot_path(&output_path)?;

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("screencapture")
                .arg("-x")
                .arg(&output_path)
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Screenshot timed out"))?
        .context("Failed to run screencapture")?;
        if output.status.success() {
            Ok(format!("Screenshot saved to {}", output_path))
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("Screenshot failed: {}", error))
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

    async fn get_cookies(&self, _tab_id: Option<&str>) -> Result<Vec<BrowserCookie>> {
        // Cookie access disabled for security — document.cookie bypasses the
        // browser_execute_js blocklist (H-4 fix)
        Err(anyhow::anyhow!(
            "Cookie access is disabled for security. Use browser_execute_js with appropriate permissions instead."
        ))
    }

    async fn get_page_url(&self, tab_id: Option<&str>) -> Result<String> {
        let tab_clause = safari_tab_clause(tab_id)?;
        let script = format!(
            r#"
tell application "Safari"
    return URL of {}
end tell
"#,
            tab_clause
        );
        let url = run_applescript(&script).await?;
        Ok(url.trim().to_string())
    }
}

/// Parse a Safari tab ID like "safari:1:2" into (window, tab) indices
fn parse_safari_tab_id(tab_id: &str) -> Result<(u32, u32)> {
    let parts: Vec<&str> = tab_id.split(':').collect();
    if parts.len() >= 3 && parts[0] == "safari" {
        let win: u32 = parts[1]
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid window index in tab_id"))?;
        let tab: u32 = parts[2]
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid tab index in tab_id"))?;
        Ok((win, tab))
    } else {
        Err(anyhow::anyhow!(
            "Invalid Safari tab_id format: expected 'safari:window:tab'"
        ))
    }
}

/// Build AppleScript clause to reference a Safari tab
fn safari_tab_clause(tab_id: Option<&str>) -> Result<String> {
    match tab_id {
        Some(id) => {
            let (win, tab) = parse_safari_tab_id(id)?;
            Ok(format!("tab {} of window {}", tab, win))
        }
        None => Ok("current tab of window 1".to_string()),
    }
}

/// Google Chrome browser automation via AppleScript
pub struct MacOsChromeBrowser;

#[async_trait]
impl BrowserProvider for MacOsChromeBrowser {
    async fn list_tabs(&self) -> Result<Vec<BrowserTab>> {
        let script = r#"
tell application "Google Chrome"
    set output to ""
    set winIdx to 1
    repeat with w in windows
        set tabIdx to 1
        repeat with t in tabs of w
            set isActive to (active tab index of w is tabIdx)
            set activeStr to "false"
            if isActive then set activeStr to "true"
            set output to output & winIdx & "|" & tabIdx & "|" & (title of t) & "|" & (URL of t) & "|" & activeStr & "\n"
            set tabIdx to tabIdx + 1
        end repeat
        set winIdx to winIdx + 1
    end repeat
    return output
end tell
"#;
        let raw = run_applescript(script).await?;
        let mut tabs = Vec::new();
        for line in raw.lines() {
            let parts: Vec<&str> = line.splitn(5, '|').collect();
            if parts.len() >= 5 {
                let win_idx: u32 = parts[0].trim().parse().unwrap_or(1);
                let tab_idx = parts[1].trim();
                tabs.push(BrowserTab {
                    id: format!("chrome:{}:{}", win_idx, tab_idx),
                    title: parts[2].to_string(),
                    url: parts[3].to_string(),
                    is_active: parts[4].trim() == "true",
                    window_index: win_idx,
                });
            }
        }
        Ok(tabs)
    }

    async fn open_tab(&self, url: &str) -> Result<BrowserTab> {
        let safe_url = sanitize_applescript_string(url);
        let script = format!(
            r#"
tell application "Google Chrome"
    activate
    tell window 1
        set newTab to make new tab with properties {{URL:"{}"}}
        return (title of newTab) & "|" & (URL of newTab)
    end tell
end tell
"#,
            safe_url
        );
        let raw = run_applescript(&script).await?;
        let parts: Vec<&str> = raw.splitn(2, '|').collect();
        Ok(BrowserTab {
            id: "chrome:1:new".to_string(),
            title: parts.first().unwrap_or(&"").to_string(),
            url: parts.get(1).unwrap_or(&url).trim().to_string(),
            is_active: true,
            window_index: 1,
        })
    }

    async fn close_tab(&self, tab_id: &str) -> Result<()> {
        let (win, tab) = parse_chrome_tab_id(tab_id)?;
        let script = format!(
            r#"
tell application "Google Chrome"
    close tab {} of window {}
end tell
"#,
            tab, win
        );
        run_applescript(&script).await?;
        Ok(())
    }

    async fn switch_tab(&self, tab_id: &str) -> Result<()> {
        let (win, tab) = parse_chrome_tab_id(tab_id)?;
        let script = format!(
            r#"
tell application "Google Chrome"
    set active tab index of window {} to {}
end tell
"#,
            win, tab
        );
        run_applescript(&script).await?;
        Ok(())
    }

    async fn get_page_content(&self, tab_id: Option<&str>) -> Result<PageContent> {
        let tab_clause = chrome_tab_clause(tab_id)?;
        let script = format!(
            r#"
tell application "Google Chrome"
    set t to {}
    set pageTitle to title of t
    set pageUrl to URL of t
    set pageText to execute t javascript "document.body.innerText.substring(0, 50000)"
    set pageHtml to execute t javascript "document.documentElement.outerHTML.substring(0, 50000)"
    return pageTitle & "|||" & pageUrl & "|||" & pageText & "|||" & pageHtml
end tell
"#,
            tab_clause
        );
        let raw = run_applescript(&script).await?;
        let parts: Vec<&str> = raw.splitn(4, "|||").collect();
        Ok(PageContent {
            title: parts.first().unwrap_or(&"").to_string(),
            url: parts.get(1).unwrap_or(&"").to_string(),
            text: parts.get(2).unwrap_or(&"").to_string(),
            html: parts.get(3).unwrap_or(&"").to_string(),
        })
    }

    async fn execute_javascript(&self, tab_id: Option<&str>, script: &str) -> Result<String> {
        let tab_clause = chrome_tab_clause(tab_id)?;
        let safe_script = sanitize_applescript_string(script);
        let applescript = format!(
            r#"
tell application "Google Chrome"
    set t to {}
    execute t javascript "{}"
end tell
"#,
            tab_clause, safe_script
        );
        run_applescript(&applescript).await
    }

    async fn click_element(&self, tab_id: Option<&str>, selector: &str) -> Result<()> {
        let safe_selector = sanitize_applescript_string(selector);
        let js = format!("document.querySelector('{}').click()", safe_selector);
        self.execute_javascript(tab_id, &js).await?;
        Ok(())
    }

    async fn fill_form(&self, tab_id: Option<&str>, selector: &str, value: &str) -> Result<()> {
        let safe_selector = sanitize_applescript_string(selector);
        let safe_value = sanitize_applescript_string(value);
        let js = format!(
            "var el = document.querySelector('{}'); el.value = '{}'; el.dispatchEvent(new Event('input', {{bubbles: true}}))",
            safe_selector, safe_value
        );
        self.execute_javascript(tab_id, &js).await?;
        Ok(())
    }

    async fn screenshot_page(&self, _tab_id: Option<&str>, path: Option<&str>) -> Result<String> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let output_path = path
            .map(|p| p.to_string())
            .unwrap_or_else(|| format!("/tmp/meepo-browser-screenshot-{}.png", timestamp));

        // Validate path to prevent writing to sensitive locations (H-3 fix)
        validate_screenshot_path(&output_path)?;

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("screencapture")
                .arg("-x")
                .arg(&output_path)
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Screenshot timed out"))?
        .context("Failed to run screencapture")?;
        if output.status.success() {
            Ok(format!("Screenshot saved to {}", output_path))
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("Screenshot failed: {}", error))
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

    async fn get_cookies(&self, _tab_id: Option<&str>) -> Result<Vec<BrowserCookie>> {
        // Cookie access disabled for security — document.cookie bypasses the
        // browser_execute_js blocklist (H-4 fix)
        Err(anyhow::anyhow!(
            "Cookie access is disabled for security. Use browser_execute_js with appropriate permissions instead."
        ))
    }

    async fn get_page_url(&self, tab_id: Option<&str>) -> Result<String> {
        let tab_clause = chrome_tab_clause(tab_id)?;
        let script = format!(
            r#"
tell application "Google Chrome"
    return URL of {}
end tell
"#,
            tab_clause
        );
        let url = run_applescript(&script).await?;
        Ok(url.trim().to_string())
    }
}

/// Parse a Chrome tab ID like "chrome:1:2" into (window, tab) indices
fn parse_chrome_tab_id(tab_id: &str) -> Result<(u32, u32)> {
    let parts: Vec<&str> = tab_id.split(':').collect();
    if parts.len() >= 3 && parts[0] == "chrome" {
        let win: u32 = parts[1]
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid window index in tab_id"))?;
        let tab: u32 = parts[2]
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid tab index in tab_id"))?;
        Ok((win, tab))
    } else {
        Err(anyhow::anyhow!(
            "Invalid Chrome tab_id format: expected 'chrome:window:tab'"
        ))
    }
}

/// Build AppleScript clause to reference a Chrome tab
fn chrome_tab_clause(tab_id: Option<&str>) -> Result<String> {
    match tab_id {
        Some(id) => {
            let (win, tab) = parse_chrome_tab_id(id)?;
            Ok(format!("tab {} of window {}", tab, win))
        }
        None => Ok("active tab of window 1".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_applescript_string() {
        assert_eq!(sanitize_applescript_string("test\\path"), "test\\\\path");
        assert_eq!(sanitize_applescript_string("test\"quote"), "test\\\"quote");
        assert_eq!(sanitize_applescript_string("test\nline"), "test line");
        assert_eq!(sanitize_applescript_string("test\rline"), "test line");
        let with_control = "test\x01\x02\x03text";
        assert_eq!(sanitize_applescript_string(with_control), "testtext");
    }

    #[test]
    fn test_sanitize_prevents_injection() {
        let attack = "test\"; do shell script \"rm -rf /\" --\"";
        let safe = sanitize_applescript_string(attack);
        assert!(!safe.contains('\n'));
        assert!(safe.contains("\\\""));
    }
}
