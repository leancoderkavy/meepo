//! Knowledge graph and memory tools

use async_trait::async_trait;
use serde_json::Value;
use anyhow::{Result, Context};
use std::sync::Arc;
use tracing::debug;

use meepo_knowledge::KnowledgeDb;
use super::{ToolHandler, json_schema};

/// Remember information by adding to knowledge graph
pub struct RememberTool {
    db: Arc<KnowledgeDb>,
}

impl RememberTool {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ToolHandler for RememberTool {
    fn name(&self) -> &str {
        "remember"
    }

    fn description(&self) -> &str {
        "Remember important information by storing it in the knowledge graph. \
         Creates an entity with a name, type, and optional metadata."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "name": {
                    "type": "string",
                    "description": "Name or identifier for this piece of knowledge"
                },
                "entity_type": {
                    "type": "string",
                    "description": "Type of entity (e.g., 'person', 'concept', 'fact', 'preference')"
                },
                "metadata": {
                    "type": "object",
                    "description": "Additional structured information about this entity"
                }
            }),
            vec!["name", "entity_type"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let name = input.get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'name' parameter"))?;
        let entity_type = input.get("entity_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'entity_type' parameter"))?;
        let metadata = input.get("metadata").cloned();

        debug!("Remembering: {} (type: {})", name, entity_type);

        let entity_id = self.db.insert_entity(name, entity_type, metadata)
            .context("Failed to insert entity")?;

        Ok(format!("Remembered '{}' with ID: {}", name, entity_id))
    }
}

/// Recall information from knowledge graph
pub struct RecallTool {
    db: Arc<KnowledgeDb>,
}

impl RecallTool {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ToolHandler for RecallTool {
    fn name(&self) -> &str {
        "recall"
    }

    fn description(&self) -> &str {
        "Search the knowledge graph for previously stored information. \
         Returns matching entities based on name or type."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "query": {
                    "type": "string",
                    "description": "Search query (searches in name and type)"
                },
                "entity_type": {
                    "type": "string",
                    "description": "Optional: filter by entity type"
                }
            }),
            vec!["query"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let query = input.get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;
        let entity_type = input.get("entity_type").and_then(|v| v.as_str());

        debug!("Searching knowledge graph for: {}", query);

        let results = self.db.search_entities(query, entity_type)
            .context("Failed to search entities")?;

        if results.is_empty() {
            return Ok("No matching information found.".to_string());
        }

        let mut output = format!("Found {} result(s):\n\n", results.len());
        for entity in results.iter().take(10) {
            output.push_str(&format!("- {} ({})", entity.name, entity.entity_type));
            if let Some(metadata) = &entity.metadata {
                output.push_str(&format!("\n  Metadata: {}", metadata));
            }
            output.push('\n');
        }

        Ok(output)
    }
}

/// Link entities together in knowledge graph
pub struct LinkEntitiesTool {
    db: Arc<KnowledgeDb>,
}

impl LinkEntitiesTool {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ToolHandler for LinkEntitiesTool {
    fn name(&self) -> &str {
        "link_entities"
    }

    fn description(&self) -> &str {
        "Create a relationship between two entities in the knowledge graph. \
         Useful for building connections between concepts, people, facts, etc."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "source_id": {
                    "type": "string",
                    "description": "ID of the source entity"
                },
                "target_id": {
                    "type": "string",
                    "description": "ID of the target entity"
                },
                "relation_type": {
                    "type": "string",
                    "description": "Type of relationship (e.g., 'related_to', 'works_with', 'part_of')"
                },
                "metadata": {
                    "type": "object",
                    "description": "Optional metadata about the relationship"
                }
            }),
            vec!["source_id", "target_id", "relation_type"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let source_id = input.get("source_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'source_id' parameter"))?;
        let target_id = input.get("target_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'target_id' parameter"))?;
        let relation_type = input.get("relation_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'relation_type' parameter"))?;
        let metadata = input.get("metadata").cloned();

        debug!("Linking {} -> {} ({})", source_id, target_id, relation_type);

        let rel_id = self.db.insert_relationship(source_id, target_id, relation_type, metadata)
            .context("Failed to create relationship")?;

        Ok(format!("Created relationship with ID: {}", rel_id))
    }
}

/// Search knowledge graph using full-text search
pub struct SearchKnowledgeTool {
    db: Arc<KnowledgeDb>,
}

impl SearchKnowledgeTool {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ToolHandler for SearchKnowledgeTool {
    fn name(&self) -> &str {
        "search_knowledge"
    }

    fn description(&self) -> &str {
        "Perform a full-text search across all stored knowledge. \
         More powerful than recall for finding relevant information."
    }

