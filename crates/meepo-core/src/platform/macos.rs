//! macOS platform implementations using AppleScript

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command;
use tracing::{debug, info, warn};

use super::{
    BrowserCookie, BrowserProvider, BrowserTab, CalendarProvider, ContactsProvider, EmailProvider,
    FinderProvider, KeychainProvider, MediaProvider, MessagesProvider, MusicProvider,
    NotesProvider, NotificationProvider, PageContent, PhotosProvider, ProductivityProvider,
    RemindersProvider, ScreenCaptureProvider, ShortcutsProvider, SpotlightProvider,
    SystemControlProvider, TerminalProvider, UiAutomation, WindowManagerProvider,
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

/// Check if an application is currently running
async fn is_app_running(app_name: &str) -> bool {
    let safe_name = sanitize_applescript_string(app_name);
    let script = format!(
        r#"tell application "System Events" to (name of processes) contains "{}"
"#,
        safe_name
    );
    match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        Command::new("osascript").arg("-e").arg(&script).output(),
    )
    .await
    {
        Ok(Ok(output)) => String::from_utf8_lossy(&output.stdout).trim() == "true",
        _ => false,
    }
}

/// Ensure Mail.app is running before executing a heavy query.
/// If not running, launches it and waits for it to be ready.
async fn ensure_mail_app_running() -> Result<()> {
    if is_app_running("Mail").await {
        return Ok(());
    }

    info!("Mail.app not running, launching it...");
    let launch_script = r#"tell application "Mail" to activate"#;
    // Note: "Mail" is hardcoded here, not user input — safe from injection
    // Best-effort launch: log warning on failure but don't abort
    match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        Command::new("osascript")
            .arg("-e")
            .arg(launch_script)
            .output(),
    )
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => warn!("Failed to launch Mail.app: {}", e),
        Err(_) => warn!("Timed out launching Mail.app"),
    }

    // Wait for Mail.app to finish launching (poll up to 30s)
    for _ in 0..15 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if is_app_running("Mail").await {
            debug!("Mail.app is now running");
            // Give it a moment to finish initial sync
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            return Ok(());
        }
    }

    warn!("Mail.app may not have fully launched, proceeding anyway");
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

/// Run an AppleScript with retry logic and configurable timeout.
/// Retries up to `max_retries` times with exponential backoff (2s, 4s, 8s...).
async fn run_applescript_with_retry(
    script: &str,
    timeout_secs: u64,
    max_retries: u32,
) -> Result<String> {
    let mut last_err = anyhow::anyhow!("AppleScript execution failed");

    for attempt in 0..=max_retries {
        if attempt > 0 {
            let backoff = std::time::Duration::from_secs(2u64.pow(attempt));
            debug!(
                "Retrying AppleScript (attempt {}/{}, backoff {:?})",
                attempt + 1,
                max_retries + 1,
                backoff
            );
            tokio::time::sleep(backoff).await;
        }

        match tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            Command::new("osascript").arg("-e").arg(script).output(),
        )
        .await
        {
            Ok(Ok(output)) if output.status.success() => {
                return Ok(String::from_utf8_lossy(&output.stdout).to_string());
            }
            Ok(Ok(output)) => {
                let error = String::from_utf8_lossy(&output.stderr).to_string();
                warn!("AppleScript failed (attempt {}): {}", attempt + 1, error);
                last_err = anyhow::anyhow!("AppleScript failed: {}", error);
            }
            Ok(Err(e)) => {
                warn!("osascript process error (attempt {}): {}", attempt + 1, e);
                last_err = anyhow::anyhow!("Failed to execute osascript: {}", e);
            }
            Err(_) => {
                warn!(
                    "AppleScript timed out after {}s (attempt {})",
                    timeout_secs,
                    attempt + 1
                );
                last_err = anyhow::anyhow!(
                    "AppleScript execution timed out after {} seconds",
                    timeout_secs
                );
            }
        }
    }

    Err(last_err)
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

        // Ensure Mail.app is running before querying — cold launch can exceed normal timeouts
        ensure_mail_app_running().await?;

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
        // Use 60s timeout with 2 retries (backoff: 2s, 4s) for resilience
        run_applescript_with_retry(&script, 60, 2).await
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

        // Ensure Mail.app is running before sending — avoids cryptic AppleScript errors
        ensure_mail_app_running().await?;

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
        // Use 60s timeout with 2 retries for resilience against slow Calendar.app responses
        run_applescript_with_retry(&script, 60, 2).await
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

// ── Notifications ──────────────────────────────────────────────────────────

pub struct MacOsNotificationProvider;

