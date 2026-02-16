//! Context loading and system prompt building

use tracing::debug;

/// Build complete system prompt from components
pub fn build_system_prompt(soul: &str, memory: &str, extra_context: &str) -> String {
    let mut prompt = String::new();

    // Add SOUL first - this is the core identity
    if !soul.is_empty() {
        prompt.push_str("# IDENTITY\n\n");
        prompt.push_str(soul);
        prompt.push_str("\n\n");
    }

    // Add MEMORY - accumulated knowledge
    if !memory.is_empty() {
        prompt.push_str("# MEMORY\n\n");
        prompt.push_str(memory);
        prompt.push_str("\n\n");
    }

    // Add extra context - conversation history, relevant entities, etc.
    if !extra_context.is_empty() {
        prompt.push_str("# CONTEXT\n\n");
        prompt.push_str(extra_context);
        prompt.push_str("\n\n");
    }

    // Add current timestamp
    prompt.push_str("# CURRENT TIME\n\n");
    prompt.push_str(&chrono::Utc::now().to_rfc3339());
    prompt.push_str("\n\n");

    // Add instructions
    prompt.push_str("# INSTRUCTIONS\n\n");
    prompt.push_str("You are an autonomous agent with access to powerful tools. ");
    prompt.push_str("Use your tools proactively to help the user. ");
    prompt.push_str("When you learn something important, use the Remember tool to store it. ");
    prompt.push_str("Be concise but thorough. ");
    prompt.push_str("Always think step-by-step about complex tasks.\n");

    debug!("Built system prompt ({} chars)", prompt.len());

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_system_prompt() {
        let soul = "I am meepo";
        let memory = "The user likes Rust";
        let context = "Recent conversation about async programming";

        let prompt = build_system_prompt(soul, memory, context);

        assert!(prompt.contains("IDENTITY"));
        assert!(prompt.contains("MEMORY"));
        assert!(prompt.contains("CONTEXT"));
        assert!(prompt.contains("meepo"));
        assert!(prompt.contains("Rust"));
    }

    #[test]
    fn test_build_system_prompt_empty() {
        let prompt = build_system_prompt("", "", "");
        assert!(prompt.contains("INSTRUCTIONS"));
        assert!(prompt.contains("CURRENT TIME"));
        // Should NOT contain IDENTITY, MEMORY, or CONTEXT sections
        assert!(!prompt.contains("IDENTITY"));
        assert!(!prompt.contains("MEMORY"));
        assert!(!prompt.contains("CONTEXT"));
    }

    #[test]
    fn test_build_system_prompt_partial() {
        // Only soul, no memory or context
        let prompt = build_system_prompt("I am meepo", "", "");
        assert!(prompt.contains("IDENTITY"));
        assert!(prompt.contains("meepo"));
        assert!(!prompt.contains("MEMORY"));
        assert!(!prompt.contains("CONTEXT"));

        // Only memory
        let prompt = build_system_prompt("", "User likes Rust", "");
        assert!(!prompt.contains("IDENTITY"));
        assert!(prompt.contains("MEMORY"));
        assert!(prompt.contains("Rust"));
        assert!(!prompt.contains("CONTEXT"));

        // Only context
        let prompt = build_system_prompt("", "", "Recent chat");
        assert!(!prompt.contains("IDENTITY"));
        assert!(!prompt.contains("MEMORY"));
        assert!(prompt.contains("CONTEXT"));
        assert!(prompt.contains("Recent chat"));
    }

    #[test]
    fn test_build_system_prompt_always_has_time_and_instructions() {
        let prompt = build_system_prompt("soul", "mem", "ctx");
        assert!(prompt.contains("CURRENT TIME"));
        assert!(prompt.contains("INSTRUCTIONS"));
        assert!(prompt.contains("autonomous agent"));
        assert!(prompt.contains("Remember tool"));
    }

    #[test]
    fn test_build_system_prompt_section_order() {
        let prompt = build_system_prompt("soul", "mem", "ctx");
        let identity_pos = prompt.find("IDENTITY").unwrap();
        let memory_pos = prompt.find("MEMORY").unwrap();
        let context_pos = prompt.find("CONTEXT").unwrap();
        let time_pos = prompt.find("CURRENT TIME").unwrap();
        let instructions_pos = prompt.find("INSTRUCTIONS").unwrap();

        assert!(identity_pos < memory_pos);
        assert!(memory_pos < context_pos);
        assert!(context_pos < time_pos);
        assert!(time_pos < instructions_pos);
    }
}
