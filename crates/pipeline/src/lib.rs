//! Pipeline Graph Architecture for EEG Data Processing
//!
//! This crate implements a dataflow graph (DAG) architecture for EEG data processing,
//! replacing the event-bus-based plugin system with explicit pipeline stages and
//! data flow contracts.
pub mod allocator;
pub mod data;
pub mod stage;
pub mod control;
pub mod graph;
pub mod config;
pub mod executor;
pub mod stages;
pub mod error;
pub mod registry;
pub mod plugin;
pub mod bridge;
pub mod daemon_protocol;
#[macro_use]
pub mod macros;

#[cfg(test)]
mod tests;

// Re-export commonly used types
pub use allocator::*;
pub use control::*;
pub use data::*;
pub use stage::*;
pub use stages::*;
pub use error::*;
pub use registry::*;
pub use plugin::*;
pub use bridge::*;