#[async_trait]
impl NotificationProvider for MacOsNotificationProvider {
    async fn send_notification(
        &self,
        title: &str,
        message: &str,
        sound: Option<&str>,
    ) -> Result<String> {
        if title.len() > 200 {
            return Err(anyhow::anyhow!("Title too long"));
        }
        if message.len() > 2000 {
            return Err(anyhow::anyhow!("Message too long"));
        }
        let safe_title = sanitize_applescript_string(title);
        let safe_message = sanitize_applescript_string(message);
        let sound_clause = if let Some(s) = sound {
            let safe_sound = sanitize_applescript_string(s);
            format!(r#" sound name "{}""#, safe_sound)
        } else {
            String::new()
        };
        debug!("Sending notification: {}", title);
        run_applescript(&format!(
            r#"display notification "{}" with title "{}"{}"#,
            safe_message, safe_title, sound_clause
        ))
        .await?;
        Ok(format!("Notification sent: {}", title))
    }
}

// ── Screen Capture ─────────────────────────────────────────────────────────

pub struct MacOsScreenCaptureProvider;

#[async_trait]
impl ScreenCaptureProvider for MacOsScreenCaptureProvider {
    async fn capture_screen(&self, path: Option<&str>) -> Result<String> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let output_path = path
            .map(|p| p.to_string())
            .unwrap_or_else(|| format!("/tmp/meepo-screenshot-{}.png", timestamp));
        if output_path.contains("..") {
            return Err(anyhow::anyhow!("Path cannot contain '..'"));
        }
        validate_screenshot_path(&output_path)?;
        debug!("Capturing screen to {}", output_path);
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("screencapture")
                .arg("-x")
                .arg(&output_path)
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Screen capture timed out"))?
        .context("Failed to run screencapture")?;
        if output.status.success() {
            Ok(format!("Screenshot saved to {}", output_path))
        } else {
            Err(anyhow::anyhow!(
                "Screen capture failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

// ── Music ──────────────────────────────────────────────────────────────────

pub struct MacOsMusicProvider;

#[async_trait]
impl MusicProvider for MacOsMusicProvider {
    async fn get_current_track(&self) -> Result<String> {
        debug!("Getting current track");
        run_applescript(
            r#"
tell application "Music"
    if player state is playing then
        set trackName to name of current track
        set trackArtist to artist of current track
        set trackAlbum to album of current track
        set trackDuration to duration of current track
        set trackPosition to player position
        return "Playing: " & trackName & " by " & trackArtist & " from " & trackAlbum & " (" & (round trackPosition) & "s / " & (round trackDuration) & "s)"
    else
        return "Not playing"
    end if
end tell"#,
        )
        .await
    }

    async fn control_playback(&self, action: &str) -> Result<String> {
        debug!("Music control: {}", action);
        let script = match action.to_lowercase().as_str() {
            "play" => r#"tell application "Music" to play
return "Playing""#
                .to_string(),
            "pause" => r#"tell application "Music" to pause
return "Paused""#
                .to_string(),
            "next" | "skip" => r#"tell application "Music" to next track
return "Skipped to next track""#
                .to_string(),
            "previous" | "prev" | "back" => r#"tell application "Music" to previous track
return "Went to previous track""#
                .to_string(),
            "toggle" => r#"tell application "Music" to playpause
return "Toggled playback""#
                .to_string(),
            _ => {
                return Err(anyhow::anyhow!(
                    "Unknown action: {}. Supported: play, pause, next, previous, toggle",
                    action
                ));
            }
        };
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

    async fn scroll(&self, tab_id: Option<&str>, direction: &str, amount: u32) -> Result<()> {
        let tab_clause = safari_tab_clause(tab_id)?;
        let js_scroll = match direction {
            "up" => format!("window.scrollBy(0, -{})", amount),
            "down" => format!("window.scrollBy(0, {})", amount),
            "left" => format!("window.scrollBy(-{}, 0)", amount),
            "right" => format!("window.scrollBy({}, 0)", amount),
            _ => return Err(anyhow::anyhow!("Invalid scroll direction: {}", direction)),
        };
        let script = format!(
            r#"tell application "Safari" to do JavaScript "{}" in {}"#,
            js_scroll, tab_clause
        );
        run_applescript(&script).await?;
        Ok(())
    }

    async fn wait_for_element(
        &self,
        tab_id: Option<&str>,
        selector: &str,
        timeout_ms: u64,
    ) -> Result<bool> {
        let tab_clause = safari_tab_clause(tab_id)?;
        let safe_selector = sanitize_applescript_string(selector);
        let poll_ms = 200;
        let max_polls = timeout_ms / poll_ms;
        let script = format!(
            r#"
tell application "Safari"
    set found to false
    repeat {} times
        set result to (do JavaScript "document.querySelector('{}') !== null" in {})
        if result is "true" then
            set found to true
            exit repeat
        end if
        delay {}
    end repeat
    return found as text
end tell
"#,
            max_polls,
            safe_selector,
            tab_clause,
            poll_ms as f64 / 1000.0
        );
        let result = run_applescript(&script).await?;
        Ok(result.trim() == "true")
    }

    async fn screenshot_tab(&self, _tab_id: Option<&str>, path: Option<&str>) -> Result<String> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let output_path = path
            .map(|p| p.to_string())
            .unwrap_or_else(|| format!("/tmp/meepo-safari-screenshot-{}.png", timestamp));
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

    async fn scroll(&self, tab_id: Option<&str>, direction: &str, amount: u32) -> Result<()> {
        let js_scroll = match direction {
            "up" => format!("window.scrollBy(0, -{})", amount),
            "down" => format!("window.scrollBy(0, {})", amount),
            "left" => format!("window.scrollBy(-{}, 0)", amount),
            "right" => format!("window.scrollBy({}, 0)", amount),
            _ => return Err(anyhow::anyhow!("Invalid scroll direction: {}", direction)),
        };
        self.execute_javascript(tab_id, &js_scroll).await?;
        Ok(())
    }

    async fn wait_for_element(
        &self,
        tab_id: Option<&str>,
        selector: &str,
        timeout_ms: u64,
    ) -> Result<bool> {
        let safe_selector = sanitize_applescript_string(selector);
        let poll_ms = 200u64;
        let max_polls = timeout_ms / poll_ms;
        for _ in 0..max_polls {
            let js = format!("document.querySelector('{}') !== null", safe_selector);
            let result = self.execute_javascript(tab_id, &js).await?;
            if result.trim() == "true" {
                return Ok(true);
            }
            tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
        }
        Ok(false)
    }

    async fn screenshot_tab(&self, _tab_id: Option<&str>, path: Option<&str>) -> Result<String> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let output_path = path
            .map(|p| p.to_string())
            .unwrap_or_else(|| format!("/tmp/meepo-chrome-screenshot-{}.png", timestamp));
        validate_screenshot_path(&output_path)?;
        let output_result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("screencapture")
                .arg("-x")
                .arg(&output_path)
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Screenshot timed out"))?
        .context("Failed to run screencapture")?;
        if output_result.status.success() {
            Ok(format!("Screenshot saved to {}", output_path))
        } else {
            let error = String::from_utf8_lossy(&output_result.stderr);
            Err(anyhow::anyhow!("Screenshot failed: {}", error))
        }
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

// ── System Control ──────────────────────────────────────────────────────────

pub struct MacOsSystemControl;

#[async_trait]
impl SystemControlProvider for MacOsSystemControl {
    async fn get_volume(&self) -> Result<String> {
        let script = r#"output volume of (get volume settings) & "," & input volume of (get volume settings) & "," & output muted of (get volume settings)"#;
        let raw = run_applescript(script).await?;
        let parts: Vec<&str> = raw.trim().split(',').collect();
        Ok(format!(
            "Output volume: {}%\nInput volume: {}%\nMuted: {}",
            parts.first().unwrap_or(&"?").trim(),
            parts.get(1).unwrap_or(&"?").trim(),
            parts.get(2).unwrap_or(&"?").trim()
        ))
    }

    async fn set_volume(&self, level: u8) -> Result<String> {
        let level = level.min(100);
        run_applescript(&format!("set volume output volume {}", level)).await?;
        Ok(format!("Volume set to {}%", level))
    }

    async fn toggle_mute(&self) -> Result<String> {
        run_applescript(
            r#"
set curMuted to output muted of (get volume settings)
set volume output muted (not curMuted)
if curMuted then
    return "Unmuted"
else
    return "Muted"
end if"#,
        )
        .await
    }

    async fn get_dark_mode(&self) -> Result<bool> {
        let result = run_applescript(
            r#"tell application "System Events" to tell appearance preferences to return dark mode"#,
        ).await?;
        Ok(result.trim() == "true")
    }

    async fn set_dark_mode(&self, enabled: bool) -> Result<String> {
        run_applescript(&format!(
            r#"tell application "System Events" to tell appearance preferences to set dark mode to {}"#,
            enabled
        )).await?;
        Ok(format!(
            "Dark mode {}",
            if enabled { "enabled" } else { "disabled" }
        ))
    }

    async fn set_do_not_disturb(&self, enabled: bool) -> Result<String> {
        debug!("Setting Do Not Disturb to {}", enabled);
        let script = if enabled {
            r#"do shell script "defaults -currentHost write com.apple.notificationcenterui doNotDisturb -boolean true && killall NotificationCenter 2>/dev/null || true"
return "Do Not Disturb enabled""#
        } else {
            r#"do shell script "defaults -currentHost write com.apple.notificationcenterui doNotDisturb -boolean false && killall NotificationCenter 2>/dev/null || true"
return "Do Not Disturb disabled""#
        };
        run_applescript(script).await
    }

    async fn get_battery_status(&self) -> Result<String> {
        debug!("Getting battery status");
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("pmset").arg("-g").arg("batt").output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Battery status timed out"))?
        .context("Failed to run pmset")?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(anyhow::anyhow!("Failed to get battery status"))
        }
    }

    async fn get_wifi_info(&self) -> Result<String> {
        debug!("Getting WiFi info");
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("networksetup")
                .args(["-getairportnetwork", "en0"])
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("WiFi info timed out"))?
        .context("Failed to run networksetup")?;
        let mut info = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let ip_output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("ipconfig").args(["getifaddr", "en0"]).output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("IP lookup timed out"))?
        .context("Failed to get IP")?;
        if ip_output.status.success() {
            info.push_str(&format!(
                "\nIP Address: {}",
                String::from_utf8_lossy(&ip_output.stdout).trim()
            ));
        }
        Ok(info)
    }

    async fn get_disk_usage(&self) -> Result<String> {
        debug!("Getting disk usage");
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("df").args(["-h", "/"]).output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Disk usage timed out"))?
        .context("Failed to run df")?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(anyhow::anyhow!("Failed to get disk usage"))
        }
    }

    async fn lock_screen(&self) -> Result<String> {
        debug!("Locking screen");
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            Command::new(
                "/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession",
            )
            .arg("-suspend")
            .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Lock screen timed out"))?
        .context("Failed to lock screen")?;
        if output.status.success() {
            Ok("Screen locked".to_string())
        } else {
            Err(anyhow::anyhow!("Failed to lock screen"))
        }
    }

    async fn sleep_display(&self) -> Result<String> {
        debug!("Sleeping display");
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            Command::new("pmset").arg("displaysleepnow").output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Sleep display timed out"))?
        .context("Failed to sleep display")?;
        if output.status.success() {
            Ok("Display sleeping".to_string())
        } else {
            Err(anyhow::anyhow!("Failed to sleep display"))
        }
    }

    async fn get_running_apps(&self) -> Result<String> {
        debug!("Getting running apps");
        run_applescript(
            r#"
tell application "System Events"
    set output to ""
    repeat with p in (every application process whose background only is false)
        set output to output & (name of p) & "\n"
    end repeat
    return output
end tell"#,
        )
        .await
    }

    async fn quit_app(&self, app_name: &str) -> Result<String> {
        let safe_name = sanitize_applescript_string(app_name);
        if safe_name.len() > 100 {
            return Err(anyhow::anyhow!("App name too long"));
        }
        debug!("Quitting app: {}", app_name);
        run_applescript(&format!(
            r#"
tell application "{}"
    quit
end tell
return "Quit {}"
"#,
            safe_name, safe_name
        ))
        .await
    }

    async fn force_quit_app(&self, app_name: &str) -> Result<String> {
        let safe_name = sanitize_applescript_string(app_name);
        if safe_name.len() > 100 {
            return Err(anyhow::anyhow!("App name too long"));
        }
        debug!("Force quitting app: {}", app_name);
        // Use pkill directly instead of do shell script to avoid shell injection
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("pkill")
                .arg("-9")
                .arg("-x")
                .arg(app_name)
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Force quit timed out"))?
        .context("Failed to run pkill")?;
        if output.status.success() {
            Ok(format!("Force quit {}", app_name))
        } else {
            let exit = output.status.code().unwrap_or(-1);
            if exit == 1 {
                Ok(format!("No process found: {}", app_name))
            } else {
                Err(anyhow::anyhow!(
                    "Force quit failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            }
        }
    }
}

// ── Finder ─────────────────────────────────────────────────────────────────

pub struct MacOsFinderProvider;

#[async_trait]
impl FinderProvider for MacOsFinderProvider {
    async fn get_selection(&self) -> Result<String> {
        debug!("Getting Finder selection");
        run_applescript(
            r#"
tell application "Finder"
    try
        set sel to selection
        if (count of sel) = 0 then return "(no selection)"
        set output to ""
        repeat with f in sel
            set output to output & (POSIX path of (f as alias)) & "\n"
        end repeat
        return output
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#,
        )
        .await
    }

    async fn reveal_in_finder(&self, path: &str) -> Result<String> {
        if path.contains("..") {
            return Err(anyhow::anyhow!("Path cannot contain '..'"));
        }
        if path.len() > 1000 {
            return Err(anyhow::anyhow!("Path too long"));
        }
        debug!("Revealing in Finder: {}", path);
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("open").arg("-R").arg(path).output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Finder reveal timed out"))?
        .context("Failed to reveal in Finder")?;
        if output.status.success() {
            Ok(format!("Revealed in Finder: {}", path))
        } else {
            Err(anyhow::anyhow!(
                "Failed to reveal: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    async fn set_tag(&self, path: &str, tag: &str, remove: bool) -> Result<String> {
        if path.contains("..") {
            return Err(anyhow::anyhow!("Path cannot contain '..'"));
        }
        if path.len() > 1000 || tag.len() > 100 {
            return Err(anyhow::anyhow!("Path or tag too long"));
        }
        let valid_tags = ["Red", "Orange", "Yellow", "Green", "Blue", "Purple", "Gray"];
        if !valid_tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            return Err(anyhow::anyhow!(
                "Invalid tag: {}. Valid: {}",
                tag,
                valid_tags.join(", ")
            ));
        }
        debug!(
            "{} tag '{}' on: {}",
            if remove { "Removing" } else { "Setting" },
            tag,
            path
        );
        // Use Command::new directly to avoid shell injection via single quotes in path
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            if remove {
                Command::new("xattr")
                    .args(["-w", "com.apple.metadata:_kMDItemUserTags", "()", path])
                    .output()
            } else {
                Command::new("xattr")
                    .args([
                        "-w",
                        "com.apple.metadata:_kMDItemUserTags",
                        &format!("(\"{tag}\")"),
                        path,
                    ])
                    .output()
            },
        )
        .await
        .map_err(|_| anyhow::anyhow!("xattr timed out"))?
        .context("Failed to run xattr")?;
        if output.status.success() {
            if remove {
                Ok(format!("Tag removed from {}", path))
            } else {
                Ok(format!("Tagged {} with {}", path, tag))
            }
        } else {
            Err(anyhow::anyhow!(
                "xattr failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    async fn quick_look(&self, path: &str) -> Result<String> {
        if path.contains("..") {
            return Err(anyhow::anyhow!("Path cannot contain '..'"));
        }
        if path.len() > 1000 {
            return Err(anyhow::anyhow!("Path too long"));
        }
        debug!("Quick Look: {}", path);
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("qlmanage").arg("-p").arg(path).output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Quick Look timed out"))?
        .context("Failed to run Quick Look")?;
        if output.status.success() {
            Ok(format!("Quick Look opened for: {}", path))
        } else {
            Err(anyhow::anyhow!(
                "Quick Look failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    async fn trash_file(&self, path: &str) -> Result<String> {
        if path.contains("..") {
            return Err(anyhow::anyhow!("Path cannot contain '..'"));
        }
        if path.len() > 1000 {
            return Err(anyhow::anyhow!("Path too long"));
        }
        let safe_path = sanitize_applescript_string(path);
        debug!("Moving to trash: {}", path);
        run_applescript(&format!(
            r#"
tell application "Finder"
    try
        move POSIX file "{}" to trash
        return "Moved to trash: {}"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#,
            safe_path, safe_path
        ))
        .await
    }

    async fn empty_trash(&self) -> Result<String> {
        debug!("Emptying trash");
        run_applescript(
            r#"
tell application "Finder"
    try
        empty trash
        return "Trash emptied"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#,
        )
        .await
    }

    async fn get_recent_files(&self, days: u64, limit: u64) -> Result<String> {
        let days = days.min(30);
        let limit = limit.min(50);
        debug!("Getting recent files (last {} days, limit {})", days, limit);
        let home = dirs::home_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("mdfind")
                .args([
                    "-onlyin",
                    &home,
                    &format!("kMDItemLastUsedDate > $time.today(-{}d)", days),
                ])
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Recent files search timed out"))?
        .context("Failed to search recent files")?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = text.lines().take(limit as usize).collect();
            if lines.is_empty() {
                Ok("No recent files found".to_string())
            } else {
                Ok(lines.join("\n"))
            }
        } else {
            Err(anyhow::anyhow!("Failed to search recent files"))
        }
    }
}

// ── Spotlight ──────────────────────────────────────────────────────────────

pub struct MacOsSpotlightProvider;

#[async_trait]
impl SpotlightProvider for MacOsSpotlightProvider {
    async fn search(&self, query: &str, limit: u64) -> Result<String> {
        if query.len() > 500 {
            return Err(anyhow::anyhow!("Query too long (max 500 characters)"));
        }
        let limit = limit.min(100);
        debug!("Spotlight search: {}", query);
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("mdfind").arg(query).output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Spotlight search timed out"))?
        .context("Failed to run mdfind")?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = text.lines().take(limit as usize).collect();
            if lines.is_empty() {
                Ok(format!("No results for: {}", query))
            } else {
                Ok(format!(
                    "Found {} results:\n{}",
                    lines.len(),
                    lines.join("\n")
                ))
            }
        } else {
            Err(anyhow::anyhow!("Spotlight search failed"))
        }
    }

    async fn get_metadata(&self, path: &str) -> Result<String> {
        if path.contains("..") {
            return Err(anyhow::anyhow!("Path cannot contain '..'"));
        }
        if path.len() > 1000 {
            return Err(anyhow::anyhow!("Path too long"));
        }
        debug!("Getting metadata for: {}", path);
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("mdls").arg(path).output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Metadata lookup timed out"))?
        .context("Failed to run mdls")?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(anyhow::anyhow!(
                "Metadata lookup failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

// ── Shortcuts ──────────────────────────────────────────────────────────────

pub struct MacOsShortcutsProvider;

#[async_trait]
impl ShortcutsProvider for MacOsShortcutsProvider {
    async fn list_shortcuts(&self) -> Result<String> {
        debug!("Listing Apple Shortcuts");
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            Command::new("shortcuts").arg("list").output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Shortcuts list timed out"))?
        .context("Failed to list shortcuts")?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                Ok("No shortcuts found".to_string())
            } else {
                Ok(text)
            }
        } else {
            Err(anyhow::anyhow!(
                "Failed to list shortcuts: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    async fn run_shortcut(&self, name: &str, input: Option<&str>) -> Result<String> {
        if name.len() > 200 {
            return Err(anyhow::anyhow!(
                "Shortcut name too long (max 200 characters)"
            ));
        }
        debug!("Running shortcut: {}", name);
        let mut cmd = Command::new("shortcuts");
        cmd.arg("run").arg(name);
        if let Some(input_text) = input {
            cmd.arg("-i").arg(input_text);
        }
        let output = tokio::time::timeout(std::time::Duration::from_secs(60), cmd.output())
            .await
            .map_err(|_| anyhow::anyhow!("Shortcut execution timed out"))?
            .context("Failed to run shortcut")?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                Ok(format!("Shortcut '{}' completed", name))
            } else {
                Ok(text)
            }
        } else {
            Err(anyhow::anyhow!(
                "Shortcut '{}' failed: {}",
                name,
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

// ── Keychain ───────────────────────────────────────────────────────────────

pub struct MacOsKeychainProvider;

#[async_trait]
impl KeychainProvider for MacOsKeychainProvider {
    async fn get_password(&self, service: &str, account: &str) -> Result<String> {
        if service.len() > 200 || account.len() > 200 {
            return Err(anyhow::anyhow!("Service or account name too long"));
        }
        debug!("Getting keychain password for service: {}", service);
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("security")
                .args(["find-generic-password", "-s", service, "-a", account, "-w"])
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Keychain lookup timed out"))?
        .context("Failed to query keychain")?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(anyhow::anyhow!(
                "Password not found for service '{}', account '{}'",
                service,
                account
            ))
        }
    }

    async fn store_password(&self, service: &str, account: &str, password: &str) -> Result<String> {
        if service.len() > 200 || account.len() > 200 {
            return Err(anyhow::anyhow!("Service or account name too long"));
        }
        if password.len() > 10_000 {
            return Err(anyhow::anyhow!("Password too long"));
        }
        debug!("Storing keychain password for service: {}", service);
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("security")
                .args([
                    "add-generic-password",
                    "-s",
                    service,
                    "-a",
                    account,
                    "-w",
                    password,
                    "-U",
                ])
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Keychain store timed out"))?
        .context("Failed to store in keychain")?;
        if output.status.success() {
            Ok(format!(
                "Password stored for service '{}', account '{}'",
                service, account
            ))
        } else {
            Err(anyhow::anyhow!(
                "Failed to store password: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

// ── Messages ───────────────────────────────────────────────────────────────

pub struct MacOsMessagesProvider;

#[async_trait]
impl MessagesProvider for MacOsMessagesProvider {
    async fn read_messages(&self, contact: &str, limit: u64) -> Result<String> {
        if contact.len() > 200 {
            return Err(anyhow::anyhow!("Contact too long"));
        }
        let limit = limit.min(50);
        // SQL-safe: escape single quotes and strip control/meta characters
        let safe_contact: String = contact
            .replace('\'', "''")
            .chars()
            .filter(|&c| {
                c.is_alphanumeric()
                    || c == '@'
                    || c == '.'
                    || c == '+'
                    || c == '-'
                    || c == '_'
                    || c == ' '
            })
            .collect();
        debug!("Reading messages from: {}", contact);
        let db_path = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
            .join("Library/Messages/chat.db");
        if db_path.exists() {
            let output = tokio::time::timeout(
                std::time::Duration::from_secs(15),
                Command::new("sqlite3")
                    .arg(db_path.to_string_lossy().to_string())
                    .arg(format!(
                        "SELECT datetime(m.date/1000000000 + 978307200, 'unixepoch', 'localtime') as date, \
                         CASE WHEN m.is_from_me = 1 THEN 'Me' ELSE h.id END as sender, \
                         m.text \
                         FROM message m \
                         LEFT JOIN handle h ON m.handle_id = h.ROWID \
                         WHERE h.id LIKE '%{}%' OR m.is_from_me = 1 \
                         ORDER BY m.date DESC LIMIT {}",
                        safe_contact, limit
                    )).output(),
            ).await.map_err(|_| anyhow::anyhow!("Message read timed out"))?.context("Failed to read messages database")?;
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if text.is_empty() {
                    Ok(format!("No messages found with '{}'", contact))
                } else {
                    Ok(text)
                }
            } else {
                Ok(format!(
                    "Could not read messages for '{}' (database access denied)",
                    contact
                ))
            }
        } else {
            Ok("Messages database not found".to_string())
        }
    }

    async fn send_message(&self, contact: &str, message: &str) -> Result<String> {
        if contact.len() > 200 {
            return Err(anyhow::anyhow!("Contact too long"));
        }
        if message.len() > 10_000 {
            return Err(anyhow::anyhow!("Message too long"));
        }
        let safe_contact = sanitize_applescript_string(contact);
        let safe_message = sanitize_applescript_string(message);
        debug!("Sending message to: {}", contact);
        run_applescript(&format!(
            r#"
tell application "Messages"
    try
        set targetService to first service whose service type is iMessage
        set targetBuddy to buddy "{}" of targetService
        send "{}" to targetBuddy
        return "Message sent to {}"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#,
            safe_contact, safe_message, safe_contact
        ))
        .await
    }

    async fn start_facetime(&self, contact: &str, audio_only: bool) -> Result<String> {
        if contact.len() > 200 {
            return Err(anyhow::anyhow!("Contact too long"));
        }
        let safe_contact = sanitize_applescript_string(contact);
        debug!("Starting FaceTime with: {}", contact);
        let scheme = if audio_only {
            "facetime-audio"
        } else {
            "facetime"
        };
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("open")
                .arg(format!("{}://{}", scheme, safe_contact))
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("FaceTime launch timed out"))?
        .context("Failed to start FaceTime")?;
        if output.status.success() {
            Ok(format!(
                "FaceTime {} call started with {}",
                if audio_only { "audio" } else { "video" },
                contact
            ))
        } else {
            Err(anyhow::anyhow!(
                "Failed to start FaceTime: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

// ── Photos ─────────────────────────────────────────────────────────────────

pub struct MacOsPhotosProvider;

#[async_trait]
impl PhotosProvider for MacOsPhotosProvider {
    async fn search_photos(&self, query: &str, limit: u64) -> Result<String> {
        if query.len() > 200 {
            return Err(anyhow::anyhow!("Query too long"));
        }
        let limit = limit.min(50);
        let safe_query = sanitize_applescript_string(query);
        debug!("Searching photos: {}", query);
        run_applescript(&format!(
            r#"
tell application "Photos"
    try
        set results to search for "{}"
        set maxCount to {}
        if (count of results) < maxCount then set maxCount to (count of results)
        set output to "Found " & (count of results) & " photos" & return & "---" & return
        repeat with i from 1 to maxCount
            set p to item i of results
            set output to output & "Name: " & (filename of p) & return
            set output to output & "Date: " & (date of p as string) & return & "---" & return
        end repeat
        return output
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#,
            safe_query, limit
        ))
        .await
    }

    async fn export_photos(&self, query: &str, destination: &str, limit: u64) -> Result<String> {
        if query.len() > 200 {
            return Err(anyhow::anyhow!("Query too long"));
        }
        if destination.contains("..") {
            return Err(anyhow::anyhow!("Destination cannot contain '..'"));
        }
        if destination.len() > 1000 {
            return Err(anyhow::anyhow!("Destination too long"));
        }
        let limit = limit.min(50);
        let safe_query = sanitize_applescript_string(query);
        let safe_dest = sanitize_applescript_string(destination);
        debug!("Exporting photos matching '{}' to {}", query, destination);
        run_applescript(&format!(
            r#"
tell application "Photos"
    try
        set results to search for "{}"
        set maxCount to {}
        if (count of results) < maxCount then set maxCount to (count of results)
        set exportItems to items 1 thru maxCount of results
        export exportItems to POSIX file "{}" with using originals
        return "Exported " & maxCount & " photos to {}"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#,
            safe_query, limit, safe_dest, safe_dest
        ))
        .await
    }
}

// ── Media ──────────────────────────────────────────────────────────────────

pub struct MacOsMediaProvider;

#[async_trait]
impl MediaProvider for MacOsMediaProvider {
    async fn record_audio(&self, duration_secs: u64, output_path: Option<&str>) -> Result<String> {
        let duration_secs = duration_secs.min(300);
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let path = output_path
            .map(|p| p.to_string())
            .unwrap_or_else(|| format!("/tmp/meepo-recording-{}.m4a", timestamp));
        if path.contains("..") {
            return Err(anyhow::anyhow!("Path cannot contain '..'"));
        }
        validate_screenshot_path(&path)?;
        debug!("Recording audio for {}s to {}", duration_secs, path);
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(duration_secs + 5),
            Command::new("rec")
                .args([&path, "trim", "0", &duration_secs.to_string()])
                .output(),
        )
        .await;
        match output {
            Ok(Ok(o)) if o.status.success() => {
                Ok(format!("Audio recorded to {} ({}s)", path, duration_secs))
            }
            _ => Err(anyhow::anyhow!(
                "Audio recording failed. Install sox (`brew install sox`) for recording support."
            )),
        }
    }

    async fn text_to_speech(&self, text: &str, voice: Option<&str>) -> Result<String> {
        if text.len() > 10_000 {
            return Err(anyhow::anyhow!("Text too long (max 10,000 characters)"));
        }
        debug!("Text to speech ({} chars)", text.len());
        let mut cmd = Command::new("say");
        if let Some(v) = voice {
            if v.len() > 50 {
                return Err(anyhow::anyhow!("Voice name too long"));
            }
            cmd.arg("-v").arg(v);
        }
        cmd.arg(text);
        let output = tokio::time::timeout(std::time::Duration::from_secs(60), cmd.output())
            .await
            .map_err(|_| anyhow::anyhow!("Text-to-speech timed out"))?
            .context("Failed to run say")?;
        if output.status.success() {
            Ok("Speech completed".to_string())
        } else {
            Err(anyhow::anyhow!(
                "Text-to-speech failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    async fn ocr_image(&self, image_path: &str) -> Result<String> {
        if image_path.contains("..") {
            return Err(anyhow::anyhow!("Path cannot contain '..'"));
        }
        if image_path.len() > 1000 {
            return Err(anyhow::anyhow!("Path too long"));
        }
        debug!("OCR on image: {}", image_path);
        let safe_path = sanitize_applescript_string(image_path);
        let swift_code = format!(
            "import Vision; import AppKit; \
             let url = URL(fileURLWithPath: \"{}\"); \
             guard let image = NSImage(contentsOf: url), let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {{ print(\"Failed to load image\"); exit(1) }}; \
             let request = VNRecognizeTextRequest(); request.recognitionLevel = .accurate; \
             let handler = VNImageRequestHandler(cgImage: cgImage, options: [:]); \
             try handler.perform([request]); \
             let results = request.results ?? []; \
             for observation in results {{ print(observation.topCandidates(1).first?.string ?? \"\") }}",
            safe_path
        );
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            Command::new("swift").arg("-e").arg(&swift_code).output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("OCR timed out"))?
        .context("Failed to run OCR")?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                Ok("No text detected in image".to_string())
            } else {
                Ok(text)
            }
        } else {
            Err(anyhow::anyhow!(
                "OCR failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

// ── Window Management ──────────────────────────────────────────────────────

pub struct MacOsWindowManager;

#[async_trait]
impl WindowManagerProvider for MacOsWindowManager {
    async fn list_windows(&self) -> Result<String> {
        debug!("Listing windows");
        run_applescript(r#"
tell application "System Events"
    set output to ""
    repeat with p in (every application process whose visible is true)
        set appName to name of p
        try
            repeat with w in (every window of p)
                set output to output & appName & " | " & (name of w) & " | pos:" & (position of w as string) & " | size:" & (size of w as string) & "\n"
            end repeat
        end try
    end repeat
    return output
end tell"#).await
    }

    async fn move_window(
        &self,
        app_name: &str,
        x: i32,
        y: i32,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<String> {
        if app_name.len() > 100 {
            return Err(anyhow::anyhow!("App name too long"));
        }
        let safe_name = sanitize_applescript_string(app_name);
        debug!("Moving window of {} to ({}, {})", app_name, x, y);
        let size_clause = if let (Some(w), Some(h)) = (width, height) {
            format!("\n            set size of window 1 to {{{}, {}}}", w, h)
        } else {
            String::new()
        };
        run_applescript(&format!(
            r#"
tell application "System Events"
    try
        tell application process "{}"
            set position of window 1 to {{{}, {}}}{}
        end tell
        return "Window moved"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#,
            safe_name, x, y, size_clause
        ))
        .await
    }

    async fn minimize_window(&self, app_name: Option<&str>) -> Result<String> {
        debug!("Minimizing window");
        let script = if let Some(name) = app_name {
            let safe_name = sanitize_applescript_string(name);
            format!(
                r#"
tell application "System Events"
    try
        tell application process "{}"
            set miniaturized of window 1 to true
        end tell
        return "Window minimized"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#,
                safe_name
            )
        } else {
            r#"
tell application "System Events"
    try
        set frontApp to first application process whose frontmost is true
        tell frontApp
            set miniaturized of window 1 to true
        end tell
        return "Window minimized"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#
                .to_string()
        };
        run_applescript(&script).await
    }

    async fn fullscreen_window(&self, app_name: Option<&str>) -> Result<String> {
        debug!("Toggling fullscreen");
        let target = if let Some(name) = app_name {
            format!(
                r#"tell application process "{}""#,
                sanitize_applescript_string(name)
            )
        } else {
            r#"tell (first application process whose frontmost is true)"#.to_string()
        };
        run_applescript(&format!(r#"
tell application "System Events"
    try
        {}
            set value of attribute "AXFullScreen" of window 1 to (not (value of attribute "AXFullScreen" of window 1))
        end tell
        return "Fullscreen toggled"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#, target)).await
    }

    async fn arrange_windows(&self, layout: &str) -> Result<String> {
        debug!("Arranging windows: {}", layout);
        match layout.to_lowercase().as_str() {
            "left-right" | "side-by-side" | "split" => {
                run_applescript(r#"
tell application "System Events"
    try
        set screenWidth to (do shell script "system_profiler SPDisplaysDataType | grep Resolution | head -1 | awk '{print $2}'") as integer
        set screenHeight to (do shell script "system_profiler SPDisplaysDataType | grep Resolution | head -1 | awk '{print $4}'") as integer
        set halfWidth to screenWidth / 2
        set frontApp to first application process whose frontmost is true
        tell frontApp
            set position of window 1 to {0, 25}
            set size of window 1 to {halfWidth, screenHeight - 25}
        end tell
        set visibleApps to every application process whose visible is true and frontmost is false
        if (count of visibleApps) > 0 then
            tell item 1 of visibleApps
                try
                    set position of window 1 to {halfWidth, 25}
                    set size of window 1 to {halfWidth, screenHeight - 25}
                end try
            end tell
        end if
        return "Windows arranged side-by-side"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#).await
            }
            "cascade" => {
                run_applescript(r#"
tell application "System Events"
    try
        set offset to 0
        repeat with p in (every application process whose visible is true)
            try
                set position of window 1 of p to {50 + offset, 50 + offset}
                set offset to offset + 30
            end try
        end repeat
        return "Windows cascaded"
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#).await
            }
            _ => Err(anyhow::anyhow!("Unknown layout: {}. Supported: split, cascade", layout)),
        }
    }
}

// ── Terminal ───────────────────────────────────────────────────────────────

pub struct MacOsTerminalProvider;

#[async_trait]
impl TerminalProvider for MacOsTerminalProvider {
    async fn list_terminal_tabs(&self) -> Result<String> {
        debug!("Listing terminal tabs");
        run_applescript(
            r#"
tell application "Terminal"
    set output to ""
    set winIdx to 1
    repeat with w in windows
        set tabIdx to 1
        repeat with t in tabs of w
            set output to output & "Window " & winIdx & ", Tab " & tabIdx & ": "
            try
                set output to output & (custom title of t)
            on error
                try
                    set output to output & (name of t)
                end try
            end try
            set output to output & " [" & (processes of t as string) & "]"
            set output to output & "\n"
            set tabIdx to tabIdx + 1
        end repeat
        set winIdx to winIdx + 1
    end repeat
    return output
end tell"#,
        )
        .await
    }

    async fn send_terminal_command(&self, command: &str, tab_index: Option<u32>) -> Result<String> {
        if command.len() > 5000 {
            return Err(anyhow::anyhow!("Command too long (max 5000 characters)"));
        }
        let safe_command = sanitize_applescript_string(command);
        debug!("Sending terminal command");
        let script = if let Some(idx) = tab_index {
            format!(
                r#"
tell application "Terminal"
    do script "{}" in tab {} of window 1
    return "Command sent to tab {}"
end tell"#,
                safe_command, idx, idx
            )
        } else {
            format!(
                r#"
tell application "Terminal"
    do script "{}" in front window
    return "Command sent"
end tell"#,
                safe_command
            )
        };
        run_applescript(&script).await
    }

    async fn get_open_ports(&self) -> Result<String> {
        debug!("Getting open ports");
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            Command::new("lsof")
                .args(["-iTCP", "-sTCP:LISTEN", "-P", "-n"])
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Open ports lookup timed out"))?
        .context("Failed to list open ports")?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                Ok("No listening ports found".to_string())
            } else {
                Ok(text)
            }
        } else {
            Err(anyhow::anyhow!("Failed to list open ports"))
        }
    }
}

// ── Productivity ───────────────────────────────────────────────────────────

pub struct MacOsProductivityProvider;

#[async_trait]
impl ProductivityProvider for MacOsProductivityProvider {
    async fn set_clipboard(&self, text: &str) -> Result<String> {
        if text.len() > 1_000_000 {
            return Err(anyhow::anyhow!("Text too long for clipboard"));
        }
        debug!("Setting clipboard ({} chars)", text.len());
        use tokio::io::AsyncWriteExt;
        let mut child = Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn pbcopy: {}", e))?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(text.as_bytes())
                .await
                .context("Failed to write to clipboard")?;
        }
        child.wait().await.context("pbcopy failed")?;
        Ok(format!("Clipboard set ({} chars)", text.len()))
    }

    async fn get_frontmost_document(&self) -> Result<String> {
        debug!("Getting frontmost document path");
        run_applescript(
            r#"
tell application "System Events"
    try
        set frontApp to first application process whose frontmost is true
        set appName to name of frontApp
        try
            tell application appName
                set docPath to path of document 1
                return "App: " & appName & "\nDocument: " & docPath
            end tell
        on error
            try
                tell application appName
                    set docName to name of document 1
                    return "App: " & appName & "\nDocument: " & docName & " (path not available)"
                end tell
            on error
                return "App: " & appName & "\n(no document open or path not accessible)"
            end try
        end try
    on error errMsg
        return "Error: " & errMsg
    end try
end tell"#,
        )
        .await
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
