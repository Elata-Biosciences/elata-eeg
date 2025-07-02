use async_trait::async_trait;
use eeg_types::{
    event::{EventFilter, FftConfig, FftPacket, PsdPacket, SensorEvent, WebSocketTopic},
    plugin::{EegPlugin, EventBus},
};
use bytes::Bytes;
use apodize::hanning_iter;
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

const FFT_SIZE: usize = 1 << 12;
const VOLTS_TO_MICROVOLTS: f32 = 1_000_000.0;

#[derive(Clone)]
pub struct BrainWavesFftPlugin {
    channel_buffers: Vec<Vec<f32>>,
    fft_planner: Arc<dyn Fft<f32>>,
    num_channels: usize,
    sample_rate: f32,
}

impl BrainWavesFftPlugin {
    pub fn new(num_channels: usize, sample_rate: f32) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        Self {
            channel_buffers: vec![Vec::with_capacity(FFT_SIZE); num_channels],
            fft_planner: fft,
            num_channels,
            sample_rate,
        }
    }

    /// Check if there are any subscribers interested in FFT data
    /// This includes both internal plugin subscribers and WebSocket clients
    async fn has_fft_subscribers(&self, bus: &Arc<dyn EventBus>) -> bool {
        // Check if there are any active subscribers on the event bus
        // This includes both internal plugins and the connection manager for WebSocket clients
        let subscriber_count = bus.subscriber_count();
        
        if subscriber_count == 0 {
            // Log periodically when skipping processing due to no subscribers
            static LAST_LOG_TIME: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
            let mut last_log = LAST_LOG_TIME.lock().unwrap();
            let now = std::time::Instant::now();
            
            if last_log.map_or(true, |last| now.duration_since(last) > std::time::Duration::from_secs(30)) {
                info!("[{}] No subscribers detected, skipping FFT calculations to save CPU", self.name());
                *last_log = Some(now);
            }
            
            return false;
        }
        
        // If there are subscribers, we should process FFT data
        // Note: This is a conservative approach - we process if ANY subscribers exist
        // A more sophisticated implementation could track topic-specific subscriptions
        // to only process when there are subscribers specifically interested in FFT data
        true
    }
}

#[async_trait]
impl EegPlugin for BrainWavesFftPlugin {
    fn name(&self) -> &'static str {
        "brain_waves_fft"
    }

    fn clone_box(&self) -> Box<dyn EegPlugin> {
        Box::new(self.clone())
    }

    fn description(&self) -> &'static str {
        "Performs FFT and PSD calculations on filtered EEG data."
    }

    fn event_filter(&self) -> Vec<EventFilter> {
        vec![EventFilter::FilteredEegOnly]
    }

    async fn initialize(&mut self) -> anyhow::Result<()> {
        info!("[{}] Initializing...", self.name());
        Ok(())
    }

    async fn run(
        &mut self,
        bus: Arc<dyn EventBus>,
        mut receiver: broadcast::Receiver<SensorEvent>,
        shutdown_token: CancellationToken,
    ) -> anyhow::Result<()> {
        info!("[{}] Starting...", self.name());

        loop {
            tokio::select! {
                biased;
                _ = shutdown_token.cancelled() => {
                    info!("[{}] Received shutdown signal", self.name());
                    break;
                }
                event_result = receiver.recv() => {
                    match event_result {
                        Ok(SensorEvent::FilteredEeg(packet)) => {
                            let samples_per_channel = packet.samples.len() / self.num_channels;
                            if samples_per_channel == 0 {
                                continue;
                            }

                            // Always accumulate samples in buffers for potential processing
                            for ch in 0..self.num_channels {
                                let start = ch * samples_per_channel;
                                let end = start + samples_per_channel;
                                if let Some(channel_samples) = packet.samples.get(start..end) {
                                    self.channel_buffers[ch].extend_from_slice(channel_samples);
                                }
                            }
                            
                            // TODO make this more effiecnet. like on/off based on subscrition
                            // Only perform expensive FFT calculations if there are subscribers
                            while self.channel_buffers.iter().any(|b| b.len() >= FFT_SIZE) {
                                // Check if there are any subscribers for FFT data before processing
                                if !self.has_fft_subscribers(&bus).await {
                                    // No subscribers - just drain the buffers to prevent memory buildup
                                    // but skip the expensive FFT calculations
                                    for ch in 0..self.num_channels {
                                        if self.channel_buffers[ch].len() >= FFT_SIZE {
                                            self.channel_buffers[ch].drain(..FFT_SIZE / 2);
                                        }
                                    }
                                    continue;
                                }
                                let mut psd_packets = Vec::with_capacity(self.num_channels);

                                for ch in 0..self.num_channels {
                                    let mut samples: Vec<f32> = self.channel_buffers[ch][..FFT_SIZE]
                                        .iter()
                                        .map(|&v| v * VOLTS_TO_MICROVOLTS)
                                        .collect();

                                    // Apply Hann window
                                    let window: Vec<f32> = hanning_iter(FFT_SIZE).map(|v| v as f32).collect();
                                    for (i, sample) in samples.iter_mut().enumerate() {
                                        *sample *= window[i];
                                    }

                                    let mut buffer: Vec<Complex<f32>> = samples
                                        .into_iter()
                                        .map(|v| Complex::new(v, 0.0))
                                        .collect();

                                    self.fft_planner.process(&mut buffer);

                                    // Calculate window scaling factor (sum of squares of window samples)
                                    let s2: f32 = window.iter().map(|&w| w * w).sum();
                                    let scaling_factor = 2.0 / (self.sample_rate * s2);

                                    let psd: Vec<f32> = buffer.iter().take(FFT_SIZE / 2)
                                        .map(|c| (c.norm_sqr() * scaling_factor).log10())
                                        .collect();

                                    psd_packets.push(PsdPacket { channel: ch, psd });

                                    // Overlapping windows: retain 50% of the data
                                    self.channel_buffers[ch].drain(..FFT_SIZE / 2);
                                }

                                let fft_packet = FftPacket {
                                    timestamp: packet.timestamps.first().cloned().unwrap_or(0),
                                    source_frame_id: packet.frame_id,
                                    psd_packets,
                                    fft_config: FftConfig {
                                        fft_size: FFT_SIZE,
                                        sample_rate: self.sample_rate,
                                        window_function: "Hann".to_string(),
                                    },
                                };

                                // Serialize the packet to JSON for the WebSocket
                               let payload_bytes = Bytes::from(serde_json::to_vec(&fft_packet)?);

                                // Broadcast the raw FFT packet for other internal plugins
                                bus.broadcast(SensorEvent::Fft(Arc::new(fft_packet))).await;

                                // Broadcast the event for the WebSocket
                                let ws_event = SensorEvent::WebSocketBroadcast {
                                    topic: WebSocketTopic::Fft,
                                    payload: payload_bytes,
                                };
                                bus.broadcast(ws_event).await;
                            }
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

        info!("[{}] Plugin stopped", self.name());
        Ok(())
    }
}