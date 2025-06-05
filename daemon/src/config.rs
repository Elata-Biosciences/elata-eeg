use serde::{Serialize, Deserialize};
use std::sync::Arc;
use eeg_driver::DriverType;

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
    /// Maximum recording length in minutes before starting a new file
    pub max_recording_length_minutes: u32,
    /// Directory where recordings are stored
    pub recordings_directory: String,
    /// Session identifier for recordings
    pub session: String,
    /// Batch size for processing data
    pub batch_size: usize,
    /// Type of board driver to use (Ads1299 or Mock)
    pub driver_type: DriverType,
    /// Configuration for the DSP filters
    pub filter_config: FilterConfig,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            max_recording_length_minutes: 60,
            recordings_directory: "./recordings/".to_string(), // Changed to ./
            session: "".to_string(),
            batch_size: 32,
            driver_type: DriverType::Mock, // Default to Mock driver for safety
            filter_config: FilterConfig::default(),
        }
    }
}

/// Load daemon configuration from file or create default if not found
pub fn load_config() -> Arc<DaemonConfig> {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| {
        println!("Warning: Could not determine current directory. Assuming project root for config path.");
        std::path::PathBuf::from(".")
    });

    let config_path_str = if current_dir.file_name().map_or(false, |name| name == "daemon") {
        "../config.json"
    } else {
        // Assumes if CWD is not '.../daemon', it's the project root.
        "config.json"
    };
    
    let config_path = std::path::Path::new(config_path_str);

    match std::fs::read_to_string(config_path) {
        Ok(contents) => {
            match serde_json::from_str(&contents) {
                Ok(config) => {
                    println!("Loaded configuration from {}", config_path.display());
                    Arc::new(config)
                },
                Err(e) => {
                    println!("Error parsing configuration file {}: {}. Using defaults.", config_path.display(), e);
                    let default_config = DaemonConfig::default();
                    
                    // Create default config file for future use at the determined path
                    if let Ok(json) = serde_json::to_string_pretty(&default_config) {
                        if let Err(write_err) = std::fs::write(config_path, json) {
                            println!("Warning: Could not create default configuration file at {}: {}", config_path.display(), write_err);
                        } else {
                            println!("Created default configuration file at {}", config_path.display());
                        }
                    }
                    
                    Arc::new(default_config)
                }
            }
        },
        Err(_) => {
            println!("Configuration file {} not found. Using defaults.", config_path.display());
            let default_config = DaemonConfig::default();
            
            // Create default config file for future use at the determined path
            if let Ok(json) = serde_json::to_string_pretty(&default_config) {
                 if let Err(write_err) = std::fs::write(config_path, json) {
                    println!("Warning: Could not create default configuration file at {}: {}", config_path.display(), write_err);
                } else {
                    println!("Created default configuration file at {}", config_path.display());
                }
            }
            
            Arc::new(default_config)
        }
    }
}