//! Multi-agent routing — agent profiles, channel routing, and agent manager
//!
//! Routes incoming messages to isolated agents based on channel type,
//! sender, and routing rules. Each agent has its own model, system prompt,
//! workspace, and tool set.

pub mod manager;
pub mod profile;

pub use manager::AgentManager;
pub use profile::{AgentProfile, ChannelRoute, RouteFilter};
