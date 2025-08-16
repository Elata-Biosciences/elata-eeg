//! Plugin system for the EEG daemon
//! 
//! This module defines the core plugin trait that all EEG processing plugins must implement.
//! Plugins are essentially pipeline stages and can be integrated directly into the pipeline.

use anyhow::Result;
use crate::stage::Stage;

/// Configuration trait that all plugin configurations must implement
pub trait PluginConfig: Send + Sync + Clone + std::fmt::Debug {
    /// Validate the configuration parameters
    fn validate(&self) -> Result<()>;
    
    /// Get a human-readable name for this configuration
    fn config_name(&self) -> &str;
}

/// Core trait that all EEG processing plugins must implement.
/// This is a wrapper around the pipeline's `Stage` trait.
pub trait EegPlugin: Stage {
    /// Get the version of this plugin
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    
    /// Get a description of what this plugin does
    fn description(&self) -> &'static str {
        "EEG processing plugin"
    }
}

impl<T: EegPlugin + ?Sized> EegPlugin for Box<T> {
    fn version(&self) -> &'static str {
        (**self).version()
    }

    fn description(&self) -> &'static str {
        (**self).description()
    }
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