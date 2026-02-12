//! SKILL.md parser — extracts YAML frontmatter and markdown body
//!
//! Compatible with OpenClaw SKILL.md format:
//! ```markdown
//! ---
//! name: skill_name
//! description: What it does
//! inputs:
//!   param_name:
//!     type: string
//!     required: true
//! commands:
//!   - gh
//!   - curl
//! ---
//! Instructions for the agent...
//! ```

use anyhow::{Result, Context, anyhow};
use serde::Deserialize;
use std::collections::HashMap;

/// Parsed skill definition
#[derive(Debug, Clone)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub inputs: HashMap<String, SkillInput>,
    pub commands: Vec<String>,
    pub instructions: String,
}

/// Skill input parameter
#[derive(Debug, Clone, Deserialize)]
pub struct SkillInput {
    #[serde(rename = "type", default = "default_type")]
    pub input_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
}

fn default_type() -> String {
    "string".to_string()
}

/// YAML frontmatter structure
#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    inputs: HashMap<String, SkillInput>,
    #[serde(default)]
    commands: Vec<String>,
}

/// Parse a SKILL.md file into a SkillDefinition
pub fn parse_skill(content: &str) -> Result<SkillDefinition> {
    let content = content.trim();

    // Must start with ---
    if !content.starts_with("---") {
        return Err(anyhow!("SKILL.md must start with YAML frontmatter (---)"));
    }

    // Find closing ---
    let rest = &content[3..];
    let end = rest.find("\n---")
        .ok_or_else(|| anyhow!("Missing closing --- in YAML frontmatter"))?;

    let yaml_str = &rest[..end];
    let instructions = rest[end + 4..].trim().to_string();

    let frontmatter: SkillFrontmatter = serde_yml::from_str(yaml_str)
        .with_context(|| format!("Failed to parse YAML frontmatter"))?;

    if frontmatter.name.is_empty() {
        return Err(anyhow!("Skill name cannot be empty"));
    }

    // Validate name is a valid identifier
    if !frontmatter.name.chars().all(|c: char| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err(anyhow!("Skill name must be alphanumeric (with _ or -)"));
    }

    Ok(SkillDefinition {
        name: frontmatter.name,
        description: frontmatter.description,
        inputs: frontmatter.inputs,
        commands: frontmatter.commands,
        instructions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_skill() {
        let content = r#"---
name: github_pr_review
description: Review a GitHub pull request
inputs:
  pr_url:
    type: string
    required: true
commands:
  - gh
---
Steps to review the PR:
1. Fetch the PR details
2. Review the changes
3. Post a review comment
"#;
        let skill = parse_skill(content).unwrap();
        assert_eq!(skill.name, "github_pr_review");
        assert_eq!(skill.description, "Review a GitHub pull request");
        assert_eq!(skill.inputs.len(), 1);
        assert!(skill.inputs["pr_url"].required);
        assert_eq!(skill.commands, vec!["gh"]);
        assert!(skill.instructions.contains("Fetch the PR details"));
    }

    #[test]
    fn test_parse_minimal_skill() {
        let content = r#"---
name: hello
description: Say hello
---
Just say hello to the user.
"#;
        let skill = parse_skill(content).unwrap();
        assert_eq!(skill.name, "hello");
        assert!(skill.inputs.is_empty());
        assert!(skill.commands.is_empty());
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let result = parse_skill("No frontmatter here");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_closing() {
        let result = parse_skill("---\nname: test\n");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_name() {
        let content = "---\nname: \"\"\ndescription: test\n---\nbody";
        let result = parse_skill(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_name() {
        let content = "---\nname: \"has spaces\"\ndescription: test\n---\nbody";
        let result = parse_skill(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_multiple_inputs() {
        let content = r#"---
name: search
description: Search things
inputs:
  query:
    type: string
    required: true
    description: The search query
  limit:
    type: integer
    required: false
---
Search for the query.
"#;
        let skill = parse_skill(content).unwrap();
        assert_eq!(skill.inputs.len(), 2);
        assert!(skill.inputs["query"].required);
        assert!(!skill.inputs["limit"].required);
        assert_eq!(skill.inputs["query"].description.as_deref(), Some("The search query"));
    }
}
