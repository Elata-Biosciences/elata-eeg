//! Shared types for the EEG daemon system
//!
//! This crate contains the core types and traits used throughout the EEG processing system,
//! including event definitions, plugin traits, and configuration types.

pub mod comms;
pub mod config;
pub mod data;
pub mod event;

// Re-export commonly used types (with explicit imports to avoid ambiguity)
pub use comms::*;
pub use config::FilterConfig as ConfigFilterConfig;
pub use config::{DaemonConfig, DriverType};
pub use data::*;
pub use event::FilterConfig as EventFilterConfig;
pub use event::{EegPacket, FilteredEegPacket, SensorEvent};
