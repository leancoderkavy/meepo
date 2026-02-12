//! meepo-core - The brain of the meepo agent
//!
//! This crate provides:
//! - Agent loop that handles incoming messages and generates responses
//! - Anthropic API client with full tool use loop support
//! - Comprehensive tool system with macOS integration, code execution, memory, and more
//! - Context loading from SOUL and MEMORY files
//! - Integration with knowledge graph and watcher scheduler

pub mod agent;
pub mod api;
pub mod autonomy;
pub mod context;
pub mod notifications;
pub mod orchestrator;
pub mod platform;
pub mod skills;
pub mod tavily;
pub mod tools;
pub mod types;

// Re-export main types for convenience
pub use agent::Agent;
pub use api::{ApiClient, ApiMessage, ApiResponse, ContentBlock, MessageContent, ToolDefinition};
pub use autonomy::{AutonomousLoop, AutonomyConfig};
pub use context::build_system_prompt;
pub use orchestrator::{
    ExecutionMode, FilteredToolExecutor, OrchestratorConfig,
    SubTask, SubTaskResult, SubTaskStatus, TaskGroup, TaskOrchestrator,
};
pub use tools::{ToolExecutor, ToolHandler, ToolRegistry};
pub use notifications::{NotificationService, NotifyConfig, NotifyEvent};
pub use types::{ChannelType, IncomingMessage, MessageKind, OutgoingMessage};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crate_exports() {
        // Just verify that all main types are exported
        let _ = std::mem::size_of::<Agent>();
        let _ = std::mem::size_of::<ApiClient>();
        let _ = std::mem::size_of::<ToolRegistry>();
        let _ = std::mem::size_of::<IncomingMessage>();
        let _ = std::mem::size_of::<OutgoingMessage>();
    }

    #[test]
    fn test_orchestrator_exports() {
        let _ = std::mem::size_of::<TaskOrchestrator>();
        let _ = std::mem::size_of::<FilteredToolExecutor>();
        let _ = std::mem::size_of::<OrchestratorConfig>();
        let _ = std::mem::size_of::<SubTask>();
        let _ = std::mem::size_of::<SubTaskResult>();
        let _ = std::mem::size_of::<TaskGroup>();
    }
}
