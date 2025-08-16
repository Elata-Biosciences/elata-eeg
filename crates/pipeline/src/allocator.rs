//! A slab-based memory allocator for pipeline packets.
//!
//! This allocator is designed to reduce heap fragmentation and improve performance
//! by reusing `Packet` buffers from a pre-allocated pool.

use std::ops::{Deref, DerefMut};
use crossbeam_queue::SegQueue;
use std::sync::Arc;

/// A shared, thread-safe handle to the central `PacketAllocator`.
pub type SharedPacketAllocator = Arc<PacketAllocator>;

/// Manages pools of reusable `Vec` buffers for different data types.
#[derive(Debug, Default)]
pub struct PacketAllocator {
    i32_pool: SegQueue<Vec<i32>>,
    f32_pool: SegQueue<Vec<f32>>,
    i32_f32_tuple_pool: SegQueue<Vec<(i32, f32)>>,
}

/// A macro to define a vector type that returns its buffer to a pool on drop.
macro_rules! define_recycled_vec {
    ($name:ident, $type:ty, $pool:ident) => {
        #[derive(Debug)]
        #[derive(Clone)]
        pub struct $name {
            vec: Vec<$type>,
            allocator: SharedPacketAllocator,
        }

        impl $name {
            /// Creates a new recycled vector, taking a buffer from the allocator's
            /// pool if available, or creating a new one otherwise.
            pub fn new(allocator: SharedPacketAllocator) -> Self {
                let vec = allocator.$pool.pop().unwrap_or_default();
                Self { vec, allocator }
            }

            /// Creates a new recycled vector with a specified capacity.
            pub fn with_capacity(allocator: SharedPacketAllocator, capacity: usize) -> Self {
                let mut vec = allocator.$pool.pop().unwrap_or_default();
                vec.reserve(capacity);
                Self { vec, allocator }
            }

            pub fn allocator(&self) -> &SharedPacketAllocator {
                &self.allocator
            }
        }

        impl From<(Vec<$type>, SharedPacketAllocator)> for $name {
            fn from((vec, allocator): (Vec<$type>, SharedPacketAllocator)) -> Self {
                Self { vec, allocator }
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                self.vec.clear();
                self.allocator.$pool.push(std::mem::take(&mut self.vec));
            }
        }

        impl Deref for $name {
            type Target = Vec<$type>;
            fn deref(&self) -> &Self::Target {
                &self.vec
            }
        }

        impl DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.vec
            }
        }
    };
}

define_recycled_vec!(RecycledI32Vec, i32, i32_pool);
define_recycled_vec!(RecycledF32Vec, f32, f32_pool);
define_recycled_vec!(RecycledI32F32TupleVec, (i32, f32), i32_f32_tuple_pool);


impl PacketAllocator {
    /// Creates a new, empty `PacketAllocator`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new `PacketAllocator` with pre-allocated capacity.
    pub fn with_capacity(
        raw_count: usize,
        voltage_count: usize,
        combo_count: usize,
        buffer_size: usize,
    ) -> Self {
        let allocator = Self::default();
        for _ in 0..raw_count {
            allocator.i32_pool.push(Vec::with_capacity(buffer_size));
        }
        for _ in 0..voltage_count {
            allocator.f32_pool.push(Vec::with_capacity(buffer_size));
        }
        for _ in 0..combo_count {
            allocator
                .i32_f32_tuple_pool
                .push(Vec::with_capacity(buffer_size));
        }
        allocator
    }
}