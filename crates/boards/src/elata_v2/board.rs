//! ADS1299 EVM Board Configuration
//!
//! This module contains the board-specific configuration structures for the TI ADS1299 EVM,
//! allowing the driver to be more board-agnostic by accepting these configurations as parameters.

use serde::{Deserialize, Serialize};

/// Hardware configuration for the ADS1299 EVM board
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElataV2BoardConfig {
    /// GPIO pin number for the START signal
    pub start_pin: u8,
    /// GPIO pin number for the DRDY signal
    pub drdy_pin: u8,
    /// How to assign CS pins to chips
    pub cs_pin_assignment: CsPinAssignment,
    /// Register configuration details
    pub register_config: RegisterConfig,
}

/// CS pin assignment strategy
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CsPinAssignment {
    /// Use predefined pins based on chip index
    /// Chip 0 -> CS pin 8 (BCM)
    Default,
    /// Use specific pins for each chip
    /// Vec length should match number of chips
    Explicit(Vec<u8>),
}

/// Register configuration settings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegisterConfig {
    /// Whether chips are daisy chained (false for Elata V2)
    pub daisy_chain: bool,
    /// Whether to enable bias for each chip (vec length should match number of chips)
    pub bias_per_chip: Vec<bool>,
}

impl RegisterConfig {
    /// Create a new register configuration with default values
    pub fn new() -> Self {
        Self {
            daisy_chain: false,
            bias_per_chip: vec![true], // Enable bias only on the first chip by default
        }
    }
}

impl Default for RegisterConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl ElataV2BoardConfig {
    /// Create a new board configuration with default values for the ADS1299 EVM board
    pub fn new() -> Self {
        Self {
            start_pin: 22,
            drdy_pin: 25,
            cs_pin_assignment: CsPinAssignment::Default,
            register_config: RegisterConfig::default(),
        }
    }

    /// Get CS pin for a specific chip index
    pub fn get_cs_pin(&self, chip_index: usize) -> Result<u8, String> {
        match &self.cs_pin_assignment {
            CsPinAssignment::Default => {
                match chip_index {
                    0 => Ok(8), // BCM 8 is hardware CE0
                    _ => Err(format!(
                        "Default Elata V2 config only supports 1 chip, but chip {} was requested",
                        chip_index
                    )),
                }
            }
            CsPinAssignment::Explicit(pins) => pins
                .get(chip_index)
                .copied()
                .ok_or_else(|| format!("No CS pin defined for chip {}", chip_index)),
        }
    }

    /// Check if bias should be enabled for a specific chip
    pub fn is_bias_enabled(&self, chip_index: usize) -> bool {
        self.register_config
            .bias_per_chip
            .get(chip_index)
            .copied()
            .unwrap_or(false)
    }
}

impl Default for ElataV2BoardConfig {
    fn default() -> Self {
        Self::new()
    }
}
