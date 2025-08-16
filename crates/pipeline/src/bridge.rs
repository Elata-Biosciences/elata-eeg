//! Message types for bridging between threads in the pipeline.

use crate::data::{PacketOwned, RtPacket};
use eeg_types::data::SensorError;
use serde::{Deserialize, Serialize};

/// Messages passed from the synchronous sensor thread to the asynchronous Tokio runtime.
///
/// This enum encapsulates the different types of events that can occur during sensor
/// data acquisition, allowing for structured communication across thread boundaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BridgeMsg {
    /// Contains a packet of sensor data.
    Data(PacketOwned),
    /// Signals that an error occurred in the sensor driver.
    Error(SensorError),
}

impl From<RtPacket> for BridgeMsg {
    fn from(runtime_packet: RtPacket) -> Self {
        BridgeMsg::Data(runtime_packet.into())
    }
}