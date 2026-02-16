//! Multi-provider LLM abstraction layer
//!
//! Supports Anthropic, OpenAI, Google Gemini, and any OpenAI-compatible endpoint.
//! Providers implement the [`LlmProvider`] trait and are composed via [`ModelRouter`]
//! for automatic failover.

pub mod anthropic;
pub mod google;
pub mod openai;
pub mod openai_compat;
pub mod router;
pub mod types;

pub use router::ModelRouter;
pub use types::{ChatMessage, ChatMessageContent, ChatResponse, ChatResponseBlock, LlmProvider};
