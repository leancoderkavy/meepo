//! Shared types for meepo-core

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Incoming message from any channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingMessage {
    pub id: String,
    pub sender: String,
    pub content: String,
    pub channel: ChannelType,
    pub timestamp: DateTime<Utc>,
}

/// What kind of outgoing message this is
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    /// Normal response message
    #[default]
    Response,
    /// Acknowledgment/typing indicator — channel decides how to display
    Acknowledgment,
}

/// Outgoing message to be sent to a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingMessage {
    pub content: String,
    pub channel: ChannelType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>, // original message id
    #[serde(default)]
    pub kind: MessageKind,
}

/// Type of communication channel
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    Discord,
    Slack,
    IMessage,
    Email,
    Internal, // for watcher-generated messages
}

impl ChannelType {
    /// Parse a channel type from a string (e.g., from watcher reply_channel)
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "discord" => Self::Discord,
            "slack" => Self::Slack,
            "imessage" => Self::IMessage,
            "email" => Self::Email,
            _ => Self::Internal,
        }
    }
}

impl std::fmt::Display for ChannelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discord => write!(f, "discord"),
            Self::Slack => write!(f, "slack"),
            Self::IMessage => write!(f, "imessage"),
            Self::Email => write!(f, "email"),
            Self::Internal => write!(f, "internal"),
        }
    }
}
