//! Autonomous agent loop — observe/think/act/reflect cycle
//!
//! Replaces the reactive message handler with a continuous tick-based loop.
//! User messages are just one input among many — the agent also processes
//! watcher events, evaluates goals, and takes proactive actions.

pub mod action_log;
pub mod goals;
pub mod planner;
pub mod user_model;

use std::sync::Arc;
use std::time::{Duration, Instant};
use chrono::{Datelike, NaiveDate, Timelike, Utc};
use tokio::sync::{Notify, mpsc};
use tracing::{debug, error, info, warn};

use crate::agent::Agent;
use crate::notifications::{NotificationService, NotifyEvent};
use crate::types::{ChannelType, IncomingMessage, MessageKind, OutgoingMessage};
use meepo_knowledge::KnowledgeDb;
use meepo_scheduler::WatcherEvent;

use self::goals::GoalEvaluator;
use self::user_model::UserModel;

/// Configuration for the autonomous loop
#[derive(Debug, Clone)]
pub struct AutonomyConfig {
    pub enabled: bool,
    pub tick_interval_secs: u64,
    pub max_goals: usize,
    /// Send acknowledgment/typing indicators before processing messages
    pub send_acknowledgments: bool,
    /// Hour (0-23) at which to generate the daily plan (default: 7)
    pub daily_plan_hour: u32,
    /// Max autonomous API calls per minute (0 = unlimited)
    pub max_calls_per_minute: u32,
}

/// Simple sliding-window rate limiter for autonomous API calls
struct RateLimiter {
    /// Timestamps of recent calls within the window
    calls: Vec<Instant>,
    /// Maximum calls allowed per window
    max_calls: u32,
    /// Window duration
    window: Duration,
}

impl RateLimiter {
    fn new(max_calls: u32, window: Duration) -> Self {
        Self {
            calls: Vec::new(),
            max_calls,
            window,
        }
    }

    /// Check if a call is allowed. If yes, record it and return true.
    fn try_acquire(&mut self) -> bool {
        if self.max_calls == 0 {
            return true; // unlimited
        }

        let now = Instant::now();
        // Prune expired entries
        self.calls.retain(|t| now.duration_since(*t) < self.window);

        if (self.calls.len() as u32) < self.max_calls {
            self.calls.push(now);
            true
        } else {
            false
        }
    }

    /// How many calls remain in the current window
    fn remaining(&mut self) -> u32 {
        if self.max_calls == 0 {
            return u32::MAX;
        }
        let now = Instant::now();
        self.calls.retain(|t| now.duration_since(*t) < self.window);
        self.max_calls.saturating_sub(self.calls.len() as u32)
    }
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

    /// Evaluates due goals and decides on actions
    goal_evaluator: GoalEvaluator,

    /// Tracks user interaction patterns
    user_model: UserModel,

    /// Rate limiter for autonomous API calls
    rate_limiter: RateLimiter,

    /// Date of the last daily plan (to avoid re-planning same day)
    daily_plan_date: Option<NaiveDate>,

    /// Receives user messages from channels
    message_rx: mpsc::Receiver<IncomingMessage>,

    /// Receives watcher events from the scheduler
    watcher_rx: mpsc::UnboundedReceiver<WatcherEvent>,

    /// Sends responses back to channels
    response_tx: mpsc::Sender<OutgoingMessage>,

    /// Proactive notification service (iMessage alerts, etc.)
    notifier: NotificationService,

    /// Notified when a new input arrives (to wake the loop immediately)
    wake: Arc<Notify>,
}

