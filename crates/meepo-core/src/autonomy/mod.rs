//! Autonomous agent loop — observe/think/act/reflect cycle
//!
//! Replaces the reactive message handler with a continuous tick-based loop.
//! User messages are just one input among many — the agent also processes
//! watcher events, evaluates goals, and takes proactive actions.

pub mod goals;
pub mod user_model;
pub mod action_log;
pub mod planner;

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Notify};
use tracing::{info, error, debug};

use crate::agent::Agent;
use crate::types::{IncomingMessage, MessageKind, OutgoingMessage, ChannelType};
use meepo_knowledge::KnowledgeDb;
use meepo_scheduler::WatcherEvent;

/// Configuration for the autonomous loop
#[derive(Debug, Clone)]
pub struct AutonomyConfig {
    pub enabled: bool,
    pub tick_interval_secs: u64,
    pub max_goals: usize,
    /// Send acknowledgment/typing indicators before processing messages
    pub send_acknowledgments: bool,
}

/// Input that the autonomous loop processes each tick
#[derive(Debug)]
enum LoopInput {
    UserMessage(IncomingMessage),
    WatcherEvent(WatcherEvent),
}

/// The autonomous loop that drives the agent
pub struct AutonomousLoop {
    agent: Arc<Agent>,
    db: Arc<KnowledgeDb>,
    config: AutonomyConfig,

    /// Receives user messages from channels
    message_rx: mpsc::Receiver<IncomingMessage>,

    /// Receives watcher events from the scheduler
    watcher_rx: mpsc::UnboundedReceiver<WatcherEvent>,

    /// Sends responses back to channels
    response_tx: mpsc::Sender<OutgoingMessage>,

    /// Notified when a new input arrives (to wake the loop immediately)
    wake: Arc<Notify>,
}

impl AutonomousLoop {
    pub fn new(
        agent: Arc<Agent>,
        db: Arc<KnowledgeDb>,
        config: AutonomyConfig,
        message_rx: mpsc::Receiver<IncomingMessage>,
        watcher_rx: mpsc::UnboundedReceiver<WatcherEvent>,
        response_tx: mpsc::Sender<OutgoingMessage>,
        wake: Arc<Notify>,
    ) -> Self {
        Self {
            agent,
            db,
            config,
            message_rx,
            watcher_rx,
            response_tx,
            wake,
        }
    }

    /// Create a Notify handle that can be shared with message producers
    /// to wake the loop immediately when new inputs arrive.
    pub fn create_wake_handle() -> Arc<Notify> {
        Arc::new(Notify::new())
    }

    /// Run the autonomous loop until cancelled
    pub async fn run(mut self, cancel: tokio_util::sync::CancellationToken) {
        info!("Autonomous loop started (tick interval: {}s)", self.config.tick_interval_secs);

        let tick_duration = Duration::from_secs(self.config.tick_interval_secs);

        loop {
            // Wait for: cancellation, tick timer, or wake signal
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Autonomous loop shutting down");
                    break;
                }
                _ = tokio::time::sleep(tick_duration) => {
                    // Periodic tick — check for due goals and process any pending inputs
                }
                _ = self.wake.notified() => {
                    // Immediate wake — new input arrived
                    debug!("Autonomous loop woken by new input");
                }
            }

            // OBSERVE: drain all pending inputs
            let inputs = self.drain_inputs();

            // Check for due goals
            let due_goals = match self.db.get_due_goals().await {
                Ok(goals) => goals,
                Err(e) => {
                    error!("Failed to get due goals: {}", e);
                    vec![]
                }
            };

            // Skip tick if nothing to do
            if inputs.is_empty() && due_goals.is_empty() {
                continue;
            }

            debug!(
                "Tick: {} inputs, {} due goals",
                inputs.len(),
                due_goals.len()
            );

            // THINK + ACT: process inputs
            // For now, handle user messages via the existing Agent::handle_message path.
            // Goal evaluation will be added in Step 2.
            for input in inputs {
                match input {
                    LoopInput::UserMessage(msg) => {
                        self.handle_user_message(msg).await;
                    }
                    LoopInput::WatcherEvent(event) => {
                        self.handle_watcher_event(event).await;
                    }
                }
            }

