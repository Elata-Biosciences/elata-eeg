# Zero-Copy Pipeline Architecture Implementation Plan

## Executive Summary

This document provides a comprehensive implementation plan for transforming the current EEG pipeline from a fake "zero-copy" system with significant performance issues into a true zero-copy, high-performance architecture optimized for Raspberry Pi 5 (ARM Cortex-A76).

**Current Performance Issues:**
- Fake "zero-copy" with ~200KB allocations per packet (2MB/s waste at 4kSPS)
- 1ms sleep polling adding terrible latency
- Unbounded channels risking memory explosion (800MB/s growth potential)
- Excessive data cloning on fan-out

**Target Performance:**
- **Latency**: 31-40μs (vs current 91-285μs) = 7-9x improvement
- **Memory**: Zero allocations during steady-state operation
- **CPU**: <5% of one Pi-5 core (vs current 9-28%)
- **Throughput**: 10,000+ SPS capability (vs current 4,000 SPS)

## Phase 1: Zero-Copy Buffer Pool Architecture (2 weeks)

### 1.1 Core Buffer Pool Design

#### Buffer Slab Structure
```rust
// crates/pipeline/src/buffer_pool.rs

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use crossbeam::queue::ArrayQueue;

/// Fixed-size buffer slab optimized for Pi-5 cache hierarchy
/// L1: 64KB, L2: 512KB per core - keep working set under L2
pub struct BufferSlab {
    /// Planar sample layout for NEON optimization: [ch0_samples][ch1_samples]...
    /// 32 channels × 400 samples × 4 bytes = 51.2KB (fits in L1)
    samples: Box<[f32; TOTAL_SAMPLES]>,
    
    /// Compact timestamp representation - single start + period
    /// Saves 40% packet size vs per-sample timestamps
    timestamp_start: u64,
    sample_period_ns: u32,  // nanoseconds between samples
    
    /// Metadata (no heap allocation)
    frame_id: u64,
    channel_count: usize,
    sample_count: usize,
    sample_rate: f32,
    
    /// Pool tracking (no redundant refcount - Arc already handles this)
    pool_id: usize,
}

/// Constants for Pi-5 optimization
const MAX_CHANNELS: usize = 32;
const MAX_SAMPLES_PER_CHANNEL: usize = 400;
const TOTAL_SAMPLES: usize = MAX_CHANNELS * MAX_SAMPLES_PER_CHANNEL;

impl BufferSlab {
    /// Create new uninitialized buffer slab
    pub fn new_uninit(pool_id: usize) -> Box<Self> {
        // Use MaybeUninit for stable Rust compatibility
        use std::mem::MaybeUninit;
        
        let mut samples = Box::new([0.0f32; TOTAL_SAMPLES]);
        
        Box::new(Self {
            samples,
            timestamp_start: 0,
            sample_period_ns: 0,
            frame_id: 0,
            channel_count: 0,
            sample_count: 0,
            sample_rate: 0.0,
            pool_id,
        })
    }
    
    /// Get mutable slice for a specific channel (planar layout)
    pub fn channel_samples_mut(&mut self, channel: usize) -> Option<&mut [f32]> {
        if channel >= self.channel_count {
            return None;
        }
        
        let start = channel * MAX_SAMPLES_PER_CHANNEL;
        let end = start + self.sample_count;
        Some(&mut self.samples[start..end])
    }
    
    /// Get immutable slice for a specific channel
    pub fn channel_samples(&self, channel: usize) -> Option<&[f32]> {
        if channel >= self.channel_count {
            return None;
        }
        
        let start = channel * MAX_SAMPLES_PER_CHANNEL;
        let end = start + self.sample_count;
        Some(&self.samples[start..end])
    }
    
    /// Get timestamp for a specific sample index
    pub fn sample_timestamp(&self, sample_idx: usize) -> u64 {
        self.timestamp_start + (sample_idx as u64 * self.sample_period_ns as u64)
    }
    
    /// Reset buffer for reuse
    pub fn reset(&mut self) {
        self.frame_id = 0;
        self.channel_count = 0;
        self.sample_count = 0;
        self.timestamp_start = 0;
        self.sample_period_ns = 0;
        // Note: Don't zero samples array - will be overwritten
    }
}
```

