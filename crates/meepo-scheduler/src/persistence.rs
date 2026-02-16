//! SQLite persistence for watchers
//!
//! This module handles saving and loading watchers from SQLite,
//! reusing the same database connection as the knowledge graph.

use crate::watcher::Watcher;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use tracing::{debug, info, warn};

/// Initialize watcher tables in the database
///
/// Creates the necessary tables for storing watchers if they don't exist.
/// Safe to call multiple times.
pub fn init_watcher_tables(conn: &Connection) -> Result<()> {
    debug!("Initializing watcher tables");

    // Use scheduler_watchers to avoid collision with meepo-knowledge's watchers table
    // (both crates share the same SQLite file)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS scheduler_watchers (
            id TEXT PRIMARY KEY,
            kind_json TEXT NOT NULL,
            action TEXT NOT NULL,
            reply_channel TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        )",
        [],
    )
    .context("Failed to create scheduler_watchers table")?;

    // Index for querying active watchers
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_sched_watchers_active ON scheduler_watchers(active)",
        [],
    )
    .context("Failed to create scheduler_watchers active index")?;

    // Table for tracking watcher events (audit trail)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS watcher_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            watcher_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            FOREIGN KEY (watcher_id) REFERENCES scheduler_watchers(id) ON DELETE CASCADE
        )",
        [],
    )
    .context("Failed to create watcher_events table")?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_watcher_events_watcher_id ON watcher_events(watcher_id)",
        [],
    )
    .context("Failed to create watcher_events index")?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_watcher_events_timestamp ON watcher_events(timestamp)",
        [],
    )
    .context("Failed to create watcher_events timestamp index")?;

    info!("Watcher tables initialized successfully");
    Ok(())
}

/// Save a watcher to the database
///
/// If a watcher with the same ID exists, it will be updated.
/// Otherwise, a new watcher will be inserted.
pub fn save_watcher(conn: &Connection, watcher: &Watcher) -> Result<()> {
    let kind_json =
        serde_json::to_string(&watcher.kind).context("Failed to serialize watcher kind")?;

    let created_at = watcher.created_at.to_rfc3339();

    conn.execute(
        "INSERT INTO scheduler_watchers (id, kind_json, action, reply_channel, active, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
            kind_json = excluded.kind_json,
            action = excluded.action,
            reply_channel = excluded.reply_channel,
            active = excluded.active",
        params![
            &watcher.id,
            &kind_json,
            &watcher.action,
            &watcher.reply_channel,
            watcher.active as i32,
            &created_at,
        ],
    )
    .context("Failed to save watcher")?;

    debug!("Saved watcher: {} ({})", watcher.id, watcher.action);
    Ok(())
}

/// Get all active watchers from the database
pub fn get_active_watchers(conn: &Connection) -> Result<Vec<Watcher>> {
    let mut stmt = conn
        .prepare("SELECT id, kind_json, action, reply_channel, active, created_at FROM scheduler_watchers WHERE active = 1")
        .context("Failed to prepare query for active watchers")?;

    let watchers: Vec<Watcher> = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let kind_json: String = row.get(1)?;
            let action: String = row.get(2)?;
            let reply_channel: String = row.get(3)?;
            let active: i32 = row.get(4)?;
            let created_at_str: String = row.get(5)?;

            Ok((id, kind_json, action, reply_channel, active, created_at_str))
        })
        .context("Failed to query active watchers")?
        .filter_map(|result| match result {
            Ok((id, kind_json, action, reply_channel, active, created_at_str)) => {
                let kind = match serde_json::from_str(&kind_json) {
                    Ok(k) => k,
                    Err(e) => {
                        warn!("Failed to deserialize watcher kind for {}: {}", id, e);
                        return None;
                    }
                };

                let created_at = match DateTime::parse_from_rfc3339(&created_at_str) {
                    Ok(dt) => dt.with_timezone(&Utc),
                    Err(e) => {
                        warn!("Failed to parse created_at for {}: {}", id, e);
                        Utc::now()
                    }
                };

                Some(Watcher {
                    id,
                    kind,
                    action,
                    reply_channel,
                    active: active != 0,
                    created_at,
                })
            }
            Err(e) => {
                warn!("Failed to read watcher row: {}", e);
                None
            }
        })
        .collect();

    debug!("Retrieved {} active watchers", watchers.len());
    Ok(watchers)
}

