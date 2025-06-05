//! Centralized DSP Coordinator
//! 
//! This module provides a unified DSP pipeline that replaces the current
//! scattered DSP processing across multiple daemon processes. It implements
//! demand-based processing that activates DSP components only when clients need them.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::board_drivers::{AdcData, AdcConfig};
use crate::dsp::filters::SignalProcessor;
use crate::ProcessedData;

/// Unique identifier for WebSocket clients
pub type ClientId = String;

/// System processing states based on client requirements
#[derive(Debug, Clone, PartialEq)]
pub enum SystemState {
    /// Hardware monitoring only - minimal CPU usage
    Idle,
    /// Raw data + basic filtering for simple clients
    BasicStreaming,
    /// Full DSP including FFT for advanced clients
    FullProcessing,
}

/// DSP requirements for different client types
#[derive(Debug, Clone)]
pub struct DspRequirements {
    /// Client needs basic voltage filtering
    pub needs_filtering: bool,
    /// Client needs FFT processing
    pub needs_fft: bool,
    /// Client needs raw ADC samples
    pub needs_raw: bool,
    /// Specific channels this client cares about
    pub channels: Vec<usize>,
}

impl DspRequirements {
    /// Create requirements for basic EEG monitoring clients
    pub fn basic_monitoring(channels: Vec<usize>) -> Self {
        Self {
            needs_filtering: true,
            needs_fft: false,
            needs_raw: false,
            channels,
        }
    }

    /// Create requirements for FFT analysis clients
    pub fn fft_analysis(channels: Vec<usize>) -> Self {
        Self {
            needs_filtering: true,
            needs_fft: true,
            needs_raw: false,
            channels,
        }
    }

    /// Create requirements for raw data recording clients
    pub fn raw_recording(channels: Vec<usize>) -> Self {
        Self {
            needs_filtering: false,
            needs_fft: false,
            needs_raw: true,
            channels,
        }
    }
}

/// Unified DSP processing pipeline
pub struct DspPipeline {
    /// Optional signal processor for filtering
    signal_processor: Option<SignalProcessor>,
    /// Current ADC configuration
    adc_config: AdcConfig,
    /// Active channels being processed
    active_channels: Vec<usize>,
    /// FFT buffers for channels that need FFT processing
    fft_buffers: HashMap<usize, Vec<f32>>,
    /// FFT window size in samples
    fft_window_size: usize,
}

impl DspPipeline {
    /// Create a new DSP pipeline with the given configuration
    pub fn new(config: AdcConfig) -> Self {
        let active_channels = (0..config.channels.len()).collect();
        
        Self {
            signal_processor: None, // Will be created on demand
            adc_config: config,
            active_channels,
            fft_buffers: HashMap::new(),
            fft_window_size: 512, // Default FFT window size
        }
    }

    /// Update the pipeline configuration
    pub fn update_config(&mut self, config: AdcConfig) {
        self.adc_config = config;
        self.active_channels = (0..self.adc_config.channels.len()).collect();
        
        // Reset signal processor if configuration changed
        self.signal_processor = None;
        
        // Clear FFT buffers as channel configuration may have changed
        self.fft_buffers.clear();
    }

    /// Enable filtering for the pipeline
    pub fn enable_filtering(&mut self) -> Result<(), String> {
        if self.signal_processor.is_none() {
            // Create signal processor with current configuration
            // Note: Using placeholder values since AdcConfig doesn't have filter params yet
            let processor = SignalProcessor::new(
                self.adc_config.sample_rate,
                self.adc_config.channels.len(),
                1.0,  // High-pass cutoff - TODO: Add to AdcConfig
                50.0, // Low-pass cutoff - TODO: Add to AdcConfig
                Some(60), // Powerline filter - TODO: Add to AdcConfig
            );
            self.signal_processor = Some(processor);
        }
        Ok(())
    }

    /// Disable filtering for the pipeline
    pub fn disable_filtering(&mut self) {
        self.signal_processor = None;
    }

