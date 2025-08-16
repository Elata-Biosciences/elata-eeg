//! Shared types for the EEG daemon system
//!
//! This crate contains the core types and traits used throughout the EEG processing system,
//! including event definitions, plugin traits, and configuration types.

pub mod event;
pub mod config;
pub mod data;
pub mod comms;

// Re-export commonly used types
pub use event::*;
pub use config::*;
pub use data::*;
pub use comms::*;