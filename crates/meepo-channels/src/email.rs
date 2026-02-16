//! Email channel adapter using Mail.app AppleScript polling

use crate::bus::MessageChannel;
use crate::rate_limit::RateLimiter;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::Utc;
use lru::LruCache;
use meepo_core::types::{ChannelType, IncomingMessage, MessageKind, OutgoingMessage};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

const MAX_EMAIL_SENDERS: usize = 500;

/// Timeout for Mail.app AppleScript polling (seconds)
const MAIL_POLL_TIMEOUT_SECS: u64 = 60;

/// Check if an application is currently running via System Events
async fn is_app_running(app_name: &str) -> bool {
    let script = format!(
        r#"tell application "System Events" to (name of processes) contains "{}"
"#,
        app_name
    );
    if let Ok(output) = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .await
    {
        String::from_utf8_lossy(&output.stdout).trim() == "true"
    } else {
        false
    }
}

/// Ensure Mail.app is running before polling. Launches it if needed.
async fn ensure_mail_app_running() {
    if is_app_running("Mail").await {
        return;
    }

    info!("Mail.app not running, launching before poll...");
    let launch_script = r#"tell application "Mail" to activate"#;
    let _ = tokio::time::timeout(
        Duration::from_secs(10),
        Command::new("osascript")
            .arg("-e")
            .arg(launch_script)
            .output(),
    )
    .await;

    // Wait for Mail.app to finish launching (poll up to 30s)
    for _ in 0..15 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if is_app_running("Mail").await {
            debug!("Mail.app is now running");
            // Give it a moment to finish initial sync
            tokio::time::sleep(Duration::from_secs(3)).await;
            return;
        }
    }

    warn!("Mail.app may not have fully launched, proceeding anyway");
}

/// Email channel adapter that polls Mail.app for incoming emails
pub struct EmailChannel {
    poll_interval: Duration,
    subject_prefix: String,
    /// Maps message_id -> (sender, original_subject) for reply routing
    message_senders: Arc<Mutex<LruCache<String, EmailMeta>>>,
    rate_limiter: RateLimiter,
}

/// Metadata about an email for reply threading
struct EmailMeta {
    sender: String,
    subject: String,
}

