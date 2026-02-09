//! Channel adapters and message bus for meepo
//!
//! This crate provides the message routing infrastructure and channel-specific
//! adapters for Discord, iMessage, and Slack.

pub mod bus;
pub mod discord;
pub mod email;
pub mod imessage;
pub mod slack;

// Re-export main types
pub use bus::{MessageBus, MessageChannel};
pub use discord::DiscordChannel;
pub use email::EmailChannel;
pub use imessage::IMessageChannel;
pub use slack::SlackChannel;
