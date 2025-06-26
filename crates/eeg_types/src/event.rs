//! Event system types for the EEG daemon
//! 
//! This module defines the core data structures and events used in the event-driven
//! architecture. All large data payloads use Arc<[T]> for zero-copy sharing between
//! plugins while maintaining data integrity through frame IDs.

use std::sync::Arc;
use bytes::Bytes;

pub const PROTOCOL_VERSION: u8 = 1;

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum WebSocketTopic {
    FilteredEeg = 0,
    Fft = 1,
    Log = 255,
}

/// Raw EEG data packet from the sensor hardware
#[derive(Debug, Clone)]
pub struct EegPacket {
    /// Per-sample timestamps (microseconds since Unix epoch)
    pub timestamps: Arc<[u64]>,
    /// Monotonic frame counter for detecting data gaps
    pub frame_id: u64,
    /// Raw ADC integer samples for all channels
    pub raw_samples: Arc<[i32]>,
    /// EEG voltage samples for all channels
    pub voltage_samples: Arc<[f32]>,
    /// Number of channels in this packet
    pub channel_count: usize,
    /// Sample rate in Hz
    pub sample_rate: f32,
}

/// Filtered EEG data packet after processing.
/// This struct is designed to be wire-compatible with EegPacket.
#[derive(Debug, Clone)]
pub struct FilteredEegPacket {
    /// Per-sample timestamps (microseconds since Unix epoch), preserved from the source packet
    pub timestamps: Arc<[u64]>,
    /// Monotonic frame counter, preserved from the source packet
    pub frame_id: u64,
    /// Filtered voltage samples for all channels
    pub samples: Arc<[f32]>,
    /// Number of channels in this packet
    pub channel_count: usize,
    /// Sample rate in Hz, preserved from the source packet
    pub sample_rate: f32,
}

/// FFT analysis result packet containing brain wave frequency data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FftPacket {
    /// Timestamp when the FFT analysis was completed
    pub timestamp: u64,
    /// Reference to the original frame that was analyzed
    pub source_frame_id: u64,
    /// Power Spectral Density data for each channel
    pub psd_packets: Vec<PsdPacket>,
    /// FFT configuration used for this analysis
    pub fft_config: FftConfig,
}

/// Power Spectral Density (PSD) data for a single channel
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PsdPacket {
    /// Channel index
    pub channel: usize,
    /// Power spectral density values (µV²/Hz)
    pub psd: Vec<f32>,
}

/// FFT analysis configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FftConfig {
    /// FFT window size (must be power of 2)
    pub fft_size: usize,
    /// Sample rate used for analysis
    pub sample_rate: f32,
    /// Window function applied
    pub window_function: String,
}


/// System status and control events
#[derive(Debug, Clone)]
pub struct SystemEvent {
    /// Timestamp of the event
    pub timestamp: u64,
    /// Event type and data
    pub event_type: SystemEventType,
}

/// Types of system events
#[derive(Debug, Clone)]
pub enum SystemEventType {
    /// Recording started with filename
    RecordingStarted(String),
    /// Recording stopped
    RecordingStopped,
    /// Configuration changed
    ConfigurationChanged,
    /// Plugin error occurred
    PluginError { plugin_name: String, error: String },
    /// System shutdown initiated
    ShutdownInitiated,
}

/// Main event enum that wraps all event types in Arc for efficient sharing
#[derive(Debug, Clone)]
pub enum SensorEvent {
    /// Raw EEG data from hardware
    RawEeg(Arc<EegPacket>),
    /// Filtered EEG data from processing plugins
    FilteredEeg(Arc<FilteredEegPacket>),
    /// FFT analysis results with brain wave data
    Fft(Arc<FftPacket>),
    /// System status and control events
    System(Arc<SystemEvent>),

    /// New variant for all data destined for the WebSocket
    WebSocketBroadcast {
        topic: WebSocketTopic,
        payload: Bytes,
    },
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    // Sensor-level events
    RawEeg(Arc<EegPacket>),
    FilteredEeg(Arc<FilteredEegPacket>),
    Fft(Arc<FftPacket>),
    System(Arc<SystemEvent>),
    // Add other app-level events here if needed
}

/// Event filter types for plugins to specify what events they want to receive
#[derive(Debug, Clone, PartialEq)]
pub enum EventFilter {
    /// Receive all events
    All,
    /// Only raw EEG events
    RawEegOnly,
    /// Only filtered EEG events
    FilteredEegOnly,
    /// Only FFT analysis events
    FftOnly,
    /// Only system events
    SystemOnly,
}