/// Get a specific watcher by ID
pub fn get_watcher_by_id(conn: &Connection, id: &str) -> Result<Option<Watcher>> {
    let mut stmt = conn
        .prepare("SELECT id, kind_json, action, reply_channel, active, created_at FROM scheduler_watchers WHERE id = ?1")
        .context("Failed to prepare query for watcher by ID")?;

    let result = stmt.query_row(params![id], |row| {
        let id: String = row.get(0)?;
        let kind_json: String = row.get(1)?;
        let action: String = row.get(2)?;
        let reply_channel: String = row.get(3)?;
        let active: i32 = row.get(4)?;
        let created_at_str: String = row.get(5)?;

        Ok((id, kind_json, action, reply_channel, active, created_at_str))
    });

    match result {
        Ok((id, kind_json, action, reply_channel, active, created_at_str)) => {
            let kind =
                serde_json::from_str(&kind_json).context("Failed to deserialize watcher kind")?;

            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .context("Failed to parse created_at")?
                .with_timezone(&Utc);

            Ok(Some(Watcher {
                id,
                kind,
                action,
                reply_channel,
                active: active != 0,
                created_at,
            }))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e).context("Failed to query watcher by ID"),
    }
}

/// Deactivate a watcher (set active = false)
///
/// This doesn't delete the watcher, just marks it as inactive.
/// The watcher runner should stop running it.
pub fn deactivate_watcher(conn: &Connection, id: &str) -> Result<bool> {
    let rows_affected = conn
        .execute(
            "UPDATE scheduler_watchers SET active = 0 WHERE id = ?1",
            params![id],
        )
        .context("Failed to deactivate watcher")?;

    if rows_affected > 0 {
        info!("Deactivated watcher: {}", id);
        Ok(true)
    } else {
        warn!("Attempted to deactivate non-existent watcher: {}", id);
        Ok(false)
    }
}

/// Permanently delete a watcher from the database
///
/// This also deletes all associated events due to the CASCADE constraint.
pub fn delete_watcher(conn: &Connection, id: &str) -> Result<bool> {
    let rows_affected = conn
        .execute("DELETE FROM scheduler_watchers WHERE id = ?1", params![id])
        .context("Failed to delete watcher")?;

    if rows_affected > 0 {
        info!("Deleted watcher: {}", id);
        Ok(true)
    } else {
        warn!("Attempted to delete non-existent watcher: {}", id);
        Ok(false)
    }
}

/// Save a watcher event to the database (for audit trail)
pub fn save_watcher_event(
    conn: &Connection,
    watcher_id: &str,
    kind: &str,
    payload: &serde_json::Value,
) -> Result<()> {
    let payload_json =
        serde_json::to_string(payload).context("Failed to serialize event payload")?;

    let timestamp = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO watcher_events (watcher_id, kind, payload_json, timestamp)
         VALUES (?1, ?2, ?3, ?4)",
        params![watcher_id, kind, &payload_json, &timestamp],
    )
    .context("Failed to save watcher event")?;

    debug!("Saved event for watcher {}: {}", watcher_id, kind);
    Ok(())
}

/// Get recent events for a watcher
pub fn get_watcher_events(
    conn: &Connection,
    watcher_id: &str,
    limit: usize,
) -> Result<Vec<(String, serde_json::Value, DateTime<Utc>)>> {
    let mut stmt = conn
        .prepare(
            "SELECT kind, payload_json, timestamp FROM watcher_events
             WHERE watcher_id = ?1
             ORDER BY timestamp DESC
             LIMIT ?2",
        )
        .context("Failed to prepare query for watcher events")?;

    let events = stmt
        .query_map(params![watcher_id, limit as i64], |row| {
            let kind: String = row.get(0)?;
            let payload_json: String = row.get(1)?;
            let timestamp_str: String = row.get(2)?;

            Ok((kind, payload_json, timestamp_str))
        })
        .context("Failed to query watcher events")?
        .filter_map(|result| match result {
            Ok((kind, payload_json, timestamp_str)) => {
                let payload = match serde_json::from_str(&payload_json) {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("Failed to deserialize event payload: {}", e);
                        return None;
                    }
                };

                let timestamp = match DateTime::parse_from_rfc3339(&timestamp_str) {
                    Ok(dt) => dt.with_timezone(&Utc),
                    Err(e) => {
                        warn!("Failed to parse event timestamp: {}", e);
                        return None;
                    }
                };

                Some((kind, payload, timestamp))
            }
            Err(e) => {
                warn!("Failed to read event row: {}", e);
                None
            }
        })
        .collect();

    Ok(events)
}

