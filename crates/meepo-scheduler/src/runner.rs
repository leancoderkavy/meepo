//! Tokio task runner for watchers
//!
//! This module manages the lifecycle of watcher tasks, spawning them as
//! tokio tasks and coordinating their execution.

use crate::watcher::{Watcher, WatcherEvent, WatcherKind};
use anyhow::{Context, Result};
use chrono::{NaiveTime, Utc};
use std::str::FromStr;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{sleep_until, Instant};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Configuration for the watcher runner
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Maximum number of concurrent watchers
    pub max_concurrent_watchers: usize,

    /// Minimum polling interval in seconds (enforced for all polling watchers)
    pub min_poll_interval_secs: u64,

    /// Active hours range (if set, polling watchers pause outside this range)
    pub active_hours: Option<(NaiveTime, NaiveTime)>,

    /// Whether to enforce active hours check
    pub enforce_active_hours: bool,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            max_concurrent_watchers: 100,
            min_poll_interval_secs: 10,
            active_hours: None,
            enforce_active_hours: false,
        }
    }
}

/// Manages the lifecycle of watcher tasks
pub struct WatcherRunner {
    /// Configuration
    config: WatcherConfig,

    /// Channel for emitting watcher events
    event_tx: mpsc::UnboundedSender<WatcherEvent>,

    /// Active watcher tasks (watcher_id -> CancellationToken)
    active_tasks: Arc<RwLock<HashMap<String, CancellationToken>>>,

    /// Global shutdown token
    shutdown_token: CancellationToken,
}

impl WatcherRunner {
    /// Create a new watcher runner
    pub fn new(event_tx: mpsc::UnboundedSender<WatcherEvent>) -> Self {
        Self::with_config(event_tx, WatcherConfig::default())
    }

    /// Create a new watcher runner with custom configuration
    pub fn with_config(
        event_tx: mpsc::UnboundedSender<WatcherEvent>,
        config: WatcherConfig,
    ) -> Self {
        Self {
            config,
            event_tx,
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            shutdown_token: CancellationToken::new(),
        }
    }

    /// Start a watcher
    pub async fn start_watcher(&self, watcher: Watcher) -> Result<()> {
        // Check if we've reached max concurrent watchers
        let active_count = self.active_tasks.read().await.len();
        if active_count >= self.config.max_concurrent_watchers {
            anyhow::bail!(
                "Maximum concurrent watchers reached: {}",
                self.config.max_concurrent_watchers
            );
        }

        // Check if already running
        if self.active_tasks.read().await.contains_key(&watcher.id) {
            warn!("Watcher {} is already running", watcher.id);
            return Ok(());
        }

        info!("Starting watcher: {} ({})", watcher.id, watcher.description());

        // Create cancellation token for this watcher
        let token = CancellationToken::new();

        // Store the token
        self.active_tasks
            .write()
            .await
            .insert(watcher.id.clone(), token.clone());

        // Spawn the appropriate task based on watcher kind
        match &watcher.kind {
            WatcherKind::EmailWatch { .. }
            | WatcherKind::CalendarWatch { .. }
            | WatcherKind::GitHubWatch { .. } => {
                self.spawn_polling_watcher(watcher, token).await?;
            }
            WatcherKind::FileWatch { .. } => {
                self.spawn_file_watcher(watcher, token).await?;
            }
            WatcherKind::MessageWatch { .. } => {
                // Message watchers are handled externally by the message handler
                // We just track that they're active
                info!(
                    "Message watcher {} registered (handled externally)",
                    watcher.id
                );
            }
            WatcherKind::Scheduled { .. } => {
                self.spawn_scheduled_watcher(watcher, token).await?;
            }
            WatcherKind::OneShot { .. } => {
                self.spawn_oneshot_watcher(watcher, token).await?;
            }
        }

        Ok(())
    }

    /// Stop a specific watcher
    pub async fn stop_watcher(&self, id: &str) -> Result<bool> {
        let mut tasks = self.active_tasks.write().await;

        if let Some(token) = tasks.remove(id) {
            info!("Stopping watcher: {}", id);
            token.cancel();
            Ok(true)
        } else {
            warn!("Attempted to stop non-running watcher: {}", id);
            Ok(false)
        }
    }

    /// Stop all watchers
    pub async fn stop_all(&self) {
        info!("Stopping all watchers");

        // Cancel global shutdown token
        self.shutdown_token.cancel();

        // Cancel all individual watcher tokens
        let mut tasks = self.active_tasks.write().await;
        for (id, token) in tasks.drain() {
            debug!("Cancelling watcher: {}", id);
            token.cancel();
        }

        info!("All watchers stopped");
    }

    /// Get the number of active watchers
    pub async fn active_count(&self) -> usize {
        self.active_tasks.read().await.len()
    }