#### Lock-Free Buffer Pool
```rust
/// High-performance buffer pool with zero-allocation steady state
pub struct BufferPool {
    /// Lock-free queue of available buffers
    available: ArrayQueue<Arc<BufferSlab>>,
    
    /// Pool configuration
    total_buffers: usize,
    buffer_size: usize,
    
    /// Metrics for monitoring
    allocation_counter: AtomicUsize,
    pool_misses: AtomicUsize,
    peak_usage: AtomicUsize,
}

impl BufferPool {
    /// Create new buffer pool optimized for expected load
    /// Rule of thumb: 2-4x max concurrent buffers in flight
    pub fn new(pool_size: usize) -> Self {
        let available = ArrayQueue::new(pool_size);
        
        // Pre-allocate all buffers at startup
        for i in 0..pool_size {
            let buffer = Arc::new(BufferSlab::new_uninit(i));
            let _ = available.push(buffer); // Can't fail - queue is empty
        }
        
        Self {
            available,
            total_buffers: pool_size,
            buffer_size: std::mem::size_of::<BufferSlab>(),
            allocation_counter: AtomicUsize::new(0),
            pool_misses: AtomicUsize::new(0),
            peak_usage: AtomicUsize::new(0),
        }
    }
    
    /// Get buffer from pool (zero-allocation in steady state)
    pub fn get_buffer(&self) -> Result<Arc<BufferSlab>, BufferPoolError> {
        match self.available.pop() {
            Some(mut buffer) => {
                // Reset buffer for reuse
                Arc::get_mut(&mut buffer)
                    .ok_or(BufferPoolError::BufferInUse)?
                    .reset();
                
                Ok(buffer)
            }
            None => {
                // Pool exhausted - this indicates backpressure
                self.pool_misses.fetch_add(1, Ordering::Relaxed);
                Err(BufferPoolError::PoolExhausted)
            }
        }
    }
    
    /// Return buffer to pool (automatic via Arc Drop)
    /// Note: This happens automatically when Arc refcount reaches 0
    pub fn return_buffer(&self, buffer: Arc<BufferSlab>) {
        if Arc::strong_count(&buffer) == 1 {
            // We're the last owner - return to pool
            if self.available.push(buffer).is_err() {
                // Pool is full - this shouldn't happen but handle gracefully
                // Buffer will be dropped and deallocated
            }
        }
        // If refcount > 1, buffer is still in use elsewhere
    }
    
    /// Get pool statistics for monitoring
    pub fn stats(&self) -> BufferPoolStats {
        let available_count = self.available.len();
        let in_use = self.total_buffers - available_count;
        
        BufferPoolStats {
            total_buffers: self.total_buffers,
            available_buffers: available_count,
            buffers_in_use: in_use,
            pool_misses: self.pool_misses.load(Ordering::Relaxed),
            allocation_counter: self.allocation_counter.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug)]
pub enum BufferPoolError {
    PoolExhausted,
    BufferInUse,
}

#[derive(Debug, Clone)]
pub struct BufferPoolStats {
    pub total_buffers: usize,
    pub available_buffers: usize,
    pub buffers_in_use: usize,
    pub pool_misses: usize,
    pub allocation_counter: usize,
}
```

### 1.2 Zero-Copy Data Types

#### New EEG Data Structure
```rust
// crates/pipeline/src/data.rs - Replace existing PipelineData

use std::sync::Arc;
use crate::buffer_pool::BufferSlab;

/// Zero-copy EEG data packet
#[derive(Debug, Clone)]
pub struct ZeroCopyEegData {
    /// Shared buffer from pool (only Arc pointer is cloned)
    buffer: Arc<BufferSlab>,
    
    /// Data type marker for type safety
    data_type: EegDataType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EegDataType {
    Raw,        // Raw ADC values
    Voltage,    // Converted to voltage
    Filtered,   // After filtering
    Fft,        // FFT analysis results
}

impl ZeroCopyEegData {
    /// Create new zero-copy data from buffer
    pub fn new(buffer: Arc<BufferSlab>, data_type: EegDataType) -> Self {
        Self { buffer, data_type }
    }
    
    /// Get read-only access to channel data
    pub fn channel_data(&self, channel: usize) -> Option<&[f32]> {
        self.buffer.channel_samples(channel)
    }
    
    /// Get mutable access to channel data (for in-place processing)
    pub fn channel_data_mut(&mut self, channel: usize) -> Option<&mut [f32]> {
        Arc::get_mut(&mut self.buffer)?
            .channel_samples_mut(channel)
    }
    
    /// Get metadata
    pub fn frame_id(&self) -> u64 { self.buffer.frame_id }
    pub fn channel_count(&self) -> usize { self.buffer.channel_count }
    pub fn sample_count(&self) -> usize { self.buffer.sample_count }
    pub fn sample_rate(&self) -> f32 { self.buffer.sample_rate }
    pub fn data_type(&self) -> EegDataType { self.data_type }
    
    /// Get timestamp for specific sample
    pub fn sample_timestamp(&self, sample_idx: usize) -> u64 {
        self.buffer.sample_timestamp(sample_idx)
    }
    
    /// Convert data type (for pipeline stages)
    pub fn with_type(mut self, new_type: EegDataType) -> Self {
        self.data_type = new_type;
        self
    }
    
    /// Check if buffer can be modified in-place
    pub fn is_mutable(&self) -> bool {
        Arc::strong_count(&self.buffer) == 1
    }
}

/// New pipeline data enum using zero-copy types
#[derive(Debug, Clone)]
pub enum PipelineData {
    /// Zero-copy EEG data
    Eeg(ZeroCopyEegData),
    
    /// Control signals (no data payload)
    Trigger,
    
    /// Error signal
    Error(String),
    
    /// Shutdown signal
    Shutdown,
}

impl PipelineData {
    /// Get timestamp if applicable
    pub fn timestamp(&self) -> Option<u64> {
        match self {
            PipelineData::Eeg(data) => Some(data.sample_timestamp(0)),
            _ => None,
        }
    }
    
    /// Get frame ID if applicable
    pub fn frame_id(&self) -> Option<u64> {
        match self {
            PipelineData::Eeg(data) => Some(data.frame_id()),
            _ => None,
        }
    }
}
```

