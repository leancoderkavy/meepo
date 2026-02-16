//! Adaptive Query Routing
//!
//! Classifies incoming queries by complexity and dynamically selects
//! the retrieval strategy — skip retrieval for simple questions,
//! single-step for factual lookups, multi-step for complex reasoning.
//! Inspired by Adaptive RAG (Jeong et al., 2024).

use anyhow::{Context, Result};
use tracing::debug;

use crate::api::{ApiClient, ApiMessage, ContentBlock, MessageContent};

/// Query complexity classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryComplexity {
    /// Direct answer from LLM knowledge, no retrieval needed
    /// e.g., "What time is it?", "What's 2+2?"
    NoRetrieval,
    /// Simple factual lookup, single knowledge search
    /// e.g., "What did I say about the project?"
    SingleStep,
    /// Needs knowledge + web search or multi-source retrieval
    /// e.g., "What's the latest on X and how does it relate to my project?"
    MultiSource,
    /// Complex multi-hop reasoning, may need sub-agent delegation
    /// e.g., "Compare my calendar this week with last week and suggest optimizations"
    MultiHop,
}

/// Retrieval strategy determined by the router
#[derive(Debug, Clone)]
pub struct RetrievalStrategy {
    pub complexity: QueryComplexity,
    /// Whether to search the knowledge graph
    pub search_knowledge: bool,
    /// Whether to search the web
    pub search_web: bool,
    /// Whether to load conversation history
    pub load_history: bool,
    /// Whether to expand results via GraphRAG
    pub graph_expand: bool,
    /// Whether to use corrective RAG (validate + refine)
    pub corrective_rag: bool,
    /// Suggested number of knowledge results to retrieve
    pub knowledge_limit: usize,
}

impl RetrievalStrategy {
    fn no_retrieval() -> Self {
        Self {
            complexity: QueryComplexity::NoRetrieval,
            search_knowledge: false,
            search_web: false,
            load_history: true, // always load some history for continuity
            graph_expand: false,
            corrective_rag: false,
            knowledge_limit: 0,
        }
    }

    fn single_step() -> Self {
        Self {
            complexity: QueryComplexity::SingleStep,
            search_knowledge: true,
            search_web: false,
            load_history: true,
            graph_expand: false,
            corrective_rag: false,
            knowledge_limit: 5,
        }
    }

    fn multi_source() -> Self {
        Self {
            complexity: QueryComplexity::MultiSource,
            search_knowledge: true,
            search_web: true,
            load_history: true,
            graph_expand: true,
            corrective_rag: false,
            knowledge_limit: 10,
        }
    }

    fn multi_hop() -> Self {
        Self {
            complexity: QueryComplexity::MultiHop,
            search_knowledge: true,
            search_web: true,
            load_history: true,
            graph_expand: true,
            corrective_rag: true,
            knowledge_limit: 15,
        }
    }
}

/// Configuration for the query router
#[derive(Debug, Clone)]
pub struct QueryRouterConfig {
    /// Whether to use LLM-based classification (vs heuristic)
    pub use_llm_classification: bool,
    /// Whether the router is enabled at all
    pub enabled: bool,
}

impl Default for QueryRouterConfig {
    fn default() -> Self {
        Self {
            use_llm_classification: false, // start with heuristics, cheaper
            enabled: true,
        }
    }
}

/// Route a query to the appropriate retrieval strategy.
///
/// Uses heuristics first (fast, free), with optional LLM classification
/// for ambiguous cases.
pub async fn route_query(
    query: &str,
    api: Option<&ApiClient>,
    config: &QueryRouterConfig,
) -> Result<RetrievalStrategy> {
    if !config.enabled {
        // Default: full retrieval
        return Ok(RetrievalStrategy::multi_source());
    }

    // First try heuristic classification
    let heuristic = classify_heuristic(query);

    if config.use_llm_classification && heuristic == QueryComplexity::MultiSource {
        // Only use LLM for ambiguous cases (MultiSource is the "unsure" default)
        if let Some(api) = api {
            match classify_with_llm(api, query).await {
                Ok(complexity) => {
                    debug!("LLM classified query as {:?}", complexity);
                    return Ok(strategy_for(complexity));
                }
                Err(e) => {
                    debug!("LLM classification failed, using heuristic: {}", e);
                }
            }
        }
    }

    debug!("Heuristic classified query as {:?}", heuristic);
    Ok(strategy_for(heuristic))
}

