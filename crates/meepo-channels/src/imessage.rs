//! iMessage channel adapter using SQLite polling and AppleScript

use crate::bus::MessageChannel;
use meepo_core::types::{IncomingMessage, OutgoingMessage, ChannelType};
use tokio::sync::mpsc;
use async_trait::async_trait;
use anyhow::{Result, anyhow};
use std::path::PathBuf;
use std::time::Duration;
use rusqlite::{Connection, params};
use tracing::{info, error, debug, warn};
use chrono::Utc;
use tokio::process::Command;
use std::sync::Arc;
use tokio::sync::RwLock;

/// iMessage channel adapter
pub struct IMessageChannel {
    poll_interval: Duration,
    trigger_prefix: String,
    allowed_contacts: Vec<String>,
    db_path: PathBuf,
    last_rowid: Arc<RwLock<Option<i64>>>,
}

impl IMessageChannel {
    /// Create a new iMessage channel adapter
    ///
    /// # Arguments
    /// * `poll_interval` - How often to poll the iMessage database
    /// * `trigger_prefix` - Prefix required for messages to be processed (e.g., "!")
    /// * `allowed_contacts` - List of phone numbers/emails allowed to send messages
    /// * `db_path` - Optional custom path to chat.db (defaults to ~/Library/Messages/chat.db)
    pub fn new(
        poll_interval: Duration,
        trigger_prefix: String,
        allowed_contacts: Vec<String>,
        db_path: Option<PathBuf>,
    ) -> Self {
        let db_path = db_path.unwrap_or_else(|| {
            let mut path = dirs::home_dir().expect("Could not find home directory");
            path.push("Library/Messages/chat.db");
            path
        });

        Self {
            poll_interval,
            trigger_prefix,
            allowed_contacts,
            db_path,
            last_rowid: Arc::new(RwLock::new(None)),
        }
    }

    /// Normalize phone number for comparison (remove +, -, spaces, etc.)
    fn normalize_contact(contact: &str) -> String {
        contact.chars()
            .filter(|c| c.is_alphanumeric() || *c == '@')
            .collect::<String>()
            .to_lowercase()
    }

    /// Check if a contact is in the allowed list
    fn is_allowed_contact(&self, contact: &str) -> bool {
        let normalized = Self::normalize_contact(contact);
        self.allowed_contacts.iter().any(|allowed| {
            Self::normalize_contact(allowed) == normalized
        })
    }

    /// Poll the iMessage database for new messages
    async fn poll_messages(&self, tx: &mpsc::Sender<IncomingMessage>) -> Result<()> {
        // Open read-only connection to chat.db
        let conn = Connection::open_with_flags(
            &self.db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;

        // Get or initialize last_rowid
        let mut last_rowid_guard = self.last_rowid.write().await;
        let last_rowid = if let Some(rowid) = *last_rowid_guard {
            rowid
        } else {
            // First run - get the current max ROWID
            let max_rowid: i64 = conn.query_row(
                "SELECT COALESCE(MAX(ROWID), 0) FROM message",
                [],
                |row| row.get(0),
            )?;
            *last_rowid_guard = Some(max_rowid);
            debug!("Initialized last_rowid to {}", max_rowid);
            max_rowid
        };
        drop(last_rowid_guard);

        // Query for new messages
        let query = r#"
            SELECT
                message.ROWID,
                message.text,
                handle.id,
                datetime(message.date/1000000000 + strftime('%s', '2001-01-01'), 'unixepoch')
            FROM message
            JOIN handle ON message.handle_id = handle.ROWID
            WHERE message.ROWID > ?
                AND message.is_from_me = 0
                AND message.text IS NOT NULL
            ORDER BY message.ROWID ASC
        "#;

        // Collect all messages from SQLite synchronously (no await while holding rusqlite types)
        let mut pending_messages = Vec::new();
        let mut new_last_rowid = last_rowid;
        {
            let mut stmt = conn.prepare(query)?;
            let mut rows = stmt.query(params![last_rowid])?;

            while let Some(row) = rows.next()? {
                let rowid: i64 = row.get(0)?;
                let text: String = row.get(1)?;
                let handle: String = row.get(2)?;
                let timestamp_str: String = row.get(3)?;

                // Update last_rowid
                new_last_rowid = new_last_rowid.max(rowid);

                // Check if message starts with trigger prefix
                if !text.starts_with(&self.trigger_prefix) {
                    debug!("Skipping message without trigger prefix: {}", text);
                    continue;
                }

                // Check if contact is allowed
                if !self.is_allowed_contact(&handle) {
                    warn!("Ignoring message from unauthorized contact: {}", handle);
                    continue;
                }

                // Remove trigger prefix from content
                let content = text.trim_start_matches(&self.trigger_prefix).trim().to_string();

                // Parse timestamp (fallback to current time if parsing fails)
                let timestamp = chrono::NaiveDateTime::parse_from_str(&timestamp_str, "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .and_then(|dt| dt.and_utc().timestamp_millis().try_into().ok())
                    .and_then(|ts: i64| chrono::DateTime::from_timestamp_millis(ts))
                    .unwrap_or_else(Utc::now);

                pending_messages.push((rowid, handle, content, timestamp));
            }
        } // stmt and rows dropped here — no longer held across await

        // Now send messages asynchronously
        let message_count = pending_messages.len();
        for (rowid, handle, content, timestamp) in pending_messages {
            let incoming = IncomingMessage {
                id: format!("imessage_{}", rowid),
                sender: handle.clone(),
                content: content.clone(),
                channel: ChannelType::IMessage,
                timestamp,
            };

            info!("Forwarding iMessage from {}: {}", handle, content);

            if let Err(e) = tx.send(incoming).await {
                error!("Failed to send iMessage to bus: {}", e);
            }
        }

        // Update last_rowid if we processed any messages
        if new_last_rowid > last_rowid {
            let mut last_rowid_guard = self.last_rowid.write().await;
            *last_rowid_guard = Some(new_last_rowid);
            debug!("Updated last_rowid to {} ({} new messages)", new_last_rowid, message_count);
        }

        Ok(())
    }

    /// Escape quotes in AppleScript strings
    fn escape_applescript(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    }

    /// Send a message via AppleScript
    async fn send_imessage(&self, recipient: &str, message: &str) -> Result<()> {
        let escaped_recipient = Self::escape_applescript(recipient);
        let escaped_message = Self::escape_applescript(message);

        let applescript = format!(
            r#"tell application "Messages"
    set targetService to 1st service whose service type = iMessage
    set targetBuddy to buddy "{}" of targetService
    send "{}" to targetBuddy
end tell"#,
            escaped_recipient, escaped_message
        );

        debug!("Executing AppleScript to send iMessage");

        let output = Command::new("osascript")
            .arg("-e")
            .arg(&applescript)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("AppleScript failed: {}", stderr));
        }

        info!("iMessage sent successfully to {}", recipient);
        Ok(())
    }
}

