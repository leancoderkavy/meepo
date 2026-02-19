//! LLM-based Intent Understanding
//!
//! Extracts structured intent from a user's natural language request using an LLM.
//! This enriches the agent's understanding beyond simple keyword matching, enabling
//! better context loading, tool selection, and response generation.
//!
//! The intent is extracted as a lightweight JSON structure containing:
//! - `action`: the primary verb/action the user wants (e.g., "send", "search", "remind")
//! - `entities`: named things mentioned (people, files, apps, topics)
//! - `parameters`: key-value pairs extracted from the request
//! - `sentiment`: tone of the request (neutral, urgent, casual)
//! - `clarification_needed`: whether the request is ambiguous and needs follow-up

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::api::{ApiClient, ApiMessage, ContentBlock, MessageContent, Usage};

/// Structured intent extracted from a user's message
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserIntent {
    /// Primary action the user wants to perform (e.g., "send_email", "search", "remind", "explain")
    pub action: String,
    /// Named entities mentioned: people, files, apps, topics, locations
    pub entities: Vec<String>,
    /// Key-value parameters extracted from the request (e.g., {"recipient": "Alice", "time": "tomorrow"})
    pub parameters: serde_json::Map<String, serde_json::Value>,
    /// Tone/urgency: "neutral", "urgent", "casual", "frustrated"
    pub sentiment: String,
    /// Whether the request is ambiguous and a clarifying question should be asked
    pub clarification_needed: bool,
    /// Short restatement of the request in canonical form (useful for logging/debugging)
    pub canonical: String,
}

/// Configuration for the intent understanding module
#[derive(Debug, Clone)]
pub struct IntentConfig {
    /// Whether intent understanding is enabled
    pub enabled: bool,
    /// Minimum message length (chars) before intent extraction is attempted.
    /// Very short messages (greetings, "ok", etc.) skip LLM extraction.
    pub min_length: usize,
}

impl Default for IntentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_length: 10,
        }
    }
}

/// Extract structured intent from a user message.
///
/// Returns `(intent, Some(usage))` if an LLM call was made, or
/// `(intent, None)` if the message was too short / config disabled.
pub async fn understand_intent(
    api: &ApiClient,
    message: &str,
    config: &IntentConfig,
) -> Result<(UserIntent, Option<Usage>)> {
    if !config.enabled || message.len() < config.min_length {
        debug!(
            "Intent understanding skipped (enabled={}, len={})",
            config.enabled,
            message.len()
        );
        return Ok((heuristic_intent(message), None));
    }

    match extract_with_llm(api, message).await {
        Ok((intent, usage)) => {
            debug!(
                "LLM extracted intent: action={:?}, entities={:?}, clarification_needed={}",
                intent.action, intent.entities, intent.clarification_needed
            );
            Ok((intent, Some(usage)))
        }
        Err(e) => {
            warn!("Intent extraction failed, using heuristic: {}", e);
            Ok((heuristic_intent(message), None))
        }
    }
}

/// Fast heuristic intent extraction (no LLM call).
/// Used as a fallback when LLM is unavailable or message is too short.
fn heuristic_intent(message: &str) -> UserIntent {
    let lower = message.to_lowercase();

    let action = if lower.starts_with("send") {
        "send"
    } else if lower.starts_with("remind") || lower.contains("remind me") {
        "remind"
    } else if lower.starts_with("search") || lower.starts_with("find") || lower.starts_with("look up") {
        "search"
    } else if lower.starts_with("create") || lower.starts_with("make") || lower.starts_with("write") {
        "create"
    } else if lower.starts_with("open") || lower.starts_with("show") || lower.starts_with("get") {
        "retrieve"
    } else if lower.starts_with("schedule") || lower.contains("calendar") {
        "schedule"
    } else if lower.starts_with("play") {
        "play"
    } else if lower.starts_with("run") || lower.starts_with("execute") {
        "execute"
    } else if lower.contains("?") || lower.starts_with("what") || lower.starts_with("how") || lower.starts_with("why") || lower.starts_with("when") || lower.starts_with("who") {
        "query"
    } else {
        "general"
    };

    UserIntent {
        action: action.to_string(),
        entities: vec![],
        parameters: serde_json::Map::new(),
        sentiment: "neutral".to_string(),
        clarification_needed: false,
        canonical: message.to_string(),
    }
}

