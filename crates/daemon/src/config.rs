use serde::{Serialize, Deserialize};
use std::sync::Arc;
use eeg_types::DriverType;
use sensors::types::AdcConfig;

/// Configuration for the DSP filters
#[derive(Clone, Debug, Serialize, Deserialize)]
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
            dsp_high_pass_cutoff_hz: 0.5,
            dsp_low_pass_cutoff_hz: 50.0,
            powerline_filter_hz: Some(60), // Default to 60Hz
        }
    }
}

/// Configuration for the daemon
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Batch size for processing data
    pub batch_size: usize,
    /// Type of board driver to use (Ads1299 or Mock)
    pub driver_type: DriverType,
    /// Configuration for the DSP filters
    pub filter_config: FilterConfig,
    /// Configuration for the ADC
    pub adc_config: AdcConfig,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            batch_size: 32,
            adc_config: AdcConfig::default(),
            driver_type: DriverType::MockEeg, // Default to Mock driver for safety
            filter_config: FilterConfig::default(),
        }
    }
}

/// Load daemon configuration from file or create default if not found
pub fn load_config() -> Arc<DaemonConfig> {
    let config_path = "./config.json";
    let contents = std::fs::read_to_string(config_path)
        .unwrap_or_else(|_| panic!("Could not read configuration file at '{}'. Please ensure the file exists in the current working directory.", config_path));

    let config: DaemonConfig = serde_json::from_str(&contents)
        .unwrap_or_else(|e| panic!("Could not parse configuration file at '{}': {}", config_path, e));

    println!("Loaded configuration from {}", config_path);
    Arc::new(config)
}