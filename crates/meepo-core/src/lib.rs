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
pub mod context;
pub mod orchestrator;
pub mod tools;
pub mod types;

// Re-export main types for convenience
pub use agent::Agent;
pub use api::{ApiClient, ApiMessage, ApiResponse, ContentBlock, MessageContent, ToolDefinition};
pub use context::{build_system_prompt, load_memory, load_soul};
pub use orchestrator::{
    ExecutionMode, FilteredToolExecutor, OrchestratorConfig,
    SubTask, SubTaskResult, SubTaskStatus, TaskGroup,
};
pub use tools::{ToolExecutor, ToolHandler, ToolRegistry};
pub use types::{ChannelType, IncomingMessage, OutgoingMessage};

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
}
