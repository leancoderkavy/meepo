//! Session management — each conversation gets its own session

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// A single chat session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub message_count: u64,
}

/// Manages all active sessions
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl SessionManager {
    /// Create a new session manager with a default "main" session
    pub fn new() -> Self {
        let mut sessions = HashMap::new();
        let now = Utc::now();
        sessions.insert(
            "main".to_string(),
            Session {
                id: "main".to_string(),
                name: "Main".to_string(),
                created_at: now,
                last_activity: now,
                message_count: 0,
            },
        );
        Self {
            sessions: Arc::new(RwLock::new(sessions)),
        }
    }

    /// List all sessions
    pub async fn list(&self) -> Vec<Session> {
        let sessions = self.sessions.read().await;
        let mut list: Vec<Session> = sessions.values().cloned().collect();
        list.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        list
    }

    /// Get a session by ID
    pub async fn get(&self, id: &str) -> Option<Session> {
        let sessions = self.sessions.read().await;
        sessions.get(id).cloned()
    }

    /// Create a new session, returns the session
    pub async fn create(&self, name: &str) -> Session {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let session = Session {
            id: id.clone(),
            name: name.to_string(),
            created_at: now,
            last_activity: now,
            message_count: 0,
        };
        let mut sessions = self.sessions.write().await;
        sessions.insert(id.clone(), session.clone());
        info!("Created session '{}' ({})", name, id);
        session
    }

    /// Record activity on a session (updates last_activity and message_count)
    pub async fn record_activity(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.last_activity = Utc::now();
            session.message_count += 1;
            debug!(
                "Session '{}' activity (messages: {})",
                session_id, session.message_count
            );
        }
    }

    /// Number of active sessions
    pub async fn count(&self) -> usize {
        self.sessions.read().await.len()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_manager_default() {
        let mgr = SessionManager::new();
        let sessions = mgr.list().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "main");
    }

    #[tokio::test]
    async fn test_create_session() {
        let mgr = SessionManager::new();
        let session = mgr.create("Research").await;
        assert_eq!(session.name, "Research");
        assert_eq!(mgr.count().await, 2);
    }

    #[tokio::test]
    async fn test_get_session() {
        let mgr = SessionManager::new();
        let session = mgr.get("main").await;
        assert!(session.is_some());
        assert_eq!(session.unwrap().name, "Main");

        let missing = mgr.get("nonexistent").await;
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_record_activity() {
        let mgr = SessionManager::new();
        mgr.record_activity("main").await;
        mgr.record_activity("main").await;
        let session = mgr.get("main").await.unwrap();
        assert_eq!(session.message_count, 2);
    }

    #[tokio::test]
    async fn test_list_sorted_by_activity() {
        let mgr = SessionManager::new();
        let _s1 = mgr.create("Older").await;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let s2 = mgr.create("Newer").await;

        let list = mgr.list().await;
        // Newest first
        assert_eq!(list[0].id, s2.id);
    }
}
