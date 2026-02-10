//! Main agent loop - the brain of meepo

use anyhow::{Result, Context};
use std::sync::Arc;
use tracing::{info, debug};

use crate::api::ApiClient;
use crate::tools::{ToolExecutor, ToolRegistry};
use crate::types::{IncomingMessage, MessageKind, OutgoingMessage};
use crate::context::build_system_prompt;

use meepo_knowledge::KnowledgeDb;

/// Maximum context size in bytes to prevent multi-MB context strings.
const MAX_CONTEXT_SIZE: usize = 100_000;

/// Main agent that handles messages and orchestrates responses
pub struct Agent {
    api: ApiClient,
    tools: Arc<ToolRegistry>,
    soul: String,
    memory: String,
    db: Arc<KnowledgeDb>,
}

impl Agent {
    /// Create a new agent instance
    pub fn new(
        api: ApiClient,
        tools: Arc<ToolRegistry>,
        soul: String,
        memory: String,
        db: Arc<KnowledgeDb>,
    ) -> Self {
        Self {
            api,
            tools,
            soul,
            memory,
            db,
        }
    }

    /// Handle an incoming message and generate a response
    pub async fn handle_message(&self, msg: IncomingMessage) -> Result<OutgoingMessage> {
        info!(
            "Handling message from {} on channel {}",
            msg.sender, msg.channel
        );

        // Store the incoming message in conversation history
        self.db.insert_conversation(
            &msg.channel.to_string(),
            &msg.sender,
            &msg.content,
            None,
        ).await.context("Failed to store conversation")?;

        // Load relevant context from knowledge graph
        let context = self.load_context(&msg).await?;

        // Build system prompt
        let system_prompt = build_system_prompt(&self.soul, &self.memory, &context);

        // Get tool definitions
        let tool_definitions = self.tools.list_tools();

        debug!("Using {} tools for this interaction", tool_definitions.len());

        // Run the tool loop to get final response
        let response_text = self.api.run_tool_loop(
            &msg.content,
            &system_prompt,
            &tool_definitions,
            self.tools.as_ref(),
        ).await.context("Failed to run agent tool loop")?;

        // Store the response in conversation history
        self.db.insert_conversation(
            &msg.channel.to_string(),
            "meepo",
            &response_text,
            None,
        ).await.context("Failed to store response")?;

        info!("Generated response ({} chars)", response_text.len());

        Ok(OutgoingMessage {
            content: response_text,
            channel: msg.channel,
            reply_to: Some(msg.id),
            kind: MessageKind::Response,
        })
    }

    /// Load relevant context for the message.
    ///
    /// Context is capped at [`MAX_CONTEXT_SIZE`] bytes to prevent multi-MB
    /// strings from being sent to the LLM API. Each major section checks the
    /// limit and stops early when exceeded.
    async fn load_context(&self, msg: &IncomingMessage) -> Result<String> {
        let mut context = String::new();
        let mut truncated = false;

        // Add recent conversation history from this channel
        let recent = self.db.get_recent_conversations(
            Some(&msg.channel.to_string()),
            10,
        ).await.context("Failed to load recent conversations")?;

        if !recent.is_empty() {
            context.push_str("## Recent Conversation\n\n");
            for conv in recent.iter().rev() {
                context.push_str(&format!("{}: {}\n", conv.sender, conv.content));
                if context.len() > MAX_CONTEXT_SIZE {
                    truncated = true;
                    break;
                }
            }
            context.push_str("\n");
        }

        // Search for relevant entities mentioned in the message
        // Simple keyword extraction - split on whitespace and search each word
        if !truncated {
            let keywords: Vec<&str> = msg.content
                .split_whitespace()
                .filter(|word| word.len() > 3)
                .take(5)
                .collect();

            if !keywords.is_empty() {
                context.push_str("## Relevant Knowledge\n\n");

                for keyword in keywords {
                    // Early termination: skip remaining keywords if context is already large
                    if context.len() > MAX_CONTEXT_SIZE {
                        truncated = true;
                        break;
                    }

                    if let Ok(entities) = self.db.search_entities(keyword, None).await {
                        for entity in entities.iter().take(3) {
                            context.push_str(&format!(
                                "- {} ({})",
                                entity.name, entity.entity_type
                            ));
                            if let Some(metadata) = &entity.metadata {
                                context.push_str(&format!(": {}", metadata));
                            }
                            context.push('\n');
                        }
                    }
                }
                context.push('\n');
            }
        }

        // Add metadata about the sender if available
        if !truncated {
            if let Ok(sender_entities) = self.db.search_entities(&msg.sender, Some("person")).await {
                if let Some(sender_info) = sender_entities.first() {
                    context.push_str("## About the Sender\n\n");
                    context.push_str(&format!("Name: {}\n", sender_info.name));
                    if let Some(metadata) = &sender_info.metadata {
                        context.push_str(&format!("Info: {}\n", metadata));
                    }
                    context.push('\n');
                }
            }
        }

        // Final truncation guard: hard-cap the string if it still exceeds the limit
        if context.len() > MAX_CONTEXT_SIZE {
            context.truncate(MAX_CONTEXT_SIZE);
            context.push_str("\n[Context truncated]");
            truncated = true;
        }

        if truncated {
            debug!("Loaded context ({} chars, truncated)", context.len());
        } else {
            debug!("Loaded context ({} chars)", context.len());
        }

        Ok(context)
    }

    /// Update the agent's memory
    pub fn update_memory(&mut self, new_memory: String) {
        self.memory = new_memory;
        info!("Updated agent memory ({} chars)", self.memory.len());
    }

    /// Update the agent's soul
    pub fn update_soul(&mut self, new_soul: String) {
        self.soul = new_soul;
        info!("Updated agent soul ({} chars)", self.soul.len());
    }

    /// Get reference to the knowledge database
    pub fn db(&self) -> &Arc<KnowledgeDb> {
        &self.db
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolRegistry;
    use crate::types::ChannelType;
    use tempfile::TempDir;
    use chrono::Utc;

    fn create_test_agent() -> (Agent, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Arc::new(KnowledgeDb::new(&db_path).unwrap());

        let api = ApiClient::new("test-key".to_string(), None);
        let tools = Arc::new(ToolRegistry::new());
        let soul = "I am a test agent".to_string();
        let memory = "Test memory".to_string();

        let agent = Agent::new(api, tools, soul, memory, db);
        (agent, temp_dir)
    }

    #[test]
    fn test_agent_creation() {
        let (agent, _temp) = create_test_agent();
        assert_eq!(agent.soul, "I am a test agent");
        assert_eq!(agent.memory, "Test memory");
    }

    #[test]
    fn test_update_memory() {
        let (mut agent, _temp) = create_test_agent();
        agent.update_memory("New memory".to_string());
        assert_eq!(agent.memory, "New memory");
    }

    #[tokio::test]
    async fn test_load_context() {
        let (agent, _temp) = create_test_agent();

        let msg = IncomingMessage {
            id: "test-1".to_string(),
            sender: "test-user".to_string(),
            content: "Hello meepo".to_string(),
            channel: ChannelType::Internal,
            timestamp: Utc::now(),
        };

        let context = agent.load_context(&msg).await.unwrap();
        // Context is a String — load_context should succeed without panic
        assert!(context.len() <= 100_000, "Context unexpectedly large");
    }
}