            // Mark due goals as checked (placeholder — real evaluation in Step 2)
            for goal in &due_goals {
                if let Err(e) = self.db.update_goal_checked(&goal.id, None).await {
                    error!("Failed to mark goal {} as checked: {}", goal.id, e);
                }
            }
        }
    }

    /// Drain all pending inputs from channels without blocking
    fn drain_inputs(&mut self) -> Vec<LoopInput> {
        let mut inputs = Vec::new();

        // Drain user messages
        while let Ok(msg) = self.message_rx.try_recv() {
            inputs.push(LoopInput::UserMessage(msg));
        }

        // Drain watcher events
        while let Ok(event) = self.watcher_rx.try_recv() {
            inputs.push(LoopInput::WatcherEvent(event));
        }

        inputs
    }

    /// Handle a user message through the existing agent path
    async fn handle_user_message(&self, msg: IncomingMessage) {
        let channel = msg.channel.clone();
        info!("Processing user message from {} on {}", msg.sender, channel);

        // Send acknowledgment so the user knows we're working on it
        if self.config.send_acknowledgments {
            let ack = OutgoingMessage {
                content: String::new(), // each channel decides what to show
                channel: msg.channel.clone(),
                reply_to: Some(msg.id.clone()),
                kind: MessageKind::Acknowledgment,
            };
            let _ = self.response_tx.send(ack).await;
        }

        match self.agent.handle_message(msg).await {
            Ok(response) => {
                if let Err(e) = self.response_tx.send(response).await {
                    error!("Failed to send response: {}", e);
                }
            }
            Err(e) => error!("Agent error: {}", e),
        }
    }

    /// Handle a watcher event — look up the watcher's reply_channel and action,
    /// then route the agent's response to the correct channel.
    async fn handle_watcher_event(&self, event: WatcherEvent) {
        info!("Processing watcher event: {} from {}", event.kind, event.watcher_id);

        // Look up the watcher to get reply_channel and action
        let (reply_channel, action) = match self.db.get_watcher(&event.watcher_id).await {
            Ok(Some(w)) => (
                ChannelType::from_string(&w.reply_channel),
                w.action,
            ),
            Ok(None) => {
                error!("Watcher {} not found in database", event.watcher_id);
                (ChannelType::Internal, String::new())
            }
            Err(e) => {
                error!("Failed to look up watcher {}: {}", event.watcher_id, e);
                (ChannelType::Internal, String::new())
            }
        };

        // Build prompt with the watcher's action context
        let content = if action.is_empty() {
            format!("Watcher {} triggered: {}", event.watcher_id, event.payload)
        } else {
            format!(
                "Watcher {} triggered: {}\nYour requested action: {}",
                event.watcher_id, event.payload, action
            )
        };

        let msg = IncomingMessage {
            id: uuid::Uuid::new_v4().to_string(),
            sender: "watcher".to_string(),
            content,
            channel: reply_channel.clone(),
            timestamp: chrono::Utc::now(),
        };

        match self.agent.handle_message(msg).await {
            Ok(mut response) => {
                // Route response to the watcher's reply_channel
                response.channel = reply_channel;
                if let Err(e) = self.response_tx.send(response).await {
                    error!("Failed to send watcher response: {}", e);
                }
            }
            Err(e) => error!("Failed to handle watcher event: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiClient;
    use crate::tools::ToolRegistry;
    use tempfile::TempDir;

    fn setup() -> (Arc<Agent>, Arc<KnowledgeDb>, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Arc::new(KnowledgeDb::new(&db_path).unwrap());
        let api = ApiClient::new("test-key".to_string(), None);
        let tools = Arc::new(ToolRegistry::new());
        let agent = Arc::new(Agent::new(api, tools, "test soul".into(), "test memory".into(), db.clone()));
        (agent, db, temp_dir)
    }

    #[tokio::test]
    async fn test_drain_inputs_empty() {
        let (agent, db, _tmp) = setup();
        let (_, msg_rx) = mpsc::channel(16);
        let (_, watcher_rx) = mpsc::unbounded_channel();
        let (resp_tx, _) = mpsc::channel(16);
        let wake = AutonomousLoop::create_wake_handle();

        let mut loop_ = AutonomousLoop::new(
            agent, db,
            AutonomyConfig { enabled: true, tick_interval_secs: 30, max_goals: 50, send_acknowledgments: true },
            msg_rx, watcher_rx, resp_tx, wake,
        );

        let inputs = loop_.drain_inputs();
        assert!(inputs.is_empty());
    }

    #[tokio::test]
    async fn test_drain_inputs_with_messages() {
        let (agent, db, _tmp) = setup();
        let (msg_tx, msg_rx) = mpsc::channel(16);
        let (_, watcher_rx) = mpsc::unbounded_channel();
        let (resp_tx, _) = mpsc::channel(16);
        let wake = AutonomousLoop::create_wake_handle();

        // Send a message before creating the loop
        msg_tx.send(IncomingMessage {
            id: "test-1".into(),
            sender: "user".into(),
            content: "hello".into(),
            channel: ChannelType::Discord,
            timestamp: chrono::Utc::now(),
        }).await.unwrap();

        let mut loop_ = AutonomousLoop::new(
            agent, db,
            AutonomyConfig { enabled: true, tick_interval_secs: 30, max_goals: 50, send_acknowledgments: true },
            msg_rx, watcher_rx, resp_tx, wake,
        );

        let inputs = loop_.drain_inputs();
        assert_eq!(inputs.len(), 1);
    }
}