    /// Process a batch of ADC data based on current requirements
    pub fn process_batch(&mut self, data_batch: &[AdcData], requirements: &DspRequirements) -> Result<ProcessedData, String> {
        if data_batch.is_empty() {
            return Err("Empty data batch".to_string());
        }

        let batch_size = data_batch.len();
        let channel_count = self.adc_config.channels.len();
        
        // Pre-allocate output vectors
        let mut voltage_samples: Vec<Vec<f32>> = Vec::with_capacity(channel_count);
        let mut raw_samples: Vec<Vec<i32>> = Vec::with_capacity(channel_count);
        
        for _ in 0..channel_count {
            voltage_samples.push(Vec::new());
            raw_samples.push(Vec::new());
        }

        // Collect raw and voltage samples
        for data in data_batch {
            for (ch_idx, channel_raw_samples) in data.raw_samples.iter().enumerate() {
                if ch_idx < channel_count {
                    raw_samples[ch_idx].extend(channel_raw_samples.iter().cloned());
                }
            }
            
            for (ch_idx, channel_voltage_samples) in data.voltage_samples.iter().enumerate() {
                if ch_idx < channel_count {
                    voltage_samples[ch_idx].extend(channel_voltage_samples.iter().cloned());
                }
            }
        }

        // Apply filtering if required and enabled
        if requirements.needs_filtering {
            if let Some(ref mut processor) = self.signal_processor {
                // Apply filtering to voltage samples
                for (ch_idx, channel_samples) in voltage_samples.iter_mut().enumerate() {
                    if requirements.channels.contains(&ch_idx) {
                        for sample in channel_samples.iter_mut() {
                            *sample = processor.process_sample(ch_idx, *sample);
                        }
                    }
                }
            }
        }

        // Handle FFT processing if required
        let (power_spectrums, frequency_bins) = if requirements.needs_fft {
            self.process_fft(&voltage_samples, &requirements.channels)?
        } else {
            (None, None)
        };

        Ok(ProcessedData {
            timestamp: data_batch.last().unwrap().timestamp,
            raw_samples: if requirements.needs_raw { raw_samples } else { Vec::new() },
            voltage_samples,
            power_spectrums,
            frequency_bins,
            error: None,
        })
    }

    /// Process FFT for specified channels
    fn process_fft(&mut self, voltage_samples: &[Vec<f32>], channels: &[usize]) -> Result<(Option<Vec<Vec<f32>>>, Option<Vec<Vec<f32>>>), String> {
        // For now, return empty FFT results
        // TODO: Implement actual FFT processing using rustfft
        // This would integrate the FFT logic from plugins/dsp/brain_waves_fft/src/lib.rs
        
        let mut power_spectrums = Vec::new();
        let frequency_bins = Vec::new(); // Single frequency bin vector shared across channels
        
        for &ch_idx in channels {
            if ch_idx < voltage_samples.len() {
                // Placeholder for FFT processing - each channel gets its own power spectrum
                power_spectrums.push(Vec::new());
            }
        }
        
        // Return single frequency bins vector (same for all channels) wrapped in Vec
        Ok((Some(power_spectrums), if frequency_bins.is_empty() { None } else { Some(vec![frequency_bins]) }))
    }
}

/// Centralized DSP coordinator that manages the entire processing pipeline
pub struct DspCoordinator {
    /// Current system processing state
    state: SystemState,
    /// Active client connections and their requirements
    active_clients: HashMap<ClientId, DspRequirements>,
    /// Unified DSP processing pipeline
    pipeline: DspPipeline,
    /// Shared ADC configuration
    adc_config: Arc<Mutex<AdcConfig>>,
}

impl DspCoordinator {
    /// Create a new DSP coordinator with the given configuration
    pub async fn new(adc_config: Arc<Mutex<AdcConfig>>) -> Self {
        let config = adc_config.lock().await.clone();
        
        Self {
            state: SystemState::Idle,
            active_clients: HashMap::new(),
            pipeline: DspPipeline::new(config),
            adc_config,
        }
    }