impl AutonomousLoop {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        agent: Arc<Agent>,
        db: Arc<KnowledgeDb>,
        config: AutonomyConfig,
        message_rx: mpsc::Receiver<IncomingMessage>,
        watcher_rx: mpsc::UnboundedReceiver<WatcherEvent>,
        response_tx: mpsc::Sender<OutgoingMessage>,
        notifier: NotificationService,
        wake: Arc<Notify>,
    ) -> Self {
        let goal_evaluator = GoalEvaluator::new(db.clone(), 0.7);
        let user_model = UserModel::new(db.clone());
        let rate_limiter = RateLimiter::new(config.max_calls_per_minute, Duration::from_secs(60));
        Self {
            agent,
            db,
            config,
            goal_evaluator,
            user_model,
            rate_limiter,
            daily_plan_date: None,
            message_rx,
            watcher_rx,
            response_tx,
            notifier,
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
        info!(
            "Autonomous loop started (tick interval: {}s)",
            self.config.tick_interval_secs
        );

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

            // THINK + ACT: process user messages and watcher events first
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

            // Check budget after processing inputs and send notifications
            self.check_and_notify_budget().await;

            // EVALUATE: process due goals through the GoalEvaluator (rate-limited)
            if !due_goals.is_empty() {
                if self.rate_limiter.try_acquire() {
                    self.evaluate_goals(due_goals).await;
                } else {
                    debug!(
                        "Rate limit hit — deferring {} goal evaluations to next tick",
                        due_goals.len()
                    );
                }
            }

            // PLAN: generate daily plan once per day at the configured hour (rate-limited)
            if self.rate_limiter.remaining() > 0 {
                self.maybe_daily_plan().await;
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

    /// Generate a daily plan if it's past the configured hour and we haven't planned today
    async fn maybe_daily_plan(&mut self) {
        let now = Utc::now();
        let today = now.date_naive();
        let current_hour = now.hour();

        // Already planned today?
        if self.daily_plan_date == Some(today) {
            return;
        }

        // Not yet time for today's plan?
        if current_hour < self.config.daily_plan_hour {
            return;
        }

        info!("Generating daily plan for {}", today);
        self.daily_plan_date = Some(today);

        // Build context for the daily plan
        let user_summary = match self.user_model.summarize_for_agent().await {
            Ok(s) => s,
            Err(e) => {
                debug!("Failed to build user summary for daily plan: {}", e);
                String::new()
            }
        };

        let due_goals = self.db.get_due_goals().await.unwrap_or_default();
        let goal_summary = if due_goals.is_empty() {
            "No goals currently due for review.".to_string()
        } else {
            let items: Vec<String> = due_goals
                .iter()
                .take(10)
                .map(|g| format!("- [P{}] {}", g.priority, g.description))
                .collect();
            format!("Goals due for review:\n{}", items.join("\n"))
        };

        let day_name = match today.weekday() {
            chrono::Weekday::Mon => "Monday",
            chrono::Weekday::Tue => "Tuesday",
            chrono::Weekday::Wed => "Wednesday",
            chrono::Weekday::Thu => "Thursday",
            chrono::Weekday::Fri => "Friday",
            chrono::Weekday::Sat => "Saturday",
            chrono::Weekday::Sun => "Sunday",
        };

        let prompt = format!(
            "It's {} {}. Generate a brief daily plan and morning briefing for the user.\n\n\
             {}\n\n{}\n\n\
             Include:\n\
             1. A friendly greeting\n\
             2. Key goals/tasks for today\n\
             3. Any reminders or follow-ups\n\
             4. Weather or calendar highlights if available\n\n\
             Keep it concise — 5-10 bullet points max.",
            day_name,
            today.format("%B %d, %Y"),
            goal_summary,
            user_summary,
        );

        let msg = IncomingMessage {
            id: uuid::Uuid::new_v4().to_string(),
            sender: "daily_planner".to_string(),
            content: prompt,
            channel: ChannelType::Internal,
            timestamp: now,
        };

        match self.agent.handle_message(msg).await {
            Ok(response) => {
                info!("Daily plan generated ({} chars)", response.content.len());

                // Send as morning digest notification
                self.notifier
                    .notify(NotifyEvent::DigestMorning {
                        summary: response.content,
                    })
                    .await;
            }
            Err(e) => {
                error!("Failed to generate daily plan: {}", e);
            }
        }
    }

    /// Check budget status and send notifications if warning/exceeded
    async fn check_and_notify_budget(&self) {
        let Some(tracker) = self.agent.usage_tracker() else {
            return;
        };

        match tracker.check_budget().await {
            Ok(crate::usage::BudgetStatus::Warning {
                period,
                spent,
                budget,
                percent,
            }) => {
                self.notifier
                    .notify(NotifyEvent::BudgetWarning {
                        period,
                        spent,
                        budget,
                        percent,
                    })
                    .await;
            }
            Ok(crate::usage::BudgetStatus::Exceeded {
                period,
                spent,
                budget,
            }) => {
                self.notifier
                    .notify(NotifyEvent::BudgetExceeded {
                        period,
                        spent,
                        budget,
                    })
                    .await;
            }
            Ok(crate::usage::BudgetStatus::Ok) => {}
            Err(e) => {
                debug!("Budget check failed: {}", e);
            }
        }
    }

    /// Evaluate due goals: build a prompt, ask the agent, parse decisions, act
    async fn evaluate_goals(&self, goals: Vec<meepo_knowledge::Goal>) {
        let goal_count = goals.len();
        debug!("Evaluating {} due goals", goal_count);

        // Build the evaluation prompt
        let prompt = match self.goal_evaluator.build_evaluation_prompt(&goals) {
            Some(p) => p,
            None => return,
        };

        // Send the evaluation prompt to the agent as an internal message
        let msg = IncomingMessage {
            id: uuid::Uuid::new_v4().to_string(),
            sender: "goal_evaluator".to_string(),
            content: prompt,
            channel: ChannelType::Internal,
            timestamp: chrono::Utc::now(),
        };

        match self.agent.handle_message(msg).await {
            Ok(response) => {
                // Parse the agent's evaluation response
                let evaluations = self.goal_evaluator.parse_evaluations(&response.content);

                if evaluations.is_empty() {
                    warn!("Agent returned no parseable goal evaluations for {} goals", goal_count);
                    // Fall back: just mark goals as checked
                    for goal in &goals {
                        if let Err(e) = self.db.update_goal_checked(&goal.id, None).await {
                            error!("Failed to mark goal {} as checked: {}", goal.id, e);
                        }
                    }
                    return;
                }

                // Apply evaluations (updates DB, filters by confidence)
                match self.goal_evaluator.apply_evaluations(&evaluations).await {
                    Ok(actions) => {
                        info!(
                            "Goal evaluation: {} evaluated, {} actions approved",
                            evaluations.len(),
                            actions.len()
                        );

                        // Execute approved goal actions
                        for action in actions {
                            if let Some(ref action_prompt) = action.action_prompt {
                                info!(
                                    "Executing goal action for {}: {}",
                                    action.goal_id,
                                    &action_prompt[..action_prompt.len().min(100)]
                                );

                                // Notify about autonomous action
                                self.notifier
                                    .notify(NotifyEvent::AutonomousAction {
                                        description: format!(
                                            "Goal [{}]: {}",
                                            action.goal_id,
                                            &action_prompt[..action_prompt.len().min(200)]
                                        ),
                                    })
                                    .await;

                                let action_msg = IncomingMessage {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    sender: "goal_action".to_string(),
                                    content: action_prompt.clone(),
                                    channel: ChannelType::Internal,
                                    timestamp: chrono::Utc::now(),
                                };

                                if let Err(e) = self.agent.handle_message(action_msg).await {
                                    error!(
                                        "Failed to execute goal action for {}: {}",
                                        action.goal_id, e
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to apply goal evaluations: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("Agent failed to evaluate goals: {}", e);
                // Mark all goals as checked so we don't retry immediately
                for goal in &goals {
                    if let Err(e) = self.db.update_goal_checked(&goal.id, None).await {
                        error!("Failed to mark goal {} as checked: {}", goal.id, e);
                    }
                }
            }
        }
    }

    /// Handle a user message through the existing agent path
    async fn handle_user_message(&self, msg: IncomingMessage) {
        let channel = msg.channel.clone();
        let sender = msg.sender.clone();
        info!("Processing user message from {} on {}", sender, channel);

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
            Err(e) => {
                error!("Agent error: {}", e);
                self.notifier
                    .notify(NotifyEvent::Error {
                        context: format!("Processing message from {} on {}", sender, channel),
                        error: e.to_string(),
                    })
                    .await;
            }
        }
    }

    /// Handle a watcher event — look up the watcher's reply_channel and action,
    /// then route the agent's response to the correct channel.
    async fn handle_watcher_event(&self, event: WatcherEvent) {
        info!(
            "Processing watcher event: {} from {}",
            event.kind, event.watcher_id
        );

        // Notify user that a watcher triggered
        self.notifier
            .notify(NotifyEvent::WatcherTriggered {
                watcher_id: event.watcher_id.clone(),
                kind: event.kind.clone(),
                payload: event.payload.to_string(),
            })
            .await;

        // Look up the watcher to get reply_channel and action
        let (reply_channel, action) = match self.db.get_watcher(&event.watcher_id).await {
            Ok(Some(w)) => (ChannelType::from_string(&w.reply_channel), w.action),
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
            Err(e) => {
                error!("Failed to handle watcher event: {}", e);
                self.notifier
                    .notify(NotifyEvent::Error {
                        context: format!(
                            "Handling watcher event {} from {}",
                            event.kind, event.watcher_id
                        ),
                        error: e.to_string(),
                    })
                    .await;
            }
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
        let agent = Arc::new(Agent::new(
            api,
            tools,
            "test soul".into(),
            "test memory".into(),
            db.clone(),
        ));
        (agent, db, temp_dir)
    }

    #[tokio::test]
    async fn test_drain_inputs_empty() {
        let (agent, db, _tmp) = setup();
        let (_, msg_rx) = mpsc::channel(16);
        let (_, watcher_rx) = mpsc::unbounded_channel();
        let (resp_tx, _) = mpsc::channel(16);
        let wake = AutonomousLoop::create_wake_handle();
        let notifier = NotificationService::disabled(resp_tx.clone());

        let mut loop_ = AutonomousLoop::new(
            agent,
            db,
            AutonomyConfig {
                enabled: true,
                tick_interval_secs: 30,
                max_goals: 50,
                send_acknowledgments: true,
                daily_plan_hour: 7,
                max_calls_per_minute: 10,
            },
            msg_rx,
            watcher_rx,
            resp_tx,
            notifier,
            wake,
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
        let notifier = NotificationService::disabled(resp_tx.clone());

        // Send a message before creating the loop
        msg_tx
            .send(IncomingMessage {
                id: "test-1".into(),
                sender: "user".into(),
                content: "hello".into(),
                channel: ChannelType::Discord,
                timestamp: chrono::Utc::now(),
            })
            .await
            .unwrap();

        let mut loop_ = AutonomousLoop::new(
            agent,
            db,
            AutonomyConfig {
                enabled: true,
                tick_interval_secs: 30,
                max_goals: 50,
                send_acknowledgments: true,
                daily_plan_hour: 7,
                max_calls_per_minute: 10,
            },
            msg_rx,
            watcher_rx,
            resp_tx,
            notifier,
            wake,
        );

        let inputs = loop_.drain_inputs();
        assert_eq!(inputs.len(), 1);
    }

    #[test]
    fn test_rate_limiter_allows_within_limit() {
        let mut limiter = RateLimiter::new(3, Duration::from_secs(60));
        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
        assert!(!limiter.try_acquire()); // 4th should fail
    }

    #[test]
    fn test_rate_limiter_unlimited() {
        let mut limiter = RateLimiter::new(0, Duration::from_secs(60));
        for _ in 0..100 {
            assert!(limiter.try_acquire());
        }
    }

    #[test]
    fn test_rate_limiter_remaining() {
        let mut limiter = RateLimiter::new(5, Duration::from_secs(60));
        assert_eq!(limiter.remaining(), 5);
        limiter.try_acquire();
        assert_eq!(limiter.remaining(), 4);
        limiter.try_acquire();
        limiter.try_acquire();
        assert_eq!(limiter.remaining(), 2);
    }
}