### 1.3 Academic-Friendly Plugin API

```rust
// crates/pipeline/src/plugin_api.rs

/// High-level API for academic plugins (hides buffer management)
pub struct EegDataView<'a> {
    /// Per-channel data slices
    channels: Vec<&'a [f32]>,
    
    /// Metadata
    frame_id: u64,
    sample_rate: f32,
    timestamp_start: u64,
    sample_period_ns: u32,
}

impl<'a> EegDataView<'a> {
    /// Create view from zero-copy data
    pub fn from_zero_copy(data: &'a ZeroCopyEegData) -> Self {
        let mut channels = Vec::with_capacity(data.channel_count());
        
        for ch in 0..data.channel_count() {
            if let Some(samples) = data.channel_data(ch) {
                channels.push(samples);
            }
        }
        
        Self {
            channels,
            frame_id: data.frame_id(),
            sample_rate: data.sample_rate(),
            timestamp_start: data.sample_timestamp(0),
            sample_period_ns: data.buffer.sample_period_ns,
        }
    }
    
    /// Get channel data
    pub fn channel(&self, idx: usize) -> Option<&[f32]> {
        self.channels.get(idx).copied()
    }
    
    /// Get all channels
    pub fn channels(&self) -> &[&[f32]] {
        &self.channels
    }
    
    /// Get timestamp for sample
    pub fn sample_timestamp(&self, sample_idx: usize) -> u64 {
        self.timestamp_start + (sample_idx as u64 * self.sample_period_ns as u64)
    }
    
    /// Metadata accessors
    pub fn frame_id(&self) -> u64 { self.frame_id }
    pub fn sample_rate(&self) -> f32 { self.sample_rate }
    pub fn channel_count(&self) -> usize { self.channels.len() }
    pub fn sample_count(&self) -> usize { 
        self.channels.first().map(|ch| ch.len()).unwrap_or(0) 
    }
}

/// Mutable view for in-place processing
pub struct EegDataViewMut<'a> {
    /// Mutable per-channel data slices
    channels: Vec<&'a mut [f32]>,
    
    /// Metadata (read-only)
    frame_id: u64,
    sample_rate: f32,
    timestamp_start: u64,
    sample_period_ns: u32,
}

impl<'a> EegDataViewMut<'a> {
    /// Create mutable view (only if buffer has single owner)
    pub fn from_zero_copy(data: &'a mut ZeroCopyEegData) -> Option<Self> {
        if !data.is_mutable() {
            return None; // Buffer is shared - can't get mutable access
        }
        
        let channel_count = data.channel_count();
        let frame_id = data.frame_id();
        let sample_rate = data.sample_rate();
        let timestamp_start = data.sample_timestamp(0);
        let sample_period_ns = data.buffer.sample_period_ns;
        
        // Get mutable references to all channels
        let buffer = Arc::get_mut(&mut data.buffer)?;
        let mut channels = Vec::with_capacity(channel_count);
        
        // This is safe because we verified single ownership above
        unsafe {
            for ch in 0..channel_count {
                if let Some(samples) = buffer.channel_samples_mut(ch) {
                    // Extend lifetime - safe because we control the buffer
                    let samples_ptr = samples.as_mut_ptr();
                    let samples_len = samples.len();
                    let extended_slice = std::slice::from_raw_parts_mut(samples_ptr, samples_len);
                    channels.push(extended_slice);
                }
            }
        }
        
        Some(Self {
            channels,
            frame_id,
            sample_rate,
            timestamp_start,
            sample_period_ns,
        })
    }
    
    /// Get mutable channel data
    pub fn channel_mut(&mut self, idx: usize) -> Option<&mut [f32]> {
        self.channels.get_mut(idx).map(|ch| &mut **ch)
    }
    
    /// Get all mutable channels
    pub fn channels_mut(&mut self) -> &mut [&mut [f32]] {
        &mut self.channels
    }
}

/// Safe plugin trait with panic recovery
pub trait SafeEegPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    
    /// Process data with read-only access
    fn process(&mut self, input: EegDataView) -> Result<Vec<f32>, PluginError>;
    
    /// Process data in-place (optional - for performance)
    fn process_inplace(&mut self, data: &mut EegDataViewMut) -> Result<(), PluginError> {
        // Default implementation: not supported
        Err(PluginError::InPlaceNotSupported)
    }
