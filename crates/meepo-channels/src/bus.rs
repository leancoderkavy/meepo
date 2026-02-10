//! Central message bus for routing messages between channels and the agent

use meepo_core::types::{IncomingMessage, OutgoingMessage, ChannelType};
#[cfg(test)]
use meepo_core::types::MessageKind;
use tokio::sync::mpsc;
use std::collections::HashMap;
use async_trait::async_trait;
use anyhow::{Result, anyhow};
use tracing::{info, error, debug};

/// Trait that all channel adapters implement
#[async_trait]
pub trait MessageChannel: Send + Sync {
    /// Start listening for messages, sending them to the provided sender
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> Result<()>;

    /// Send a message through this channel
    async fn send(&self, msg: OutgoingMessage) -> Result<()>;

    /// Which channel type this adapter handles
    fn channel_type(&self) -> ChannelType;
}

/// Central message bus that routes messages between channels and the agent
pub struct MessageBus {
    channels: HashMap<ChannelType, Box<dyn MessageChannel>>,
    incoming_tx: mpsc::Sender<IncomingMessage>,
    incoming_rx: mpsc::Receiver<IncomingMessage>,
}

impl MessageBus {
    /// Create a new message bus with the specified buffer size for incoming messages
    pub fn new(buffer_size: usize) -> Self {
        let (tx, rx) = mpsc::channel(buffer_size);
        info!("Created message bus with buffer size {}", buffer_size);
        Self {
            channels: HashMap::new(),
            incoming_tx: tx,
            incoming_rx: rx,
        }
    }

    /// Register a channel adapter with the bus
    pub fn register(&mut self, channel: Box<dyn MessageChannel>) {
        let channel_type = channel.channel_type();
        info!("Registering channel: {}", channel_type);
        self.channels.insert(channel_type, channel);
    }

    /// Start all registered channel listeners
    /// Each channel runs in its own tokio task
    pub async fn start_all(&self) -> Result<()> {
        info!("Starting all {} registered channels", self.channels.len());

        for (channel_type, channel) in &self.channels {
            let tx = self.incoming_tx.clone();
            let channel_type = channel_type.clone();

            // We need to work around the trait object limitation
            // by having each channel implementation handle its own async execution
            debug!("Starting channel: {}", channel_type);

            // Clone the sender for this channel's task
            let tx_clone = tx.clone();

            // Start the channel (this should spawn its own task internally)
            if let Err(e) = channel.start(tx_clone).await {
                error!("Failed to start channel {}: {}", channel_type, e);
                return Err(anyhow!("Failed to start channel {}: {}", channel_type, e));
            }

            info!("Successfully started channel: {}", channel_type);
        }

        info!("All channels started successfully");
        Ok(())
    }

    /// Receive the next incoming message from any channel
    /// Returns None if all channel senders have been dropped
    pub async fn recv(&mut self) -> Option<IncomingMessage> {
        self.incoming_rx.recv().await
    }

    /// Send an outgoing message to the appropriate channel
    pub async fn send(&self, msg: OutgoingMessage) -> Result<()> {
        let channel_type = &msg.channel;
        debug!("Routing outgoing message to channel: {}", channel_type);

        let channel = self.channels
            .get(channel_type)
            .ok_or_else(|| anyhow!("No channel registered for type: {}", channel_type))?;

        channel.send(msg).await?;
        Ok(())
    }

    /// Get the number of registered channels
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Check if a specific channel type is registered
    pub fn has_channel(&self, channel_type: &ChannelType) -> bool {
        self.channels.contains_key(channel_type)
    }

    /// Split the bus into a receiver and a sender handle.
    /// This allows the receiver to be used in a select! loop while the sender
    /// is cloned into spawned tasks for routing responses.
    pub fn split(self) -> (mpsc::Receiver<IncomingMessage>, BusSender) {
        let sender = BusSender {
            channels: self.channels,
        };
        (self.incoming_rx, sender)
    }
}

/// Send-only handle for the message bus
/// Separated from the receiver to allow concurrent send/receive
pub struct BusSender {
    channels: HashMap<ChannelType, Box<dyn MessageChannel>>,
}

impl BusSender {
    /// Send an outgoing message to the appropriate channel
    pub async fn send(&self, msg: OutgoingMessage) -> Result<()> {
        let channel_type = &msg.channel;
        debug!("Routing outgoing message to channel: {}", channel_type);

        let channel = self.channels
            .get(channel_type)
            .ok_or_else(|| anyhow!("No channel registered for type: {}", channel_type))?;

        channel.send(msg).await?;
        Ok(())
    }

