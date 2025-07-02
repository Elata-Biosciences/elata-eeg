//! Plugin Supervisor for the EEG Daemon
//!
//! This module is responsible for initializing, running, and managing the
//! lifecycle of all registered EEG plugins.

use anyhow::Result;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use log::{info, error};

use crate::event_bus::EventBus;
use eeg_types::plugin::EegPlugin;

/// Manages the lifecycle of all registered EEG plugins.
pub struct PluginSupervisor {
    plugins: Vec<Box<dyn EegPlugin>>,
    handles: Vec<JoinHandle<Result<()>>>,
    bus: Arc<EventBus>,
}

impl PluginSupervisor {
    /// Creates a new `PluginSupervisor`.
    pub fn new(bus: Arc<EventBus>) -> Self {
        Self {
            plugins: Vec::new(),
            handles: Vec::new(),
            bus,
        }
    }

    /// Adds a plugin to the supervisor.
    pub fn add_plugin(&mut self, plugin: Box<dyn EegPlugin>) {
        self.plugins.push(plugin);
    }

    /// Initializes all registered plugins.
    pub async fn initialize_plugins(&mut self) {
        info!("Initializing plugins...");
        for plugin in &mut self.plugins {
            match plugin.initialize().await {
                Ok(_) => info!("Plugin '{}' initialized.", plugin.name()),
                Err(e) => error!("Failed to initialize plugin '{}': {}", plugin.name(), e),
            }
        }
    }

    /// Starts all registered plugins, each in its own async task.
    pub fn start_all(&mut self, shutdown_token: CancellationToken) {
        info!("Starting all plugins...");

        for plugin in &self.plugins {
            let plugin_name = plugin.name();
            let plugin_version = plugin.version();
            let bus = Arc::clone(&self.bus);
            let shutdown = shutdown_token.clone();
            
            // Create a new receiver from the event bus for each plugin
            let receiver = bus.subscribe();

            // We need to create a new Box for the run method.
            // This is a bit of a workaround for `self` lifetime issues with async traits.
            let mut plugin_instance = plugin.clone_box();

            let handle = tokio::spawn(async move {
                info!("Starting plugin: {} v{}", plugin_name, plugin_version);
                let result = plugin_instance.run(bus, receiver, shutdown).await;
                if let Err(e) = &result {
                    error!("Plugin '{}' exited with an error: {}", plugin_name, e);
                }
                result
            });

            self.handles.push(handle);
        }
    }

    /// Waits for all plugin tasks to complete.
    pub async fn join_all(&mut self) {
        for handle in self.handles.drain(..) {
            if let Err(e) = handle.await {
                error!("Error joining plugin task: {:?}", e);
            }
        }
    }
}
