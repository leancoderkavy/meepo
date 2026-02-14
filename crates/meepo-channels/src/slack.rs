//! Slack channel adapter using Web API polling

use crate::bus::MessageChannel;
use crate::rate_limit::RateLimiter;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;
use meepo_core::types::{ChannelType, IncomingMessage, MessageKind, OutgoingMessage};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

const MAX_MESSAGE_SIZE: usize = 10_240;

/// Slack channel adapter using Web API polling
pub struct SlackChannel {
    bot_token: String,
    poll_interval: Duration,
    bot_user_id: Arc<RwLock<Option<String>>>,
    /// Slack user IDs allowed to interact with the agent.
    /// Empty means all users are allowed (open access).
    allowed_users: Vec<String>,
    /// Maps Slack user_id -> DM channel_id for routing replies
    channel_map: Arc<DashMap<String, String>>,
    /// Maps original message_id -> (channel_id, message_ts) for pending ack messages
    /// Used to update "Thinking..." placeholders with the real response
    pending_acks: Arc<DashMap<String, (String, String)>>,
}

impl SlackChannel {
    /// Create a new Slack channel adapter
    ///
    /// # Arguments
    /// * `bot_token` - Slack bot token (starts with xoxb-)
    /// * `poll_interval` - How often to poll for new messages
    /// * `allowed_users` - Slack user IDs allowed to interact (empty = all allowed)
    pub fn new(bot_token: String, poll_interval: Duration, allowed_users: Vec<String>) -> Self {
        Self {
            bot_token,
            poll_interval,
            bot_user_id: Arc::new(RwLock::new(None)),
            allowed_users,
            channel_map: Arc::new(DashMap::new()),
            pending_acks: Arc::new(DashMap::new()),
        }
    }

    /// Call a Slack Web API method
    async fn api_call(
        client: &reqwest::Client,
        token: &str,
        method: &str,
        params: &[(&str, &str)],
    ) -> Result<serde_json::Value> {
        let url = format!("https://slack.com/api/{}", method);
        let response = client
            .get(&url)
            .bearer_auth(token)
            .query(params)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!("Slack API HTTP error: {}", response.status()));
        }

        let body: serde_json::Value = response.json().await?;

        if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let err = body
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(anyhow!("Slack API error: {}", err));
        }

        Ok(body)
    }

    /// Post a message to a Slack channel, returning the message timestamp (ts)
    async fn post_message(
        client: &reqwest::Client,
        token: &str,
        channel: &str,
        text: &str,
    ) -> Result<String> {
        let url = "https://slack.com/api/chat.postMessage";
        let body = serde_json::json!({
            "channel": channel,
            "text": text,
        });

        let response = client
            .post(url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        let result: serde_json::Value = response.json().await?;

        if result.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let err = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(anyhow!("Slack chat.postMessage error: {}", err));
        }

        let ts = result
            .get("ts")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(ts)
    }

    /// Update an existing Slack message (used to replace "Thinking..." with real response)
    async fn update_message(
        client: &reqwest::Client,
        token: &str,
        channel: &str,
        ts: &str,
        text: &str,
    ) -> Result<()> {
        let url = "https://slack.com/api/chat.update";
        let body = serde_json::json!({
            "channel": channel,
            "ts": ts,
            "text": text,
        });

        let response = client
            .post(url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        let result: serde_json::Value = response.json().await?;

        if result.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let err = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(anyhow!("Slack chat.update error: {}", err));
        }

        Ok(())
    }
}