    /// Check if a specific channel type is registered
    pub fn has_channel(&self, channel_type: &ChannelType) -> bool {
        self.channels.contains_key(channel_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Mock channel for testing
    struct MockChannel {
        channel_type: ChannelType,
        sent: Arc<AtomicBool>,
    }

    impl MockChannel {
        fn new(channel_type: ChannelType) -> Self {
            Self {
                channel_type,
                sent: Arc::new(AtomicBool::new(false)),
            }
        }

    }

    #[async_trait]
    impl MessageChannel for MockChannel {
        async fn start(&self, _tx: mpsc::Sender<IncomingMessage>) -> Result<()> {
            Ok(())
        }

        async fn send(&self, _msg: OutgoingMessage) -> Result<()> {
            self.sent.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn channel_type(&self) -> ChannelType {
            self.channel_type.clone()
        }
    }

    #[test]
    fn test_bus_creation() {
        let bus = MessageBus::new(32);
        assert_eq!(bus.channel_count(), 0);
    }

    #[test]
    fn test_bus_register() {
        let mut bus = MessageBus::new(32);
        bus.register(Box::new(MockChannel::new(ChannelType::Discord)));
        assert_eq!(bus.channel_count(), 1);
        assert!(bus.has_channel(&ChannelType::Discord));
        assert!(!bus.has_channel(&ChannelType::Slack));
    }

    #[test]
    fn test_bus_register_multiple() {
        let mut bus = MessageBus::new(32);
        bus.register(Box::new(MockChannel::new(ChannelType::Discord)));
        bus.register(Box::new(MockChannel::new(ChannelType::Slack)));
        bus.register(Box::new(MockChannel::new(ChannelType::IMessage)));
        assert_eq!(bus.channel_count(), 3);
    }

    #[tokio::test]
    async fn test_bus_start_all() {
        let mut bus = MessageBus::new(32);
        bus.register(Box::new(MockChannel::new(ChannelType::Discord)));
        let result = bus.start_all().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_bus_split() {
        let mut bus = MessageBus::new(32);
        bus.register(Box::new(MockChannel::new(ChannelType::Discord)));
        bus.start_all().await.unwrap();

        let (_rx, sender) = bus.split();
        assert!(sender.has_channel(&ChannelType::Discord));
        assert!(!sender.has_channel(&ChannelType::Slack));
    }

    #[tokio::test]
    async fn test_bus_sender_send() {
        let mut bus = MessageBus::new(32);
        let mock = MockChannel::new(ChannelType::Discord);
        let sent_flag = mock.sent.clone();
        bus.register(Box::new(mock));
        bus.start_all().await.unwrap();

        let (_rx, sender) = bus.split();

        let msg = OutgoingMessage {
            content: "test".to_string(),
            channel: ChannelType::Discord,
            reply_to: None,
            kind: MessageKind::Response,
        };
        sender.send(msg).await.unwrap();
        assert!(sent_flag.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_bus_sender_unknown_channel() {
        let mut bus = MessageBus::new(32);
        bus.register(Box::new(MockChannel::new(ChannelType::Discord)));
        bus.start_all().await.unwrap();

        let (_rx, sender) = bus.split();

        let msg = OutgoingMessage {
            content: "test".to_string(),
            channel: ChannelType::Slack,
            reply_to: None,
            kind: MessageKind::Response,
        };
        let result = sender.send(msg).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_bus_incoming_messages() {
        let mut bus = MessageBus::new(32);
        let mock = MockChannel::new(ChannelType::Discord);
        bus.register(Box::new(mock));

        // Get the tx before start_all (it's stored in the bus)
        let tx = bus.incoming_tx.clone();
        bus.start_all().await.unwrap();

        let (mut rx, _sender) = bus.split();

        // Send a message through the tx
        let incoming = IncomingMessage {
            id: "test-1".to_string(),
            sender: "user".to_string(),
            content: "hello".to_string(),
            channel: ChannelType::Discord,
            timestamp: chrono::Utc::now(),
        };
        tx.send(incoming).await.unwrap();

        // Receive it
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.id, "test-1");
        assert_eq!(msg.content, "hello");
    }
}
