//! iMessage channel adapter using SQLite polling and AppleScript

use crate::bus::MessageChannel;
use crate::rate_limit::RateLimiter;
use meepo_core::types::{IncomingMessage, MessageKind, OutgoingMessage, ChannelType};
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
use std::num::NonZeroUsize;
use tokio::sync::{Mutex, RwLock};
use lru::LruCache;

const MAX_MESSAGE_SENDERS: usize = 1000;
const MAX_MESSAGE_SIZE: usize = 10_240;

/// Acknowledgment text sent by Meepo (used to skip echo/auto-reply loops)
const ACK_TEXT: &str = "On it, thinking...";

/// iMessage channel adapter
pub struct IMessageChannel {
    poll_interval: Duration,
    allowed_contacts: Vec<String>,
    db_path: PathBuf,
    last_rowid: Arc<RwLock<Option<i64>>>,
    /// Maps message_id -> sender contact for reply-to tracking (LRU-bounded)
    message_senders: Arc<Mutex<LruCache<String, String>>>,
    rate_limiter: RateLimiter,
}

impl IMessageChannel {
    /// Create a new iMessage channel adapter
    ///
    /// # Arguments
    /// * `poll_interval` - How often to poll the iMessage database
    /// * `allowed_contacts` - List of phone numbers/emails allowed to send messages
    /// * `db_path` - Optional custom path to chat.db (defaults to ~/Library/Messages/chat.db)
    pub fn new(
        poll_interval: Duration,
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
            allowed_contacts,
            db_path,
            last_rowid: Arc::new(RwLock::new(None)),
            message_senders: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(MAX_MESSAGE_SENDERS).unwrap(),
            ))),
            rate_limiter: RateLimiter::new(10, Duration::from_secs(60)),
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
        // Note: We open a fresh connection on each poll rather than maintaining a persistent connection
        // because: (1) Messages.app may lock the database, so a stale connection could fail,
        // (2) SQLite read-only connections are lightweight (~1ms overhead),
        // (3) This ensures we always have a valid connection without complex error recovery.
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

                // Check if contact is allowed
                if !self.is_allowed_contact(&handle) {
                    warn!("Ignoring message from unauthorized contact: {}", handle);
                    continue;
                }

                let content = text.trim().to_string();

                // Skip messages that match our own ack text (prevents echo loops
                // when the recipient has auto-reply or AI assistants enabled)
                if content == ACK_TEXT {
                    debug!("Skipping echo of our ack message from {}", handle);
                    new_last_rowid = new_last_rowid.max(rowid);
                    continue;
                }

                // Check message size limit
                if content.len() > MAX_MESSAGE_SIZE {
                    warn!(
                        "Dropping oversized iMessage from {} ({} bytes, limit {} bytes)",
                        handle,
                        content.len(),
                        MAX_MESSAGE_SIZE,
                    );
                    continue;
                }

                // Check rate limit
                if !self.rate_limiter.check_and_record(&handle) {
                    continue;
                }

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
            let msg_id = format!("imessage_{}", rowid);

            // Store message_id -> sender mapping for reply-to tracking (LRU auto-evicts oldest)
            {
                let mut lru = self.message_senders.lock().await;
                lru.put(msg_id.clone(), handle.clone());
            }

            let incoming = IncomingMessage {
                id: msg_id,
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

    /// After sending an ack, bump the ROWID watermark so the poller
    /// skips any auto-reply that arrives in response to our ack.
    async fn bump_watermark_after_send(&self) {
        // Small delay to let the sent message propagate to chat.db
        tokio::time::sleep(Duration::from_millis(500)).await;

        if let Ok(conn) = Connection::open_with_flags(
            &self.db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) {
            if let Ok(max_rowid) = conn.query_row::<i64, _, _>(
                "SELECT COALESCE(MAX(ROWID), 0) FROM message",
                [],
                |row| row.get(0),
            ) {
                let mut guard = self.last_rowid.write().await;
                if let Some(current) = *guard {
                    if max_rowid > current {
                        debug!("Bumping last_rowid from {} to {} after ack send", current, max_rowid);
                        *guard = Some(max_rowid);
                    }
                }
            }
        }
    }
}

