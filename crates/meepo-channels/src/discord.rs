//! Discord channel adapter using Serenity

use crate::bus::MessageChannel;
use meepo_core::types::{IncomingMessage, OutgoingMessage, ChannelType};
use serenity::{
    async_trait,
    model::prelude::*,
    prelude::*,
    model::gateway::Ready,
};
use tokio::sync::mpsc;
use std::sync::Arc;
use std::num::NonZeroUsize;
use std::time::Duration;
use dashmap::DashMap;
use lru::LruCache;
use anyhow::{Result, anyhow};
use tracing::{info, error, debug, warn};
use chrono::Utc;
use tokio::sync::{Mutex, RwLock};

const MAX_MESSAGE_CHANNELS: usize = 1000;

/// Type key for storing the incoming message sender in Serenity's TypeMap
struct MessageSender;

impl TypeMapKey for MessageSender {
    type Value = mpsc::Sender<IncomingMessage>;
}

/// Type key for storing the user-to-channel mapping
struct UserChannelMap;

impl TypeMapKey for UserChannelMap {
    type Value = Arc<DashMap<UserId, ChannelId>>;
}

/// Type key for storing message_id -> channel_id mapping for replies (LRU-bounded)
struct MessageChannelMap;

impl TypeMapKey for MessageChannelMap {
    type Value = Arc<Mutex<LruCache<String, ChannelId>>>;
}

/// Type key for storing allowed users
struct AllowedUsers;

impl TypeMapKey for AllowedUsers {
    type Value = Vec<UserId>;
}

/// Event handler for Discord messages
struct DiscordHandler;

#[async_trait]
impl EventHandler for DiscordHandler {
    async fn message(&self, ctx: Context, msg: Message) {
        // Ignore bot messages
        if msg.author.bot {
            return;
        }

        // Only process direct messages (guild_id is None for DMs)
        if msg.guild_id.is_some() {
            return;
        }

        debug!("Received DM from user: {} ({})", msg.author.name, msg.author.id);

        // Check if user is allowed
        let data = ctx.data.read().await;
        let allowed_users = match data.get::<AllowedUsers>() {
            Some(users) => users,
            None => {
                error!("AllowedUsers not initialized in TypeMap");
                return;
            }
        };

        if !allowed_users.contains(&msg.author.id) {
            warn!("Ignoring DM from unauthorized user: {}", msg.author.id);
            return;
        }

        // Store the channel mapping for replies
        let user_channel_map = data.get::<UserChannelMap>().expect("UserChannelMap not initialized");
        user_channel_map.insert(msg.author.id, msg.channel_id);

        // Store message_id -> channel_id mapping for reply tracking (LRU-bounded)
        let message_channel_map = data.get::<MessageChannelMap>().expect("MessageChannelMap not initialized").clone();
        let msg_id = format!("discord_{}", msg.id);
        {
            let mut lru = message_channel_map.lock().await;
            lru.put(msg_id.clone(), msg.channel_id);
        }

        // Get the message sender
        let tx = data.get::<MessageSender>().expect("MessageSender not initialized").clone();
        drop(data); // Release the lock

        // Convert to IncomingMessage
        let incoming = IncomingMessage {
            id: msg_id,
            sender: match msg.author.discriminator {
                Some(d) => format!("{}#{:04}", msg.author.name, d),
                None => msg.author.name.clone(),
            },
            content: msg.content.clone(),
            channel: ChannelType::Discord,
            timestamp: Utc::now(),
        };

        info!("Forwarding Discord message from {}", incoming.sender);

        // Send to the bus
        if let Err(e) = tx.send(incoming).await {
            error!("Failed to send Discord message to bus: {}", e);
        }
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!("Discord bot connected as {}", ready.user.name);
    }
}

/// Discord channel adapter
pub struct DiscordChannel {
    token: String,
    allowed_users: Vec<String>, // Discord user IDs to accept DMs from
    http: Arc<RwLock<Option<Arc<serenity::http::Http>>>>,
    user_channel_map: Arc<DashMap<UserId, ChannelId>>,
    /// Maps message_id -> channel_id for reply-to tracking (LRU-bounded)
    message_channels: Arc<Mutex<LruCache<String, ChannelId>>>,
}

impl DiscordChannel {
    /// Create a new Discord channel adapter
    ///
    /// # Arguments
    /// * `token` - Discord bot token
    /// * `allowed_users` - List of Discord user IDs (as strings) allowed to send messages
    pub fn new(token: String, allowed_users: Vec<String>) -> Self {
        Self {
            token,
            allowed_users,
            http: Arc::new(RwLock::new(None)),
            user_channel_map: Arc::new(DashMap::new()),
            message_channels: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(MAX_MESSAGE_CHANNELS).unwrap(),
            ))),
        }
    }

    /// Parse user IDs from strings to UserId
    fn parse_user_ids(&self) -> Result<Vec<UserId>> {
        self.allowed_users
            .iter()
            .map(|id_str| {
                id_str.parse::<u64>()
                    .map(UserId::new)
                    .map_err(|e| anyhow!("Invalid Discord user ID '{}': {}", id_str, e))
            })
            .collect()
    }
}

