//! SQLite database layer for knowledge storage

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};
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

/// Autonomous goal tracked by the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub description: String,
    pub status: String,          // active|paused|completed|failed
    pub priority: i32,           // 1 (low) to 5 (critical)
    pub success_criteria: Option<String>,
    pub strategy: Option<String>,
    pub check_interval_secs: i64,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub source_channel: Option<String>,
    pub source: String,          // "user" or "template:<name>"
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Learned user preference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreference {
    pub id: String,
    pub category: String,        // communication|schedule|code|workflow
    pub key: String,
    pub value: JsonValue,
    pub confidence: f64,         // 0.0 to 1.0
    pub learned_from: Option<String>,
    pub last_confirmed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Log of autonomous actions taken
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionLogEntry {
    pub id: String,
    pub goal_id: Option<String>,
    pub action_type: String,
    pub description: String,
    pub outcome: String,         // success|failed|pending|unknown
    pub user_feedback: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Background task spawned by the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTask {
    pub id: String,
    pub description: String,
    pub status: String,           // pending, running, completed, failed, cancelled
    pub reply_channel: String,
    pub spawned_by: String,       // "agent" or watcher ID
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub result: Option<String>,
}

/// SQLite database wrapper (thread-safe via Arc<Mutex>)
pub struct KnowledgeDb {
    conn: Arc<Mutex<Connection>>,
}

impl KnowledgeDb {
    /// Initialize database with schema
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path.as_ref())
            .context("Failed to open SQLite database")?;

        info!("Initializing knowledge database at {:?}", path.as_ref());

        // Security note: The knowledge database stores conversation history, entities,
        // goals, and action logs in plaintext. Consider using SQLCipher for encryption
        // if the host machine is shared or the data is sensitive.
        warn!("Knowledge database is NOT encrypted. Conversation history and agent data are stored in plaintext at {:?}", path.as_ref());

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

        // Create goals table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS goals (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                priority INTEGER NOT NULL DEFAULT 3,
                success_criteria TEXT,
                strategy TEXT,
                check_interval_secs INTEGER NOT NULL DEFAULT 1800,
                last_checked_at TEXT,
                source_channel TEXT,
                source TEXT NOT NULL DEFAULT 'user',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_goals_status ON goals(status)",
            [],
        )?;

        // Migration: Add source column to existing goals tables
        let _ = conn.execute(
            "ALTER TABLE goals ADD COLUMN source TEXT NOT NULL DEFAULT 'user'",
            [],
        );

        // Create user_preferences table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS user_preferences (
                id TEXT PRIMARY KEY,
                category TEXT NOT NULL,
                key TEXT NOT NULL UNIQUE,
                value TEXT NOT NULL,
                confidence REAL NOT NULL DEFAULT 0.3,
                learned_from TEXT,
                last_confirmed_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_preferences_category ON user_preferences(category)",
            [],
        )?;

        // Create action_log table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS action_log (
                id TEXT PRIMARY KEY,
                goal_id TEXT,
                action_type TEXT NOT NULL,
                description TEXT NOT NULL,
                outcome TEXT NOT NULL DEFAULT 'pending',
                user_feedback TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (goal_id) REFERENCES goals(id)
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_action_log_goal ON action_log(goal_id)",
            [],
        )?;

        // Create background_tasks table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS background_tasks (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                reply_channel TEXT NOT NULL,
                spawned_by TEXT NOT NULL DEFAULT 'agent',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                result TEXT
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_background_tasks_status ON background_tasks(status)",
            [],
        )?;

        debug!("Database schema initialized successfully");

        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    /// Insert a new entity
    pub async fn insert_entity(
        &self,
        name: &str,
        entity_type: &str,
        metadata: Option<JsonValue>,
    ) -> Result<String> {
        let conn = Arc::clone(&self.conn);
        let name = name.to_owned();
        let entity_type = entity_type.to_owned();

        tokio::task::spawn_blocking(move || {
            let id = Uuid::new_v4().to_string();
            let now = Utc::now();
            let metadata_json = metadata.map(|m| serde_json::to_string(&m)).transpose()?;
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });

            conn.execute(
                "INSERT INTO entities (id, name, entity_type, metadata, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    &id,
                    &name,
                    &entity_type,
                    metadata_json,
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ],
            )?;

            debug!("Inserted entity: {} ({})", name, id);
            Ok(id)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Get entity by ID
    pub async fn get_entity(&self, id: &str) -> Result<Option<Entity>> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_owned();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let result = conn
                .query_row(
                    "SELECT id, name, entity_type, metadata, created_at, updated_at
                     FROM entities WHERE id = ?1",
                    params![&id],
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
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Search entities by name or type
    pub async fn search_entities(&self, query: &str, entity_type: Option<&str>) -> Result<Vec<Entity>> {
        let conn = Arc::clone(&self.conn);
        let query = query.to_owned();
        let entity_type = entity_type.map(|s| s.to_owned());

        tokio::task::spawn_blocking(move || {
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
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let mut stmt = conn.prepare(sql)?;

            let entities = if let Some(etype) = entity_type.as_deref() {
                stmt.query_map(params![&pattern, etype], Self::row_to_entity)?
            } else {
                stmt.query_map(params![&pattern], Self::row_to_entity)?
            }
            .collect::<Result<Vec<_>, _>>()?;

            Ok(entities)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Get all entities (capped to prevent OOM on large databases)
    pub async fn get_all_entities(&self) -> Result<Vec<Entity>> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let mut stmt = conn.prepare(
                "SELECT id, name, entity_type, metadata, created_at, updated_at
                 FROM entities
                 ORDER BY updated_at DESC
                 LIMIT 50000"
            )?;

            let entities = stmt
                .query_map([], Self::row_to_entity)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(entities)
        })
        .await
        .context("spawn_blocking task panicked")?
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
    pub async fn insert_relationship(
        &self,
        source_id: &str,
        target_id: &str,
        relation_type: &str,
        metadata: Option<JsonValue>,
    ) -> Result<String> {
        let conn = Arc::clone(&self.conn);
        let source_id = source_id.to_owned();
        let target_id = target_id.to_owned();
        let relation_type = relation_type.to_owned();

        tokio::task::spawn_blocking(move || {
            let id = Uuid::new_v4().to_string();
            let now = Utc::now();
            let metadata_json = metadata.map(|m| serde_json::to_string(&m)).transpose()?;
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });

            conn.execute(
                "INSERT INTO relationships (id, source_id, target_id, relation_type, metadata, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    &id,
                    &source_id,
                    &target_id,
                    &relation_type,
                    metadata_json,
                    now.to_rfc3339(),
                ],
            )?;

            debug!("Inserted relationship: {} -> {} ({})", source_id, target_id, relation_type);
            Ok(id)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Get relationships for an entity
    pub async fn get_relationships_for(&self, entity_id: &str) -> Result<Vec<Relationship>> {
        let conn = Arc::clone(&self.conn);
        let entity_id = entity_id.to_owned();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let mut stmt = conn.prepare(
                "SELECT id, source_id, target_id, relation_type, metadata, created_at
                 FROM relationships
                 WHERE source_id = ?1 OR target_id = ?1
                 ORDER BY created_at DESC",
            )?;

            let relationships = stmt
                .query_map(params![&entity_id], |row| {
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
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Insert a conversation
    pub async fn insert_conversation(
        &self,
        channel: &str,
        sender: &str,
        content: &str,
        metadata: Option<JsonValue>,
    ) -> Result<String> {
        let conn = Arc::clone(&self.conn);
        let channel = channel.to_owned();
        let sender = sender.to_owned();
        let content = content.to_owned();

        tokio::task::spawn_blocking(move || {
            let id = Uuid::new_v4().to_string();
            let now = Utc::now();
            let metadata_json = metadata.map(|m| serde_json::to_string(&m)).transpose()?;
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });

            conn.execute(
                "INSERT INTO conversations (id, channel, sender, content, metadata, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    &id,
                    &channel,
                    &sender,
                    &content,
                    metadata_json,
                    now.to_rfc3339(),
                ],
            )?;

            debug!("Inserted conversation in channel {}", channel);
            Ok(id)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Get recent conversations
    pub async fn get_recent_conversations(&self, channel: Option<&str>, limit: usize) -> Result<Vec<Conversation>> {
        let conn = Arc::clone(&self.conn);
        let channel = channel.map(|s| s.to_owned());
        let limit = limit;

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let (sql, params_vec): (String, Vec<String>) = if let Some(ref ch) = channel {
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
        })
        .await
        .context("spawn_blocking task panicked")?
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
    pub async fn insert_watcher(
        &self,
        kind: &str,
        config: JsonValue,
        action: &str,
        reply_channel: &str,
    ) -> Result<String> {
        let conn = Arc::clone(&self.conn);
        let kind = kind.to_owned();
        let action = action.to_owned();
        let reply_channel = reply_channel.to_owned();

        tokio::task::spawn_blocking(move || {
            let id = format!("w-{}", Uuid::new_v4());
            let now = Utc::now();
            let config_json = serde_json::to_string(&config)?;
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });

            conn.execute(
                "INSERT INTO watchers (id, kind, config, action, reply_channel, active, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                params![
                    &id,
                    &kind,
                    config_json,
                    &action,
                    &reply_channel,
                    now.to_rfc3339(),
                ],
            )?;

            debug!("Inserted watcher: {} ({})", kind, id);
            Ok(id)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Get active watchers
    pub async fn get_active_watchers(&self) -> Result<Vec<Watcher>> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
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
        })
        .await
        .context("spawn_blocking task panicked")?
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

    /// Get a single watcher by ID
    pub async fn get_watcher(&self, id: &str) -> Result<Option<Watcher>> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_owned();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let mut stmt = conn.prepare(
                "SELECT id, kind, config, action, reply_channel, active, created_at
                 FROM watchers
                 WHERE id = ?1",
            )?;

            let mut rows = stmt.query_map(params![&id], Self::row_to_watcher)?;
            match rows.next() {
                Some(Ok(w)) => Ok(Some(w)),
                Some(Err(e)) => Err(e.into()),
                None => Ok(None),
            }
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Update watcher active status
    pub async fn update_watcher_active(&self, id: &str, active: bool) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_owned();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            conn.execute(
                "UPDATE watchers SET active = ?1 WHERE id = ?2",
                params![active as i64, &id],
            )?;

            debug!("Updated watcher {} active status to {}", id, active);
            Ok(())
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Delete a watcher
    pub async fn delete_watcher(&self, id: &str) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_owned();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            conn.execute("DELETE FROM watchers WHERE id = ?1", params![&id])?;
            debug!("Deleted watcher {}", id);
            Ok(())
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Insert a new goal
    pub async fn insert_goal(
        &self,
        description: &str,
        priority: i32,
        check_interval_secs: i64,
        success_criteria: Option<&str>,
        source_channel: Option<&str>,
        source: &str,
    ) -> Result<String> {
        let conn = Arc::clone(&self.conn);
        let description = description.to_owned();
        let success_criteria = success_criteria.map(|s| s.to_owned());
        let source_channel = source_channel.map(|s| s.to_owned());
        let source = source.to_owned();

        tokio::task::spawn_blocking(move || {
            let id = Uuid::new_v4().to_string();
            let now = Utc::now();
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            conn.execute(
                "INSERT INTO goals (id, description, status, priority, success_criteria, check_interval_secs, source_channel, source, created_at, updated_at)
                 VALUES (?1, ?2, 'active', ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![&id, &description, priority, success_criteria, check_interval_secs, source_channel, &source, now.to_rfc3339(), now.to_rfc3339()],
            )?;
            debug!("Inserted goal: {} ({})", description, id);
            Ok(id)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Get active goals that are due for checking
    pub async fn get_due_goals(&self) -> Result<Vec<Goal>> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let mut stmt = conn.prepare(
                "SELECT id, description, status, priority, success_criteria, strategy,
                        check_interval_secs, last_checked_at, source_channel, source, created_at, updated_at
                 FROM goals
                 WHERE status = 'active'
                   AND (last_checked_at IS NULL
                        OR strftime('%s', 'now') - strftime('%s', last_checked_at) >= check_interval_secs)
                 ORDER BY priority DESC, created_at ASC",
            )?;

            let goals = stmt
                .query_map([], Self::row_to_goal)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(goals)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Get all active goals
    pub async fn get_active_goals(&self) -> Result<Vec<Goal>> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let mut stmt = conn.prepare(
                "SELECT id, description, status, priority, success_criteria, strategy,
                        check_interval_secs, last_checked_at, source_channel, source, created_at, updated_at
                 FROM goals WHERE status = 'active'
                 ORDER BY priority DESC, created_at ASC",
            )?;
            let goals = stmt
                .query_map([], Self::row_to_goal)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(goals)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Update goal status
    pub async fn update_goal_status(&self, id: &str, status: &str) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_owned();
        let status = status.to_owned();

        tokio::task::spawn_blocking(move || {
            let now = Utc::now();
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            conn.execute(
                "UPDATE goals SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![&status, now.to_rfc3339(), &id],
            )?;
            Ok(())
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Update goal strategy and mark as checked
    pub async fn update_goal_checked(&self, id: &str, strategy: Option<&str>) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_owned();
        let strategy = strategy.map(|s| s.to_owned());

        tokio::task::spawn_blocking(move || {
            let now = Utc::now();
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            conn.execute(
                "UPDATE goals SET last_checked_at = ?1, strategy = COALESCE(?2, strategy), updated_at = ?3 WHERE id = ?4",
                params![now.to_rfc3339(), strategy, now.to_rfc3339(), &id],
            )?;
            Ok(())
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Delete all goals with a given source (e.g. "template:stock-analyst")
    pub async fn delete_goals_by_source(&self, source: &str) -> Result<usize> {
        let conn = Arc::clone(&self.conn);
        let source = source.to_owned();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let count = conn.execute(
                "DELETE FROM goals WHERE source = ?1",
                params![&source],
            )?;
            debug!("Deleted {} goals with source: {}", count, source);
            Ok(count)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Helper to convert row to Goal
    fn row_to_goal(row: &rusqlite::Row) -> rusqlite::Result<Goal> {
        Ok(Goal {
            id: row.get(0)?,
            description: row.get(1)?,
            status: row.get(2)?,
            priority: row.get(3)?,
            success_criteria: row.get(4)?,
            strategy: row.get(5)?,
            check_interval_secs: row.get(6)?,
            last_checked_at: row.get::<_, Option<String>>(7)?
                .and_then(|s| s.parse().ok()),
            source_channel: row.get(8)?,
            source: row.get(9)?,
            created_at: row.get::<_, String>(10)?.parse().unwrap_or_else(|_| Utc::now()),
            updated_at: row.get::<_, String>(11)?.parse().unwrap_or_else(|_| Utc::now()),
        })
    }

    /// Upsert a user preference (insert or update by key)
    pub async fn upsert_preference(
        &self,
        category: &str,
        key: &str,
        value: JsonValue,
        confidence: f64,
        learned_from: Option<&str>,
    ) -> Result<String> {
        let conn = Arc::clone(&self.conn);
        let category = category.to_owned();
        let key = key.to_owned();
        let learned_from = learned_from.map(|s| s.to_owned());

        tokio::task::spawn_blocking(move || {
            let now = Utc::now();
            let value_str = serde_json::to_string(&value)?;
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });

            // Try update first
            let updated = conn.execute(
                "UPDATE user_preferences SET value = ?1, confidence = ?2, learned_from = COALESCE(?3, learned_from), updated_at = ?4 WHERE key = ?5",
                params![&value_str, confidence, learned_from, now.to_rfc3339(), &key],
            )?;

            if updated > 0 {
                // Return existing id
                let id: String = conn.query_row(
                    "SELECT id FROM user_preferences WHERE key = ?1",
                    params![&key],
                    |row| row.get(0),
                )?;
                return Ok(id);
            }

            // Insert new preference
            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO user_preferences (id, category, key, value, confidence, learned_from, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![&id, &category, &key, &value_str, confidence, learned_from, now.to_rfc3339(), now.to_rfc3339()],
            )?;
            debug!("Upserted preference: {} = {:?}", key, value);
            Ok(id)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Get all preferences, optionally filtered by category
    pub async fn get_preferences(&self, category: Option<&str>) -> Result<Vec<UserPreference>> {
        let conn = Arc::clone(&self.conn);
        let category = category.map(|s| s.to_owned());

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });

            let (sql, params_vec): (&str, Vec<String>) = if let Some(ref cat) = category {
                ("SELECT id, category, key, value, confidence, learned_from, last_confirmed_at, created_at, updated_at
                  FROM user_preferences WHERE category = ?1 ORDER BY confidence DESC",
                 vec![cat.clone()])
            } else {
                ("SELECT id, category, key, value, confidence, learned_from, last_confirmed_at, created_at, updated_at
                  FROM user_preferences ORDER BY confidence DESC",
                 vec![])
            };

            let mut stmt = conn.prepare(sql)?;
            let prefs = if category.is_some() {
                stmt.query_map(params![&params_vec[0]], Self::row_to_preference)?
            } else {
                stmt.query_map([], Self::row_to_preference)?
            }
            .collect::<Result<Vec<_>, _>>()?;
            Ok(prefs)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Helper to convert row to UserPreference
    fn row_to_preference(row: &rusqlite::Row) -> rusqlite::Result<UserPreference> {
        let value_str: String = row.get(3)?;
        let value = serde_json::from_str(&value_str)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(e),
            ))?;

        Ok(UserPreference {
            id: row.get(0)?,
            category: row.get(1)?,
            key: row.get(2)?,
            value,
            confidence: row.get(4)?,
            learned_from: row.get(5)?,
            last_confirmed_at: row.get::<_, Option<String>>(6)?.and_then(|s| s.parse().ok()),
            created_at: row.get::<_, String>(7)?.parse().unwrap_or_else(|_| Utc::now()),
            updated_at: row.get::<_, String>(8)?.parse().unwrap_or_else(|_| Utc::now()),
        })
    }

    /// Insert an action log entry
    pub async fn insert_action_log(
        &self,
        goal_id: Option<&str>,
        action_type: &str,
        description: &str,
        outcome: &str,
    ) -> Result<String> {
        let conn = Arc::clone(&self.conn);
        let goal_id = goal_id.map(|s| s.to_owned());
        let action_type = action_type.to_owned();
        let description = description.to_owned();
        let outcome = outcome.to_owned();

        tokio::task::spawn_blocking(move || {
            let id = Uuid::new_v4().to_string();
            let now = Utc::now();
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            conn.execute(
                "INSERT INTO action_log (id, goal_id, action_type, description, outcome, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![&id, goal_id, &action_type, &description, &outcome, now.to_rfc3339()],
            )?;
            debug!("Inserted action log: {} - {}", action_type, description);
            Ok(id)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Get recent action log entries
    pub async fn get_recent_actions(&self, limit: usize) -> Result<Vec<ActionLogEntry>> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let mut stmt = conn.prepare(
                "SELECT id, goal_id, action_type, description, outcome, user_feedback, created_at
                 FROM action_log ORDER BY created_at DESC LIMIT ?1",
            )?;
            let entries = stmt
                .query_map(params![limit as i64], |row| {
                    Ok(ActionLogEntry {
                        id: row.get(0)?,
                        goal_id: row.get(1)?,
                        action_type: row.get(2)?,
                        description: row.get(3)?,
                        outcome: row.get(4)?,
                        user_feedback: row.get(5)?,
                        created_at: row.get::<_, String>(6)?.parse().unwrap_or_else(|_| Utc::now()),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(entries)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Clean up old conversations (keep only last N days)
    pub async fn cleanup_old_conversations(&self, retain_days: u32) -> Result<usize> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let deleted = conn.execute(
                "DELETE FROM conversations WHERE created_at < datetime('now', ?)",
                params![format!("-{} days", retain_days)],
            )?;
            if deleted > 0 {
                info!("Cleaned up {} old conversations", deleted);
            }
            Ok(deleted)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Insert a new background task
    pub async fn insert_background_task(
        &self,
        id: &str,
        description: &str,
        reply_channel: &str,
        spawned_by: &str,
    ) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_owned();
        let description = description.to_owned();
        let reply_channel = reply_channel.to_owned();
        let spawned_by = spawned_by.to_owned();

        tokio::task::spawn_blocking(move || {
            let now = Utc::now();
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            conn.execute(
                "INSERT INTO background_tasks (id, description, status, reply_channel, spawned_by, created_at, updated_at)
                 VALUES (?1, ?2, 'pending', ?3, ?4, ?5, ?6)",
                params![&id, &description, &reply_channel, &spawned_by, now.to_rfc3339(), now.to_rfc3339()],
            )?;
            debug!("Inserted background task: {} ({})", description, id);
            Ok(())
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Update background task status and optionally set result
    pub async fn update_background_task(
        &self,
        id: &str,
        status: &str,
        result: Option<&str>,
    ) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_owned();
        let status = status.to_owned();
        let result = result.map(|s| s.to_owned());

        tokio::task::spawn_blocking(move || {
            let now = Utc::now();
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            conn.execute(
                "UPDATE background_tasks SET status = ?1, result = COALESCE(?2, result), updated_at = ?3 WHERE id = ?4",
                params![&status, result, now.to_rfc3339(), &id],
            )?;
            Ok(())
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Get active (pending or running) background tasks
    pub async fn get_active_background_tasks(&self) -> Result<Vec<BackgroundTask>> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let mut stmt = conn.prepare(
                "SELECT id, description, status, reply_channel, spawned_by, created_at, updated_at, result
                 FROM background_tasks WHERE status IN ('pending', 'running')
                 ORDER BY created_at DESC",
            )?;
            let tasks = stmt
                .query_map([], Self::row_to_background_task)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(tasks)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    /// Get recently completed/failed background tasks
    pub async fn get_recent_background_tasks(&self, limit: usize) -> Result<Vec<BackgroundTask>> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|poisoned| {
                warn!("Database mutex was poisoned, recovering");
                poisoned.into_inner()
            });
            let mut stmt = conn.prepare(
                "SELECT id, description, status, reply_channel, spawned_by, created_at, updated_at, result
                 FROM background_tasks WHERE status IN ('completed', 'failed')
                 ORDER BY updated_at DESC LIMIT ?1",
            )?;
            let tasks = stmt
                .query_map(params![limit as i64], Self::row_to_background_task)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(tasks)
        })
        .await
        .context("spawn_blocking task panicked")?
    }

    fn row_to_background_task(row: &rusqlite::Row) -> rusqlite::Result<BackgroundTask> {
        Ok(BackgroundTask {
            id: row.get(0)?,
            description: row.get(1)?,
            status: row.get(2)?,
            reply_channel: row.get(3)?,
            spawned_by: row.get(4)?,
            created_at: row.get::<_, String>(5)?.parse().unwrap_or_else(|_| Utc::now()),
            updated_at: row.get::<_, String>(6)?.parse().unwrap_or_else(|_| Utc::now()),
            result: row.get(7)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn test_entity_operations() -> Result<()> {
        let temp_path = env::temp_dir().join("test_entities.db");
        let _ = std::fs::remove_file(&temp_path);

        let db = KnowledgeDb::new(&temp_path)?;

        // Insert entity
        let id = db.insert_entity("test_entity", "concept", None).await?;
        assert!(!id.is_empty());

        // Get entity
        let entity = db.get_entity(&id).await?;
        assert!(entity.is_some());
        assert_eq!(entity.unwrap().name, "test_entity");

        // Search entities
        let results = db.search_entities("test", None).await?;
        assert!(!results.is_empty());

        let _ = std::fs::remove_file(&temp_path);
        Ok(())
    }

    #[tokio::test]
    async fn test_relationship_operations() -> Result<()> {
        let temp_path = env::temp_dir().join("test_relationships.db");
        let _ = std::fs::remove_file(&temp_path);

        let db = KnowledgeDb::new(&temp_path)?;

        // Create entities
        let source_id = db.insert_entity("source", "concept", None).await?;
        let target_id = db.insert_entity("target", "concept", None).await?;

        // Create relationship
        let rel_id = db.insert_relationship(&source_id, &target_id, "relates_to", None).await?;
        assert!(!rel_id.is_empty());

        // Get relationships
        let rels = db.get_relationships_for(&source_id).await?;
        assert_eq!(rels.len(), 1);

        let _ = std::fs::remove_file(&temp_path);
        Ok(())
    }

    #[tokio::test]
    async fn test_goal_operations() -> Result<()> {
        let temp_path = env::temp_dir().join("test_goals.db");
        let _ = std::fs::remove_file(&temp_path);
        let db = KnowledgeDb::new(&temp_path)?;

        // Insert goal with user source
        let id = db.insert_goal("Review PRs daily", 3, 3600, Some("All PRs reviewed"), Some("discord"), "user").await?;
        assert!(!id.is_empty());

        // Get active goals
        let goals = db.get_active_goals().await?;
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].description, "Review PRs daily");
        assert_eq!(goals[0].source, "user");

        // Get due goals (should be due immediately since last_checked_at is NULL)
        let due = db.get_due_goals().await?;
        assert_eq!(due.len(), 1);

        // Mark as checked
        db.update_goal_checked(&id, Some("Check GitHub PRs tool")).await?;

        // Should no longer be due (just checked, interval is 3600s)
        let due = db.get_due_goals().await?;
        assert_eq!(due.len(), 0);

        // Update status
        db.update_goal_status(&id, "completed").await?;
        let active = db.get_active_goals().await?;
        assert_eq!(active.len(), 0);

        // Test template goals and delete by source
        let _template_id = db.insert_goal("Monitor stocks", 4, 900, None, None, "template:stock-analyst").await?;
        let active = db.get_active_goals().await?;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].source, "template:stock-analyst");

        let deleted = db.delete_goals_by_source("template:stock-analyst").await?;
        assert_eq!(deleted, 1);
        let remaining = db.get_active_goals().await?;
        assert_eq!(remaining.len(), 0);

        let _ = std::fs::remove_file(&temp_path);
        Ok(())
    }

    #[tokio::test]
    async fn test_preference_operations() -> Result<()> {
        let temp_path = env::temp_dir().join("test_prefs.db");
        let _ = std::fs::remove_file(&temp_path);
        let db = KnowledgeDb::new(&temp_path)?;

        let id = db.upsert_preference("schedule", "morning_summary", serde_json::json!(true), 0.5, Some("user asked 3 times")).await?;
        assert!(!id.is_empty());

        let prefs = db.get_preferences(Some("schedule")).await?;
        assert_eq!(prefs.len(), 1);
        assert_eq!(prefs[0].key, "morning_summary");

        // Upsert same key updates
        let id2 = db.upsert_preference("schedule", "morning_summary", serde_json::json!(true), 0.8, None).await?;
        assert_eq!(id, id2);
        let prefs = db.get_preferences(None).await?;
        assert_eq!(prefs.len(), 1);
        assert!((prefs[0].confidence - 0.8).abs() < 0.01);

        let _ = std::fs::remove_file(&temp_path);
        Ok(())
    }

    #[tokio::test]
    async fn test_action_log_operations() -> Result<()> {
        let temp_path = env::temp_dir().join("test_actions.db");
        let _ = std::fs::remove_file(&temp_path);
        let db = KnowledgeDb::new(&temp_path)?;

        let id = db.insert_action_log(None, "sent_email", "Sent morning summary", "success").await?;
        assert!(!id.is_empty());

        let actions = db.get_recent_actions(10).await?;
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_type, "sent_email");

        let _ = std::fs::remove_file(&temp_path);
        Ok(())
    }

    #[tokio::test]
    async fn test_background_task_operations() -> Result<()> {
        let temp_path = env::temp_dir().join(format!("test_bg_tasks_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&temp_path);
        let db = KnowledgeDb::new(&temp_path)?;

        // Insert task
        db.insert_background_task("t-123", "Research competitors", "slack", "agent").await?;

        // Get active tasks
        let active = db.get_active_background_tasks().await?;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "t-123");
        assert_eq!(active[0].status, "pending");

        // Update to running
        db.update_background_task("t-123", "running", None).await?;
        let active = db.get_active_background_tasks().await?;
        assert_eq!(active[0].status, "running");

        // Complete with result
        db.update_background_task("t-123", "completed", Some("Found 3 competitors")).await?;
        let active = db.get_active_background_tasks().await?;
        assert_eq!(active.len(), 0);

        let recent = db.get_recent_background_tasks(10).await?;
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].result.as_deref(), Some("Found 3 competitors"));

        let _ = std::fs::remove_file(&temp_path);
        Ok(())
    }
}