/// Use the LLM to extract structured intent from the user's message.
async fn extract_with_llm(api: &ApiClient, message: &str) -> Result<(UserIntent, Usage)> {
    let prompt = format!(
        r#"Extract structured intent from this user message. Respond with ONLY valid JSON, no explanation.

User message: {message}

JSON schema to follow:
{{
  "action": "<primary action verb, e.g. send_email, search_web, create_reminder, explain, query, general>",
  "entities": ["<named entity 1>", "<named entity 2>"],
  "parameters": {{"<key>": "<value>"}},
  "sentiment": "<one of: neutral, urgent, casual, frustrated>",
  "clarification_needed": <true if ambiguous, false otherwise>,
  "canonical": "<one-sentence restatement of the request>"
}}

Rules:
- action must be a single snake_case string
- entities: names of people, files, apps, topics, locations mentioned
- parameters: specific values like recipient names, times, file paths, amounts
- sentiment: infer from tone and word choice
- clarification_needed: true only if the request cannot be acted on without more info
- canonical: rephrase the request clearly and concisely

JSON:"#,
        message = message
    );

    let messages = vec![ApiMessage {
        role: "user".to_string(),
        content: MessageContent::Text(prompt),
    }];

    let response = api
        .chat(
            &messages,
            &[],
            "You are an intent extraction system. Output only valid JSON.",
        )
        .await
        .context("Failed to extract intent via LLM")?;

    let text: String = response
        .content
        .iter()
        .filter_map(|b| {
            if let ContentBlock::Text { text } = b {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();

    let intent = parse_intent_json(&text).unwrap_or_else(|e| {
        debug!("Failed to parse intent JSON: {} — raw: {:?}", e, text);
        heuristic_intent(message)
    });

    Ok((intent, response.usage))
}

/// Parse the LLM's JSON response into a `UserIntent`.
/// Tolerates partial JSON by filling in defaults for missing fields.
fn parse_intent_json(text: &str) -> Result<UserIntent> {
    // Find the JSON object in the response (LLM may add preamble)
    let start = text.find('{').context("No JSON object found in response")?;
    let end = text.rfind('}').context("No closing brace found in response")?;
    let json_str = &text[start..=end];

    let value: serde_json::Value =
        serde_json::from_str(json_str).context("Failed to parse intent JSON")?;

    let action = value
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("general")
        .to_string();

    let entities: Vec<String> = value
        .get("entities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let parameters = value
        .get("parameters")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let sentiment = value
        .get("sentiment")
        .and_then(|v| v.as_str())
        .unwrap_or("neutral")
        .to_string();

    let clarification_needed = value
        .get("clarification_needed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let canonical = value
        .get("canonical")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(UserIntent {
        action,
        entities,
        parameters,
        sentiment,
        clarification_needed,
        canonical,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heuristic_intent_send() {
        let intent = heuristic_intent("send an email to Alice");
        assert_eq!(intent.action, "send");
    }

    #[test]
    fn test_heuristic_intent_remind() {
        let intent = heuristic_intent("remind me to call Bob tomorrow");
        assert_eq!(intent.action, "remind");
    }

    #[test]
    fn test_heuristic_intent_search() {
        let intent = heuristic_intent("search for Rust tutorials");
        assert_eq!(intent.action, "search");
    }

    #[test]
    fn test_heuristic_intent_query() {
        let intent = heuristic_intent("what is the capital of France?");
        assert_eq!(intent.action, "query");
    }

    #[test]
    fn test_heuristic_intent_create() {
        let intent = heuristic_intent("create a new note about the meeting");
        assert_eq!(intent.action, "create");
    }

    #[test]
    fn test_heuristic_intent_schedule() {
        let intent = heuristic_intent("schedule a meeting for tomorrow at 3pm");
        assert_eq!(intent.action, "schedule");
    }

    #[test]
    fn test_heuristic_intent_play() {
        let intent = heuristic_intent("play some jazz music");
        assert_eq!(intent.action, "play");
    }

    #[test]
    fn test_heuristic_intent_execute() {
        let intent = heuristic_intent("run the test suite");
        assert_eq!(intent.action, "execute");
    }

    #[test]
    fn test_heuristic_intent_general() {
        let intent = heuristic_intent("I need help with something");
        assert_eq!(intent.action, "general");
    }

    #[test]
    fn test_heuristic_intent_defaults() {
        let intent = heuristic_intent("hello");
        assert_eq!(intent.sentiment, "neutral");
        assert!(!intent.clarification_needed);
        assert!(intent.entities.is_empty());
    }

    #[test]
    fn test_parse_intent_json_full() {
        let json = r#"{
            "action": "send_email",
            "entities": ["Alice", "Bob"],
            "parameters": {"subject": "Meeting", "time": "3pm"},
            "sentiment": "neutral",
            "clarification_needed": false,
            "canonical": "Send an email to Alice and Bob about the meeting at 3pm"
        }"#;
        let intent = parse_intent_json(json).unwrap();
        assert_eq!(intent.action, "send_email");
        assert_eq!(intent.entities, vec!["Alice", "Bob"]);
        assert_eq!(
            intent.parameters.get("subject").and_then(|v| v.as_str()),
            Some("Meeting")
        );
        assert_eq!(intent.sentiment, "neutral");
        assert!(!intent.clarification_needed);
        assert!(intent.canonical.contains("email"));
    }

    #[test]
    fn test_parse_intent_json_partial() {
        let json = r#"{"action": "search"}"#;
        let intent = parse_intent_json(json).unwrap();
        assert_eq!(intent.action, "search");
        assert!(intent.entities.is_empty());
        assert_eq!(intent.sentiment, "neutral");
        assert!(!intent.clarification_needed);
    }

    #[test]
    fn test_parse_intent_json_with_preamble() {
        let text = r#"Here is the JSON: {"action": "remind", "entities": ["Alice"], "parameters": {}, "sentiment": "casual", "clarification_needed": false, "canonical": "Remind Alice"}"#;
        let intent = parse_intent_json(text).unwrap();
        assert_eq!(intent.action, "remind");
        assert_eq!(intent.entities, vec!["Alice"]);
    }

    #[test]
    fn test_parse_intent_json_invalid() {
        let result = parse_intent_json("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn test_intent_config_default() {
        let config = IntentConfig::default();
        assert!(config.enabled);
        assert_eq!(config.min_length, 10);
    }

    #[tokio::test]
    async fn test_understand_intent_disabled() {
        let config = IntentConfig {
            enabled: false,
            ..Default::default()
        };
        let api = ApiClient::new("test-key".to_string(), None);
        let (intent, usage) = understand_intent(&api, "send an email to Alice", &config)
            .await
            .unwrap();
        assert!(usage.is_none());
        assert_eq!(intent.action, "send");
    }

    #[tokio::test]
    async fn test_understand_intent_short_message() {
        let config = IntentConfig::default();
        let api = ApiClient::new("test-key".to_string(), None);
        let (intent, usage) = understand_intent(&api, "hi", &config).await.unwrap();
        assert!(usage.is_none()); // too short, no LLM call
        assert_eq!(intent.canonical, "hi");
    }
}
