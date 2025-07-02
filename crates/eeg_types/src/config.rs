//! Configuration types for the EEG daemon system

use std::sync::Arc;
use serde::{Deserialize, Serialize};

/// Basic daemon configuration that plugins might need
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Maximum recording length in minutes
    pub max_recording_length_minutes: u32,
    /// Directory for storing recordings
    pub recordings_directory: String,
    /// Batch size for processing
    pub batch_size: usize,
    /// Session identifier
    pub session: String,
    /// Filter configuration
    pub filter_config: FilterConfig,
    /// Driver type
    pub driver_type: DriverType,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            max_recording_length_minutes: 60,
            recordings_directory: "recordings".to_string(),
            batch_size: 128,
            session: "default_session".to_string(),
            filter_config: FilterConfig::default(),
            driver_type: DriverType::MockEeg,
        }
    }
}

/// Filter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    /// High-pass filter cutoff frequency in Hz
    pub dsp_high_pass_cutoff_hz: f32,
    /// Low-pass filter cutoff frequency in Hz
    pub dsp_low_pass_cutoff_hz: f32,
    /// Powerline filter frequency in Hz (50Hz, 60Hz, or None for off)
    pub powerline_filter_hz: Option<u32>,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            dsp_high_pass_cutoff_hz: 1.0,
            dsp_low_pass_cutoff_hz: 50.0,
            powerline_filter_hz: Some(60),
        }
    }
}

/// Types of supported sensor drivers
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DriverType {
    /// ADS1299 EEG chip driver
    Ads1299,
    /// Mock driver for testing
    MockEeg,
}