/// Heuristic-based query classification (fast, no API call)
fn classify_heuristic(query: &str) -> QueryComplexity {
    let lower = query.to_lowercase();
    let word_count = query.split_whitespace().count();

    // Very short queries or greetings → no retrieval
    if word_count <= 3 {
        let greetings = [
            "hi",
            "hello",
            "hey",
            "thanks",
            "thank you",
            "bye",
            "ok",
            "yes",
            "no",
        ];
        if greetings.iter().any(|g| lower.trim() == *g) {
            return QueryComplexity::NoRetrieval;
        }
    }

    // Time/date queries → no retrieval
    if lower.contains("what time") || lower.contains("what day") || lower.contains("what date") {
        return QueryComplexity::NoRetrieval;
    }

    // Simple math → no retrieval
    if lower.starts_with("what is ")
        && lower
            .chars()
            .any(|c| c == '+' || c == '-' || c == '*' || c == '/')
    {
        return QueryComplexity::NoRetrieval;
    }

    // Recall/memory queries → single step
    let recall_signals = [
        "what did i",
        "do you remember",
        "recall",
        "what do you know about",
        "tell me about",
        "who is",
        "what is my",
    ];
    if recall_signals.iter().any(|s| lower.contains(s)) {
        return QueryComplexity::SingleStep;
    }

    // Web search signals → multi source
    let web_signals = [
        "latest",
        "news",
        "current",
        "today",
        "search for",
        "look up",
        "find out",
        "what's happening",
        "trending",
    ];
    if web_signals.iter().any(|s| lower.contains(s)) {
        return QueryComplexity::MultiSource;
    }

    // Complex reasoning signals → multi hop
    let complex_signals = [
        "compare",
        "analyze",
        "summarize all",
        "across",
        "relationship between",
        "how does",
        "why did",
        "what are the implications",
        "plan for",
        "step by step",
        "research",
    ];
    if complex_signals.iter().any(|s| lower.contains(s)) {
        return QueryComplexity::MultiHop;
    }

    // Action commands → single step (tools will handle it)
    let action_signals = [
        "send", "create", "open", "play", "set", "schedule", "remind", "write", "make", "run",
        "execute", "start", "stop",
    ];
    if action_signals.iter().any(|s| lower.starts_with(s)) {
        return QueryComplexity::SingleStep;
    }

    // Default: single step for medium queries, multi-source for longer ones
    if word_count > 15 {
        QueryComplexity::MultiSource
    } else {
        QueryComplexity::SingleStep
    }
}