#[async_trait]
impl MessageChannel for SlackChannel {
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> Result<()> {
        info!("Starting Slack channel adapter");

        if self.bot_token.is_empty() {
            return Err(anyhow!("Slack bot token is empty"));
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        // Verify token and get bot user ID
        let auth_result = Self::api_call(&client, &self.bot_token, "auth.test", &[]).await?;
        let bot_user_id = auth_result
            .get("user_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Could not get bot user_id from auth.test"))?
            .to_string();

        info!("Slack bot authenticated as user_id: {}", bot_user_id);

        {
            let mut uid = self.bot_user_id.write().await;
            *uid = Some(bot_user_id.clone());
        }

        // Discover existing DM channels
        let convos = Self::api_call(
            &client,
            &self.bot_token,
            "conversations.list",
            &[("types", "im"), ("limit", "100")],
        )
        .await?;

        if let Some(channels) = convos.get("channels").and_then(|v| v.as_array()) {
            for ch in channels {
                let ch_id = ch.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let user = ch.get("user").and_then(|v| v.as_str()).unwrap_or("");
                if !ch_id.is_empty() && !user.is_empty() {
                    self.channel_map.insert(user.to_string(), ch_id.to_string());
                }
            }
            info!("Discovered {} Slack DM channels", self.channel_map.len());
        }

        // Clone data for the polling task
        // Note: Discovery (auth.test + conversations.list) is complete before spawning,
        // preventing race conditions where messages arrive before bot_user_id is set
        let token = self.bot_token.clone();
        let poll_interval = self.poll_interval;
        let channel_map = self.channel_map.clone();
        let bot_uid = bot_user_id;
        let allowed_users = self.allowed_users.clone();
        let rate_limiter = RateLimiter::new(10, Duration::from_secs(60));

        // Spawn polling task (safe: all initialization is complete)
        tokio::spawn(async move {
            info!("Slack polling task started");
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client for Slack polling");

            // Track latest timestamp per channel
            let mut latest_ts: HashMap<String, String> = HashMap::new();

            // Initialize latest_ts to "now" so we don't replay old messages
            let now_ts = format!("{}.000000", Utc::now().timestamp());
            for entry in channel_map.iter() {
                latest_ts.insert(entry.value().clone(), now_ts.clone());
            }

            let mut interval = tokio::time::interval(poll_interval);

            loop {
                interval.tick().await;
                debug!("Polling Slack for new messages");

                // Wrap the entire polling logic in a catch-all error handler to prevent panics
                let poll_result: Result<()> = async {
                    // Refresh DM channel list periodically
                if let Ok(convos) = Self::api_call(
                    &client,
                    &token,
                    "conversations.list",
                    &[("types", "im"), ("limit", "100")],
                )
                .await
                    && let Some(channels) = convos.get("channels").and_then(|v| v.as_array()) {
                        for ch in channels {
                            let ch_id = ch.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            let user = ch.get("user").and_then(|v| v.as_str()).unwrap_or("");
                            if !ch_id.is_empty() && !user.is_empty() {
                                channel_map.insert(user.to_string(), ch_id.to_string());
                                latest_ts.entry(ch_id.to_string()).or_insert_with(|| {
                                    format!("{}.000000", Utc::now().timestamp())
                                });
                            }
                        }
                    }

                // Poll each DM channel for new messages
                let channel_ids: Vec<String> = channel_map
                    .iter()
                    .map(|entry| entry.value().clone())
                    .collect();

                for channel_id in &channel_ids {
                    let oldest = latest_ts
                        .get(channel_id)
                        .cloned()
                        .unwrap_or_else(|| "0".to_string());

                    let history = match Self::api_call(
                        &client,
                        &token,
                        "conversations.history",
                        &[
                            ("channel", channel_id),
                            ("oldest", &oldest),
                            ("limit", "10"),
                        ],
                    )
                    .await
                    {
                        Ok(h) => h,
                        Err(e) => {
                            debug!("Failed to poll channel {}: {}", channel_id, e);
                            continue;
                        }
                    };

                    let messages = match history.get("messages").and_then(|v| v.as_array()) {
                        Some(msgs) => msgs,
                        None => continue,
                    };

                    let mut max_ts = oldest.clone();

                    for msg in messages {
                        let ts = msg.get("ts").and_then(|v| v.as_str()).unwrap_or("");
                        let user = msg.get("user").and_then(|v| v.as_str()).unwrap_or("");
                        let text = msg.get("text").and_then(|v| v.as_str()).unwrap_or("");

                        // Skip bot's own messages
                        if user == bot_uid {
                            if ts > max_ts.as_str() {
                                max_ts = ts.to_string();
                            }
                            continue;
                        }

                        // Check user authorization (M-3 fix)
                        if !allowed_users.is_empty() && !allowed_users.contains(&user.to_string()) {
                            debug!("Ignoring Slack message from unauthorized user: {}", user);
                            if ts > max_ts.as_str() {
                                max_ts = ts.to_string();
                            }
                            continue;
                        }

                        // Skip empty messages
                        if text.is_empty() {
                            continue;
                        }

                        // Check message size limit
                        if text.len() > MAX_MESSAGE_SIZE {
                            warn!(
                                "Dropping oversized Slack message from {} ({} bytes, limit {} bytes)",
                                user,
                                text.len(),
                                MAX_MESSAGE_SIZE,
                            );
                            if ts > max_ts.as_str() {
                                max_ts = ts.to_string();
                            }
                            continue;
                        }

                        // Check rate limit
                        if !rate_limiter.check_and_record(user) {
                            if ts > max_ts.as_str() {
                                max_ts = ts.to_string();
                            }
                            continue;
                        }

                        // Track max timestamp
                        if ts > max_ts.as_str() {
                            max_ts = ts.to_string();
                        }

                        // Store user -> channel mapping for replies
                        if !user.is_empty() {
                            channel_map.insert(user.to_string(), channel_id.clone());
                        }

                        // Convert to IncomingMessage
                        let incoming = IncomingMessage {
                            id: format!("slack_{}_{}", channel_id, ts),
                            sender: user.to_string(),
                            content: text.to_string(),
                            channel: ChannelType::Slack,
                            timestamp: Utc::now(),
                        };

                        info!("Forwarding Slack message from {} ({} chars)", user, text.len());

                        if let Err(e) = tx.send(incoming).await {
                            error!("Failed to send Slack message to bus: {}", e);
                        }
                    }

                    // Update the latest timestamp for this channel
                    if max_ts > oldest {
                        latest_ts.insert(channel_id.clone(), max_ts);
                    }
                }

                Ok(())
                }.await;

                // Log any errors but continue polling
                if let Err(e) = poll_result {
                    error!("Error during Slack polling cycle: {}", e);
                }
            }
        });

        info!("Slack channel adapter started");
        Ok(())
    }

