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
    pub async fn add_entity(
        &self,
        name: &str,
        entity_type: &str,
        metadata: Option<JsonValue>,
    ) -> Result<String> {
        debug!("Adding entity: {} ({})", name, entity_type);

        // Insert into SQLite
        let id = self
            .db
            .insert_entity(name, entity_type, metadata.clone())
            .await?;

        // Index in Tantivy
        let content = format!(
            "{} {} {}",
            name,
            entity_type,
            metadata.as_ref().map(|m| m.to_string()).unwrap_or_default()
        );

        self.index
            .index_document(&id, &content, entity_type, &chrono::Utc::now().to_rfc3339())?;

        info!("Added entity: {} with ID {}", name, id);
        Ok(id)
    }

    /// Link two entities with a relationship
    pub async fn link_entities(
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
            .get_entity(source_id)
            .await?
            .context("Source entity not found")?;
        self.db
            .get_entity(target_id)
            .await?
            .context("Target entity not found")?;

        let id = self
            .db
            .insert_relationship(source_id, target_id, relation_type, metadata)
            .await?;

        info!("Linked entities with relationship ID {}", id);
        Ok(id)
    }

    /// Search the knowledge graph
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        debug!("Searching knowledge graph for: {}", query);
        self.index.search(query, limit)
    }

    /// Get full context for an entity
    pub async fn get_context_for(&self, entity_id: &str) -> Result<EntityContext> {
        debug!("Getting context for entity: {}", entity_id);

        // Get the entity
        let entity = self
            .db
            .get_entity(entity_id)
            .await?
            .context("Entity not found")?;

        // Get relationships
        let relationships = self.db.get_relationships_for(entity_id).await?;

        // Get related entities
        let mut related_entities = Vec::new();
        for rel in &relationships {
            let related_id = if rel.source_id == entity_id {
                &rel.target_id
            } else {
                &rel.source_id
            };

            if let Some(related) = self.db.get_entity(related_id).await? {
                related_entities.push(related);
            }
        }

        // Get recent conversations (limit to 20)
        let recent_conversations = self.db.get_recent_conversations(None, 20).await?;

        Ok(EntityContext {
            entity,
            related_entities,
            relationships,
            recent_conversations,
        })
    }

    /// Remember something (store as entity and conversation)
    pub async fn remember(
        &self,
        content: &str,
        entity_type: &str,
        channel: Option<&str>,
    ) -> Result<String> {
        debug!("Remembering: {} ({})", content, entity_type);

        // Create entity with content as name (char-safe to avoid slicing mid-UTF-8)
        let name = if content.chars().count() > 100 {
            let truncated: String = content.chars().take(97).collect();
            format!("{}...", truncated)
        } else {
            content.to_string()
        };

        let metadata = serde_json::json!({
            "full_content": content,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        let entity_id = self.add_entity(&name, entity_type, Some(metadata)).await?;

        // Also store as conversation if channel provided
        if let Some(ch) = channel {
            self.db
                .insert_conversation(
                    ch,
                    "system",
                    content,
                    Some(serde_json::json!({"entity_id": entity_id})),
                )
                .await?;
        }

        info!("Remembered content as entity {}", entity_id);
        Ok(entity_id)
    }

    /// Recall information by query
    pub async fn recall(&self, query: &str, limit: usize) -> Result<Vec<EntityContext>> {
        debug!("Recalling: {}", query);

        // Search using Tantivy
        let results = self.search(query, limit)?;

        // Get full context for each result
        let mut contexts = Vec::new();
        for result in results {
            if let Ok(context) = self.get_context_for(&result.id).await {
                contexts.push(context);
            }
        }

        info!("Recalled {} contexts for query: {}", contexts.len(), query);
        Ok(contexts)
    }

    /// Get entity by ID
    pub async fn get_entity(&self, id: &str) -> Result<Option<Entity>> {
        self.db.get_entity(id).await
    }

    /// Search entities in database
    pub async fn search_entities(
        &self,
        query: &str,
        entity_type: Option<&str>,
    ) -> Result<Vec<Entity>> {
        self.db.search_entities(query, entity_type).await
    }

    /// Get relationships for an entity
    pub async fn get_relationships(&self, entity_id: &str) -> Result<Vec<Relationship>> {
        self.db.get_relationships_for(entity_id).await
    }

    /// Store a conversation
    pub async fn store_conversation(
        &self,
        channel: &str,
        sender: &str,
        content: &str,
        metadata: Option<JsonValue>,
    ) -> Result<String> {
        self.db
            .insert_conversation(channel, sender, content, metadata)
            .await
    }

    /// Get recent conversations
    pub async fn get_conversations(
        &self,
        channel: Option<&str>,
        limit: usize,
    ) -> Result<Vec<crate::sqlite::Conversation>> {
        self.db.get_recent_conversations(channel, limit).await
    }

    /// Create a watcher
    pub async fn create_watcher(
        &self,
        kind: &str,
        config: JsonValue,
        action: &str,
        reply_channel: &str,
    ) -> Result<String> {
        self.db
            .insert_watcher(kind, config, action, reply_channel)
            .await
    }

    /// Get active watchers
    pub async fn get_active_watchers(&self) -> Result<Vec<crate::sqlite::Watcher>> {
        self.db.get_active_watchers().await
    }

    /// Update watcher status
    pub async fn update_watcher(&self, id: &str, active: bool) -> Result<()> {
        self.db.update_watcher_active(id, active).await
    }

    /// Delete a watcher
    pub async fn delete_watcher(&self, id: &str) -> Result<()> {
        self.db.delete_watcher(id).await
    }

    /// Reindex all entities in Tantivy
    pub async fn reindex(&self) -> Result<()> {
        info!("Reindexing all entities");
        let entities = self.db.get_all_entities().await?;
        self.index.reindex_all_from_entities(&entities)
    }

    /// Get all entities
    pub async fn get_all_entities(&self) -> Result<Vec<Entity>> {
        self.db.get_all_entities().await
    }

    /// Get a reference to the underlying database
    ///
    /// This allows access to the database for operations that don't need
    /// the full knowledge graph functionality, avoiding duplicate connections.
    pub fn db(&self) -> Arc<KnowledgeDb> {
        Arc::clone(&self.db)
    }

    /// Clean up old conversations (keep only last N days)
    pub async fn cleanup_old_conversations(&self, retain_days: u32) -> Result<usize> {
        self.db.cleanup_old_conversations(retain_days).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn test_add_and_search_entity() -> Result<()> {
        let temp_dir = env::temp_dir();
        let db_path = temp_dir.join("test_graph.db");
        let index_path = temp_dir.join("test_graph_index");

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);

        let graph = KnowledgeGraph::new(&db_path, &index_path)?;

        // Add entity
        let id = graph
            .add_entity(
                "Rust programming language",
                "concept",
                Some(serde_json::json!({"description": "Systems programming language"})),
            )
            .await?;

        // Search
        let results = graph.search("Rust", 10)?;
        assert!(!results.is_empty());
        assert_eq!(results[0].id, id);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);
        Ok(())
    }

    #[tokio::test]
    async fn test_link_entities() -> Result<()> {
        let temp_dir = env::temp_dir();
        let db_path = temp_dir.join("test_graph_link.db");
        let index_path = temp_dir.join("test_graph_link_index");

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);

        let graph = KnowledgeGraph::new(&db_path, &index_path)?;

        // Add entities
        let rust_id = graph.add_entity("Rust", "language", None).await?;
        let systems_id = graph
            .add_entity("Systems Programming", "domain", None)
            .await?;

        // Link them
        let rel_id = graph
            .link_entities(&rust_id, &systems_id, "used_for", None)
            .await?;
        assert!(!rel_id.is_empty());

        // Get context
        let context = graph.get_context_for(&rust_id).await?;
        assert_eq!(context.entity.id, rust_id);
        assert_eq!(context.relationships.len(), 1);
        assert_eq!(context.related_entities.len(), 1);

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);
        Ok(())
    }

    #[tokio::test]
    async fn test_remember_and_recall() -> Result<()> {
        let temp_dir = env::temp_dir();
        let db_path = temp_dir.join("test_graph_memory.db");
        let index_path = temp_dir.join("test_graph_memory_index");

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);

        let graph = KnowledgeGraph::new(&db_path, &index_path)?;

        // Remember something
        let id = graph
            .remember(
                "Rust is a systems programming language focused on safety and performance",
                "fact",
                Some("test_channel"),
            )
            .await?;
        assert!(!id.is_empty());

        // Recall it
        let contexts = graph.recall("Rust systems programming", 10).await?;
        assert!(!contexts.is_empty());

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);
        Ok(())
    }

    #[tokio::test]
    async fn test_watcher_operations() -> Result<()> {
        let temp_dir = env::temp_dir();
        let db_path = temp_dir.join("test_graph_watcher.db");
        let index_path = temp_dir.join("test_graph_watcher_index");

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);

        let graph = KnowledgeGraph::new(&db_path, &index_path)?;

        // Create watcher
        let config = serde_json::json!({"path": "/test/path", "pattern": "*.rs"});
        let watcher_id = graph
            .create_watcher("file", config, "notify", "test_channel")
            .await?;
        assert!(!watcher_id.is_empty());

        // Get active watchers
        let watchers = graph.get_active_watchers().await?;
        assert_eq!(watchers.len(), 1);

        // Deactivate watcher
        graph.update_watcher(&watcher_id, false).await?;
        let watchers = graph.get_active_watchers().await?;
        assert_eq!(watchers.len(), 0);

        // Delete watcher
        graph.delete_watcher(&watcher_id).await?;

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&index_path);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_entity() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        let id = graph.add_entity("TestEntity", "type_a", None).await?;
        let entity = graph.get_entity(&id).await?;
        assert!(entity.is_some());
        assert_eq!(entity.unwrap().name, "TestEntity");

        let missing = graph.get_entity("nonexistent").await?;
        assert!(missing.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn test_search_entities() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        graph.add_entity("Rust Lang", "language", None).await?;
        graph.add_entity("Python Lang", "language", None).await?;
        graph.add_entity("Rust Crate", "package", None).await?;

        let all_rust = graph.search_entities("Rust", None).await?;
        assert_eq!(all_rust.len(), 2);

        let rust_lang = graph.search_entities("Rust", Some("language")).await?;
        assert_eq!(rust_lang.len(), 1);
        assert_eq!(rust_lang[0].name, "Rust Lang");
        Ok(())
    }

    #[tokio::test]
    async fn test_get_relationships() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        let a = graph.add_entity("A", "node", None).await?;
        let b = graph.add_entity("B", "node", None).await?;
        graph.link_entities(&a, &b, "connects", None).await?;

        let rels = graph.get_relationships(&a).await?;
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].relation_type, "connects");

        let no_rels = graph.get_relationships("nonexistent").await?;
        assert!(no_rels.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_store_and_get_conversations() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        graph
            .store_conversation("discord", "user1", "Hello!", None)
            .await?;
        graph
            .store_conversation("discord", "meepo", "Hi there!", None)
            .await?;
        graph
            .store_conversation("slack", "user2", "Hey", None)
            .await?;

        let all = graph.get_conversations(None, 10).await?;
        assert_eq!(all.len(), 3);

        let discord_only = graph.get_conversations(Some("discord"), 10).await?;
        assert_eq!(discord_only.len(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_all_entities() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        assert!(graph.get_all_entities().await?.is_empty());

        graph.add_entity("E1", "type", None).await?;
        graph.add_entity("E2", "type", None).await?;

        let all = graph.get_all_entities().await?;
        assert_eq!(all.len(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn test_db_accessor() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        let db = graph.db();
        // Verify it's the same DB by inserting via db and reading via graph
        db.insert_entity("Direct", "node", None).await?;
        let entities = graph.get_all_entities().await?;
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "Direct");
        Ok(())
    }

    #[tokio::test]
    async fn test_reindex() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        graph.add_entity("Searchable Item", "concept", None).await?;

        // Reindex should succeed
        graph.reindex().await?;

        // Search should still work after reindex
        let results = graph.search("Searchable", 10)?;
        assert!(!results.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_remember_long_content() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        // Content longer than 100 chars should be truncated in entity name
        let long_content = "A".repeat(200);
        let id = graph.remember(&long_content, "note", None).await?;

        let entity = graph.get_entity(&id).await?.unwrap();
        assert!(entity.name.len() < 200);
        assert!(entity.name.ends_with("..."));
        Ok(())
    }

    #[tokio::test]
    async fn test_remember_without_channel() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        let id = graph.remember("No channel note", "note", None).await?;
        assert!(!id.is_empty());

        // No conversation should be stored
        let convos = graph.get_conversations(None, 10).await?;
        assert!(convos.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_link_nonexistent_entity() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        let a = graph.add_entity("A", "node", None).await?;
        let result = graph.link_entities(&a, "nonexistent", "rel", None).await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_cleanup_old_conversations() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        graph
            .store_conversation("ch", "user", "recent msg", None)
            .await?;

        // Cleanup with 30 day retention should keep recent messages
        let removed = graph.cleanup_old_conversations(30).await?;
        assert_eq!(removed, 0);

        let convos = graph.get_conversations(None, 10).await?;
        assert_eq!(convos.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_recall_empty_graph() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        let results = graph.recall("anything", 10).await?;
        assert!(results.is_empty());
        Ok(())
    }

    #[test]
    fn test_entity_context_debug() {
        let ctx = EntityContext {
            entity: Entity {
                id: "e1".to_string(),
                name: "Test".to_string(),
                entity_type: "node".to_string(),
                metadata: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            related_entities: vec![],
            relationships: vec![],
            recent_conversations: vec![],
        };
        let debug = format!("{:?}", ctx);
        assert!(debug.contains("Test"));
    }

    #[tokio::test]
    async fn test_get_context_for() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        let id = graph.add_entity("Alice", "person", None).await?;
        let ctx = graph.get_context_for(&id).await?;
        assert_eq!(ctx.entity.name, "Alice");
        assert!(ctx.related_entities.is_empty());
        assert!(ctx.relationships.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_update_and_delete_watcher() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        let id = graph
            .create_watcher("email", serde_json::json!({}), "alert", "slack")
            .await?;

        let watchers = graph.get_active_watchers().await?;
        assert_eq!(watchers.len(), 1);

        graph.update_watcher(&id, false).await?;
        let watchers = graph.get_active_watchers().await?;
        assert!(watchers.is_empty());

        graph.delete_watcher(&id).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_search_full_text() -> Result<()> {
        let temp = tempfile::TempDir::new()?;
        let graph = KnowledgeGraph::new(temp.path().join("t.db"), temp.path().join("idx"))?;

        graph.add_entity("Rust Programming", "language", None).await?;
        graph.add_entity("Python Programming", "language", None).await?;

        let results = graph.search("Rust", 10)?;
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.content.contains("Rust")));
        Ok(())
    }
}
