use eeg_types::data::{PacketHeader, SensorMeta};
use serde::Serialize;

/// A JSON message sent to the frontend to provide the full metadata for a stream.
/// This is sent once when a stream begins or whenever the metadata changes.
#[derive(Serialize, Debug)]
pub struct MetaUpdateMsg<'a> {
    pub message_type: &'static str,
    pub topic: &'a str,
    pub meta: &'a SensorMeta,
}

/// The minimal JSON header for a `data_packet` message.
/// This is sent with every binary data payload.
#[derive(Serialize, Debug)]
pub struct DataPacketHeader<'a> {
    pub message_type: &'static str,
    pub topic: &'a str,
    pub packet_type: &'a str,
    pub ts_ns: u64,
    pub batch_size: u32,
    pub num_channels: u32,
    pub meta_rev: u32,
}

impl<'a> DataPacketHeader<'a> {
    /// Creates a new `DataPacketHeader` from a `PacketHeader` and a topic.
    pub fn new(header: &'a PacketHeader, topic: &'a str, packet_type: &'a str) -> Self {
        Self {
            message_type: "data_packet",
            topic,
            packet_type,
            ts_ns: header.ts_ns,
            batch_size: header.batch_size,
            num_channels: header.num_channels,
            meta_rev: header.meta.meta_rev,
        }
    }
}