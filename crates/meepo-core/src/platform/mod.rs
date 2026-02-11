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
    async fn send_email(&self, to: &str, subject: &str, body: &str, cc: Option<&str>, in_reply_to: Option<&str>) -> Result<String>;
}

/// Calendar provider for reading and creating events
#[async_trait]
pub trait CalendarProvider: Send + Sync {
    async fn read_events(&self, days_ahead: u64) -> Result<String>;
    async fn create_event(&self, summary: &str, start_time: &str, duration_minutes: u64) -> Result<String>;
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
    async fn create_reminder(&self, name: &str, list_name: Option<&str>, due_date: Option<&str>, notes: Option<&str>) -> Result<String>;
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
    async fn send_notification(&self, title: &str, message: &str, sound: Option<&str>) -> Result<String>;
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
pub fn create_email_provider() -> Box<dyn EmailProvider> {
    #[cfg(target_os = "macos")]
    { Box::new(macos::MacOsEmailProvider) }
    #[cfg(target_os = "windows")]
    { Box::new(windows::WindowsEmailProvider) }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    { panic!("Email provider not available on this platform") }
}

/// Create platform calendar provider
pub fn create_calendar_provider() -> Box<dyn CalendarProvider> {
    #[cfg(target_os = "macos")]
    { Box::new(macos::MacOsCalendarProvider) }
    #[cfg(target_os = "windows")]
    { Box::new(windows::WindowsCalendarProvider) }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    { panic!("Calendar provider not available on this platform") }
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
pub fn create_ui_automation() -> Box<dyn UiAutomation> {
    #[cfg(target_os = "macos")]
    { Box::new(macos::MacOsUiAutomation) }
    #[cfg(target_os = "windows")]
    { Box::new(windows::WindowsUiAutomation) }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    { panic!("UI automation not available on this platform") }
}

/// Create platform reminders provider (macOS only)
pub fn create_reminders_provider() -> Box<dyn RemindersProvider> {
    #[cfg(target_os = "macos")]
    { Box::new(macos::MacOsRemindersProvider) }
    #[cfg(not(target_os = "macos"))]
    { panic!("Reminders provider only available on macOS") }
}

/// Create platform notes provider (macOS only)
pub fn create_notes_provider() -> Box<dyn NotesProvider> {
    #[cfg(target_os = "macos")]
    { Box::new(macos::MacOsNotesProvider) }
    #[cfg(not(target_os = "macos"))]
    { panic!("Notes provider only available on macOS") }
}

/// Create platform notification provider (macOS only)
pub fn create_notification_provider() -> Box<dyn NotificationProvider> {
    #[cfg(target_os = "macos")]
    { Box::new(macos::MacOsNotificationProvider) }
    #[cfg(not(target_os = "macos"))]
    { panic!("Notification provider only available on macOS") }
}

/// Create platform screen capture provider (macOS only)
pub fn create_screen_capture_provider() -> Box<dyn ScreenCaptureProvider> {
    #[cfg(target_os = "macos")]
    { Box::new(macos::MacOsScreenCaptureProvider) }
    #[cfg(not(target_os = "macos"))]
    { panic!("Screen capture provider only available on macOS") }
}

/// Create platform music provider (macOS only)
pub fn create_music_provider() -> Box<dyn MusicProvider> {
    #[cfg(target_os = "macos")]
    { Box::new(macos::MacOsMusicProvider) }
    #[cfg(not(target_os = "macos"))]
    { panic!("Music provider only available on macOS") }
}

/// Create platform contacts provider (macOS only)
pub fn create_contacts_provider() -> Box<dyn ContactsProvider> {
    #[cfg(target_os = "macos")]
    { Box::new(macos::MacOsContactsProvider) }
    #[cfg(not(target_os = "macos"))]
    { panic!("Contacts provider only available on macOS") }
}

/// Create platform browser provider (macOS: Safari via AppleScript)
pub fn create_browser_provider() -> Box<dyn BrowserProvider> {
    #[cfg(target_os = "macos")]
    { Box::new(macos::MacOsSafariBrowser) }
    #[cfg(not(target_os = "macos"))]
    { panic!("Browser provider not yet available on this platform") }
}

/// Cross-platform clipboard using `arboard` crate
pub struct CrossPlatformClipboard;

#[async_trait]
impl ClipboardProvider for CrossPlatformClipboard {
    async fn get_clipboard(&self) -> Result<String> {
        tokio::task::spawn_blocking(|| {
            let mut clipboard = arboard::Clipboard::new()
                .map_err(|e| anyhow::anyhow!("Failed to access clipboard: {}", e))?;
            clipboard.get_text()
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
                Err(anyhow::anyhow!("Failed to open {}: {}", name, stderr.trim()))
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            tokio::task::spawn_blocking(move || {
                open::that(&name)
                    .map_err(|e| anyhow::anyhow!("Failed to open {}: {}", name, e))?;
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
        let _email = create_email_provider();
        let _calendar = create_calendar_provider();
        let _ui = create_ui_automation();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_macos_providers_create() {
        let _reminders = create_reminders_provider();
        let _notes = create_notes_provider();
        let _notification = create_notification_provider();
        let _screen = create_screen_capture_provider();
        let _music = create_music_provider();
        let _contacts = create_contacts_provider();
        let _browser = create_browser_provider();
    }
}
