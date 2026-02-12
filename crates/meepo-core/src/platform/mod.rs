//! Platform abstraction layer for OS-specific functionality
//!
//! Provides trait definitions and platform-specific implementations.
//! On macOS: AppleScript-based implementations.
//! On Windows: PowerShell/COM-based implementations.

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

use anyhow::Result;
use async_trait::async_trait;

/// Email provider for reading and sending emails
#[async_trait]
pub trait EmailProvider: Send + Sync {
    async fn read_emails(&self, limit: u64, mailbox: &str, search: Option<&str>) -> Result<String>;
    async fn send_email(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        cc: Option<&str>,
        in_reply_to: Option<&str>,
    ) -> Result<String>;
}

/// Calendar provider for reading and creating events
#[async_trait]
pub trait CalendarProvider: Send + Sync {
    async fn read_events(&self, days_ahead: u64) -> Result<String>;
    async fn create_event(
        &self,
        summary: &str,
        start_time: &str,
        duration_minutes: u64,
    ) -> Result<String>;
}

/// Clipboard provider for reading clipboard contents
#[async_trait]
pub trait ClipboardProvider: Send + Sync {
    async fn get_clipboard(&self) -> Result<String>;
}

/// Application launcher
#[async_trait]
pub trait AppLauncher: Send + Sync {
    async fn open_app(&self, app_name: &str) -> Result<String>;
}

/// UI automation for accessibility
#[async_trait]
pub trait UiAutomation: Send + Sync {
    async fn read_screen(&self) -> Result<String>;
    async fn click_element(&self, element_name: &str, element_type: &str) -> Result<String>;
    async fn type_text(&self, text: &str) -> Result<String>;
}

/// Reminders provider for reading and creating reminders
#[async_trait]
pub trait RemindersProvider: Send + Sync {
    async fn list_reminders(&self, list_name: Option<&str>) -> Result<String>;
    async fn create_reminder(
        &self,
        name: &str,
        list_name: Option<&str>,
        due_date: Option<&str>,
        notes: Option<&str>,
    ) -> Result<String>;
}

/// Notes provider for reading and creating notes
#[async_trait]
pub trait NotesProvider: Send + Sync {
    async fn list_notes(&self, folder: Option<&str>, limit: u64) -> Result<String>;
    async fn create_note(&self, title: &str, body: &str, folder: Option<&str>) -> Result<String>;
}

/// Notification provider for sending system notifications
#[async_trait]
pub trait NotificationProvider: Send + Sync {
    async fn send_notification(
        &self,
        title: &str,
        message: &str,
        sound: Option<&str>,
    ) -> Result<String>;
}

/// Screen capture provider
#[async_trait]
pub trait ScreenCaptureProvider: Send + Sync {
    async fn capture_screen(&self, path: Option<&str>) -> Result<String>;
}

/// Music control provider (Apple Music / Spotify)
#[async_trait]
pub trait MusicProvider: Send + Sync {
    async fn get_current_track(&self) -> Result<String>;
    async fn control_playback(&self, action: &str) -> Result<String>;
}

/// Contacts provider for searching contacts
#[async_trait]
pub trait ContactsProvider: Send + Sync {
    async fn search_contacts(&self, query: &str) -> Result<String>;
}

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
    async fn list_tabs(&self) -> Result<Vec<BrowserTab>>;
    async fn open_tab(&self, url: &str) -> Result<BrowserTab>;
    async fn close_tab(&self, tab_id: &str) -> Result<()>;
    async fn switch_tab(&self, tab_id: &str) -> Result<()>;
    async fn get_page_content(&self, tab_id: Option<&str>) -> Result<PageContent>;
    async fn execute_javascript(&self, tab_id: Option<&str>, script: &str) -> Result<String>;
    async fn click_element(&self, tab_id: Option<&str>, selector: &str) -> Result<()>;
    async fn fill_form(&self, tab_id: Option<&str>, selector: &str, value: &str) -> Result<()>;
    async fn screenshot_page(&self, tab_id: Option<&str>, path: Option<&str>) -> Result<String>;
    async fn go_back(&self, tab_id: Option<&str>) -> Result<()>;
    async fn go_forward(&self, tab_id: Option<&str>) -> Result<()>;
    async fn reload(&self, tab_id: Option<&str>) -> Result<()>;
    async fn get_cookies(&self, tab_id: Option<&str>) -> Result<Vec<BrowserCookie>>;
    async fn get_page_url(&self, tab_id: Option<&str>) -> Result<String>;
}

/// Create platform email provider
pub fn create_email_provider() -> Result<Box<dyn EmailProvider>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsEmailProvider))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WindowsEmailProvider))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(anyhow::anyhow!("Email provider not available on this platform"))
    }
}

/// Create platform calendar provider
pub fn create_calendar_provider() -> Result<Box<dyn CalendarProvider>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsCalendarProvider))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WindowsCalendarProvider))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(anyhow::anyhow!("Calendar provider not available on this platform"))
    }
}

/// Create cross-platform clipboard provider
pub fn create_clipboard_provider() -> Box<dyn ClipboardProvider> {
    Box::new(CrossPlatformClipboard)
}

