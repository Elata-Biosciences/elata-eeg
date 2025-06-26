//! Shared types for the EEG daemon system
//! 
//! This crate contains the core types and traits used throughout the EEG processing system,
//! including event definitions, plugin traits, and configuration types.

pub mod event;
pub mod plugin;
pub mod config;

// Re-export commonly used types
pub use event::*;
pub use plugin::*;
pub use config::*;