#[async_trait]
impl MessageChannel for IMessageChannel {
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> Result<()> {
        info!("Starting iMessage channel adapter");
        info!("Database path: {:?}", self.db_path);
        info!("Poll interval: {:?}", self.poll_interval);
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
        let allowed_contacts = self.allowed_contacts.clone();
        let message_senders = self.message_senders.clone();
        let rate_limiter = self.rate_limiter.clone();

        // Create a new channel instance for the task
        let channel = IMessageChannel {
            poll_interval,
            allowed_contacts,
            db_path,
            last_rowid,
            message_senders,
            rate_limiter,
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
        // Look up recipient from reply_to message tracking (LRU cache)
        let recipient = if let Some(reply_to) = &msg.reply_to {
            let mut lru = self.message_senders.lock().await;
            if let Some(sender) = lru.get(reply_to) {
                debug!("Found recipient from reply_to: {}", sender);
                sender.clone()
            } else {
                warn!("reply_to '{}' not found in message tracking, falling back to first allowed contact", reply_to);
                if self.allowed_contacts.is_empty() {
                    return Err(anyhow!("No allowed contacts configured for iMessage"));
                }
                self.allowed_contacts[0].clone()
            }
        } else {
            if self.allowed_contacts.is_empty() {
                return Err(anyhow!("No allowed contacts configured for iMessage"));
            }
            self.allowed_contacts[0].clone()
        };

        // Handle acknowledgment: send a quick "thinking" message
        if msg.kind == MessageKind::Acknowledgment {
            debug!("Sending iMessage acknowledgment to {}", recipient);
            if let Err(e) = self.send_imessage(&recipient, ACK_TEXT).await {
                warn!("Failed to send iMessage acknowledgment: {}", e);
            } else {
                // Bump watermark to skip any auto-reply triggered by our ack
                self.bump_watermark_after_send().await;
            }
            return Ok(());
        }

        // Normal response
        self.send_imessage(&recipient, &msg.content).await?;
        info!("iMessage sent successfully to {}", recipient);
        Ok(())
    }

    fn channel_type(&self) -> ChannelType {
        ChannelType::IMessage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_contact_phone() {
        assert_eq!(
            IMessageChannel::normalize_contact("+1 (555) 123-4567"),
            "15551234567"
        );
    }

    #[tokio::test]
    async fn test_message_sender_tracking() {
        let channel = IMessageChannel::new(
            Duration::from_secs(3),
            vec!["+1-555-123-4567".to_string()],
            None,
        );

        // Simulate adding message sender mappings
        {
            let mut lru = channel.message_senders.lock().await;
            lru.put("imessage_123".to_string(), "+15551234567".to_string());
            lru.put("imessage_456".to_string(), "+15559999999".to_string());
        }

        // Verify lookups work
        {
            let mut lru = channel.message_senders.lock().await;
            assert_eq!(lru.get("imessage_123").unwrap(), "+15551234567");
            assert_eq!(lru.get("imessage_456").unwrap(), "+15559999999");
        }
    }

    #[tokio::test]
    async fn test_message_sender_lru_eviction() {
        let channel = IMessageChannel::new(
            Duration::from_secs(3),
            vec![],
            None,
        );

        // Fill the LRU cache beyond capacity
        {
            let mut lru = channel.message_senders.lock().await;
            for i in 0..MAX_MESSAGE_SENDERS + 10 {
                lru.put(format!("msg_{}", i), format!("sender_{}", i));
            }
            // Should not exceed capacity
            assert_eq!(lru.len(), MAX_MESSAGE_SENDERS);
            // Oldest entries should be evicted
            assert!(lru.peek("msg_0").is_none());
            assert!(lru.peek("msg_9").is_none());
            // Newest entries should still be present
            assert!(lru.peek(&format!("msg_{}", MAX_MESSAGE_SENDERS + 9)).is_some());
        }
    }

    #[test]
    fn test_normalize_contact_email() {
        assert_eq!(
            IMessageChannel::normalize_contact("User@Example.COM"),
            "user@examplecom"
        );
    }

    #[test]
    fn test_is_allowed_contact() {
        let channel = IMessageChannel::new(
            Duration::from_secs(3),
            vec!["+1-555-123-4567".to_string(), "user@test.com".to_string()],
            None,
        );

        assert!(channel.is_allowed_contact("+15551234567"));
        assert!(channel.is_allowed_contact("User@Test.com"));
        assert!(!channel.is_allowed_contact("unknown@other.com"));
    }

    #[test]
    fn test_is_allowed_empty_list() {
        let channel = IMessageChannel::new(
            Duration::from_secs(3),
            vec![],
            None,
        );
        assert!(!channel.is_allowed_contact("anyone"));
    }

    #[test]
    fn test_escape_applescript() {
        assert_eq!(
            IMessageChannel::escape_applescript("Hello \"world\""),
            "Hello \\\"world\\\""
        );
        assert_eq!(
            IMessageChannel::escape_applescript("line1\nline2"),
            "line1\\nline2"
        );
    }

    #[test]
    fn test_channel_type() {
        let channel = IMessageChannel::new(
            Duration::from_secs(3),
            vec![],
            None,
        );
        assert!(matches!(channel.channel_type(), ChannelType::IMessage));
    }
}