/// Helper function to check if an event matches a filter
pub fn event_matches_filter(event: &SensorEvent, filter: &EventFilter) -> bool {
    match filter {
        EventFilter::All => true,
        EventFilter::RawEegOnly => matches!(event, SensorEvent::RawEeg(_)),
        EventFilter::FilteredEegOnly => matches!(event, SensorEvent::FilteredEeg(_)),
        EventFilter::FftOnly => matches!(event, SensorEvent::Fft(_)),
        EventFilter::SystemOnly => matches!(event, SensorEvent::System(_)),
    }
}

impl SensorEvent {
    /// Get the timestamp of any event type
    pub fn timestamp(&self) -> u64 {
        match self {
            // Return the timestamp of the first sample in the packet
            SensorEvent::RawEeg(packet) => packet.timestamps.first().cloned().unwrap_or(0),
            SensorEvent::FilteredEeg(packet) => packet.timestamps.first().cloned().unwrap_or(0),
            SensorEvent::Fft(packet) => packet.timestamp,
            SensorEvent::System(event) => event.timestamp,
            SensorEvent::WebSocketBroadcast { .. } => {
                // This event is for outgoing data, timestamp is not applicable in the same way
                // but we should return a value. Let's use the current time.
                // Note: This may need refinement depending on how it's used.
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u64
            }
        }
    }

    /// Get a human-readable description of the event type
    pub fn event_type_name(&self) -> &'static str {
        match self {
            SensorEvent::RawEeg(_) => "RawEeg",
            SensorEvent::FilteredEeg(_) => "FilteredEeg",
            SensorEvent::Fft(_) => "Fft",
            SensorEvent::System(_) => "System",
            SensorEvent::WebSocketBroadcast { .. } => "WebSocketBroadcast",
        }
    }
}

impl EegPacket {
    /// Create a new EEG packet with the given parameters
    pub fn new(
        timestamps: Vec<u64>,
        frame_id: u64,
        raw_samples: Vec<i32>,
        voltage_samples: Vec<f32>,
        channel_count: usize,
        sample_rate: f32,
    ) -> Self {
        Self {
            timestamps: timestamps.into(),
            frame_id,
            raw_samples: raw_samples.into(),
            voltage_samples: voltage_samples.into(),
            channel_count,
            sample_rate,
        }
    }

    /// Get voltage samples for a specific channel
    pub fn channel_voltage_samples(&self, channel: usize) -> Option<&[f32]> {
        if channel >= self.channel_count {
            return None;
        }
        
        let samples_per_channel = self.voltage_samples.len() / self.channel_count;
        let start = channel * samples_per_channel;
        let end = start + samples_per_channel;
        
        self.voltage_samples.get(start..end)
    }

    /// Get raw samples for a specific channel
    pub fn channel_raw_samples(&self, channel: usize) -> Option<&[i32]> {
        if channel >= self.channel_count {
            return None;
        }
        
        let samples_per_channel = self.raw_samples.len() / self.channel_count;
        let start = channel * samples_per_channel;
        let end = start + samples_per_channel;
        
        self.raw_samples.get(start..end)
    }

    /// Converts the packet to a binary format for the frontend, sending only voltage data.
    pub fn to_binary(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        // Format: [total_samples_u32_le][timestamps_u64_le...][voltage_samples_f32_le...]
        let total_samples = self.voltage_samples.len();
        
        // Ensure timestamps and samples have the same number of items.
        let num_timestamps = self.timestamps.len();

        if num_timestamps != total_samples {
            let effective_len = std::cmp::min(num_timestamps, total_samples);
            buffer.extend_from_slice(&(effective_len as u32).to_le_bytes());
            
            for &ts in self.timestamps.iter().take(effective_len) {
                buffer.extend_from_slice(&ts.to_le_bytes());
            }
            for &sample in self.voltage_samples.iter().take(effective_len) {
                buffer.extend_from_slice(&sample.to_le_bytes());
            }
        } else {
            buffer.extend_from_slice(&(total_samples as u32).to_le_bytes());
            for &ts in self.timestamps.iter() {
                buffer.extend_from_slice(&ts.to_le_bytes());
            }
            for &sample in self.voltage_samples.iter() {
                buffer.extend_from_slice(&sample.to_le_bytes());
            }
        }
        buffer
    }
}

impl FilteredEegPacket {
    /// Create a new filtered EEG packet
    pub fn new(
        timestamps: Arc<[u64]>,
        frame_id: u64,
        samples: Vec<f32>,
        channel_count: usize,
        sample_rate: f32,
    ) -> Self {
        Self {
            timestamps,
            frame_id,
            samples: samples.into(),
            channel_count,
            sample_rate,
        }
    }

    /// Get filtered samples for a specific channel
    pub fn channel_samples(&self, channel: usize) -> Option<&[f32]> {
        if channel >= self.channel_count {
            return None;
        }
        
        let samples_per_channel = self.samples.len() / self.channel_count;
        let start = channel * samples_per_channel;
        let end = start + samples_per_channel;
        
        self.samples.get(start..end)
    }

