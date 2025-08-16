use crate::allocator::{RecycledF32Vec, RecycledI32F32TupleVec};
pub use eeg_types::data::{PacketData, PacketHeader, PacketOwned};

/// The different types of data packets that can exist in the pipeline at runtime.
/// This enum is intentionally NOT `Clone`. Cloning a packet should be a deliberate
/// act of either cloning the `Arc<RtPacket>` (cheap) or performing a deep copy
/// into a `PacketOwned` (expensive).
#[derive(Debug)]
pub enum RtPacket {
    RawI32(PacketData<Vec<i32>>),
    Voltage(PacketData<RecycledF32Vec>),
    VoltageF32(PacketData<Vec<f32>>),
    RawAndVoltage(PacketData<RecycledI32F32TupleVec>),
}

impl RtPacket {
    /// Performs an explicit, deep clone of the packet data, allocating new recycled buffers.
    pub fn deep_clone(&self) -> Self {
        match self {
            RtPacket::RawI32(data) => RtPacket::RawI32(PacketData {
                header: data.header.clone(),
                samples: data.samples.clone(),
            }),
            RtPacket::Voltage(data) => RtPacket::Voltage(PacketData {
                header: data.header.clone(),
                samples: (data.samples.to_vec(), data.samples.allocator().clone()).into(),
            }),
            RtPacket::VoltageF32(data) => RtPacket::VoltageF32(PacketData {
                header: data.header.clone(),
                samples: data.samples.clone(),
            }),
            RtPacket::RawAndVoltage(data) => RtPacket::RawAndVoltage(PacketData {
                header: data.header.clone(),
                samples: (data.samples.to_vec(), data.samples.allocator().clone()).into(),
            }),
        }
    }

    pub fn new_raw_i32(samples: Vec<i32>) -> Self {
        let packet_data = PacketData {
            header: PacketHeader::default(),
            samples,
        };
        RtPacket::RawI32(packet_data)
    }
}

impl From<RtPacket> for PacketOwned {
    fn from(runtime_packet: RtPacket) -> Self {
        match runtime_packet {
            RtPacket::RawI32(data) => PacketOwned::RawI32(PacketData {
                header: data.header,
                samples: data.samples.to_vec(),
            }),
            RtPacket::Voltage(data) => PacketOwned::Voltage(PacketData {
                header: data.header,
                samples: data.samples.to_vec(),
            }),
            RtPacket::VoltageF32(data) => PacketOwned::VoltageF32(PacketData {
                header: data.header,
                samples: data.samples,
            }),
            RtPacket::RawAndVoltage(data) => PacketOwned::RawAndVoltage(PacketData {
                header: data.header,
                samples: data.samples.to_vec(),
            }),
        }
    }
}

impl From<PacketOwned> for RtPacket {
    fn from(owned_packet: PacketOwned) -> Self {
        match owned_packet {
            PacketOwned::RawI32(data) => RtPacket::RawI32(PacketData {
                header: data.header,
                samples: data.samples,
            }),
            PacketOwned::Voltage(data) => RtPacket::Voltage(PacketData {
                header: data.header,
                samples: (data.samples, Default::default()).into(),
            }),
            PacketOwned::VoltageF32(data) => RtPacket::VoltageF32(PacketData {
                header: data.header,
                samples: data.samples,
            }),
            PacketOwned::RawAndVoltage(data) => RtPacket::RawAndVoltage(PacketData {
                header: data.header,
                samples: (data.samples, Default::default()).into(),
            }),
        }
    }
}
/// A read-only, zero-copy view into a runtime packet's data.
///
/// This is the primary way that stages and plugins should interact with packet
/// data. It exposes the data as simple slices, completely hiding the underlying
/// allocator-specific vector types.
pub enum PacketView<'a> {
    RawI32 {
        header: &'a PacketHeader,
        data: &'a [i32],
    },
    Voltage {
        header: &'a PacketHeader,
        data: &'a [f32],
    },
    VoltageF32 {
        header: &'a PacketHeader,
        data: &'a [f32],
    },
    RawAndVoltage {
        header: &'a PacketHeader,
        data: &'a [(i32, f32)],
    },
}

impl<'a> From<&'a RtPacket> for PacketView<'a> {
    fn from(packet: &'a RtPacket) -> Self {
        match packet {
            RtPacket::RawI32(d) => PacketView::RawI32 {
                header: &d.header,
                data: &d.samples,
            },
            RtPacket::Voltage(d) => PacketView::Voltage {
                header: &d.header,
                data: &d.samples,
            },
            RtPacket::VoltageF32(d) => PacketView::VoltageF32 {
                header: &d.header,
                data: &d.samples,
            },
            RtPacket::RawAndVoltage(d) => PacketView::RawAndVoltage {
                header: &d.header,
                data: &d.samples,
            },
        }
    }
}