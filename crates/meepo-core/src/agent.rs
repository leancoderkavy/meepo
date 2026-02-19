//! Main agent loop - the brain of meepo

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{debug, info};

use crate::api::ApiClient;
use crate::context::build_system_prompt;
use crate::guardrails::{GuardrailContext, GuardrailPipeline};
use crate::intent::{self, IntentConfig, UserIntent};
use crate::middleware::{MiddlewareChain, MiddlewareContext};
use crate::query_router::{self, QueryRouterConfig, RetrievalStrategy};
use crate::summarization::{self, SummarizationConfig};
use crate::tool_selector::{self, ToolSelectorConfig};
use crate::tools::{GuardedToolExecutor, ToolExecutor, ToolRegistry};
use crate::types::{IncomingMessage, MessageKind, OutgoingMessage};
use crate::usage::{UsageSource, UsageTracker};

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
    /// Middleware chain for pre/post processing
    middleware: MiddlewareChain,
    /// Query routing configuration
    router_config: QueryRouterConfig,
    /// Conversation summarization configuration
    summarization_config: SummarizationConfig,
    /// Tool selection configuration
    tool_selector_config: ToolSelectorConfig,
    /// Usage tracker for cost monitoring
    usage_tracker: Option<Arc<UsageTracker>>,
    /// Guardrails pipeline for input safety checks
    guardrails: Option<GuardrailPipeline>,
    /// Intent understanding configuration
    intent_config: IntentConfig,
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
            middleware: MiddlewareChain::new(),
            router_config: QueryRouterConfig::default(),
            summarization_config: SummarizationConfig::default(),
            tool_selector_config: ToolSelectorConfig::default(),
            usage_tracker: None,
            guardrails: None,
            intent_config: IntentConfig::default(),
        }
    }

    /// Set the middleware chain
    pub fn with_middleware(mut self, middleware: MiddlewareChain) -> Self {
        self.middleware = middleware;
        self
    }

    /// Set the query router configuration
    pub fn with_router_config(mut self, config: QueryRouterConfig) -> Self {
        self.router_config = config;
        self
    }

    /// Set the summarization configuration
    pub fn with_summarization_config(mut self, config: SummarizationConfig) -> Self {
        self.summarization_config = config;
        self
    }

    /// Set the tool selector configuration
    pub fn with_tool_selector_config(mut self, config: ToolSelectorConfig) -> Self {
        self.tool_selector_config = config;
        self
    }

    /// Set the usage tracker
    pub fn with_usage_tracker(mut self, tracker: Arc<UsageTracker>) -> Self {
        self.usage_tracker = Some(tracker);
        self
    }

    /// Set the guardrails pipeline
    pub fn with_guardrails(mut self, pipeline: GuardrailPipeline) -> Self {
        self.guardrails = Some(pipeline);
        self
    }

    /// Set the intent understanding configuration
    pub fn with_intent_config(mut self, config: IntentConfig) -> Self {
        self.intent_config = config;
        self
    }

    /// Handle an incoming message and generate a response
    pub async fn handle_message(&self, msg: IncomingMessage) -> Result<OutgoingMessage> {
        info!(
            "Handling message from {} on channel {}",
            msg.sender, msg.channel
        );

        // Run guardrails check on incoming message
        if let Some(guardrails) = &self.guardrails {
            let ctx = GuardrailContext {
                source: msg.sender.clone(),
                channel: msg.channel.to_string(),
                is_tool_output: false,
            };
            let result = guardrails.evaluate(&msg.content, &ctx).await?;
            if !result.passed {
                let violations: Vec<String> = result
                    .violations
                    .iter()
                    .map(|v| format!("{} ({:?})", v.rule, v.severity))
                    .collect();
                tracing::warn!(
                    "Guardrails blocked message from {}: {:?}",
                    msg.sender,
                    violations
                );
                return Ok(OutgoingMessage {
                    channel: msg.channel,
                    content:
                        "I'm unable to process that request as it was flagged by safety checks."
                            .to_string(),
                    reply_to: Some(msg.id.clone()),
                    kind: MessageKind::Response,
                });
            }
        }

        // Store the incoming message in conversation history
        self.db
            .insert_conversation(&msg.channel.to_string(), &msg.sender, &msg.content, None)
            .await
            .context("Failed to store conversation")?;

        // Understand the user's intent via LLM (with usage tracking)
        let (intent, intent_usage) =
            intent::understand_intent(&self.api, &msg.content, &self.intent_config)
                .await
                .unwrap_or_else(|e| {
                    debug!("Intent understanding failed, using defaults: {}", e);
                    (UserIntent::default(), None)
                });

        debug!(
            "Intent: action={:?}, entities={:?}, clarification_needed={}",
            intent.action, intent.entities, intent.clarification_needed
        );

        // Record intent LLM usage if any
        if let (Some(tracker), Some(usage)) = (&self.usage_tracker, &intent_usage) {
            let precall_usage = crate::usage::AccumulatedUsage::from_tokens(
                usage.input_tokens,
                usage.output_tokens,
            );
            if let Err(e) = tracker
                .record(
                    self.api.model(),
                    &precall_usage,
                    &UsageSource::User,
                    Some(&msg.channel.to_string()),
                )
                .await
            {
                debug!("Failed to record intent usage: {}", e);
            }
        }

        // If the intent signals clarification is needed, ask before proceeding
        if intent.clarification_needed {
            let clarification_prompt = if intent.canonical.is_empty() {
                msg.content.clone()
            } else {
                intent.canonical.clone()
            };
            debug!("Intent flagged clarification needed for: {:?}", clarification_prompt);
        }

        // Route the query to determine retrieval strategy (with usage tracking)
        let (strategy, router_usage) =
            query_router::route_query_tracked(&msg.content, Some(&self.api), &self.router_config)
                .await
                .unwrap_or_else(|e| {
                    debug!("Query routing failed, using default strategy: {}", e);
                    (
                        RetrievalStrategy {
                            complexity: query_router::QueryComplexity::SingleStep,
                            search_knowledge: true,
                            search_web: false,
                            load_history: true,
                            graph_expand: false,
                            corrective_rag: false,
                            knowledge_limit: 5,
                        },
                        None,
                    )
                });

        // Record router LLM usage if any
        if let (Some(tracker), Some(usage)) = (&self.usage_tracker, &router_usage) {
            let precall_usage = crate::usage::AccumulatedUsage::from_tokens(
                usage.input_tokens,
                usage.output_tokens,
            );
            if let Err(e) = tracker
                .record(
                    self.api.model(),
                    &precall_usage,
                    &UsageSource::User,
                    Some(&msg.channel.to_string()),
                )
                .await
            {
                debug!("Failed to record router usage: {}", e);
            }
        }

        debug!("Query routed as {:?}", strategy.complexity);

        // Load relevant context from knowledge graph (guided by strategy and intent)
        let context = self.load_context(&msg, &strategy, &intent).await?;

        // Build system prompt
        let system_prompt = build_system_prompt(&self.soul, &self.memory, &context);

        // Get tool definitions (with optional LLM selection + usage tracking)
        let all_tools = self.tools.list_tools();
        let (tool_definitions, selector_usage) = tool_selector::select_tools_tracked(
            &self.api,
            &msg.content,
            &all_tools,
            &self.tool_selector_config,
        )
        .await
        .unwrap_or((all_tools, None));

        // Record selector LLM usage if any
        if let (Some(tracker), Some(usage)) = (&self.usage_tracker, &selector_usage) {
            let precall_usage = crate::usage::AccumulatedUsage::from_tokens(
                usage.input_tokens,
                usage.output_tokens,
            );
            if let Err(e) = tracker
                .record(
                    self.api.model(),
                    &precall_usage,
                    &UsageSource::User,
                    Some(&msg.channel.to_string()),
                )
                .await
            {
                debug!("Failed to record selector usage: {}", e);
            }
        }

        debug!(
            "Using {} tools for this interaction",
            tool_definitions.len()
        );

        // Check budget before making API call
        if let Some(tracker) = &self.usage_tracker {
            match tracker.check_budget().await {
                Ok(crate::usage::BudgetStatus::Exceeded {
                    period,
                    spent,
                    budget,
                }) => {
                    return Ok(OutgoingMessage {
                        content: format!(
                            "I've reached my {} budget limit (${:.2} of ${:.2}). \
                             Please increase the budget in config.toml or wait for the next period.",
                            period, spent, budget
                        ),
                        channel: msg.channel,
                        reply_to: Some(msg.id),
                        kind: MessageKind::Response,
                    });
                }
                Ok(crate::usage::BudgetStatus::Warning {
                    period,
                    spent,
                    budget,
                    percent,
                }) => {
                    debug!(
                        "Budget warning: {} at {:.0}% (${:.2} of ${:.2})",
                        period, percent, spent, budget
                    );
                }
                Ok(crate::usage::BudgetStatus::Ok) => {}
                Err(e) => {
                    debug!("Budget check failed (proceeding anyway): {}", e);
                }
            }
        }

        // Build the tool executor — wrap with guardrails if configured to scan tool outputs
        // for indirect prompt injection (e.g. malicious content in web pages, emails, files)
        let tool_executor: Arc<dyn ToolExecutor> = if self.guardrails.is_some() {
            Arc::new(GuardedToolExecutor::new(
                self.tools.clone(),
                Arc::new(GuardrailPipeline::with_defaults()),
            ))
        } else {
            self.tools.clone()
        };

        // Run the tool loop to get final response
        let (response_text, usage) = self
            .api
            .run_tool_loop(
                &msg.content,
                &system_prompt,
                &tool_definitions,
                tool_executor.as_ref(),
            )
            .await
            .context("Failed to run agent tool loop")?;

        // Run middleware after_agent hooks on the final response
        let mw_ctx = MiddlewareContext {
            query: msg.content.clone(),
            channel: msg.channel.to_string(),
            sender: msg.sender.clone(),
            metadata: serde_json::Value::Null,
        };
        let response_text = self
            .middleware
            .run_after_agent(response_text, &mw_ctx)
            .await
            .unwrap_or_else(|e| {
                debug!("Middleware after_agent failed: {}", e);
                String::from("[Response processing error]")
            });

        // Record usage
        if let Some(tracker) = &self.usage_tracker
            && let Err(e) = tracker
                .record(
                    self.api.model(),
                    &usage,
                    &UsageSource::User,
                    Some(&msg.channel.to_string()),
                )
                .await
        {
            debug!("Failed to record usage: {}", e);
        }

        // Store the response in conversation history
        self.db
            .insert_conversation(&msg.channel.to_string(), "meepo", &response_text, None)
            .await
            .context("Failed to store response")?;

        info!(
            "Generated response ({} chars, {} tokens)",
            response_text.len(),
            usage.total_tokens()
        );

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
    async fn load_context(
        &self,
        msg: &IncomingMessage,
        strategy: &RetrievalStrategy,
        intent: &UserIntent,
    ) -> Result<String> {
        let mut context = String::new();
        let mut truncated = false;

        // Add recent conversation history from this channel (with summarization)
        if strategy.load_history {
            let recent = self
                .db
                .get_recent_conversations(Some(&msg.channel.to_string()), 30)
                .await
                .context("Failed to load recent conversations")?;

            if !recent.is_empty() {
                // Convert to (sender, content) pairs for summarization
                let conv_pairs: Vec<(String, String)> = recent
                    .iter()
                    .rev()
                    .map(|c| (c.sender.clone(), c.content.clone()))
                    .collect();

                // Try summarization for long histories
                match summarization::build_summarized_context(
                    &self.api,
                    &conv_pairs,
                    &self.summarization_config,
                )
                .await
                {
                    Ok(summarized) => {
                        context.push_str(&summarized);
                    }
                    Err(e) => {
                        // Fall back to raw history on summarization failure
                        debug!("Summarization failed, using raw history: {}", e);
                        context.push_str("## Recent Conversation\n\n");
                        for (sender, content) in conv_pairs.iter().take(10) {
                            context.push_str(&format!("{}: {}\n", sender, content));
                            if context.len() > MAX_CONTEXT_SIZE {
                                truncated = true;
                                break;
                            }
                        }
                        context.push('\n');
                    }
                }
            }
        }

        // Search for relevant entities mentioned in the message.
        // Prefer LLM-extracted entities from intent; fall back to keyword splitting.
        if !truncated && strategy.search_knowledge {
            let intent_keywords: Vec<&str> = intent.entities.iter().map(|s| s.as_str()).collect();
            let fallback_keywords: Vec<&str> = msg
                .content
                .split_whitespace()
                .filter(|word| word.len() > 3)
                .take(5)
                .collect();
            let keywords: Vec<&str> = if !intent_keywords.is_empty() {
                intent_keywords
            } else {
                fallback_keywords
            };

            if !keywords.is_empty() {
                context.push_str("## Relevant Knowledge\n\n");

                for keyword in keywords {
                    // Early termination: skip remaining keywords if context is already large
                    if context.len() > MAX_CONTEXT_SIZE {
                        truncated = true;
                        break;
                    }

                    if let Ok(entities) = self.db.search_entities(keyword, None).await {
                        for entity in entities.iter().take(strategy.knowledge_limit.min(3)) {
                            context
                                .push_str(&format!("- {} ({})", entity.name, entity.entity_type));
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
        if !truncated
            && let Ok(sender_entities) = self.db.search_entities(&msg.sender, Some("person")).await
            && let Some(sender_info) = sender_entities.first()
        {
            context.push_str("## About the Sender\n\n");
            context.push_str(&format!("Name: {}\n", sender_info.name));
            if let Some(metadata) = &sender_info.metadata {
                context.push_str(&format!("Info: {}\n", metadata));
            }
            context.push('\n');
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

    /// Get reference to the API client
    pub fn api(&self) -> &ApiClient {
        &self.api
    }

    /// Get reference to the usage tracker
    pub fn usage_tracker(&self) -> Option<&Arc<UsageTracker>> {
        self.usage_tracker.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolRegistry;
    use crate::types::ChannelType;
    use chrono::Utc;
    use tempfile::TempDir;

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

        let strategy = RetrievalStrategy {
            complexity: query_router::QueryComplexity::SingleStep,
            search_knowledge: true,
            search_web: false,
            load_history: true,
            graph_expand: false,
            corrective_rag: false,
            knowledge_limit: 5,
        };
        let context = agent.load_context(&msg, &strategy, &UserIntent::default()).await.unwrap();
        // Context is a String — load_context should succeed without panic
        assert!(context.len() <= 100_000, "Context unexpectedly large");
    }

    #[test]
    fn test_update_soul() {
        let (mut agent, _temp) = create_test_agent();
        agent.update_soul("New soul".to_string());
        assert_eq!(agent.soul, "New soul");
    }

    #[test]
    fn test_db_accessor() {
        let (agent, _temp) = create_test_agent();
        let _db = agent.db();
    }

    #[test]
    fn test_api_accessor() {
        let (agent, _temp) = create_test_agent();
        let _api = agent.api();
    }

    #[test]
    fn test_usage_tracker_none_by_default() {
        let (agent, _temp) = create_test_agent();
        assert!(agent.usage_tracker().is_none());
    }

    #[test]
    fn test_with_usage_tracker() {
        let (agent, _temp) = create_test_agent();
        let tracker = Arc::new(UsageTracker::new(
            Arc::clone(agent.db()),
            crate::usage::UsageConfig::default(),
        ));
        let agent = agent.with_usage_tracker(tracker);
        assert!(agent.usage_tracker().is_some());
    }

    #[test]
    fn test_with_router_config() {
        let (agent, _temp) = create_test_agent();
        let config = QueryRouterConfig {
            enabled: false,
            ..Default::default()
        };
        let agent = agent.with_router_config(config);
        assert!(!agent.router_config.enabled);
    }

    #[test]
    fn test_with_summarization_config() {
        let (agent, _temp) = create_test_agent();
        let config = SummarizationConfig {
            enabled: false,
            ..Default::default()
        };
        let agent = agent.with_summarization_config(config);
        assert!(!agent.summarization_config.enabled);
    }

    #[test]
    fn test_with_tool_selector_config() {
        let (agent, _temp) = create_test_agent();
        let config = ToolSelectorConfig {
            enabled: false,
            ..Default::default()
        };
        let agent = agent.with_tool_selector_config(config);
        assert!(!agent.tool_selector_config.enabled);
    }

    #[tokio::test]
    async fn test_load_context_no_history() {
        let (agent, _temp) = create_test_agent();

        let msg = IncomingMessage {
            id: "test-2".to_string(),
            sender: "user".to_string(),
            content: "Hello".to_string(),
            channel: ChannelType::Internal,
            timestamp: Utc::now(),
        };

        let strategy = RetrievalStrategy {
            complexity: query_router::QueryComplexity::SingleStep,
            search_knowledge: false,
            search_web: false,
            load_history: false,
            graph_expand: false,
            corrective_rag: false,
            knowledge_limit: 0,
        };
        let context = agent.load_context(&msg, &strategy, &UserIntent::default()).await.unwrap();
        assert!(context.is_empty() || context.len() < 100);
    }

    #[tokio::test]
    async fn test_load_context_with_knowledge() {
        let (agent, _temp) = create_test_agent();

        // Insert an entity so knowledge search finds something
        agent
            .db
            .insert_entity("Rust Language", "concept", None)
            .await
            .unwrap();

        let msg = IncomingMessage {
            id: "test-3".to_string(),
            sender: "user".to_string(),
            content: "Tell me about Rust Language please".to_string(),
            channel: ChannelType::Internal,
            timestamp: Utc::now(),
        };

        let strategy = RetrievalStrategy {
            complexity: query_router::QueryComplexity::SingleStep,
            search_knowledge: true,
            search_web: false,
            load_history: false,
            graph_expand: false,
            corrective_rag: false,
            knowledge_limit: 5,
        };
        let context = agent.load_context(&msg, &strategy, &UserIntent::default()).await.unwrap();
        assert!(context.contains("Rust Language"));
    }
}