#[async_trait]
impl MessageChannel for DiscordChannel {
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> Result<()> {
        info!("Starting Discord channel adapter");

        // Parse user IDs
        let user_ids = self.parse_user_ids()?;
        info!("Allowed Discord users: {:?}", user_ids);

        // Clone data needed inside the spawned task
        let token = self.token.clone();
        let user_channel_map = self.user_channel_map.clone();
        let message_channels = self.message_channels.clone();
        let http_arc = self.http.clone();

        // Spawn the Discord client in a background task with retry logic
        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            let max_backoff = Duration::from_secs(60);
            let mut retry_count = 0;

            loop {
                retry_count += 1;
                info!("Discord client starting (attempt #{})", retry_count);

                // Set up intents
                let intents = GatewayIntents::DIRECT_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

                // Build the client
                let mut client = match Client::builder(&token, intents)
                    .event_handler(DiscordHandler)
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to create Discord client: {}", e);
                        warn!("Retrying in {:?}...", backoff);
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(max_backoff);
                        continue;
                    }
                };

                // Store the HTTP client for sending messages
                let http = client.http.clone();

                // Store data in TypeMap
                {
                    let mut data = client.data.write().await;
                    data.insert::<MessageSender>(tx.clone());
                    data.insert::<UserChannelMap>(user_channel_map.clone());
                    data.insert::<MessageChannelMap>(message_channels.clone());
                    data.insert::<AllowedUsers>(user_ids.clone());
                }

                // Store HTTP client for sending messages
                {
                    let mut http_guard = http_arc.write().await;
                    *http_guard = Some(http);
                }

                // Start the client
                match client.start().await {
                    Ok(_) => {
                        info!("Discord client stopped cleanly");
                        break;
                    }
                    Err(e) => {
                        error!("Discord client error: {}", e);
                        warn!("Retrying in {:?}...", backoff);
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(max_backoff);
                    }
                }
            }

            info!("Discord client task exiting");
        });

        info!("Discord channel adapter started");
        Ok(())
    }

    async fn send(&self, msg: OutgoingMessage) -> Result<()> {
        let http_guard = self.http.read().await;
        let http = http_guard.as_ref()
            .ok_or_else(|| anyhow!("Discord channel not started yet"))?;

        debug!("Sending Discord message");

        // Look up channel from reply_to if present
        let channel_id = if let Some(reply_to) = &msg.reply_to {
            // Look up the channel from our LRU message_channels map
            let mut lru = self.message_channels.lock().await;
            if let Some(channel) = lru.get(reply_to) {
                debug!("Found channel from reply_to: {}", reply_to);
                Some(*channel)
            } else {
                // reply_to not found, fall back to first available channel
                warn!("reply_to '{}' not found in message tracking, falling back to first available channel", reply_to);
                self.user_channel_map.iter().next().map(|entry| *entry.value())
            }
        } else {
            // No reply_to specified, use first available channel
            self.user_channel_map.iter().next().map(|entry| *entry.value())
        };

        if let Some(channel_id) = channel_id {
            // Send the message
            channel_id.say(http, &msg.content).await
                .map_err(|e| anyhow!("Failed to send Discord message: {}", e))?;

            info!("Discord message sent successfully to channel {}", channel_id);
            Ok(())
        } else {
            Err(anyhow!("No Discord users have messaged the bot yet"))
        }
    }

    fn channel_type(&self) -> ChannelType {
        ChannelType::Discord
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discord_creation() {
        let channel = DiscordChannel::new(
            "test-token".to_string(),
            vec!["12345".to_string()],
        );
        assert!(matches!(channel.channel_type(), ChannelType::Discord));
    }

    #[test]
    fn test_parse_valid_user_ids() {
        let channel = DiscordChannel::new(
            "token".to_string(),
            vec!["123456789".to_string(), "987654321".to_string()],
        );
        let ids = channel.parse_user_ids().unwrap();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_parse_invalid_user_id() {
        let channel = DiscordChannel::new(
            "token".to_string(),
            vec!["not-a-number".to_string()],
        );
        let result = channel.parse_user_ids();
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_user_ids() {
        let channel = DiscordChannel::new(
            "token".to_string(),
            vec![],
        );
        let ids = channel.parse_user_ids().unwrap();
        assert_eq!(ids.len(), 0);
    }
}
