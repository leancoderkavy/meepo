//! Knowledge graph operations combining SQLite and Tantivy

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info};

use crate::sqlite::{Entity, KnowledgeDb, Relationship};
use crate::tantivy::{SearchResult, TantivyIndex};

/// Context for an entity including relationships and conversations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityContext {
    pub entity: Entity,
    pub related_entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
    pub recent_conversations: Vec<crate::sqlite::Conversation>,
}

/// Knowledge graph combining SQLite and Tantivy
pub struct KnowledgeGraph {
    db: Arc<KnowledgeDb>,
    index: TantivyIndex,
}

impl KnowledgeGraph {
    /// Create a new knowledge graph
    pub fn new<P: AsRef<Path>, Q: AsRef<Path>>(db_path: P, index_path: Q) -> Result<Self> {
        info!(
            "Initializing knowledge graph with db at {:?} and index at {:?}",
            db_path.as_ref(),
            index_path.as_ref()
        );

        let db = Arc::new(KnowledgeDb::new(db_path)?);
        let index = TantivyIndex::new(index_path)?;

        Ok(Self { db, index })
    }

    /// Add an entity to the knowledge graph
    pub fn add_entity(
        &self,
        name: &str,
        entity_type: &str,
        metadata: Option<JsonValue>,
    ) -> Result<String> {
        debug!("Adding entity: {} ({})", name, entity_type);

        // Insert into SQLite
        let id = self.db.insert_entity(name, entity_type, metadata.clone())?;

        // Index in Tantivy
        let content = format!(
            "{} {} {}",
            name,
            entity_type,
            metadata
                .as_ref()
                .map(|m| m.to_string())
                .unwrap_or_default()
        );

        self.index.index_document(
            &id,
            &content,
            entity_type,
            &chrono::Utc::now().to_rfc3339(),
        )?;

        info!("Added entity: {} with ID {}", name, id);
        Ok(id)
    }

    /// Link two entities with a relationship
    pub fn link_entities(
        &self,
        source_id: &str,
        target_id: &str,
        relation_type: &str,
        metadata: Option<JsonValue>,
    ) -> Result<String> {
        debug!(
            "Linking entities: {} -> {} ({})",
            source_id, target_id, relation_type
        );

        // Verify both entities exist
        self.db
            .get_entity(source_id)?
            .context("Source entity not found")?;
        self.db
            .get_entity(target_id)?
            .context("Target entity not found")?;

        let id = self
            .db
            .insert_relationship(source_id, target_id, relation_type, metadata)?;

        info!("Linked entities with relationship ID {}", id);
        Ok(id)
    }

