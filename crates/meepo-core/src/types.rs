//! Shared types for meepo-core

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
    Alexa,
    Reminders,
    Notes,
    Contacts,
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
            "alexa" => Self::Alexa,
            "reminders" => Self::Reminders,
            "notes" => Self::Notes,
            "contacts" => Self::Contacts,
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
            Self::Alexa => write!(f, "alexa"),
            Self::Reminders => write!(f, "reminders"),
            Self::Notes => write!(f, "notes"),
            Self::Contacts => write!(f, "contacts"),
            Self::Internal => write!(f, "internal"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    // ── ChannelType::from_string ────────────────────────────────

    #[test]
    fn test_channel_type_from_string_all_variants() {
        assert_eq!(ChannelType::from_string("discord"), ChannelType::Discord);
        assert_eq!(ChannelType::from_string("slack"), ChannelType::Slack);
        assert_eq!(ChannelType::from_string("imessage"), ChannelType::IMessage);
        assert_eq!(ChannelType::from_string("email"), ChannelType::Email);
        assert_eq!(ChannelType::from_string("alexa"), ChannelType::Alexa);
        assert_eq!(ChannelType::from_string("reminders"), ChannelType::Reminders);
        assert_eq!(ChannelType::from_string("notes"), ChannelType::Notes);
        assert_eq!(ChannelType::from_string("contacts"), ChannelType::Contacts);
    }

    #[test]
    fn test_channel_type_from_string_case_insensitive() {
        assert_eq!(ChannelType::from_string("Discord"), ChannelType::Discord);
        assert_eq!(ChannelType::from_string("SLACK"), ChannelType::Slack);
        assert_eq!(ChannelType::from_string("IMessage"), ChannelType::IMessage);
        assert_eq!(ChannelType::from_string("EMAIL"), ChannelType::Email);
    }

    #[test]
    fn test_channel_type_from_string_unknown_defaults_internal() {
        assert_eq!(ChannelType::from_string("unknown"), ChannelType::Internal);
        assert_eq!(ChannelType::from_string(""), ChannelType::Internal);
        assert_eq!(ChannelType::from_string("telegram"), ChannelType::Internal);
    }

    // ── ChannelType Display ─────────────────────────────────────

    #[test]
    fn test_channel_type_display_all_variants() {
        assert_eq!(ChannelType::Discord.to_string(), "discord");
        assert_eq!(ChannelType::Slack.to_string(), "slack");
        assert_eq!(ChannelType::IMessage.to_string(), "imessage");
        assert_eq!(ChannelType::Email.to_string(), "email");
        assert_eq!(ChannelType::Alexa.to_string(), "alexa");
        assert_eq!(ChannelType::Reminders.to_string(), "reminders");
        assert_eq!(ChannelType::Notes.to_string(), "notes");
        assert_eq!(ChannelType::Contacts.to_string(), "contacts");
        assert_eq!(ChannelType::Internal.to_string(), "internal");
    }

    #[test]
    fn test_channel_type_display_roundtrip() {
        let variants = [
            ChannelType::Discord,
            ChannelType::Slack,
            ChannelType::IMessage,
            ChannelType::Email,
            ChannelType::Alexa,
            ChannelType::Reminders,
            ChannelType::Notes,
            ChannelType::Contacts,
        ];
        for v in &variants {
            let s = v.to_string();
            let parsed = ChannelType::from_string(&s);
            assert_eq!(&parsed, v, "roundtrip failed for {s}");
        }
    }

    // ── ChannelType Hash + Eq ───────────────────────────────────

    #[test]
    fn test_channel_type_hash_eq() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ChannelType::Discord);
        set.insert(ChannelType::Discord);
        assert_eq!(set.len(), 1);
        set.insert(ChannelType::Slack);
        assert_eq!(set.len(), 2);
    }

    // ── MessageKind ─────────────────────────────────────────────

    #[test]
    fn test_message_kind_default() {
        assert_eq!(MessageKind::default(), MessageKind::Response);
    }

    #[test]
    fn test_message_kind_equality() {
        assert_eq!(MessageKind::Response, MessageKind::Response);
        assert_eq!(MessageKind::Acknowledgment, MessageKind::Acknowledgment);
        assert_ne!(MessageKind::Response, MessageKind::Acknowledgment);
    }

    // ── Serde roundtrips ────────────────────────────────────────

    #[test]
    fn test_channel_type_serde_roundtrip() {
        let ct = ChannelType::Discord;
        let json = serde_json::to_string(&ct).unwrap();
        assert_eq!(json, "\"discord\"");
        let parsed: ChannelType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ct);
    }

    #[test]
    fn test_channel_type_serde_all_variants() {
        let variants = [
            (ChannelType::Discord, "\"discord\""),
            (ChannelType::Slack, "\"slack\""),
            (ChannelType::IMessage, "\"imessage\""),
            (ChannelType::Email, "\"email\""),
            (ChannelType::Alexa, "\"alexa\""),
            (ChannelType::Reminders, "\"reminders\""),
            (ChannelType::Notes, "\"notes\""),
            (ChannelType::Contacts, "\"contacts\""),
            (ChannelType::Internal, "\"internal\""),
        ];
        for (variant, expected_json) in &variants {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(&json, expected_json);
            let parsed: ChannelType = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, variant);
        }
    }

    #[test]
    fn test_message_kind_serde_roundtrip() {
        let mk = MessageKind::Acknowledgment;
        let json = serde_json::to_string(&mk).unwrap();
        assert_eq!(json, "\"acknowledgment\"");
        let parsed: MessageKind = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, mk);
    }

    #[test]
    fn test_incoming_message_serde_roundtrip() {
        let msg = IncomingMessage {
            id: "msg-1".to_string(),
            sender: "user@test".to_string(),
            content: "hello".to_string(),
            channel: ChannelType::Discord,
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: IncomingMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, msg.id);
        assert_eq!(parsed.sender, msg.sender);
        assert_eq!(parsed.content, msg.content);
        assert_eq!(parsed.channel, msg.channel);
    }

    #[test]
    fn test_outgoing_message_serde_roundtrip() {
        let msg = OutgoingMessage {
            content: "response".to_string(),
            channel: ChannelType::Slack,
            reply_to: Some("msg-1".to_string()),
            kind: MessageKind::Response,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: OutgoingMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, msg.content);
        assert_eq!(parsed.channel, msg.channel);
        assert_eq!(parsed.reply_to, msg.reply_to);
        assert_eq!(parsed.kind, msg.kind);
    }

    #[test]
    fn test_outgoing_message_reply_to_none_skipped() {
        let msg = OutgoingMessage {
            content: "hi".to_string(),
            channel: ChannelType::Email,
            reply_to: None,
            kind: MessageKind::default(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("reply_to"));
    }

    #[test]
    fn test_outgoing_message_kind_defaults_to_response() {
        let json = r#"{"content":"hi","channel":"discord"}"#;
        let parsed: OutgoingMessage = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.kind, MessageKind::Response);
    }
}