impl EmailChannel {
    pub fn new(poll_interval: Duration, subject_prefix: String) -> Self {
        Self {
            poll_interval,
            subject_prefix,
            message_senders: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(MAX_EMAIL_SENDERS).unwrap(),
            ))),
            rate_limiter: RateLimiter::new(10, Duration::from_secs(60)),
        }
    }

    /// Sanitize a string for safe use in AppleScript.
    /// Escapes special characters and strips control characters to prevent injection.
    fn escape_applescript(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .chars()
            .filter(|&c| c >= ' ' || c == '\t')
            .collect()
    }

    /// Poll Mail.app for unread emails matching the subject prefix
    async fn poll_emails(&self, tx: &mpsc::Sender<IncomingMessage>) -> Result<()> {
        // Ensure Mail.app is running before polling — avoids timeout on cold launch
        ensure_mail_app_running().await;

        let prefix = Self::escape_applescript(&self.subject_prefix);

        let script = format!(
            r#"
tell application "Mail"
    try
        set output to ""
        set unreadMsgs to (every message of inbox whose read status is false and subject begins with "{prefix}")
        repeat with m in unreadMsgs
            set msgSubject to subject of m
            set msgSender to sender of m
            set msgDate to date received of m as string
            set msgId to id of m
            set msgBody to content of m
            if length of msgBody > 2000 then
                set msgBody to text 1 thru 2000 of msgBody
            end if
            set output to output & "<<MSG_START>>" & "\n"
            set output to output & "ID: " & msgId & "\n"
            set output to output & "From: " & msgSender & "\n"
            set output to output & "Subject: " & msgSubject & "\n"
            set output to output & "Date: " & msgDate & "\n"
            set output to output & "Body: " & msgBody & "\n"
            set output to output & "<<MSG_END>>" & "\n"
            set read status of m to true
        end repeat
        return output
    on error errMsg
        return "ERROR: " & errMsg
    end try
end tell
"#
        );

        // Try once with 60s timeout; on timeout, retry once after a 4s backoff
        let output = match tokio::time::timeout(
            Duration::from_secs(MAIL_POLL_TIMEOUT_SECS),
            Command::new("osascript").arg("-e").arg(&script).output(),
        )
        .await
        {
            Ok(result) => result.map_err(|e| anyhow!("Failed to run osascript: {}", e))?,
            Err(_) => {
                warn!(
                    "Mail.app poll timed out after {}s, retrying once...",
                    MAIL_POLL_TIMEOUT_SECS
                );
                tokio::time::sleep(Duration::from_secs(4)).await;
                tokio::time::timeout(
                    Duration::from_secs(MAIL_POLL_TIMEOUT_SECS),
                    Command::new("osascript").arg("-e").arg(&script).output(),
                )
                .await
                .map_err(|_| anyhow!("Mail.app polling timed out after retry"))?
                .map_err(|e| anyhow!("Failed to run osascript: {}", e))?
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Mail.app poll failed: {}", stderr);
            return Ok(());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() || stdout.starts_with("ERROR:") {
            if stdout.starts_with("ERROR:") {
                warn!("Mail.app error: {}", stdout);
            }
            return Ok(());
        }

        for block in stdout.split("<<MSG_START>>") {
            let block = block.trim();
            if block.is_empty() || !block.contains("<<MSG_END>>") {
                continue;
            }

            let block = block.replace("<<MSG_END>>", "");
            let mut id = String::new();
            let mut sender = String::new();
            let mut subject = String::new();
            let mut body = String::new();

            for line in block.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("ID: ") {
                    id = val.to_string();
                } else if let Some(val) = line.strip_prefix("From: ") {
                    sender = val.to_string();
                } else if let Some(val) = line.strip_prefix("Subject: ") {
                    subject = val.to_string();
                } else if let Some(val) = line.strip_prefix("Body: ") {
                    body = val.to_string();
                }
            }

            if id.is_empty() || sender.is_empty() {
                debug!(
                    "Skipping email with missing id='{}' or sender='{}'",
                    id, sender
                );
                continue;
            }

            // Check rate limit
            if !self.rate_limiter.check_and_record(&sender) {
                continue;
            }

            let stripped_subject = subject
                .strip_prefix(&self.subject_prefix)
                .unwrap_or(&subject)
                .trim()
                .to_string();

            let content = if stripped_subject.is_empty() {
                body.clone()
            } else if body.is_empty() {
                stripped_subject.clone()
            } else {
                format!("{}\n\n{}", stripped_subject, body)
            };

            let msg_id = format!("email_{}", id);

            {
                let mut lru = self.message_senders.lock().await;
                lru.put(
                    msg_id.clone(),
                    EmailMeta {
                        sender: sender.clone(),
                        subject: subject.clone(),
                    },
                );
            }

            let incoming = IncomingMessage {
                id: msg_id,
                sender: sender.clone(),
                content,
                channel: ChannelType::Email,
                timestamp: Utc::now(),
            };

            info!("New email from {}: {}", sender, stripped_subject);

            if let Err(e) = tx.send(incoming).await {
                error!("Failed to send email message to bus: {}", e);
            }
        }

        Ok(())
    }

    /// Reply to an email using Mail.app threading
    async fn reply_to_email(
        &self,
        original_subject: &str,
        sender: &str,
        reply_body: &str,
    ) -> Result<()> {
        let safe_subject = Self::escape_applescript(original_subject);
        let safe_body = Self::escape_applescript(reply_body);
        let safe_sender = Self::escape_applescript(sender);

        let script = format!(
            r#"
tell application "Mail"
    try
        set targetMsgs to (every message of inbox whose subject is "{safe_subject}" and sender contains "{safe_sender}")
        if (count of targetMsgs) > 0 then
            set originalMsg to item 1 of targetMsgs
            set replyMsg to reply originalMsg with opening window
            set content of replyMsg to "{safe_body}"
            send replyMsg
            return "Reply sent (threaded)"
        else
            set newMsg to make new outgoing message with properties {{subject:"Re: {safe_subject}", content:"{safe_body}", visible:true}}
            tell newMsg
                make new to recipient at end of to recipients with properties {{address:"{safe_sender}"}}
                send
            end tell
            return "Reply sent (new message)"
        end if
    on error errMsg
        return "Error: " & errMsg
    end try
end tell
"#
        );

        let output = tokio::time::timeout(
            Duration::from_secs(30),
            Command::new("osascript").arg("-e").arg(&script).output(),
        )
        .await
        .map_err(|_| anyhow!("Email reply timed out"))?
        .map_err(|e| anyhow!("Failed to run osascript: {}", e))?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout);
            info!("Email reply result: {}", result.trim());
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow!("Failed to reply to email: {}", stderr))
        }
    }
}