    /// Check if a watcher is currently running
    pub async fn is_running(&self, id: &str) -> bool {
        self.active_tasks.read().await.contains_key(id)
    }

    /// Check if we're within active hours
    fn is_within_active_hours(&self) -> bool {
        if !self.config.enforce_active_hours {
            return true;
        }

        let Some((start, end)) = self.config.active_hours else {
            return true;
        };

        let now = Utc::now().time();

        if start < end {
            // Normal case: e.g., 9:00 - 17:00
            now >= start && now <= end
        } else {
            // Overnight case: e.g., 22:00 - 06:00
            now >= start || now <= end
        }
    }

    /// Spawn a polling-based watcher task
    async fn spawn_polling_watcher(
        &self,
        watcher: Watcher,
        cancel_token: CancellationToken,
    ) -> Result<()> {
        let event_tx = self.event_tx.clone();
        let config = self.config.clone();
        let global_shutdown = self.shutdown_token.clone();
        let active_tasks = self.active_tasks.clone();

        tokio::spawn(async move {
            let interval_secs = match &watcher.kind {
                WatcherKind::EmailWatch { interval_secs, .. } => *interval_secs,
                WatcherKind::CalendarWatch { interval_secs, .. } => *interval_secs,
                WatcherKind::GitHubWatch { interval_secs, .. } => *interval_secs,
                _ => unreachable!(),
            };

            // Enforce minimum interval
            let interval_secs = interval_secs.max(config.min_poll_interval_secs);
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            debug!(
                "Polling watcher {} started with interval {}s",
                watcher.id, interval_secs
            );

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        info!("Watcher {} cancelled", watcher.id);
                        break;
                    }
                    _ = global_shutdown.cancelled() => {
                        info!("Watcher {} stopped due to global shutdown", watcher.id);
                        break;
                    }
                    _ = interval.tick() => {
                        // Check active hours
                        if config.enforce_active_hours {
                            if let Some((start, end)) = config.active_hours {
                                let now = Utc::now().time();
                                let is_active = if start < end {
                                    now >= start && now <= end
                                } else {
                                    now >= start || now <= end
                                };

                                if !is_active {
                                    debug!("Watcher {} paused outside active hours", watcher.id);
                                    continue;
                                }
                            }
                        }

                        // Execute the poll
                        if let Err(e) = poll_watcher(&watcher, &event_tx).await {
                            error!("Error polling watcher {}: {}", watcher.id, e);
                        }
                    }
                }
            }

