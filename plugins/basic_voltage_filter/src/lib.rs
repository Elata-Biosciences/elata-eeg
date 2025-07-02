use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tokio_util::sync::CancellationToken;
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, error, debug, warn};

use eeg_types::{
    event::{SensorEvent, FilteredEegPacket, EventFilter, WebSocketTopic},
    config::DaemonConfig,
};
use bytes::Bytes;
use eeg_types::plugin::{EegPlugin, PluginConfig, EventBus};

mod dsp;
use dsp::SignalProcessor;

/// Configuration for the Basic Voltage Filter Plugin
#[derive(Clone, Debug)]
pub struct BasicVoltageFilterConfig {
    pub daemon_config: Arc<DaemonConfig>,
    pub sample_rate: u32,
    pub num_channels: usize,
}

impl Default for BasicVoltageFilterConfig {
    fn default() -> Self {
        Self {
            daemon_config: Arc::new(DaemonConfig::default()),
            sample_rate: 500, // Default sample rate
            num_channels: 8,   // Default channel count
        }
    }
}

impl PluginConfig for BasicVoltageFilterConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.num_channels == 0 {
            return Err(anyhow::anyhow!("Number of channels must be greater than 0"));
        }
        if self.sample_rate == 0 {
            return Err(anyhow::anyhow!("Sample rate must be greater than 0"));
        }
        Ok(())
    }
    
    fn config_name(&self) -> &str {
        "basic_voltage_filter_config"
    }
}

/// Basic Voltage Filter Plugin - applies DSP filtering to raw EEG data
pub struct BasicVoltageFilterPlugin {
    config: BasicVoltageFilterConfig,
    signal_processor: Arc<Mutex<SignalProcessor>>,
}

impl Clone for BasicVoltageFilterPlugin {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            signal_processor: Arc::clone(&self.signal_processor),
        }
    }
}

impl BasicVoltageFilterPlugin {
    pub fn new() -> Self {
        let config = BasicVoltageFilterConfig::default();
        let signal_processor = SignalProcessor::new(
            config.sample_rate,
            config.num_channels,
            config.daemon_config.filter_config.dsp_high_pass_cutoff_hz,
            config.daemon_config.filter_config.dsp_low_pass_cutoff_hz,
            config.daemon_config.filter_config.powerline_filter_hz,
        );

        Self {
            config,
            signal_processor: Arc::new(Mutex::new(signal_processor)),
        }
    }

}

#[async_trait]
impl EegPlugin for BasicVoltageFilterPlugin {
    fn name(&self) -> &'static str {
        "basic_voltage_filter"
    }

    fn clone_box(&self) -> Box<dyn EegPlugin> {
        Box::new(self.clone())
    }
    
    fn description(&self) -> &'static str {
        "Applies DSP filtering (high-pass, low-pass, powerline) to raw EEG data"
    }
    
    fn event_filter(&self) -> Vec<EventFilter> {
        vec![EventFilter::RawEegOnly]
    }

    async fn initialize(&mut self) -> Result<()> {
        info!("[{}] Initializing...", self.name());
        self.config.validate()
    }

    async fn run(
        &mut self,
        bus: Arc<dyn EventBus>,
        mut receiver: broadcast::Receiver<SensorEvent>,
        shutdown_token: CancellationToken,
    ) -> Result<()> {
        info!("[{}] Starting...", self.name());
        
        loop {
            tokio::select! {
                biased; // Prioritize shutdown
                _ = shutdown_token.cancelled() => {
                    info!("[{}] Received shutdown signal", self.name());
                    break;
                }
                event_result = receiver.recv() => {
                    match event_result {
                        Ok(SensorEvent::RawEeg(packet)) => {
                            debug!("[{}] Processing raw EEG packet with frame_id: {}",
                                   self.name(), packet.frame_id);
                            
                            let num_channels = self.config.num_channels;
                            if num_channels == 0 {
                                error!("[{}] Number of channels is zero, cannot process packet", self.name());
                                continue;
                            }

                            // TODO: This is a temporary solution. The vref and gain values should be
                            // retrieved from the AdcConfig, not hardcoded.
                            let vref = 4.5;
                            let gain = 24.0;

                            let voltage_samples: Vec<f32> = packet.raw_samples
                                .iter()
                                .map(|&raw_sample| eeg_sensor::ads1299::helpers::ch_raw_to_voltage(raw_sample, vref, gain))
                                .collect();

                            let samples_per_channel = voltage_samples.len() / num_channels;
                            if samples_per_channel == 0 {
                                continue;
                            }

                            let mut processed_samples = voltage_samples;
                            let mut signal_processor = self.signal_processor.lock().await;

                            for channel_idx in 0..num_channels {
                                let start = channel_idx * samples_per_channel;
                                let end = start + samples_per_channel;
                                
                                if let Some(channel_chunk) = processed_samples.get_mut(start..end) {
                                    let input_chunk = channel_chunk.to_vec();
                                    if let Err(e) = signal_processor.process_chunk(channel_idx, &input_chunk, channel_chunk) {
                                        error!("[{}] Error processing channel {}: {}", self.name(), channel_idx, e);
                                        processed_samples[start..end].copy_from_slice(&input_chunk);
                                    }
                                }
                            }
                            
                            let filtered_packet = FilteredEegPacket {
                                timestamps: packet.timestamps.clone(),
                                frame_id: packet.frame_id,
                                samples: processed_samples.into(),
                                channel_count: num_channels,
                                sample_rate: packet.sample_rate,
                            };

                            // Serialize the packet for the WebSocket
                            let payload_bytes = Bytes::from(filtered_packet.to_binary());

                            // Broadcast the raw filtered packet for other internal plugins
                            let internal_event = SensorEvent::FilteredEeg(Arc::new(filtered_packet));
                            bus.broadcast(internal_event).await;

                            // Broadcast the event for the WebSocket
                            let ws_event = SensorEvent::WebSocketBroadcast {
                                topic: WebSocketTopic::FilteredEeg,
                                payload: payload_bytes,
                            };
                            bus.broadcast(ws_event).await;
                        }
                        Ok(_) => {} // Ignore other event types
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("[{}] Lagged by {} messages", self.name(), n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("[{}] Receiver channel closed.", self.name());
                            break;
                        }
                    }
                }
            }
        }
        
        info!("[{}] Basic voltage filter plugin stopped", self.name());
        Ok(())
    }
}