/// Record the last successful run time for a cron watcher.
/// Used for catch-up mechanism (OpenClaw #10403) — when the daemon restarts,
/// it can check if any cron jobs were missed and run them.
pub fn record_last_run(conn: &Connection, watcher_id: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    // Create the table if it doesn't exist (idempotent)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS watcher_last_run (
            watcher_id TEXT PRIMARY KEY,
            last_run_at TEXT NOT NULL,
            consecutive_errors INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )
    .context("Failed to create watcher_last_run table")?;

    conn.execute(
        "INSERT INTO watcher_last_run (watcher_id, last_run_at, consecutive_errors)
         VALUES (?1, ?2, 0)
         ON CONFLICT(watcher_id) DO UPDATE SET
            last_run_at = excluded.last_run_at,
            consecutive_errors = 0",
        params![watcher_id, &now],
    )
    .context("Failed to record last run")?;

    debug!("Recorded last run for watcher {}", watcher_id);
    Ok(())
}

/// Record an error for a cron watcher (increments consecutive error count).
/// After `max_errors` consecutive errors, the watcher is automatically deactivated.
pub fn record_run_error(conn: &Connection, watcher_id: &str, max_errors: u32) -> Result<bool> {
    // Create the table if it doesn't exist (idempotent)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS watcher_last_run (
            watcher_id TEXT PRIMARY KEY,
            last_run_at TEXT NOT NULL,
            consecutive_errors INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )
    .context("Failed to create watcher_last_run table")?;

    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO watcher_last_run (watcher_id, last_run_at, consecutive_errors)
         VALUES (?1, ?2, 1)
         ON CONFLICT(watcher_id) DO UPDATE SET
            last_run_at = excluded.last_run_at,
            consecutive_errors = consecutive_errors + 1",
        params![watcher_id, &now],
    )
    .context("Failed to record run error")?;

    // Check if we've exceeded max errors
    let errors: u32 = conn
        .query_row(
            "SELECT consecutive_errors FROM watcher_last_run WHERE watcher_id = ?1",
            params![watcher_id],
            |row| row.get(0),
        )
        .context("Failed to query consecutive errors")?;

    if errors >= max_errors {
        warn!(
            "Watcher {} has {} consecutive errors (max {}), deactivating",
            watcher_id, errors, max_errors
        );
        deactivate_watcher(conn, watcher_id)?;
        return Ok(true); // deactivated
    }

    debug!(
        "Watcher {} error count: {}/{}",
        watcher_id, errors, max_errors
    );
    Ok(false) // still active
}

/// Get the last run time for a watcher (for catch-up scheduling)
pub fn get_last_run(conn: &Connection, watcher_id: &str) -> Result<Option<DateTime<Utc>>> {
    // Table might not exist yet
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='watcher_last_run'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(None);
    }

    let result = conn.query_row(
        "SELECT last_run_at FROM watcher_last_run WHERE watcher_id = ?1",
        params![watcher_id],
        |row| {
            let ts: String = row.get(0)?;
            Ok(ts)
        },
    );

    match result {
        Ok(ts) => {
            let dt = DateTime::parse_from_rfc3339(&ts)
                .context("Failed to parse last_run_at")?
                .with_timezone(&Utc);
            Ok(Some(dt))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e).context("Failed to query last run"),
    }
}

