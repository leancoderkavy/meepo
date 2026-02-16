//! Agent manager — routes incoming messages to the correct agent

use std::collections::HashMap;
use tracing::{debug, info, warn};

use super::profile::AgentProfile;
use crate::types::ChannelType;

/// Manages multiple agent profiles and routes messages to the correct one
pub struct AgentManager {
    profiles: HashMap<String, AgentProfile>,
    default_agent_id: String,
}

impl AgentManager {
    /// Create a new agent manager with a default agent
    pub fn new(default_profile: AgentProfile) -> Self {
        let default_id = default_profile.id.clone();
        let mut profiles = HashMap::new();
        profiles.insert(default_profile.id.clone(), default_profile);
        info!("AgentManager: initialized with default agent '{}'", default_id);
        Self {
            profiles,
            default_agent_id: default_id,
        }
    }

    /// Add an agent profile
    pub fn add_profile(&mut self, profile: AgentProfile) {
        info!(
            "AgentManager: added agent '{}' ({}) with {} channel routes",
            profile.id,
            profile.name,
            profile.channels.len()
        );
        self.profiles.insert(profile.id.clone(), profile);
    }

    /// Get an agent profile by ID
    pub fn get_profile(&self, id: &str) -> Option<&AgentProfile> {
        self.profiles.get(id)
    }

    /// Get the default agent profile
    pub fn default_profile(&self) -> &AgentProfile {
        self.profiles
            .get(&self.default_agent_id)
            .expect("default agent must exist")
    }

    /// Route a message to the correct agent based on channel and sender
    pub fn route(&self, channel: &ChannelType, sender: &str) -> &AgentProfile {
        for profile in self.profiles.values() {
            if profile.id == self.default_agent_id {
                continue; // skip default, it's the fallback
            }
            if profile.matches_route(channel, sender) {
                debug!(
                    "AgentManager: routed {}/{} → agent '{}'",
                    channel, sender, profile.id
                );
                return profile;
            }
        }
        debug!(
            "AgentManager: routed {}/{} → default agent '{}'",
            channel, sender, self.default_agent_id
        );
        self.default_profile()
    }

    /// List all agent profile IDs
    pub fn list_agents(&self) -> Vec<&str> {
        self.profiles.keys().map(|k| k.as_str()).collect()
    }

    /// Number of registered agents
    pub fn count(&self) -> usize {
        self.profiles.len()
    }

    /// Remove an agent profile (cannot remove default)
    pub fn remove_profile(&mut self, id: &str) -> bool {
        if id == self.default_agent_id {
            warn!("AgentManager: cannot remove default agent '{}'", id);
            return false;
        }
        self.profiles.remove(id).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::profile::{ChannelRoute, RouteFilter};

    fn default_profile() -> AgentProfile {
        AgentProfile::new("default", "Default Agent")
    }

    fn work_profile() -> AgentProfile {
        let mut p = AgentProfile::new("work", "Work Agent");
        p.channels = vec![ChannelRoute {
            channel_type: ChannelType::Slack,
            filter: RouteFilter::default(),
        }];
        p.tools = vec!["email".to_string(), "calendar".to_string()];
        p
    }

    fn personal_profile() -> AgentProfile {
        let mut p = AgentProfile::new("personal", "Personal Agent");
        p.channels = vec![
            ChannelRoute::new(ChannelType::Discord),
            ChannelRoute::new(ChannelType::IMessage),
        ];
        p
    }

    #[test]
    fn test_agent_manager_new() {
        let mgr = AgentManager::new(default_profile());
        assert_eq!(mgr.count(), 1);
        assert_eq!(mgr.default_profile().id, "default");
    }

    #[test]
    fn test_add_and_list() {
        let mut mgr = AgentManager::new(default_profile());
        mgr.add_profile(work_profile());
        mgr.add_profile(personal_profile());
        assert_eq!(mgr.count(), 3);
        let agents = mgr.list_agents();
        assert!(agents.contains(&"default"));
        assert!(agents.contains(&"work"));
        assert!(agents.contains(&"personal"));
    }

    #[test]
    fn test_route_to_work() {
        let mut mgr = AgentManager::new(default_profile());
        mgr.add_profile(work_profile());
        mgr.add_profile(personal_profile());

        let routed = mgr.route(&ChannelType::Slack, "coworker");
        assert_eq!(routed.id, "work");
    }

    #[test]
    fn test_route_to_personal() {
        let mut mgr = AgentManager::new(default_profile());
        mgr.add_profile(work_profile());
        mgr.add_profile(personal_profile());

        let routed = mgr.route(&ChannelType::Discord, "friend");
        assert_eq!(routed.id, "personal");

        let routed = mgr.route(&ChannelType::IMessage, "mom");
        assert_eq!(routed.id, "personal");
    }

    #[test]
    fn test_route_to_default() {
        let mut mgr = AgentManager::new(default_profile());
        mgr.add_profile(work_profile());

        // Email has no route → falls back to default
        let routed = mgr.route(&ChannelType::Email, "someone");
        assert_eq!(routed.id, "default");
    }

    #[test]
    fn test_route_with_sender_filter() {
        let mut mgr = AgentManager::new(default_profile());
        let mut vip = AgentProfile::new("vip", "VIP Agent");
        vip.channels = vec![ChannelRoute {
            channel_type: ChannelType::Discord,
            filter: RouteFilter {
                sender_allowlist: vec!["boss".to_string()],
                workspace: None,
                server: None,
            },
        }];
        mgr.add_profile(vip);

        let routed = mgr.route(&ChannelType::Discord, "boss");
        assert_eq!(routed.id, "vip");

        let routed = mgr.route(&ChannelType::Discord, "random");
        assert_eq!(routed.id, "default");
    }

    #[test]
    fn test_remove_profile() {
        let mut mgr = AgentManager::new(default_profile());
        mgr.add_profile(work_profile());
        assert_eq!(mgr.count(), 2);

        assert!(mgr.remove_profile("work"));
        assert_eq!(mgr.count(), 1);

        // Cannot remove default
        assert!(!mgr.remove_profile("default"));
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn test_get_profile() {
        let mut mgr = AgentManager::new(default_profile());
        mgr.add_profile(work_profile());

        assert!(mgr.get_profile("work").is_some());
        assert!(mgr.get_profile("nonexistent").is_none());
    }
}
