//! Event bus — broadcast events to all connected WebSocket clients

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::debug;

use crate::protocol::GatewayEvent;

/// Broadcast event bus for the gateway
#[derive(Clone)]
pub struct EventBus {
    sender: Arc<broadcast::Sender<GatewayEvent>>,
}

impl EventBus {
    /// Create a new event bus with the given channel capacity
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender: Arc::new(sender),
        }
    }

    /// Subscribe to events (each WebSocket connection gets its own receiver)
    pub fn subscribe(&self) -> broadcast::Receiver<GatewayEvent> {
        self.sender.subscribe()
    }

    /// Broadcast an event to all connected clients
    pub fn broadcast(&self, event: GatewayEvent) {
        let receivers = self.sender.receiver_count();
        if receivers > 0 {
            debug!(
                "Broadcasting event '{}' to {} receivers",
                event.event, receivers
            );
            // Ignore send error (no receivers is fine)
            let _ = self.sender.send(event);
        }
    }

    /// Number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_bus_broadcast() {
        let bus = EventBus::new(16);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        let event = GatewayEvent::new("test.event", serde_json::json!({"key": "value"}));
        bus.broadcast(event);

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();
        assert_eq!(e1.event, "test.event");
        assert_eq!(e2.event, "test.event");
    }

    #[test]
    fn test_event_bus_no_receivers() {
        let bus = EventBus::new(16);
        // Should not panic even with no receivers
        bus.broadcast(GatewayEvent::new("test", serde_json::json!({})));
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[test]
    fn test_event_bus_subscriber_count() {
        let bus = EventBus::new(16);
        assert_eq!(bus.subscriber_count(), 0);
        let _rx1 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);
        let _rx2 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);
        drop(_rx1);
        assert_eq!(bus.subscriber_count(), 1);
    }
}