#[async_trait]
impl MessageChannel for EmailChannel {
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> Result<()> {
        info!("Starting Email channel adapter");
        info!("Poll interval: {:?}", self.poll_interval);
        info!("Subject prefix: {}", self.subject_prefix);

        let poll_interval = self.poll_interval;
        let subject_prefix = self.subject_prefix.clone();
        let message_senders = self.message_senders.clone();
        let rate_limiter = self.rate_limiter.clone();

        let channel = EmailChannel {
            poll_interval,
            subject_prefix,
            message_senders,
            rate_limiter,
        };

        tokio::spawn(async move {
            info!("Email polling task started");
            let mut interval = tokio::time::interval(channel.poll_interval);

            loop {
                interval.tick().await;
                debug!("Polling Mail.app for new emails");

                if let Err(e) = channel.poll_emails(&tx).await {
                    error!("Error polling emails: {}", e);
                }
            }
        });

        info!("Email channel adapter started");
        Ok(())
    }

    async fn send(&self, msg: OutgoingMessage) -> Result<()> {
        if let Some(reply_to) = &msg.reply_to {
            let lru = self.message_senders.lock().await;
            if let Some(meta) = lru.peek(reply_to) {
                let subject = meta.subject.clone();
                let sender = meta.sender.clone();
                drop(lru);

                // Handle acknowledgment: send auto-reply
                if msg.kind == MessageKind::Acknowledgment {
                    debug!("Sending email acknowledgment to {}", sender);
                    if let Err(e) = self
                        .reply_to_email(
                            &subject,
                            &sender,
                            "Your message has been received. Working on a response...",
                        )
                        .await
                    {
                        warn!("Failed to send email acknowledgment: {}", e);
                    }
                    return Ok(());
                }

                // Normal response
                return self.reply_to_email(&subject, &sender, &msg.content).await;
            }
        }

        // Acknowledgments without reply context are silently ignored
        if msg.kind == MessageKind::Acknowledgment {
            debug!("Skipping email acknowledgment — no reply context");
            return Ok(());
        }

        warn!("Cannot send email without reply context (no reply_to or sender unknown)");
        Err(anyhow!("Cannot send email: no reply context available"))
    }

    fn channel_type(&self) -> ChannelType {
        ChannelType::Email
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_channel_creation() {
        let channel = EmailChannel::new(Duration::from_secs(10), "[meepo]".to_string());
        assert_eq!(channel.channel_type(), ChannelType::Email);
    }

    #[test]
    fn test_escape_applescript() {
        assert_eq!(
            EmailChannel::escape_applescript("Hello \"world\""),
            "Hello \\\"world\\\""
        );
        assert_eq!(
            EmailChannel::escape_applescript("line1\nline2"),
            "line1\\nline2"
        );
    }

    #[tokio::test]
    async fn test_email_meta_tracking() {
        let channel = EmailChannel::new(Duration::from_secs(10), "[meepo]".to_string());

        {
            let mut lru = channel.message_senders.lock().await;
            lru.put(
                "email_123".to_string(),
                EmailMeta {
                    sender: "user@example.com".to_string(),
                    subject: "[meepo] test subject".to_string(),
                },
            );
        }

        {
            let lru = channel.message_senders.lock().await;
            let meta = lru.peek("email_123").unwrap();
            assert_eq!(meta.sender, "user@example.com");
            assert_eq!(meta.subject, "[meepo] test subject");
        }
    }

    #[tokio::test]
    async fn test_send_without_context_fails() {
        let channel = EmailChannel::new(Duration::from_secs(10), "[meepo]".to_string());

        let msg = OutgoingMessage {
            content: "test reply".to_string(),
            channel: ChannelType::Email,
            reply_to: None,
            kind: MessageKind::Response,
        };

        let result = channel.send(msg).await;
        assert!(result.is_err());
    }
}
