pub mod filters;  // Make the filters module public
pub mod coordinator;  // Add the coordinator module

pub use filters::SignalProcessor;
pub use coordinator::{DspCoordinator, DspRequirements, SystemState, ClientId};