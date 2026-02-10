//! Knowledge graph and persistence layer for meepo
//!
//! This crate provides:
//! - SQLite storage for entities, relationships, conversations, and watchers
//! - Tantivy full-text search index
//! - Knowledge graph operations combining both
//! - MEMORY.md synchronization

pub mod sqlite;
pub mod tantivy;
pub mod graph;
pub mod memory_sync;

// Re-export main types
pub use sqlite::{KnowledgeDb, Entity, Relationship, Conversation, Watcher, Goal, UserPreference, ActionLogEntry, BackgroundTask};
pub use tantivy::{TantivyIndex, SearchResult};
pub use graph::KnowledgeGraph;
pub use memory_sync::{load_memory, save_memory, load_soul};

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test_basic_integration() -> Result<()> {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join("test_knowledge.db");
        let tantivy_path = temp_dir.join("test_tantivy_index");

        // Clean up any existing test files
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&tantivy_path);

        let graph = KnowledgeGraph::new(&db_path, &tantivy_path)?;

        // Test adding an entity
        let entity_id = graph.add_entity("test_entity", "concept", None).await?;
        assert!(!entity_id.is_empty());

        // Test search
        let results = graph.search("test_entity", 10)?;
        assert!(!results.is_empty());

        // Clean up
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&tantivy_path);

        Ok(())
    }
}