            // Clean up
            active_tasks.write().await.remove(&watcher.id);
            debug!("Polling watcher {} task ended", watcher.id);
        });

        Ok(())
    }

    /// Spawn a file watcher task
    async fn spawn_file_watcher(
        &self,
        watcher: Watcher,
        cancel_token: CancellationToken,
    ) -> Result<()> {
        let path = match &watcher.kind {
            WatcherKind::FileWatch { path } => path.clone(),
            _ => unreachable!(),
        };
        let event_tx = self.event_tx.clone();
        let watcher_id = watcher.id.clone();
        let global_shutdown = self.shutdown_token.clone();
        let active_tasks = self.active_tasks.clone();

        tokio::spawn(async move {
            // Create a channel for file events
            let (tx, mut rx) = mpsc::unbounded_channel();

            // Create the file watcher
            let mut file_watcher: RecommendedWatcher = match notify::recommended_watcher(
                move |res: Result<Event, notify::Error>| match res {
                    Ok(event) => {
                        if tx.send(event).is_err() {
                            error!("Failed to send file event");
                        }
                    }
                    Err(e) => error!("File watch error: {:?}", e),
                },
            ) {
                Ok(w) => w,
                Err(e) => {
                    error!("Failed to create file watcher for {}: {}", path, e);
                    return;
                }
            };

            // Start watching
            if let Err(e) = file_watcher.watch(Path::new(&path), RecursiveMode::Recursive) {
                error!("Failed to watch path {}: {}", path, e);
                return;
            }

            info!("File watcher started for: {}", path);

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        info!("File watcher {} cancelled", watcher_id);
                        break;
                    }
                    _ = global_shutdown.cancelled() => {
                        info!("File watcher {} stopped due to global shutdown", watcher_id);
                        break;
                    }
                    Some(event) = rx.recv() => {
                        debug!("File event for {}: {:?}", watcher_id, event);

                        let change_type = match event.kind {
                            notify::EventKind::Create(_) => "created",
                            notify::EventKind::Modify(_) => "modified",
                            notify::EventKind::Remove(_) => "removed",
                            _ => "changed",
                        };

                        for path in event.paths {
                            let watcher_event = WatcherEvent::file_changed(
                                watcher_id.clone(),
                                path.to_string_lossy().to_string(),
                                change_type.to_string(),
                            );

                            if let Err(e) = event_tx.send(watcher_event) {
                                error!("Failed to send watcher event: {}", e);
                            }
                        }
                    }
                }
            }

            // Clean up
            active_tasks.write().await.remove(&watcher_id);
            debug!("File watcher {} task ended", watcher_id);
        });

        Ok(())
    }

    /// Spawn a scheduled (cron) watcher task
    async fn spawn_scheduled_watcher(
        &self,
        watcher: Watcher,
        cancel_token: CancellationToken,
    ) -> Result<()> {
        let (cron_expr, task) = match &watcher.kind {
            WatcherKind::Scheduled { cron_expr, task } => (cron_expr.clone(), task.clone()),
            _ => unreachable!(),
        };

        // Parse cron expression
        let schedule = cron::Schedule::from_str(&cron_expr)
            .with_context(|| format!("Invalid cron expression: {}", cron_expr))?;

        let event_tx = self.event_tx.clone();
        let watcher_id = watcher.id.clone();
        let task_name = task.clone();
        let global_shutdown = self.shutdown_token.clone();
        let active_tasks = self.active_tasks.clone();

        tokio::spawn(async move {
            info!("Scheduled watcher {} started: {}", watcher_id, cron_expr);

            loop {
                // Get next occurrence
                let now = Utc::now();
                let next = match schedule.after(&now).next() {
                    Some(n) => n,
                    None => {
                        error!("No next occurrence for cron expression");
                        break;
                    }
                };

                let duration: Duration = (next - now).to_std().unwrap_or(Duration::from_secs(60));
                let wake_time = Instant::now() + duration;

                debug!(
                    "Scheduled watcher {} next run at {} (in {:?})",
                    watcher_id, next, duration
                );

                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        info!("Scheduled watcher {} cancelled", watcher_id);
                        break;
                    }
                    _ = global_shutdown.cancelled() => {
                        info!("Scheduled watcher {} stopped due to global shutdown", watcher_id);
                        break;
                    }
                    _ = sleep_until(wake_time) => {
                        // Execute the task
                        let watcher_event = WatcherEvent::task(
                            watcher_id.clone(),
                            task_name.clone(),
                        );

                        if let Err(e) = event_tx.send(watcher_event) {
                            error!("Failed to send scheduled task event: {}", e);
                        } else {
                            info!("Scheduled task '{}' triggered", task_name);
                        }
                    }
                }
            }

            // Clean up
            active_tasks.write().await.remove(&watcher_id);
            debug!("Scheduled watcher {} task ended", watcher_id);
        });

        Ok(())
    }

    /// Spawn a one-shot watcher task
    async fn spawn_oneshot_watcher(
        &self,
        watcher: Watcher,
        cancel_token: CancellationToken,
    ) -> Result<()> {
        let (target_time, task_name) = match &watcher.kind {
            WatcherKind::OneShot { at, task } => (*at, task.clone()),
            _ => unreachable!(),
        };
        let event_tx = self.event_tx.clone();
        let watcher_id = watcher.id.clone();
        let global_shutdown = self.shutdown_token.clone();
        let active_tasks = self.active_tasks.clone();

        tokio::spawn(async move {
            let now = Utc::now();

            if target_time <= now {
                warn!(
                    "One-shot watcher {} target time {} is in the past",
                    watcher_id, target_time
                );
                // Execute immediately
                let watcher_event = WatcherEvent::task(watcher_id.clone(), task_name.clone());

                if let Err(e) = event_tx.send(watcher_event) {
                    error!("Failed to send one-shot task event: {}", e);
                }

                active_tasks.write().await.remove(&watcher_id);
                return;
            }

            let duration = (target_time - now).to_std().unwrap_or(Duration::from_secs(0));
            let wake_time = Instant::now() + duration;

            info!(
                "One-shot watcher {} scheduled for {} (in {:?})",
                watcher_id, target_time, duration
            );

            tokio::select! {
                _ = cancel_token.cancelled() => {
                    info!("One-shot watcher {} cancelled", watcher_id);
                }
                _ = global_shutdown.cancelled() => {
                    info!("One-shot watcher {} stopped due to global shutdown", watcher_id);
                }
                _ = sleep_until(wake_time) => {
                    // Execute the task
                    let watcher_event = WatcherEvent::task(
                        watcher_id.clone(),
                        task_name.clone(),
                    );

                    if let Err(e) = event_tx.send(watcher_event) {
                        error!("Failed to send one-shot task event: {}", e);
                    } else {
                        info!("One-shot task '{}' triggered", task_name);
                    }
                }
            }

            // Clean up (one-shot always removes itself)
            active_tasks.write().await.remove(&watcher_id);
            debug!("One-shot watcher {} task ended", watcher_id);
        });

        Ok(())
    }
}