    /// Register a new client with specific DSP requirements
    pub async fn register_client(&mut self, client_id: ClientId, requirements: DspRequirements) -> Result<(), String> {
        println!("DSP Coordinator: Registering client {} with requirements: {:?}", client_id, requirements);
        
        self.active_clients.insert(client_id, requirements);
        self.update_processing_state().await?;
        
        Ok(())
    }

    /// Unregister a client and update processing state
    pub async fn unregister_client(&mut self, client_id: &ClientId) -> Result<(), String> {
        println!("DSP Coordinator: Unregistering client {}", client_id);
        
        self.active_clients.remove(client_id);
        self.update_processing_state().await?;
        
        Ok(())
    }

    /// Process a batch of samples based on current client requirements
    pub async fn process_sample_batch(&mut self, raw_data: &[AdcData]) -> Result<ProcessedData, String> {
        match self.state {
            SystemState::Idle => self.minimal_processing(raw_data).await,
            SystemState::BasicStreaming => self.basic_processing(raw_data).await,
            SystemState::FullProcessing => self.full_processing(raw_data).await,
        }
    }

    /// Update the ADC configuration
    pub async fn update_config(&mut self, new_config: AdcConfig) -> Result<(), String> {
        println!("DSP Coordinator: Updating configuration");
        
        // Update shared config
        {
            let mut config_guard = self.adc_config.lock().await;
            *config_guard = new_config.clone();
        }
        
        // Update pipeline
        self.pipeline.update_config(new_config);
        
        // Re-evaluate processing state
        self.update_processing_state().await?;
        
        Ok(())
    }

    /// Get current system state
    pub fn get_state(&self) -> SystemState {
        self.state.clone()
    }

    /// Get number of active clients
    pub fn get_active_client_count(&self) -> usize {
        self.active_clients.len()
    }

    /// Update processing state based on current client requirements
    async fn update_processing_state(&mut self) -> Result<(), String> {
        let new_state = if self.active_clients.is_empty() {
            SystemState::Idle
        } else if self.active_clients.values().any(|req| req.needs_fft) {
            SystemState::FullProcessing
        } else {
            SystemState::BasicStreaming
        };

        if new_state != self.state {
            println!("DSP Coordinator: State transition {:?} -> {:?}", self.state, new_state);
            
            match new_state {
                SystemState::Idle => {
                    self.pipeline.disable_filtering();
                }
                SystemState::BasicStreaming => {
                    self.pipeline.enable_filtering()?;
                }
                SystemState::FullProcessing => {
                    self.pipeline.enable_filtering()?;
                    // Additional FFT setup would go here
                }
            }
            
            self.state = new_state;
        }

        Ok(())
    }

    /// Minimal processing for idle state
    async fn minimal_processing(&mut self, raw_data: &[AdcData]) -> Result<ProcessedData, String> {
        // Just pass through minimal data structure
        Ok(ProcessedData {
            timestamp: raw_data.last().map_or(0, |d| d.timestamp),
            raw_samples: Vec::new(),
            voltage_samples: Vec::new(),
            power_spectrums: None,
            frequency_bins: None,
            error: None,
        })
    }

    /// Basic processing for streaming clients
    async fn basic_processing(&mut self, raw_data: &[AdcData]) -> Result<ProcessedData, String> {
        // Combine requirements from all basic clients
        let combined_requirements = self.combine_client_requirements(false);
        self.pipeline.process_batch(raw_data, &combined_requirements)
    }

    /// Full processing including FFT
    async fn full_processing(&mut self, raw_data: &[AdcData]) -> Result<ProcessedData, String> {
        // Combine requirements from all clients including FFT
        let combined_requirements = self.combine_client_requirements(true);
        self.pipeline.process_batch(raw_data, &combined_requirements)
    }

