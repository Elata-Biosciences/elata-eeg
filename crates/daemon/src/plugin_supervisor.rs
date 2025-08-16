//! Plugin Supervisor for the EEG Daemon
//!
//! This module is responsible for initializing, running, and managing the
//! lifecycle of all registered EEG plugins.


use pipeline::plugin::EegPlugin;
use pipeline::stage::Stage;

/// Manages the lifecycle of all registered EEG plugins.
pub struct PluginSupervisor {
    plugins: Vec<Box<dyn Stage>>,
}

impl PluginSupervisor {
    /// Creates a new `PluginSupervisor`.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Adds a plugin to the supervisor.
    pub fn add_plugin<T: EegPlugin + 'static>(&mut self, plugin: T) {
        self.plugins.push(Box::new(plugin));
    }

    /// Returns the supervised plugins as a vector of stages.
    pub fn into_stages(self) -> Vec<Box<dyn Stage>> {
        self.plugins
    }
}