#[async_trait]
impl MessageChannel for IMessageChannel {
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> Result<()> {
        info!("Starting iMessage channel adapter");
        info!("Database path: {:?}", self.db_path);
        info!("Poll interval: {:?}", self.poll_interval);
        info!("Trigger prefix: {}", self.trigger_prefix);

        // Verify database exists
        if !self.db_path.exists() {
            return Err(anyhow!("iMessage database not found at {:?}", self.db_path));
        }

        // Verify we can open the database
        Connection::open_with_flags(
            &self.db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;

        // Clone necessary data for the polling task
        let poll_interval = self.poll_interval;
        let last_rowid = self.last_rowid.clone();
        let db_path = self.db_path.clone();
        let trigger_prefix = self.trigger_prefix.clone();
        let allowed_contacts = self.allowed_contacts.clone();

        // Create a new channel instance for the task
        let channel = IMessageChannel {
            poll_interval,
            trigger_prefix,
            allowed_contacts,
            db_path,
            last_rowid,
        };

        // Spawn polling task
        tokio::spawn(async move {
            info!("iMessage polling task started");
            let mut interval = tokio::time::interval(channel.poll_interval);

            loop {
                interval.tick().await;
                debug!("Polling iMessage database");

                if let Err(e) = channel.poll_messages(&tx).await {
                    error!("Error polling iMessage database: {}", e);
                }
            }
        });

        info!("iMessage channel adapter started");
        Ok(())
    }

    async fn send(&self, msg: OutgoingMessage) -> Result<()> {
        debug!("Sending iMessage");

        // Extract recipient from reply_to
        // The message ID format is "imessage_{rowid}", but we need the actual recipient
        // This is a limitation - we need to track sender info separately
        // For now, we'll require the recipient in a different way

        // In a real implementation, we'd want to:
        // 1. Store a mapping of message IDs to senders when we receive messages
        // 2. Look up the sender from the reply_to field
        // For this initial version, we'll just send to the first allowed contact

        if self.allowed_contacts.is_empty() {
            return Err(anyhow!("No allowed contacts configured for iMessage"));
        }

        // Use the first allowed contact as recipient
        // In production, this should be improved to track actual conversations
        let recipient = &self.allowed_contacts[0];

        self.send_imessage(recipient, &msg.content).await?;

        info!("iMessage sent successfully");
        Ok(())
    }

    fn channel_type(&self) -> ChannelType {
        ChannelType::IMessage
    }
}