/// Poll a watcher (placeholder implementation)
async fn poll_watcher(
    watcher: &Watcher,
    _event_tx: &mpsc::UnboundedSender<WatcherEvent>,
) -> Result<()> {
    match &watcher.kind {
        WatcherKind::EmailWatch {
            from,
            subject_contains,
            ..
        } => {
            debug!(
                "Polling email watcher {} (from: {:?}, subject: {:?})",
                watcher.id, from, subject_contains
            );

            // TODO: Implement actual email polling
            // For now, just log that we would check
            // In a real implementation, this would:
            // 1. Connect to email server (IMAP)
            // 2. Search for emails matching criteria
            // 3. Emit events for new emails
        }
        WatcherKind::CalendarWatch {
            lookahead_hours, ..
        } => {
            debug!(
                "Polling calendar watcher {} (lookahead: {}h)",
                watcher.id, lookahead_hours
            );

            // TODO: Implement actual calendar polling
            // For now, just log that we would check
            // In a real implementation, this would:
            // 1. Connect to calendar API (Google Calendar, etc.)
            // 2. Fetch events within lookahead window
            // 3. Emit events for upcoming meetings
        }
        WatcherKind::GitHubWatch { repo, events, .. } => {
            debug!(
                "Polling GitHub watcher {} (repo: {}, events: {:?})",
                watcher.id, repo, events
            );

            // TODO: Implement actual GitHub polling
            // For now, just log that we would check
            // In a real implementation, this would:
            // 1. Call GitHub API for repository events
            // 2. Filter by requested event types
            // 3. Emit events for new activity
        }
        _ => {
            // Non-polling watcher types should not reach here
            warn!(
                "poll_watcher called on non-polling watcher: {}",
                watcher.id
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watcher::{Watcher, WatcherKind};

    #[tokio::test]
    async fn test_runner_creation() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let runner = WatcherRunner::new(tx);

        assert_eq!(runner.active_count().await, 0);
    }

    #[tokio::test]
    async fn test_start_stop_watcher() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let runner = WatcherRunner::new(tx);

        let watcher = Watcher::new(
            WatcherKind::EmailWatch {
                from: None,
                subject_contains: None,
                interval_secs: 60,
            },
            "Test".to_string(),
            "test".to_string(),
        );

        let watcher_id = watcher.id.clone();

        runner.start_watcher(watcher).await.unwrap();
        assert_eq!(runner.active_count().await, 1);
        assert!(runner.is_running(&watcher_id).await);

        runner.stop_watcher(&watcher_id).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(runner.active_count().await, 0);
        assert!(!runner.is_running(&watcher_id).await);
    }

    #[tokio::test]
    async fn test_stop_all_watchers() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let runner = WatcherRunner::new(tx);

        for i in 0..3 {
            let watcher = Watcher::new(
                WatcherKind::EmailWatch {
                    from: None,
                    subject_contains: None,
                    interval_secs: 60,
                },
                format!("Test {}", i),
                "test".to_string(),
            );

            runner.start_watcher(watcher).await.unwrap();
        }

        assert_eq!(runner.active_count().await, 3);

        runner.stop_all().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(runner.active_count().await, 0);
    }

    #[tokio::test]
    async fn test_oneshot_watcher_immediate_execution() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let runner = WatcherRunner::new(tx);

        // Create a one-shot watcher in the past (should execute immediately)
        let past_time = Utc::now() - chrono::Duration::seconds(10);
        let watcher = Watcher::new(
            WatcherKind::OneShot {
                at: past_time,
                task: "Immediate task".to_string(),
            },
            "Test immediate".to_string(),
            "test".to_string(),
        );

        runner.start_watcher(watcher).await.unwrap();

        // Should receive event almost immediately
        let event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("Timeout waiting for event")
            .expect("Channel closed");

        assert_eq!(event.kind, "task_triggered");
    }

    #[tokio::test]
    async fn test_max_concurrent_watchers() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let config = WatcherConfig {
            max_concurrent_watchers: 2,
            ..Default::default()
        };
        let runner = WatcherRunner::with_config(tx, config);

        // Start 2 watchers - should succeed
        for i in 0..2 {
            let watcher = Watcher::new(
                WatcherKind::EmailWatch {
                    from: None,
                    subject_contains: None,
                    interval_secs: 60,
                },
                format!("Test {}", i),
                "test".to_string(),
            );

            runner.start_watcher(watcher).await.unwrap();
        }

        assert_eq!(runner.active_count().await, 2);

        // Try to start a 3rd - should fail
        let watcher3 = Watcher::new(
            WatcherKind::EmailWatch {
                from: None,
                subject_contains: None,
                interval_secs: 60,
            },
            "Test 3".to_string(),
            "test".to_string(),
        );

        let result = runner.start_watcher(watcher3).await;
        assert!(result.is_err());
    }
}