    async fn send(&self, msg: OutgoingMessage) -> Result<()> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        // Find the channel to send to
        let channel_id = if let Some(reply_to) = &msg.reply_to
            && let Some(stripped) = reply_to.strip_prefix("slack_")
        {
            stripped.split('_').next().unwrap_or("").to_string()
        } else {
            String::new()
        };

        let channel_id = if channel_id.is_empty() {
            self.channel_map
                .iter()
                .next()
                .map(|entry| entry.value().clone())
                .ok_or_else(|| anyhow!("No Slack DM channels available for sending"))?
        } else {
            channel_id
        };

        // Handle acknowledgment: post "Thinking..." placeholder
        if msg.kind == MessageKind::Acknowledgment {
            debug!("Sending Slack acknowledgment to channel {}", channel_id);
            match Self::post_message(&client, &self.bot_token, &channel_id, "Thinking...").await {
                Ok(ts) => {
                    if let Some(reply_to) = &msg.reply_to {
                        self.pending_acks.insert(reply_to.clone(), (channel_id, ts));
                    }
                }
                Err(e) => warn!("Failed to send Slack acknowledgment: {}", e),
            }
            return Ok(());
        }

        // Normal response: check if there's a pending ack to update
        if let Some(reply_to) = &msg.reply_to
            && let Some((_, (ack_channel, ack_ts))) = self.pending_acks.remove(reply_to)
        {
            debug!("Updating Slack acknowledgment message with response");
            match Self::update_message(
                &client,
                &self.bot_token,
                &ack_channel,
                &ack_ts,
                &msg.content,
            )
            .await
            {
                Ok(()) => {
                    info!("Slack message updated successfully (replaced Thinking...)");
                    return Ok(());
                }
                Err(e) => {
                    warn!("Failed to update Slack message, posting new one: {}", e);
                    // Fall through to post as new message
                }
            }
        }

        Self::post_message(&client, &self.bot_token, &channel_id, &msg.content).await?;
        info!("Slack message sent successfully");
        Ok(())
    }

    fn channel_type(&self) -> ChannelType {
        ChannelType::Slack
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slack_channel_creation() {
        let channel = SlackChannel::new("xoxb-test-token".to_string(), Duration::from_secs(3), Vec::new());
        assert!(matches!(channel.channel_type(), ChannelType::Slack));
    }

    #[tokio::test]
    async fn test_slack_empty_token() {
        let channel = SlackChannel::new(String::new(), Duration::from_secs(3), Vec::new());
        let (tx, _rx) = mpsc::channel(10);
        let result = channel.start(tx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_slack_send_no_channels() {
        let channel = SlackChannel::new("xoxb-test".to_string(), Duration::from_secs(3), Vec::new());
        let msg = OutgoingMessage {
            content: "test".to_string(),
            channel: ChannelType::Slack,
            reply_to: None,
            kind: MessageKind::Response,
        };
        let result = channel.send(msg).await;
        assert!(result.is_err()); // No channels mapped yet
    }
}