    /// Converts the packet to a binary format compatible with the frontend's EegDataHandler.
    /// The format is identical to EegPacket::to_binary.
    pub fn to_binary(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        // Format: [total_samples_u32_le][timestamps_u64_le...][samples_f32_le...]
        let total_samples = self.samples.len();
        
        // Ensure timestamps and samples have the same number of items per channel.
        // The number of timestamps should equal the total number of samples.
        let num_timestamps = self.timestamps.len();

        if num_timestamps != total_samples {
            // This indicates a data inconsistency. We'll handle it gracefully by logging
            // and sending a truncated packet, but this should ideally not happen.
            let effective_len = std::cmp::min(num_timestamps, total_samples);
            buffer.extend_from_slice(&(effective_len as u32).to_le_bytes());
            
            for &ts in self.timestamps.iter().take(effective_len) {
                buffer.extend_from_slice(&ts.to_le_bytes());
            }
            for &sample in self.samples.iter().take(effective_len) {
                buffer.extend_from_slice(&sample.to_le_bytes());
            }
        } else {
            buffer.extend_from_slice(&(total_samples as u32).to_le_bytes());
            for &ts in self.timestamps.iter() {
                buffer.extend_from_slice(&ts.to_le_bytes());
            }
            for &sample in self.samples.iter() {
                buffer.extend_from_slice(&sample.to_le_bytes());
            }
        }
        buffer
    }
}

impl FftPacket {
    /// Create a new FFT packet
    pub fn new(
        timestamp: u64,
        source_frame_id: u64,
        psd_packets: Vec<PsdPacket>,
        fft_config: FftConfig,
    ) -> Self {
        Self {
            timestamp,
            source_frame_id,
            psd_packets,
            fft_config,
        }
    }

    /// Get PSD data for a specific channel
    pub fn channel_psd(&self, channel: usize) -> Option<&PsdPacket> {
        self.psd_packets.iter().find(|p| p.channel == channel)
    }
}

impl FftConfig {
    /// Create a new FFT configuration
    pub fn new(fft_size: usize, sample_rate: f32, window_function: String) -> Self {
        Self {
            fft_size,
            sample_rate,
            window_function,
        }
    }

    /// Get frequency resolution for this configuration
    pub fn frequency_resolution(&self) -> f32 {
        self.sample_rate / self.fft_size as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eeg_packet_creation() {
        let voltage_samples = vec![1.0, 2.0, 3.0, 4.0]; // 2 channels, 2 samples each
        let raw_samples = vec![10, 20, 30, 40];
        let timestamps = vec![1000, 1002, 1000, 1002];
        let packet = EegPacket::new(timestamps.clone(), 1, raw_samples.clone(), voltage_samples.clone(), 2, 250.0);
        
        assert_eq!(packet.timestamps.as_ref(), timestamps.as_slice());
        assert_eq!(packet.frame_id, 1);
        assert_eq!(packet.channel_count, 2);
        assert_eq!(packet.sample_rate, 250.0);
        assert_eq!(packet.raw_samples.as_ref(), raw_samples.as_slice());
        assert_eq!(packet.voltage_samples.as_ref(), voltage_samples.as_slice());
    }

    #[test]
    fn test_channel_samples() {
        let voltage_samples = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]; // 3 channels, 2 samples each
        let raw_samples = vec![10, 20, 30, 40, 50, 60];
        let timestamps = vec![1000, 1002, 1000, 1002, 1000, 1002];
        let packet = EegPacket::new(timestamps, 1, raw_samples, voltage_samples, 3, 250.0);
        
        assert_eq!(packet.channel_voltage_samples(0), Some([1.0, 2.0].as_slice()));
        assert_eq!(packet.channel_voltage_samples(1), Some([3.0, 4.0].as_slice()));
        assert_eq!(packet.channel_voltage_samples(2), Some([5.0, 6.0].as_slice()));
        assert_eq!(packet.channel_voltage_samples(3), None);
        
        assert_eq!(packet.channel_raw_samples(0), Some([10, 20].as_slice()));
        assert_eq!(packet.channel_raw_samples(1), Some([30, 40].as_slice()));
        assert_eq!(packet.channel_raw_samples(2), Some([50, 60].as_slice()));
        assert_eq!(packet.channel_raw_samples(3), None);
    }

    #[test]
    fn test_sensor_event_timestamp() {
        let timestamps = vec![1000, 1002];
        let eeg_packet = Arc::new(EegPacket::new(timestamps, 1, vec![10, 20], vec![1.0, 2.0], 1, 250.0));
        let event = SensorEvent::RawEeg(eeg_packet);
        
        assert_eq!(event.timestamp(), 1000); // Should return the first timestamp
        assert_eq!(event.event_type_name(), "RawEeg");
    }
}