/// Create cross-platform app launcher
pub fn create_app_launcher() -> Box<dyn AppLauncher> {
    Box::new(CrossPlatformAppLauncher)
}

/// Create platform UI automation provider
pub fn create_ui_automation() -> Result<Box<dyn UiAutomation>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsUiAutomation))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WindowsUiAutomation))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(anyhow::anyhow!("UI automation not available on this platform"))
    }
}

/// Create platform reminders provider (macOS only)
pub fn create_reminders_provider() -> Result<Box<dyn RemindersProvider>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsRemindersProvider))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(anyhow::anyhow!("Reminders provider is only available on macOS"))
    }
}

/// Create platform notes provider (macOS only)
pub fn create_notes_provider() -> Result<Box<dyn NotesProvider>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsNotesProvider))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(anyhow::anyhow!("Notes provider is only available on macOS"))
    }
}

/// Create platform notification provider (macOS only)
pub fn create_notification_provider() -> Result<Box<dyn NotificationProvider>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsNotificationProvider))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(anyhow::anyhow!("Notification provider is only available on macOS"))
    }
}

/// Create platform screen capture provider (macOS only)
pub fn create_screen_capture_provider() -> Result<Box<dyn ScreenCaptureProvider>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsScreenCaptureProvider))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(anyhow::anyhow!("Screen capture provider is only available on macOS"))
    }
}

/// Create platform music provider (macOS only)
pub fn create_music_provider() -> Result<Box<dyn MusicProvider>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsMusicProvider))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(anyhow::anyhow!("Music provider is only available on macOS"))
    }
}

/// Create platform contacts provider (macOS only)
pub fn create_contacts_provider() -> Result<Box<dyn ContactsProvider>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsContactsProvider))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(anyhow::anyhow!("Contacts provider is only available on macOS"))
    }
}

/// Create Safari browser provider (macOS only)
pub fn create_browser_provider() -> Result<Box<dyn BrowserProvider>> {
    create_browser_provider_for("safari")
}

/// Create browser provider for a specific browser
pub fn create_browser_provider_for(browser: &str) -> Result<Box<dyn BrowserProvider>> {
    match browser.to_lowercase().as_str() {
        "safari" => {
            #[cfg(target_os = "macos")]
            {
                Ok(Box::new(macos::MacOsSafariBrowser))
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err(anyhow::anyhow!("Safari browser provider is only available on macOS"))
            }
        }
        "chrome" | "google chrome" => {
            #[cfg(target_os = "macos")]
            {
                Ok(Box::new(macos::MacOsChromeBrowser))
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err(anyhow::anyhow!("Chrome browser provider is only available on macOS"))
            }
        }
        _ => Err(anyhow::anyhow!(
            "Unsupported browser: {}. Supported: safari, chrome",
            browser
        )),
    }
}

/// Cross-platform clipboard using `arboard` crate
pub struct CrossPlatformClipboard;

#[async_trait]
impl ClipboardProvider for CrossPlatformClipboard {
    async fn get_clipboard(&self) -> Result<String> {
        tokio::task::spawn_blocking(|| {
            let mut clipboard = arboard::Clipboard::new()
                .map_err(|e| anyhow::anyhow!("Failed to access clipboard: {}", e))?;
            clipboard
                .get_text()
                .map_err(|e| anyhow::anyhow!("Failed to read clipboard: {}", e))
        })
        .await?
    }
}

/// Cross-platform app launcher
pub struct CrossPlatformAppLauncher;

#[async_trait]
impl AppLauncher for CrossPlatformAppLauncher {
    async fn open_app(&self, app_name: &str) -> Result<String> {
        let name = app_name.to_string();
        #[cfg(target_os = "macos")]
        {
            // On macOS, use `open -a` to launch apps by name
            let output = tokio::process::Command::new("open")
                .arg("-a")
                .arg(&name)
                .output()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to launch {}: {}", name, e))?;

            if output.status.success() {
                Ok(format!("Successfully opened {}", name))
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(anyhow::anyhow!(
                    "Failed to open {}: {}",
                    name,
                    stderr.trim()
                ))
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            tokio::task::spawn_blocking(move || {
                open::that(&name).map_err(|e| anyhow::anyhow!("Failed to open {}: {}", name, e))?;
                Ok(format!("Successfully opened {}", name))
            })
            .await?
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_provider_creates() {
        let _provider = create_clipboard_provider();
    }

    #[test]
    fn test_app_launcher_creates() {
        let _launcher = create_app_launcher();
    }

    #[test]
    fn test_platform_providers_create() {
        let _email = create_email_provider().unwrap();
        let _calendar = create_calendar_provider().unwrap();
        let _ui = create_ui_automation().unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_macos_providers_create() {
        let _reminders = create_reminders_provider().unwrap();
        let _notes = create_notes_provider().unwrap();
        let _notification = create_notification_provider().unwrap();
        let _screen = create_screen_capture_provider().unwrap();
        let _music = create_music_provider().unwrap();
        let _contacts = create_contacts_provider().unwrap();
        let _browser = create_browser_provider().unwrap();
    }
}