    /// Search the knowledge graph
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        debug!("Searching knowledge graph for: {}", query);
        self.index.search(query, limit)
    }

    /// Get full context for an entity
    pub fn get_context_for(&self, entity_id: &str) -> Result<EntityContext> {
        debug!("Getting context for entity: {}", entity_id);

        // Get the entity
        let entity = self
            .db
            .get_entity(entity_id)?
            .context("Entity not found")?;

        // Get relationships
        let relationships = self.db.get_relationships_for(entity_id)?;

        // Get related entities
        let mut related_entities = Vec::new();
        for rel in &relationships {
            let related_id = if rel.source_id == entity_id {
                &rel.target_id
            } else {
                &rel.source_id
            };

            if let Some(related) = self.db.get_entity(related_id)? {
                related_entities.push(related);
            }
        }

        // Get recent conversations (limit to 20)
        let recent_conversations = self.db.get_recent_conversations(None, 20)?;

        Ok(EntityContext {
            entity,
            related_entities,
            relationships,
            recent_conversations,
        })
    }

    /// Remember something (store as entity and conversation)
    pub fn remember(
        &self,
        content: &str,
        entity_type: &str,
        channel: Option<&str>,
    ) -> Result<String> {
        debug!("Remembering: {} ({})", content, entity_type);

        // Create entity with content as name
        let name = if content.len() > 100 {
            format!("{}...", &content[..97])
        } else {
            content.to_string()
        };

        let metadata = serde_json::json!({
            "full_content": content,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        let entity_id = self.add_entity(&name, entity_type, Some(metadata))?;

        // Also store as conversation if channel provided
        if let Some(ch) = channel {
            self.db.insert_conversation(
                ch,
                "system",
                content,
                Some(serde_json::json!({"entity_id": entity_id})),
            )?;
        }

        info!("Remembered content as entity {}", entity_id);
        Ok(entity_id)
    }

    /// Recall information by query
    pub fn recall(&self, query: &str, limit: usize) -> Result<Vec<EntityContext>> {
        debug!("Recalling: {}", query);

        // Search using Tantivy
        let results = self.search(query, limit)?;

        // Get full context for each result
        let mut contexts = Vec::new();
        for result in results {
            if let Ok(context) = self.get_context_for(&result.id) {
                contexts.push(context);
            }
        }

        info!("Recalled {} contexts for query: {}", contexts.len(), query);
        Ok(contexts)
    }

    /// Get entity by ID
    pub fn get_entity(&self, id: &str) -> Result<Option<Entity>> {
        self.db.get_entity(id)
    }

    /// Search entities in database
    pub fn search_entities(&self, query: &str, entity_type: Option<&str>) -> Result<Vec<Entity>> {
        self.db.search_entities(query, entity_type)
    }

    /// Get relationships for an entity
    pub fn get_relationships(&self, entity_id: &str) -> Result<Vec<Relationship>> {
        self.db.get_relationships_for(entity_id)
    }

    /// Store a conversation
    pub fn store_conversation(
        &self,
        channel: &str,
        sender: &str,
        content: &str,
        metadata: Option<JsonValue>,
    ) -> Result<String> {
        self.db.insert_conversation(channel, sender, content, metadata)
    }

    /// Get recent conversations
    pub fn get_conversations(&self, channel: Option<&str>, limit: usize) -> Result<Vec<crate::sqlite::Conversation>> {
        self.db.get_recent_conversations(channel, limit)
    }

    /// Create a watcher
    pub fn create_watcher(
        &self,
        kind: &str,
        config: JsonValue,
        action: &str,
        reply_channel: &str,
    ) -> Result<String> {
        self.db.insert_watcher(kind, config, action, reply_channel)
    }

    /// Get active watchers
    pub fn get_active_watchers(&self) -> Result<Vec<crate::sqlite::Watcher>> {
        self.db.get_active_watchers()
    }

    /// Update watcher status
    pub fn update_watcher(&self, id: &str, active: bool) -> Result<()> {
        self.db.update_watcher_active(id, active)
    }

    /// Delete a watcher
    pub fn delete_watcher(&self, id: &str) -> Result<()> {
        self.db.delete_watcher(id)
    }

    /// Reindex all entities in Tantivy
    pub fn reindex(&self) -> Result<()> {
        info!("Reindexing all entities");
        self.index.reindex_all(&self.db)
    }

    /// Get all entities
    pub fn get_all_entities(&self) -> Result<Vec<Entity>> {
        self.db.get_all_entities()
    }

    /// Get a reference to the underlying database
    ///
    /// This allows access to the database for operations that don't need
    /// the full knowledge graph functionality, avoiding duplicate connections.
    pub fn db(&self) -> Arc<KnowledgeDb> {
        Arc::clone(&self.db)
    }

    /// Clean up old conversations (keep only last N days)
    pub fn cleanup_old_conversations(&self, retain_days: u32) -> Result<usize> {
        self.db.cleanup_old_conversations(retain_days)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_add_and_search_entity() -> Result<()> {
        let temp_dir = env::temp_dir();
        let db_path = temp_dir.join("test_graph.db");
        let index_path = temp_dir.join("test_graph_index");

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);

        let graph = KnowledgeGraph::new(&db_path, &index_path)?;

        // Add entity
        let id = graph.add_entity(
            "Rust programming language",
            "concept",
            Some(serde_json::json!({"description": "Systems programming language"})),
        )?;

        // Search
        let results = graph.search("Rust", 10)?;
        assert!(!results.is_empty());
        assert_eq!(results[0].id, id);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);
        Ok(())
    }

    #[test]
    fn test_link_entities() -> Result<()> {
        let temp_dir = env::temp_dir();
        let db_path = temp_dir.join("test_graph_link.db");
        let index_path = temp_dir.join("test_graph_link_index");

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);

        let graph = KnowledgeGraph::new(&db_path, &index_path)?;

        // Add entities
        let rust_id = graph.add_entity("Rust", "language", None)?;
        let systems_id = graph.add_entity("Systems Programming", "domain", None)?;

        // Link them
        let rel_id = graph.link_entities(&rust_id, &systems_id, "used_for", None)?;
        assert!(!rel_id.is_empty());

        // Get context
        let context = graph.get_context_for(&rust_id)?;
        assert_eq!(context.entity.id, rust_id);
        assert_eq!(context.relationships.len(), 1);
        assert_eq!(context.related_entities.len(), 1);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);
        Ok(())
    }

    #[test]
    fn test_remember_and_recall() -> Result<()> {
        let temp_dir = env::temp_dir();
        let db_path = temp_dir.join("test_graph_memory.db");
        let index_path = temp_dir.join("test_graph_memory_index");

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);

        let graph = KnowledgeGraph::new(&db_path, &index_path)?;

        // Remember something
        let id = graph.remember(
            "Rust is a systems programming language focused on safety and performance",
            "fact",
            Some("test_channel"),
        )?;
        assert!(!id.is_empty());

        // Recall it
        let contexts = graph.recall("Rust systems programming", 10)?;
        assert!(!contexts.is_empty());

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);
        Ok(())
    }

    #[test]
    fn test_watcher_operations() -> Result<()> {
        let temp_dir = env::temp_dir();
        let db_path = temp_dir.join("test_graph_watcher.db");
        let index_path = temp_dir.join("test_graph_watcher_index");

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);

        let graph = KnowledgeGraph::new(&db_path, &index_path)?;

        // Create watcher
        let config = serde_json::json!({"path": "/test/path", "pattern": "*.rs"});
        let watcher_id = graph.create_watcher("file", config, "notify", "test_channel")?;
        assert!(!watcher_id.is_empty());

        // Get active watchers
        let watchers = graph.get_active_watchers()?;
        assert_eq!(watchers.len(), 1);

        // Deactivate watcher
        graph.update_watcher(&watcher_id, false)?;
        let watchers = graph.get_active_watchers()?;
        assert_eq!(watchers.len(), 0);

        // Delete watcher
        graph.delete_watcher(&watcher_id)?;

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);
        Ok(())
    }
}