/// LLM-based query classification for ambiguous cases
async fn classify_with_llm(api: &ApiClient, query: &str) -> Result<QueryComplexity> {
    let classification_prompt = format!(
        "Classify this query's complexity for retrieval. Respond with ONLY one word:\n\
         - NONE: Simple greeting, math, or direct knowledge (no retrieval needed)\n\
         - SIMPLE: Factual lookup from stored knowledge\n\
         - MULTI: Needs multiple sources (knowledge + web)\n\
         - COMPLEX: Multi-step reasoning across sources\n\n\
         Query: {}\n\nClassification:",
        query
    );

    let messages = vec![ApiMessage {
        role: "user".to_string(),
        content: MessageContent::Text(classification_prompt),
    }];

    let response = api
        .chat(
            &messages,
            &[],
            "You are a query classifier. Respond with exactly one word.",
        )
        .await
        .context("Failed to classify query")?;

    let text = response
        .content
        .iter()
        .filter_map(|b| {
            if let ContentBlock::Text { text } = b {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<String>();

    let trimmed = text.trim().to_uppercase();
    Ok(match trimmed.as_str() {
        "NONE" => QueryComplexity::NoRetrieval,
        "SIMPLE" => QueryComplexity::SingleStep,
        "MULTI" => QueryComplexity::MultiSource,
        "COMPLEX" => QueryComplexity::MultiHop,
        _ => QueryComplexity::SingleStep, // safe default
    })
}

fn strategy_for(complexity: QueryComplexity) -> RetrievalStrategy {
    match complexity {
        QueryComplexity::NoRetrieval => RetrievalStrategy::no_retrieval(),
        QueryComplexity::SingleStep => RetrievalStrategy::single_step(),
        QueryComplexity::MultiSource => RetrievalStrategy::multi_source(),
        QueryComplexity::MultiHop => RetrievalStrategy::multi_hop(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heuristic_greetings() {
        assert_eq!(classify_heuristic("hello"), QueryComplexity::NoRetrieval);
        assert_eq!(classify_heuristic("hi"), QueryComplexity::NoRetrieval);
        assert_eq!(classify_heuristic("thanks"), QueryComplexity::NoRetrieval);
    }

    #[test]
    fn test_heuristic_time() {
        assert_eq!(
            classify_heuristic("what time is it?"),
            QueryComplexity::NoRetrieval
        );
    }

    #[test]
    fn test_heuristic_recall() {
        assert_eq!(
            classify_heuristic("what did I say about the project?"),
            QueryComplexity::SingleStep
        );
        assert_eq!(
            classify_heuristic("do you remember my preference?"),
            QueryComplexity::SingleStep
        );
    }

    #[test]
    fn test_heuristic_web() {
        assert_eq!(
            classify_heuristic("what's the latest news on AI?"),
            QueryComplexity::MultiSource
        );
    }

    #[test]
    fn test_heuristic_complex() {
        assert_eq!(
            classify_heuristic("compare my calendar this week with last week"),
            QueryComplexity::MultiHop
        );
        assert_eq!(
            classify_heuristic("analyze the relationship between these projects"),
            QueryComplexity::MultiHop
        );
    }

    #[test]
    fn test_heuristic_actions() {
        assert_eq!(
            classify_heuristic("send an email to John"),
            QueryComplexity::SingleStep
        );
        assert_eq!(
            classify_heuristic("create a reminder for tomorrow"),
            QueryComplexity::SingleStep
        );
    }

    #[tokio::test]
    async fn test_route_disabled() {
        let config = QueryRouterConfig {
            enabled: false,
            ..Default::default()
        };
        let strategy = route_query("hello", None, &config).await.unwrap();
        assert_eq!(strategy.complexity, QueryComplexity::MultiSource);
    }

    #[tokio::test]
    async fn test_route_heuristic() {
        let config = QueryRouterConfig::default();
        let strategy = route_query("hello", None, &config).await.unwrap();
        assert_eq!(strategy.complexity, QueryComplexity::NoRetrieval);
        assert!(!strategy.search_knowledge);
    }

    #[test]
    fn test_heuristic_math() {
        assert_eq!(
            classify_heuristic("what is 2 + 2?"),
            QueryComplexity::NoRetrieval
        );
        assert_eq!(
            classify_heuristic("what is 10 * 5?"),
            QueryComplexity::NoRetrieval
        );
    }

    #[test]
    fn test_heuristic_date_queries() {
        assert_eq!(
            classify_heuristic("what day is it today?"),
            QueryComplexity::NoRetrieval
        );
        assert_eq!(
            classify_heuristic("what date is the meeting?"),
            QueryComplexity::NoRetrieval
        );
    }

    #[test]
    fn test_heuristic_more_greetings() {
        assert_eq!(classify_heuristic("bye"), QueryComplexity::NoRetrieval);
        assert_eq!(classify_heuristic("ok"), QueryComplexity::NoRetrieval);
        assert_eq!(classify_heuristic("yes"), QueryComplexity::NoRetrieval);
        assert_eq!(classify_heuristic("no"), QueryComplexity::NoRetrieval);
        assert_eq!(
            classify_heuristic("thank you"),
            QueryComplexity::NoRetrieval
        );
    }

    #[test]
    fn test_heuristic_recall_variants() {
        assert_eq!(
            classify_heuristic("tell me about the project"),
            QueryComplexity::SingleStep
        );
        assert_eq!(
            classify_heuristic("who is Alice?"),
            QueryComplexity::SingleStep
        );
        assert_eq!(
            classify_heuristic("what is my schedule?"),
            QueryComplexity::SingleStep
        );
    }

    #[test]
    fn test_heuristic_web_signals() {
        assert_eq!(
            classify_heuristic("search for Rust tutorials"),
            QueryComplexity::MultiSource
        );
        assert_eq!(
            classify_heuristic("what's happening in the world?"),
            QueryComplexity::MultiSource
        );
        assert_eq!(
            classify_heuristic("trending topics on Twitter"),
            QueryComplexity::MultiSource
        );
    }

    #[test]
    fn test_heuristic_complex_signals() {
        assert_eq!(
            classify_heuristic("research the best approach for this"),
            QueryComplexity::MultiHop
        );
        assert_eq!(
            classify_heuristic("step by step guide to deploy"),
            QueryComplexity::MultiHop
        );
        assert_eq!(
            classify_heuristic("what are the implications of this change?"),
            QueryComplexity::MultiHop
        );
        assert_eq!(
            classify_heuristic("plan for next week"),
            QueryComplexity::MultiHop
        );
    }

    #[test]
    fn test_heuristic_action_commands() {
        assert_eq!(
            classify_heuristic("schedule a meeting for tomorrow"),
            QueryComplexity::SingleStep
        );
        assert_eq!(
            classify_heuristic("remind me to call Bob"),
            QueryComplexity::SingleStep
        );
        assert_eq!(
            classify_heuristic("run the tests"),
            QueryComplexity::SingleStep
        );
        assert_eq!(
            classify_heuristic("stop the server"),
            QueryComplexity::SingleStep
        );
    }

    #[test]
    fn test_heuristic_long_query_defaults_multi_source() {
        let long = "I need you to look at this really long and detailed query that has many words and should trigger the multi-source default path";
        assert_eq!(classify_heuristic(long), QueryComplexity::MultiSource);
    }

    #[test]
    fn test_heuristic_medium_query_defaults_single_step() {
        assert_eq!(
            classify_heuristic("how are you doing today my friend"),
            QueryComplexity::SingleStep
        );
    }

    #[test]
    fn test_strategy_fields() {
        let no = RetrievalStrategy::no_retrieval();
        assert!(!no.search_knowledge);
        assert!(!no.search_web);
        assert!(no.load_history);
        assert_eq!(no.knowledge_limit, 0);

        let single = RetrievalStrategy::single_step();
        assert!(single.search_knowledge);
        assert!(!single.search_web);
        assert_eq!(single.knowledge_limit, 5);

        let multi = RetrievalStrategy::multi_source();
        assert!(multi.search_knowledge);
        assert!(multi.search_web);
        assert!(multi.graph_expand);
        assert_eq!(multi.knowledge_limit, 10);

        let hop = RetrievalStrategy::multi_hop();
        assert!(hop.corrective_rag);
        assert_eq!(hop.knowledge_limit, 15);
    }

    #[test]
    fn test_query_router_config_default() {
        let config = QueryRouterConfig::default();
        assert!(!config.use_llm_classification);
        assert!(config.enabled);
    }
}
