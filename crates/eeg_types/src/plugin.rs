//! Plugin system for the EEG daemon
//! 
//! This module defines the core plugin trait that all EEG processing plugins must implement.
//! Plugins run in isolated async tasks and communicate through the event bus system.

use std::sync::Arc;
use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use anyhow::Result;

use crate::event::{SensorEvent, EventFilter, event_matches_filter};

/// Configuration trait that all plugin configurations must implement
pub trait PluginConfig: Send + Sync + Clone + std::fmt::Debug {
    /// Validate the configuration parameters
    fn validate(&self) -> Result<()>;
    
    /// Get a human-readable name for this configuration
    fn config_name(&self) -> &str;
}

/// Core trait that all EEG processing plugins must implement
#[async_trait]
pub trait EegPlugin: Send + Sync {
    /// Get the unique name of this plugin
    fn name(&self) -> &'static str;
    
    /// Get the version of this plugin
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    
    /// Get a description of what this plugin does
    fn description(&self) -> &'static str {
        "EEG processing plugin"
    }
    
    /// Get the types of events this plugin is interested in receiving
    fn event_filter(&self) -> Vec<EventFilter> {
        vec![EventFilter::All]
    }
    
    /// Main plugin execution method
    ///
    /// This method runs in a dedicated async task and should:
    /// - Listen for events on the receiver channel
    /// - Process events according to the plugin's logic
    /// - Optionally publish new events back to the bus
    /// - Respect the shutdown signal from the cancellation token
    /// - Return Ok(()) on graceful shutdown or Err on failure
    async fn run(
        &mut self,
        bus: Arc<dyn EventBus>,
        receiver: broadcast::Receiver<SensorEvent>,
        shutdown_token: CancellationToken,
    ) -> Result<()>;
    
    /// Optional initialization method called before run()
    async fn initialize(&mut self) -> Result<()> {
        Ok(())
    }
    
    /// Optional cleanup method called after run() completes
    async fn cleanup(&self) -> Result<()> {
        Ok(())
    }
    
    /// Get plugin-specific metrics (optional)
    fn get_metrics(&self) -> Vec<PluginMetric> {
        vec![]
    }

    /// Create a new, boxed clone of this plugin.
    fn clone_box(&self) -> Box<dyn EegPlugin>;
}


/// Event bus trait for plugins to publish events
#[async_trait]
pub trait EventBus: Send + Sync {
    /// Broadcast an event to all subscribers
    async fn broadcast(&self, event: SensorEvent);
    
    /// Get the number of active subscribers to the event bus
    /// This can be used by plugins to optimize processing when no subscribers are present
    fn subscriber_count(&self) -> usize;
}


/// Plugin-specific metrics
#[derive(Debug, Clone)]
pub struct PluginMetric {
    /// Metric name
    pub name: String,
    /// Metric value
    pub value: f64,
    /// Metric unit (e.g., "events/sec", "ms", "bytes")
    pub unit: String,
    /// Optional description
    pub description: Option<String>,
}

/// Plugin runtime statistics
#[derive(Debug, Clone)]
pub struct PluginStats {
    /// Plugin name
    pub name: String,
    /// Number of events processed
    pub events_processed: u64,
    /// Number of events dropped due to full buffer
    pub events_dropped: u64,
    /// Number of events published by this plugin
    pub events_published: u64,
    /// Number of errors encountered
    pub error_count: u64,
    /// Plugin uptime in milliseconds
    pub uptime_ms: u64,
    /// Current queue depth
    pub queue_depth: usize,
    /// Last error message (if any)
    pub last_error: Option<String>,
}

/// Plugin supervisor configuration
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// Maximum number of restart attempts
    pub max_retries: u8,
    /// Initial backoff delay in milliseconds
    pub initial_backoff_ms: u64,
    /// Maximum backoff delay in milliseconds
    pub max_backoff_ms: u64,
    /// Backoff multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Channel buffer size for plugin event queue
    pub channel_buffer_size: usize,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff_ms: 1000,
            max_backoff_ms: 30000,
            backoff_multiplier: 2.0,
            channel_buffer_size: 128,
        }
    }
}

impl SupervisorConfig {
    /// Calculate backoff delay for a given attempt number
    pub fn calculate_backoff(&self, attempt: u8) -> std::time::Duration {
        let delay_ms = (self.initial_backoff_ms as f64 
            * self.backoff_multiplier.powi(attempt as i32 - 1))
            .min(self.max_backoff_ms as f64) as u64;
        
        std::time::Duration::from_millis(delay_ms)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EegPacket, SensorEvent, EventFilter, event_matches_filter};

    #[test]
    fn test_supervisor_config_backoff() {
        let config = SupervisorConfig::default();
        
        assert_eq!(config.calculate_backoff(1), std::time::Duration::from_millis(1000));
        assert_eq!(config.calculate_backoff(2), std::time::Duration::from_millis(2000));
        assert_eq!(config.calculate_backoff(3), std::time::Duration::from_millis(4000));
    }

    #[test]
    fn test_event_filter_matching() {
        let timestamps = vec![1000];
        let raw_samples = vec![10];
        let voltage_samples = vec![1.0];
        let eeg_packet = Arc::new(EegPacket::new(timestamps, 1, raw_samples, voltage_samples, 1, 250.0));
        let raw_event = SensorEvent::RawEeg(eeg_packet.clone());
        
        assert!(event_matches_filter(&raw_event, &EventFilter::All));
        assert!(event_matches_filter(&raw_event, &EventFilter::RawEegOnly));
        assert!(!event_matches_filter(&raw_event, &EventFilter::FilteredEegOnly));
        assert!(!event_matches_filter(&raw_event, &EventFilter::FftOnly));
        assert!(!event_matches_filter(&raw_event, &EventFilter::SystemOnly));
    }
}