/// Clean up old watcher events (keep only last N days)
pub fn cleanup_old_events(conn: &Connection, days_to_keep: u32) -> Result<usize> {
    let cutoff = Utc::now() - chrono::Duration::days(days_to_keep as i64);
    let cutoff_str = cutoff.to_rfc3339();

    let rows_deleted = conn
        .execute(
            "DELETE FROM watcher_events WHERE timestamp < ?1",
            params![&cutoff_str],
        )
        .context("Failed to cleanup old events")?;

    if rows_deleted > 0 {
        info!("Cleaned up {} old watcher events", rows_deleted);
    }

    Ok(rows_deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watcher::{Watcher, WatcherKind};

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_watcher_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn test_save_and_load_watcher() {
        let conn = setup_test_db();

        let watcher = Watcher::new(
            WatcherKind::EmailWatch {
                from: Some("test@example.com".to_string()),
                subject_contains: None,
                interval_secs: 300,
            },
            "Test action".to_string(),
            "test-channel".to_string(),
        );

        save_watcher(&conn, &watcher).unwrap();

        let loaded = get_watcher_by_id(&conn, &watcher.id).unwrap().unwrap();
        assert_eq!(loaded.id, watcher.id);
        assert_eq!(loaded.action, watcher.action);
        assert_eq!(loaded.reply_channel, watcher.reply_channel);
    }

    #[test]
    fn test_get_active_watchers() {
        let conn = setup_test_db();

        let watcher1 = Watcher::new(
            WatcherKind::FileWatch {
                path: "/tmp/test".to_string(),
            },
            "Watch test file".to_string(),
            "alerts".to_string(),
        );

        let mut watcher2 = Watcher::new(
            WatcherKind::CalendarWatch {
                lookahead_hours: 24,
                interval_secs: 600,
            },
            "Calendar check".to_string(),
            "calendar".to_string(),
        );
        watcher2.active = false;

        save_watcher(&conn, &watcher1).unwrap();
        save_watcher(&conn, &watcher2).unwrap();

        let active = get_active_watchers(&conn).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, watcher1.id);
    }

    #[test]
    fn test_deactivate_watcher() {
        let conn = setup_test_db();

        let watcher = Watcher::new(
            WatcherKind::FileWatch {
                path: "/tmp/test".to_string(),
            },
            "Test".to_string(),
            "test".to_string(),
        );

        save_watcher(&conn, &watcher).unwrap();
        assert!(deactivate_watcher(&conn, &watcher.id).unwrap());

        let loaded = get_watcher_by_id(&conn, &watcher.id).unwrap().unwrap();
        assert!(!loaded.active);
    }

    #[test]
    fn test_delete_watcher() {
        let conn = setup_test_db();

        let watcher = Watcher::new(
            WatcherKind::FileWatch {
                path: "/tmp/test".to_string(),
            },
            "Test".to_string(),
            "test".to_string(),
        );

        save_watcher(&conn, &watcher).unwrap();
        assert!(delete_watcher(&conn, &watcher.id).unwrap());

        let loaded = get_watcher_by_id(&conn, &watcher.id).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_record_last_run() {
        let conn = setup_test_db();
        let watcher = Watcher::new(
            WatcherKind::FileWatch {
                path: "/tmp".to_string(),
            },
            "Test".to_string(),
            "test".to_string(),
        );
        save_watcher(&conn, &watcher).unwrap();

        // No last run initially
        assert!(get_last_run(&conn, &watcher.id).unwrap().is_none());

        // Record a run
        record_last_run(&conn, &watcher.id).unwrap();
        let last = get_last_run(&conn, &watcher.id).unwrap();
        assert!(last.is_some());
    }

    #[test]
    fn test_record_run_error_deactivates() {
        let conn = setup_test_db();
        let watcher = Watcher::new(
            WatcherKind::FileWatch {
                path: "/tmp".to_string(),
            },
            "Test".to_string(),
            "test".to_string(),
        );
        save_watcher(&conn, &watcher).unwrap();

        // 3 errors with max_errors=3 should deactivate
        assert!(!record_run_error(&conn, &watcher.id, 3).unwrap());
        assert!(!record_run_error(&conn, &watcher.id, 3).unwrap());
        assert!(record_run_error(&conn, &watcher.id, 3).unwrap()); // deactivated

        let loaded = get_watcher_by_id(&conn, &watcher.id).unwrap().unwrap();
        assert!(!loaded.active);
    }

    #[test]
    fn test_record_last_run_resets_errors() {
        let conn = setup_test_db();
        let watcher = Watcher::new(
            WatcherKind::FileWatch {
                path: "/tmp".to_string(),
            },
            "Test".to_string(),
            "test".to_string(),
        );
        save_watcher(&conn, &watcher).unwrap();

        // Record 2 errors
        record_run_error(&conn, &watcher.id, 5).unwrap();
        record_run_error(&conn, &watcher.id, 5).unwrap();

        // Successful run resets error count
        record_last_run(&conn, &watcher.id).unwrap();

        // Should need 5 more errors to deactivate
        for _ in 0..4 {
            assert!(!record_run_error(&conn, &watcher.id, 5).unwrap());
        }
        assert!(record_run_error(&conn, &watcher.id, 5).unwrap());
    }

    #[test]
    fn test_save_and_retrieve_events() {
        let conn = setup_test_db();

        let watcher = Watcher::new(
            WatcherKind::FileWatch {
                path: "/tmp".to_string(),
            },
            "Test".to_string(),
            "test".to_string(),
        );

        save_watcher(&conn, &watcher).unwrap();

        let payload = serde_json::json!({
            "file": "test.txt",
            "change": "modified"
        });

        save_watcher_event(&conn, &watcher.id, "file_changed", &payload).unwrap();

        let events = get_watcher_events(&conn, &watcher.id, 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "file_changed");
    }

    #[test]
    fn test_deactivate_nonexistent() {
        let conn = setup_test_db();
        let result = deactivate_watcher(&conn, "nonexistent").unwrap();
        assert!(!result);
    }

    #[test]
    fn test_delete_nonexistent() {
        let conn = setup_test_db();
        let result = delete_watcher(&conn, "nonexistent").unwrap();
        assert!(!result);
    }

    #[test]
    fn test_cleanup_old_events_empty() {
        let conn = setup_test_db();
        let deleted = cleanup_old_events(&conn, 30).unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn test_get_watcher_events_limit() {
        let conn = setup_test_db();
        let watcher = Watcher::new(
            WatcherKind::FileWatch {
                path: "/tmp".to_string(),
            },
            "Test".to_string(),
            "test".to_string(),
        );
        save_watcher(&conn, &watcher).unwrap();

        for i in 0..5 {
            save_watcher_event(
                &conn,
                &watcher.id,
                "file_changed",
                &serde_json::json!({"idx": i}),
            )
            .unwrap();
        }

        let events = get_watcher_events(&conn, &watcher.id, 3).unwrap();
        assert_eq!(events.len(), 3);

        let all = get_watcher_events(&conn, &watcher.id, 100).unwrap();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn test_get_last_run_nonexistent() {
        let conn = setup_test_db();
        let last = get_last_run(&conn, "nonexistent").unwrap();
        assert!(last.is_none());
    }

    #[test]
    fn test_init_tables_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_watcher_tables(&conn).unwrap();
        init_watcher_tables(&conn).unwrap(); // should not error
    }

    #[test]
    fn test_save_watcher_all_kinds() {
        let conn = setup_test_db();

        let kinds = vec![
            WatcherKind::EmailWatch {
                from: Some("a@b.com".to_string()),
                subject_contains: Some("urgent".to_string()),
                interval_secs: 60,
            },
            WatcherKind::CalendarWatch {
                lookahead_hours: 12,
                interval_secs: 300,
            },
            WatcherKind::GitHubWatch {
                repo: "user/repo".to_string(),
                events: vec!["push".to_string()],
                github_token: None,
                interval_secs: 120,
            },
            WatcherKind::FileWatch {
                path: "/tmp/watch".to_string(),
            },
            WatcherKind::MessageWatch {
                keyword: "deploy".to_string(),
            },
            WatcherKind::Scheduled {
                cron_expr: "0 * * * *".to_string(),
                task: "hourly check".to_string(),
            },
        ];

        for (i, kind) in kinds.into_iter().enumerate() {
            let watcher = Watcher::new(kind, format!("Action {}", i), "ch".to_string());
            save_watcher(&conn, &watcher).unwrap();
            let loaded = get_watcher_by_id(&conn, &watcher.id).unwrap().unwrap();
            assert_eq!(loaded.action, format!("Action {}", i));
        }

        let active = get_active_watchers(&conn).unwrap();
        assert_eq!(active.len(), 6);
    }
}
