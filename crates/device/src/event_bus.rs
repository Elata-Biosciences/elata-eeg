//! Event bus implementation for the EEG daemon
//! 
//! This module provides a high-performance, non-blocking event distribution system
//! that allows plugins to communicate through events while maintaining system stability
//! through back-pressure handling.

use tokio::sync::broadcast;
use tracing::{debug, warn};
use async_trait::async_trait;

use eeg_types::SensorEvent;

const EVENT_BUS_CAPACITY: usize = 256;

/// High-performance event bus for distributing sensor events to plugins.
///
/// This is a thin wrapper around Tokio's broadcast channel, which provides
/// a multi-producer, multi-consumer, non-blocking channel suitable for
/// high-throughput event distribution.
#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<SensorEvent>,
}

impl EventBus {
    /// Create a new event bus.
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        Self { sender }
    }

    /// Subscribe to the event bus.
    ///
    /// Returns a `Receiver` that will receive all events broadcast on the bus.
    pub fn subscribe(&self) -> broadcast::Receiver<SensorEvent> {
        self.sender.subscribe()
    }

    /// Broadcast an event to all subscribers.
    ///
    /// If there are no active subscribers, the event is dropped and a warning
    /// is logged.
    pub async fn broadcast_event(&self, event: SensorEvent) {
        if self.sender.receiver_count() > 0 {
            if let Err(e) = self.sender.send(event) {
                warn!("Failed to broadcast event: {}", e);
            }
        } else {
            debug!("No subscribers, dropping event.");
        }
    }

    /// Get the number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Returns a clone of the broadcast sender.
    pub fn sender(&self) -> broadcast::Sender<SensorEvent> {
        self.sender.clone()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics for event bus performance monitoring
#[derive(Debug)]
pub struct EventBusMetrics {
    pub events_broadcast: u64,
    pub events_delivered: u64,
    pub events_dropped: u64,
    pub dead_subscribers_removed: u64,
    pub active_subscribers: usize,
}

impl Clone for EventBusMetrics {
    fn clone(&self) -> Self {
        Self {
            events_broadcast: self.events_broadcast,
            events_delivered: self.events_delivered,
            events_dropped: self.events_dropped,
            dead_subscribers_removed: self.dead_subscribers_removed,
            active_subscribers: self.active_subscribers,
        }
    }
}

#[async_trait]
impl eeg_types::plugin::EventBus for EventBus {
    async fn broadcast(&self, event: SensorEvent) {
        self.broadcast_event(event).await;
    }
    
    fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use eeg_types::{EegPacket, SensorEvent};
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_event_bus_basic_functionality() {
        let bus = EventBus::new();
        
        // Subscribe to events
        let mut receiver = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);
        
        // Create and broadcast an event
        let timestamps = vec![1000, 1002];
        let raw_samples = vec![10, 20];
        let voltage_samples = vec![1.0, 2.0];
        let packet = Arc::new(EegPacket::new(timestamps, 1, raw_samples, voltage_samples, 1, 250.0));
        let event = SensorEvent::RawEeg(packet);
        
        bus.broadcast_event(event.clone()).await;
        
        // Receive the event
        let received = timeout(Duration::from_millis(100), receiver.recv())
            .await
            .expect("Should receive event within timeout")
            .expect("Should receive an event");
        
        // Compare the events (assuming they implement PartialEq or we can compare specific fields)
        match (&received, &event) {
            (SensorEvent::RawEeg(recv_packet), SensorEvent::RawEeg(orig_packet)) => {
                assert_eq!(recv_packet.frame_id, orig_packet.frame_id);
            }
            _ => panic!("Event types don't match"),
        }
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new();
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);

        let timestamps = vec![1000];
        let raw_samples = vec![10];
        let voltage_samples = vec![1.0];
        let event = SensorEvent::RawEeg(Arc::new(EegPacket::new(timestamps, 1, raw_samples, voltage_samples, 1, 250.0)));
        bus.broadcast_event(event).await;

        assert!(rx1.recv().await.is_ok());
        assert!(rx2.recv().await.is_ok());
    }

    #[tokio::test]
    async fn test_dead_subscriber_removal() {
        let bus = EventBus::new();
        
        // Create a subscriber and then drop the receiver
        {
            let _receiver = bus.subscribe();
            assert_eq!(bus.subscriber_count(), 1);
        } // receiver is dropped here
        
        // The count should reflect the drop immediately
        assert_eq!(bus.subscriber_count(), 0);
    }
}