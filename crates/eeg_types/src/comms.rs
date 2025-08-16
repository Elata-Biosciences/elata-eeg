use bytes::Bytes;
use serde::{Deserialize, Serialize};

pub mod pipeline {
    use super::*;
    /// The payload sent from a pipeline stage to the broker.
    /// This can be either a metadata update or a data packet.
    #[derive(Debug, Clone, Serialize)]
    pub enum BrokerPayload {
        /// A JSON string containing a `MetaUpdateMsg`.
        Meta { json: String, meta_rev: u32 },
        /// A binary blob containing a `data_packet` (header + samples).
        Data(Bytes),
    }

    /// Message sent from a pipeline stage to the daemon's broker.
    #[derive(Debug, Clone, Serialize)]
    pub enum BrokerMessage {
        Data {
            topic: String,
            payload: BrokerPayload,
        },
        RegisterTopic {
            topic: String,
            epoch: u32,
        },
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct DataPacketHeader {
        pub topic: String,
        pub packet_type: String,
        pub ts_ns: u64,
        pub batch_size: u32,
        pub num_channels: u32,
        pub meta_rev: u32,
    }
}

pub use pipeline::*;

pub mod client {
    use super::*;
    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    #[serde(rename_all = "camelCase")]
    pub struct SubscribedAck {
    	pub topic: String,
    	#[serde(skip_serializing_if = "Option::is_none")]
    	pub meta_rev: Option<u64>,
    }
   
    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    #[serde(rename_all = "camelCase")]
    pub enum ServerMessage {
    	Subscribed(SubscribedAck),
    	Error(String),
    }
   
    #[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
    #[serde(tag = "type", rename_all = "camelCase")]
    pub enum ClientMessage {
        Subscribe { topic: String, epoch: u32 },
        Unsubscribe { topic: String },
    }
}