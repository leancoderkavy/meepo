//! SQLite database layer for knowledge storage

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::Path;
use std::sync::Mutex;
use tracing::{debug, info};
use uuid::Uuid;

/// Entity in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Relationship between entities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub relation_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
    pub created_at: DateTime<Utc>,
}

/// Conversation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub channel: String,
    pub sender: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
    pub created_at: DateTime<Utc>,
}

/// Watcher configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Watcher {
    pub id: String,
    pub kind: String,
    pub config: JsonValue,
    pub action: String,
    pub reply_channel: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

/// SQLite database wrapper (thread-safe via Mutex)
pub struct KnowledgeDb {
    conn: Mutex<Connection>,
}

impl KnowledgeDb {
    /// Initialize database with schema
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path.as_ref())
            .context("Failed to open SQLite database")?;

        info!("Initializing knowledge database at {:?}", path.as_ref());

        // Enable foreign keys
        conn.execute("PRAGMA foreign_keys = ON", [])?;

        // Create entities table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS entities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                metadata TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create relationships table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS relationships (
                id TEXT PRIMARY KEY,
                source_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                relation_type TEXT NOT NULL,
                metadata TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY(source_id) REFERENCES entities(id) ON DELETE CASCADE,
                FOREIGN KEY(target_id) REFERENCES entities(id) ON DELETE CASCADE
            )",
            [],
        )?;

        // Create conversations table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                channel TEXT NOT NULL,
                sender TEXT NOT NULL,
                content TEXT NOT NULL,
                metadata TEXT,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create watchers table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS watchers (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                config TEXT NOT NULL,
                action TEXT NOT NULL,
                reply_channel TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create indices for better query performance
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(name)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_relationships_source ON relationships(source_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_relationships_target ON relationships(target_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conversations_channel ON conversations(channel)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_conversations_created ON conversations(created_at)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_watchers_active ON watchers(active)",
            [],
        )?;

        debug!("Database schema initialized successfully");

        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Insert a new entity
    pub fn insert_entity(
        &self,
        name: &str,
        entity_type: &str,
        metadata: Option<JsonValue>,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let metadata_json = metadata.map(|m| serde_json::to_string(&m)).transpose()?;
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;

        conn.execute(
            "INSERT INTO entities (id, name, entity_type, metadata, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &id,
                name,
                entity_type,
                metadata_json,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;

        debug!("Inserted entity: {} ({})", name, id);
        Ok(id)
    }

    /// Get entity by ID
    pub fn get_entity(&self, id: &str) -> Result<Option<Entity>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        let result = conn
            .query_row(
                "SELECT id, name, entity_type, metadata, created_at, updated_at
                 FROM entities WHERE id = ?1",
                params![id],
                |row| {
                    let metadata_str: Option<String> = row.get(3)?;
                    let metadata = metadata_str
                        .map(|s| serde_json::from_str(&s))
                        .transpose()
                        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        ))?;

                    Ok(Entity {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        entity_type: row.get(2)?,
                        metadata,
                        created_at: row.get::<_, String>(4)?.parse().unwrap_or_else(|_| Utc::now()),
                        updated_at: row.get::<_, String>(5)?.parse().unwrap_or_else(|_| Utc::now()),
                    })
                },
            )
            .optional()?;

        Ok(result)
    }

    /// Search entities by name or type
    pub fn search_entities(&self, query: &str, entity_type: Option<&str>) -> Result<Vec<Entity>> {
        let sql = if entity_type.is_some() {
            "SELECT id, name, entity_type, metadata, created_at, updated_at
             FROM entities
             WHERE (name LIKE ?1 OR entity_type LIKE ?1) AND entity_type = ?2
             ORDER BY updated_at DESC
             LIMIT 100"
        } else {
            "SELECT id, name, entity_type, metadata, created_at, updated_at
             FROM entities
             WHERE name LIKE ?1 OR entity_type LIKE ?1
             ORDER BY updated_at DESC
             LIMIT 100"
        };

        let pattern = format!("%{}%", query);
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        let mut stmt = conn.prepare(sql)?;

        let entities = if let Some(etype) = entity_type {
            stmt.query_map(params![&pattern, etype], Self::row_to_entity)?
        } else {
            stmt.query_map(params![&pattern], Self::row_to_entity)?
        }
        .collect::<Result<Vec<_>, _>>()?;

        Ok(entities)
    }

    /// Get all entities
    pub fn get_all_entities(&self) -> Result<Vec<Entity>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, name, entity_type, metadata, created_at, updated_at
             FROM entities
             ORDER BY updated_at DESC"
        )?;

        let entities = stmt
            .query_map([], Self::row_to_entity)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entities)
    }

    /// Helper to convert row to Entity
    fn row_to_entity(row: &rusqlite::Row) -> rusqlite::Result<Entity> {
        let metadata_str: Option<String> = row.get(3)?;
        let metadata = metadata_str
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(e),
            ))?;

        Ok(Entity {
            id: row.get(0)?,
            name: row.get(1)?,
            entity_type: row.get(2)?,
            metadata,
            created_at: row.get::<_, String>(4)?.parse().unwrap_or_else(|_| Utc::now()),
            updated_at: row.get::<_, String>(5)?.parse().unwrap_or_else(|_| Utc::now()),
        })
    }

    /// Insert a relationship
    pub fn insert_relationship(
        &self,
        source_id: &str,
        target_id: &str,
        relation_type: &str,
        metadata: Option<JsonValue>,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let metadata_json = metadata.map(|m| serde_json::to_string(&m)).transpose()?;
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;

        conn.execute(
            "INSERT INTO relationships (id, source_id, target_id, relation_type, metadata, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &id,
                source_id,
                target_id,
                relation_type,
                metadata_json,
                now.to_rfc3339(),
            ],
        )?;

        debug!("Inserted relationship: {} -> {} ({})", source_id, target_id, relation_type);
        Ok(id)
    }

    /// Get relationships for an entity
    pub fn get_relationships_for(&self, entity_id: &str) -> Result<Vec<Relationship>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, source_id, target_id, relation_type, metadata, created_at
             FROM relationships
             WHERE source_id = ?1 OR target_id = ?1
             ORDER BY created_at DESC",
        )?;

        let relationships = stmt
            .query_map(params![entity_id], |row| {
                let metadata_str: Option<String> = row.get(4)?;
                let metadata = metadata_str
                    .map(|s| serde_json::from_str(&s))
                    .transpose()
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    ))?;

                Ok(Relationship {
                    id: row.get(0)?,
                    source_id: row.get(1)?,
                    target_id: row.get(2)?,
                    relation_type: row.get(3)?,
                    metadata,
                    created_at: row.get::<_, String>(5)?.parse().unwrap_or_else(|_| Utc::now()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(relationships)
    }

    /// Insert a conversation
    pub fn insert_conversation(
        &self,
        channel: &str,
        sender: &str,
        content: &str,
        metadata: Option<JsonValue>,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let metadata_json = metadata.map(|m| serde_json::to_string(&m)).transpose()?;
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;

        conn.execute(
            "INSERT INTO conversations (id, channel, sender, content, metadata, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &id,
                channel,
                sender,
                content,
                metadata_json,
                now.to_rfc3339(),
            ],
        )?;

        debug!("Inserted conversation in channel {}", channel);
        Ok(id)
    }

    /// Get recent conversations
    pub fn get_recent_conversations(&self, channel: Option<&str>, limit: usize) -> Result<Vec<Conversation>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        let (sql, params_vec): (String, Vec<String>) = if let Some(ch) = channel {
            (
                "SELECT id, channel, sender, content, metadata, created_at
                 FROM conversations
                 WHERE channel = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2".to_string(),
                vec![ch.to_string(), limit.to_string()],
            )
        } else {
            (
                "SELECT id, channel, sender, content, metadata, created_at
                 FROM conversations
                 ORDER BY created_at DESC
                 LIMIT ?1".to_string(),
                vec![limit.to_string()],
            )
        };

        let mut stmt = conn.prepare(&sql)?;

        let conversations = if channel.is_some() {
            stmt.query_map(params![&params_vec[0], &params_vec[1]], Self::row_to_conversation)?
        } else {
            stmt.query_map(params![&params_vec[0]], Self::row_to_conversation)?
        }
        .collect::<Result<Vec<_>, _>>()?;

        Ok(conversations)
    }

    /// Helper to convert row to Conversation
    fn row_to_conversation(row: &rusqlite::Row) -> rusqlite::Result<Conversation> {
        let metadata_str: Option<String> = row.get(4)?;
        let metadata = metadata_str
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(e),
            ))?;

        Ok(Conversation {
            id: row.get(0)?,
            channel: row.get(1)?,
            sender: row.get(2)?,
            content: row.get(3)?,
            metadata,
            created_at: row.get::<_, String>(5)?.parse().unwrap_or_else(|_| Utc::now()),
        })
    }

    /// Insert a watcher
    pub fn insert_watcher(
        &self,
        kind: &str,
        config: JsonValue,
        action: &str,
        reply_channel: &str,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let config_json = serde_json::to_string(&config)?;
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;

        conn.execute(
            "INSERT INTO watchers (id, kind, config, action, reply_channel, active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
            params![
                &id,
                kind,
                config_json,
                action,
                reply_channel,
                now.to_rfc3339(),
            ],
        )?;

        debug!("Inserted watcher: {} ({})", kind, id);
        Ok(id)
    }

    /// Get active watchers
    pub fn get_active_watchers(&self) -> Result<Vec<Watcher>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, kind, config, action, reply_channel, active, created_at
             FROM watchers
             WHERE active = 1
             ORDER BY created_at DESC",
        )?;

        let watchers = stmt
            .query_map([], Self::row_to_watcher)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(watchers)
    }

    /// Helper to convert row to Watcher
    fn row_to_watcher(row: &rusqlite::Row) -> rusqlite::Result<Watcher> {
        let config_str: String = row.get(2)?;
        let config = serde_json::from_str(&config_str)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(e),
            ))?;

        Ok(Watcher {
            id: row.get(0)?,
            kind: row.get(1)?,
            config,
            action: row.get(3)?,
            reply_channel: row.get(4)?,
            active: row.get::<_, i64>(5)? != 0,
            created_at: row.get::<_, String>(6)?.parse().unwrap_or_else(|_| Utc::now()),
        })
    }

    /// Update watcher active status
    pub fn update_watcher_active(&self, id: &str, active: bool) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        conn.execute(
            "UPDATE watchers SET active = ?1 WHERE id = ?2",
            params![active as i64, id],
        )?;

        debug!("Updated watcher {} active status to {}", id, active);
        Ok(())
    }

    /// Delete a watcher
    pub fn delete_watcher(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        conn.execute("DELETE FROM watchers WHERE id = ?1", params![id])?;
        debug!("Deleted watcher {}", id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_entity_operations() -> Result<()> {
        let temp_path = env::temp_dir().join("test_entities.db");
        let _ = std::fs::remove_file(&temp_path);

        let db = KnowledgeDb::new(&temp_path)?;

        // Insert entity
        let id = db.insert_entity("test_entity", "concept", None)?;
        assert!(!id.is_empty());

        // Get entity
        let entity = db.get_entity(&id)?;
        assert!(entity.is_some());
        assert_eq!(entity.unwrap().name, "test_entity");

        // Search entities
        let results = db.search_entities("test", None)?;
        assert!(!results.is_empty());

        let _ = std::fs::remove_file(&temp_path);
        Ok(())
    }

    #[test]
    fn test_relationship_operations() -> Result<()> {
        let temp_path = env::temp_dir().join("test_relationships.db");
        let _ = std::fs::remove_file(&temp_path);

        let db = KnowledgeDb::new(&temp_path)?;

        // Create entities
        let source_id = db.insert_entity("source", "concept", None)?;
        let target_id = db.insert_entity("target", "concept", None)?;

        // Create relationship
        let rel_id = db.insert_relationship(&source_id, &target_id, "relates_to", None)?;
        assert!(!rel_id.is_empty());

        // Get relationships
        let rels = db.get_relationships_for(&source_id)?;
        assert_eq!(rels.len(), 1);

        let _ = std::fs::remove_file(&temp_path);
        Ok(())
    }
}