    /// Combine requirements from all active clients
    fn combine_client_requirements(&self, include_fft: bool) -> DspRequirements {
        let mut needs_filtering = false;
        let mut needs_fft = false;
        let mut needs_raw = false;
        let mut all_channels = Vec::new();

        for req in self.active_clients.values() {
            needs_filtering |= req.needs_filtering;
            needs_fft |= req.needs_fft && include_fft;
            needs_raw |= req.needs_raw;
            
            for &ch in &req.channels {
                if !all_channels.contains(&ch) {
                    all_channels.push(ch);
                }
            }
        }

        all_channels.sort();

        DspRequirements {
            needs_filtering,
            needs_fft,
            needs_raw,
            channels: all_channels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_drivers::{DriverType, AdcData};

    fn create_test_config() -> AdcConfig {
        AdcConfig {
            board_driver: DriverType::Mock,
            sample_rate: 250,
            channels: vec![0, 1, 2, 3],
            gain: 1.0,
            batch_size: 10,
            Vref: 4.5,
        }
    }

    fn create_test_data() -> Vec<AdcData> {
        vec![AdcData {
            timestamp: 1000,
            raw_samples: vec![vec![100, 200], vec![150, 250], vec![120, 220], vec![180, 280]],
            voltage_samples: vec![vec![0.1, 0.2], vec![0.15, 0.25], vec![0.12, 0.22], vec![0.18, 0.28]],
        }]
    }

    #[tokio::test]
    async fn test_coordinator_creation() {
        let config = Arc::new(Mutex::new(create_test_config()));
        let coordinator = DspCoordinator::new(config).await;
        
        assert_eq!(coordinator.get_state(), SystemState::Idle);
        assert_eq!(coordinator.get_active_client_count(), 0);
    }

    #[tokio::test]
    async fn test_client_registration() {
        let config = Arc::new(Mutex::new(create_test_config()));
        let mut coordinator = DspCoordinator::new(config).await;
        
        let requirements = DspRequirements::basic_monitoring(vec![0, 1]);
        coordinator.register_client("client1".to_string(), requirements).await.unwrap();
        
        assert_eq!(coordinator.get_state(), SystemState::BasicStreaming);
        assert_eq!(coordinator.get_active_client_count(), 1);
    }

    #[tokio::test]
    async fn test_state_transitions() {
        let config = Arc::new(Mutex::new(create_test_config()));
        let mut coordinator = DspCoordinator::new(config).await;
        
        // Start idle
        assert_eq!(coordinator.get_state(), SystemState::Idle);
        
        // Add basic client
        let basic_req = DspRequirements::basic_monitoring(vec![0, 1]);
        coordinator.register_client("basic".to_string(), basic_req).await.unwrap();
        assert_eq!(coordinator.get_state(), SystemState::BasicStreaming);
        
        // Add FFT client
        let fft_req = DspRequirements::fft_analysis(vec![0, 1]);
        coordinator.register_client("fft".to_string(), fft_req).await.unwrap();
        assert_eq!(coordinator.get_state(), SystemState::FullProcessing);
        
        // Remove FFT client
        coordinator.unregister_client(&"fft".to_string()).await.unwrap();
        assert_eq!(coordinator.get_state(), SystemState::BasicStreaming);
        
        // Remove all clients
        coordinator.unregister_client(&"basic".to_string()).await.unwrap();
        assert_eq!(coordinator.get_state(), SystemState::Idle);
    }

    #[tokio::test]
    async fn test_processing_pipeline() {
        let config = Arc::new(Mutex::new(create_test_config()));
        let mut coordinator = DspCoordinator::new(config).await;
        
        let test_data = create_test_data();
        
        // Test idle processing
        let result = coordinator.process_sample_batch(&test_data).await.unwrap();
        assert!(result.voltage_samples.is_empty());
        
        // Register client and test basic processing
        let requirements = DspRequirements::basic_monitoring(vec![0, 1]);
        coordinator.register_client("client1".to_string(), requirements).await.unwrap();
        
        let result = coordinator.process_sample_batch(&test_data).await.unwrap();
        assert!(!result.voltage_samples.is_empty());
    }
}
