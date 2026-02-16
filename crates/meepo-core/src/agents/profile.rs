//! Agent profile — defines an agent's identity, model, tools, and routing rules

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::types::ChannelType;

/// An agent profile defines a distinct agent persona with its own
/// model, system prompt, workspace, and tool configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub soul_file: Option<PathBuf>,
    #[serde(default)]
    pub memory_file: Option<PathBuf>,
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub denied_tools: Vec<String>,
    #[serde(default)]
    pub channels: Vec<ChannelRoute>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

impl AgentProfile {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            model: None,
            soul_file: None,
            memory_file: None,
            workspace: None,
            tools: Vec::new(),
            denied_tools: Vec::new(),
            channels: Vec::new(),
            max_tokens: None,
        }
    }

    /// Check if a tool is allowed for this agent
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        if self.denied_tools.contains(&tool_name.to_string()) {
            return false;
        }
        if self.tools.is_empty() {
            return true; // empty allowlist = all tools allowed
        }
        self.tools.contains(&tool_name.to_string())
    }

    /// Check if this agent should handle a message from the given channel/sender
    pub fn matches_route(&self, channel: &ChannelType, sender: &str) -> bool {
        if self.channels.is_empty() {
            return false; // no routes = don't match (use default agent)
        }
        self.channels
            .iter()
            .any(|route| route.matches(channel, sender))
    }
}

/// A channel routing rule that maps incoming messages to an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelRoute {
    pub channel_type: ChannelType,
    #[serde(default)]
    pub filter: RouteFilter,
}

impl ChannelRoute {
    pub fn new(channel_type: ChannelType) -> Self {
        Self {
            channel_type,
            filter: RouteFilter::default(),
        }
    }

    pub fn matches(&self, channel: &ChannelType, sender: &str) -> bool {
        if &self.channel_type != channel {
            return false;
        }
        self.filter.matches(sender)
    }
}

/// Filter for routing — can match by sender allowlist, workspace, or server
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteFilter {
    #[serde(default)]
    pub sender_allowlist: Vec<String>,
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
}

impl RouteFilter {
    pub fn matches(&self, sender: &str) -> bool {
        // If no filters set, match everything on this channel
        if self.sender_allowlist.is_empty() && self.workspace.is_none() && self.server.is_none() {
            return true;
        }
        // If sender allowlist is set, check it
        if !self.sender_allowlist.is_empty() {
            return self.sender_allowlist.iter().any(|s| s == sender);
        }
        // workspace/server filters are checked at a higher level
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_profile_new() {
        let profile = AgentProfile::new("work", "Work Agent");
        assert_eq!(profile.id, "work");
        assert_eq!(profile.name, "Work Agent");
        assert!(profile.tools.is_empty());
        assert!(profile.denied_tools.is_empty());
    }

    #[test]
    fn test_tool_allowed_empty_lists() {
        let profile = AgentProfile::new("test", "Test");
        assert!(profile.is_tool_allowed("any_tool"));
    }

    #[test]
    fn test_tool_allowed_allowlist() {
        let mut profile = AgentProfile::new("test", "Test");
        profile.tools = vec!["read_file".to_string(), "write_file".to_string()];
        assert!(profile.is_tool_allowed("read_file"));
        assert!(!profile.is_tool_allowed("run_command"));
    }

    #[test]
    fn test_tool_denied() {
        let mut profile = AgentProfile::new("test", "Test");
        profile.denied_tools = vec!["run_command".to_string()];
        assert!(!profile.is_tool_allowed("run_command"));
        assert!(profile.is_tool_allowed("read_file"));
    }

    #[test]
    fn test_deny_overrides_allow() {
        let mut profile = AgentProfile::new("test", "Test");
        profile.tools = vec!["run_command".to_string()];
        profile.denied_tools = vec!["run_command".to_string()];
        assert!(!profile.is_tool_allowed("run_command"));
    }

    #[test]
    fn test_matches_route_no_channels() {
        let profile = AgentProfile::new("test", "Test");
        assert!(!profile.matches_route(&ChannelType::Discord, "user123"));
    }

    #[test]
    fn test_matches_route_channel_match() {
        let mut profile = AgentProfile::new("test", "Test");
        profile.channels = vec![ChannelRoute::new(ChannelType::Discord)];
        assert!(profile.matches_route(&ChannelType::Discord, "user123"));
        assert!(!profile.matches_route(&ChannelType::Slack, "user123"));
    }

    #[test]
    fn test_matches_route_sender_filter() {
        let mut profile = AgentProfile::new("test", "Test");
        profile.channels = vec![ChannelRoute {
            channel_type: ChannelType::Discord,
            filter: RouteFilter {
                sender_allowlist: vec!["alice".to_string(), "bob".to_string()],
                workspace: None,
                server: None,
            },
        }];
        assert!(profile.matches_route(&ChannelType::Discord, "alice"));
        assert!(profile.matches_route(&ChannelType::Discord, "bob"));
        assert!(!profile.matches_route(&ChannelType::Discord, "charlie"));
    }

    #[test]
    fn test_route_filter_empty() {
        let filter = RouteFilter::default();
        assert!(filter.matches("anyone"));
    }

    #[test]
    fn test_route_filter_allowlist() {
        let filter = RouteFilter {
            sender_allowlist: vec!["alice".to_string()],
            workspace: None,
            server: None,
        };
        assert!(filter.matches("alice"));
        assert!(!filter.matches("bob"));
    }
}