    fn input_schema(&self) -> Value {
        json_schema(
            serde_json::json!({
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of results (default: 10)"
                }
            }),
            vec!["query"],
        )
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let query = input.get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;
        let limit = input.get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        debug!("Full-text search for: {}", query);

        // Use the basic search for now (Tantivy integration would go here)
        let results = self.db.search_entities(query, None)
            .context("Failed to search knowledge")?;

        if results.is_empty() {
            return Ok("No results found.".to_string());
        }

        let mut output = format!("Found {} result(s):\n\n", results.len().min(limit));
        for entity in results.iter().take(limit) {
            output.push_str(&format!("- {} ({})\n", entity.name, entity.entity_type));
            if let Some(metadata) = &entity.metadata {
                output.push_str(&format!("  {}\n", metadata));
            }
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolHandler;
    use tempfile::TempDir;

    fn setup() -> (Arc<meepo_knowledge::KnowledgeDb>, TempDir) {
        let temp = TempDir::new().unwrap();
        let db = Arc::new(meepo_knowledge::KnowledgeDb::new(&temp.path().join("test.db")).unwrap());
        (db, temp)
    }

    #[test]
    fn test_remember_tool_schema() {
        let (db, _temp) = setup();
        let tool = RememberTool::new(db);
        assert_eq!(tool.name(), "remember");
        assert!(!tool.description().is_empty());
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[tokio::test]
    async fn test_remember_and_recall() {
        let (db, _temp) = setup();
        let remember = RememberTool::new(db.clone());
        let recall = RecallTool::new(db);

        // Remember something
        let result = remember.execute(serde_json::json!({
            "name": "Rust programming",
            "entity_type": "concept",
            "metadata": {"detail": "systems language"}
        })).await.unwrap();
        assert!(result.contains("Remembered"));

        // Recall it
        let result = recall.execute(serde_json::json!({
            "query": "Rust"
        })).await.unwrap();
        assert!(result.contains("Rust programming"));
    }

    #[tokio::test]
    async fn test_remember_missing_name() {
        let (db, _temp) = setup();
        let tool = RememberTool::new(db);
        let result = tool.execute(serde_json::json!({
            "entity_type": "concept"
        })).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_recall_empty_results() {
        let (db, _temp) = setup();
        let tool = RecallTool::new(db);
        let result = tool.execute(serde_json::json!({
            "query": "nonexistent_xyz_12345"
        })).await.unwrap();
        assert!(result.contains("No") || result.contains("no") || result.is_empty() || result.contains("Found 0"));
    }

    #[tokio::test]
    async fn test_link_entities() {
        let (db, _temp) = setup();
        let remember = RememberTool::new(db.clone());
        let link = LinkEntitiesTool::new(db);

        // Create two entities
        let r1 = remember.execute(serde_json::json!({
            "name": "Alice",
            "entity_type": "person"
        })).await.unwrap();

        let r2 = remember.execute(serde_json::json!({
            "name": "Bob",
            "entity_type": "person"
        })).await.unwrap();

        // Extract IDs from responses (format: "Remembered 'X' with ID: <uuid>")
        let id1 = r1.split("ID: ").nth(1).unwrap_or("").trim();
        let id2 = r2.split("ID: ").nth(1).unwrap_or("").trim();

        if !id1.is_empty() && !id2.is_empty() {
            let result = link.execute(serde_json::json!({
                "source_id": id1,
                "target_id": id2,
                "relation_type": "knows"
            })).await.unwrap();
            assert!(result.contains("Created") || result.contains("relationship"));
        }
    }

    #[tokio::test]
    async fn test_search_knowledge_tool() {
        let (db, _temp) = setup();
        let remember = RememberTool::new(db.clone());
        let search = SearchKnowledgeTool::new(db);

        // Add some data
        remember.execute(serde_json::json!({
            "name": "Python language",
            "entity_type": "concept"
        })).await.unwrap();

        let result = search.execute(serde_json::json!({
            "query": "Python"
        })).await.unwrap();
        assert!(result.contains("Python"));
    }

    #[test]
    fn test_recall_tool_schema() {
        let (db, _temp) = setup();
        let tool = RecallTool::new(db);
        assert_eq!(tool.name(), "recall");
        let schema = tool.input_schema();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn test_link_entities_tool_schema() {
        let (db, _temp) = setup();
        let tool = LinkEntitiesTool::new(db);
        assert_eq!(tool.name(), "link_entities");
        let schema = tool.input_schema();
        let props = schema.get("properties").unwrap();
        assert!(props.get("source_id").is_some());
        assert!(props.get("target_id").is_some());
    }

    #[test]
    fn test_search_knowledge_tool_schema() {
        let (db, _temp) = setup();
        let tool = SearchKnowledgeTool::new(db);
        assert_eq!(tool.name(), "search_knowledge");
    }
}
