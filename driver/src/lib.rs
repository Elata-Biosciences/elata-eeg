pub mod board_drivers;
pub mod dsp; // Re-enabled for performance optimization
pub mod eeg_system;

// Re-export the main types that users need
pub use eeg_system::EegSystem;
pub use board_drivers::types::{AdcConfig, DriverType, DriverStatus};
pub use dsp::{DspCoordinator, DspRequirements, SystemState, ClientId};
use serde::{Serialize, Deserialize};

/// Processed EEG data structure
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessedData {
    pub timestamp: u64,
    pub raw_samples: Vec<Vec<i32>>,
    pub voltage_samples: Vec<Vec<f32>>, // Renamed from processed_voltage_samples
    /// Optional FFT power spectrums, populated by downstream applet DSPs.
    pub power_spectrums: Option<Vec<Vec<f32>>>,
    /// Optional FFT frequency bins, populated by downstream applet DSPs.
    pub frequency_bins: Option<Vec<Vec<f32>>>,
    /// Optional error message if processing failed
    pub error: Option<String>,
}

/// EEG batch data structure for WebSocket streaming
/// This is used by both the daemon and DSP modules for data exchange
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EegBatchData {
    pub channels: Vec<Vec<f32>>,  // Each inner Vec represents a channel's data for the batch
    pub timestamp: u64,           // Timestamp for the start of the batch (milliseconds)
    pub power_spectrums: Option<Vec<Vec<f32>>>, // Optional FFT power spectrums
    pub frequency_bins: Option<Vec<Vec<f32>>>,   // Optional FFT frequency bins
    pub error: Option<String>,    // Optional error message from the driver
}

impl Default for ProcessedData {
    fn default() -> Self {
        Self {
            timestamp: 0,
            raw_samples: Vec::new(),
            voltage_samples: Vec::new(),
            power_spectrums: None,
            frequency_bins: None,
            error: None,
        }
    }
}

// Optionally expose lower-level access through a raw module
pub mod raw {
    pub use crate::board_drivers::*;
    pub use crate::dsp::*; // Re-enabled for performance optimization
}