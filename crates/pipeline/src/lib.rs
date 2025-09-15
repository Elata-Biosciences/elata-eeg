//! Pipeline Graph Architecture for EEG Data Processing
//!
//! This crate implements a dataflow graph (DAG) architecture for EEG data processing,
//! replacing the event-bus-based plugin system with explicit pipeline stages and
//! data flow contracts.
pub mod allocator;
pub mod bridge;
pub mod config;
pub mod control;
pub mod daemon_protocol;
pub mod data;
pub mod error;
pub mod executor;
pub mod graph;
pub mod plugin;
pub mod registry;
pub mod stage;
pub mod stages;
#[macro_use]
pub mod macros;

#[cfg(test)]
mod tests;

// Re-export commonly used types
pub use allocator::*;
pub use bridge::*;
pub use control::*;
pub use data::*;
pub use error::*;
pub use plugin::*;
pub use registry::*;
pub use stage::*;
pub use stages::*;
