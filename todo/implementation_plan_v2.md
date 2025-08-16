# Pipeline Implementation Plan (v2): Arc-Meta Architecture

**Version History:** This document is a revision of the original implementation plan, updated to incorporate expert feedback regarding performance, future-proofing, and developer experience.

## 1. Core Architecture Recap

*   **Data Flow:** `Sensor Driver` -> `Packet<i32>` -> `ToVoltage Stage` -> `Packet<f32>` -> `Downstream Stages`
*   **Metadata:** A `Arc<SensorMeta>` struct, created by the driver, travels with every packet, making the data self-describing.
*   **Goal:** Decouple hardware-specific logic from the main processing pipeline, ensuring correctness, maintainability, and testability.

---

## 2. Data Structure Definitions (`crates/pipeline/src/data.rs`)

### `SensorMeta` Struct (v2)

This struct is expanded to be more future-proof.

```rust
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SensorMeta {
    pub schema_ver: u8,
    pub source_type: String,
    pub v_ref: f32,
    pub adc_bits: u8,
    pub gain: f32,
    pub sample_rate: u32,

    // v2 additions based on feedback
    /// The digital value corresponding to 0V.
    #[serde(default)]
    pub offset_code: i32,
    /// True if the ADC output is two's complement.
    #[serde(default = "true_default")]
    pub is_twos_complement: bool,
    /// Optional feature-gated tags for user-defined metadata.
    #[cfg(feature = "meta-tags")]
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

fn true_default() -> bool { true }
```

### `Packet` and `PacketHeader` Structs (v2)

`batch_size` is updated to `u32` to handle large bursts.

```rust
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Packet<T> {
    pub header: PacketHeader,
    /// Note: Consider `Arc<[T]>` or `Box<[T]>` in the future for true zero-copy from DMA buffers.
    pub samples: Vec<T>,
}

#[repr(C)] // For potential C FFI
#[derive(Clone, Debug)]
pub struct PacketHeader {
    /// Monotonic timestamp from the driver's sample acquisition clock.
    pub ts_ns: u64,
    pub batch_size: u32, // Changed from u16
    pub meta: Arc<SensorMeta>,
}
```

---

## 3. Step-by-Step Implementation Guide

### Step 1: Implement the `ToVoltage` Stage (v2)

This version includes the "sticky cache" optimization for the scale factor.

*   **File:** `crates/pipeline/src/stages/to_voltage.rs`

```rust
use crate::stage::{Stage, StageContext};
use crate::data::{Packet, SensorMeta};
use crate::error::StageError;
use std::sync::Arc;

pub struct ToVoltage {
    // Sticky cache for performance
    cached_meta_ptr: usize,
    cached_scale_factor: f32,
    cached_offset: i32,
}

impl Default for ToVoltage {
    fn default() -> Self {
        Self {
            cached_meta_ptr: 0,
            cached_scale_factor: 1.0,
            cached_offset: 0,
        }
    }
}

#[async_trait::async_trait]
impl Stage<i32, f32> for ToVoltage {
    async fn process(&mut self, packet: Packet<i32>, _ctx: &mut StageContext) -> Result<Option<Packet<f32>>, StageError> {
        let meta_ptr = Arc::as_ptr(&packet.header.meta) as usize;

        // If the metadata pointer hasn't changed, use the cached values.
        if self.cached_meta_ptr != meta_ptr {
            let meta = &packet.header.meta;
            // This logic runs only when the configuration changes.
            let full_scale_range = if meta.is_twos_complement {
                1i32 << (meta.adc_bits - 1)
            } else {
                1i32 << meta.adc_bits
            };
            self.cached_scale_factor = (meta.v_ref / meta.gain) / full_scale_range as f32;
            self.cached_offset = meta.offset_code;
            self.cached_meta_ptr = meta_ptr;
        }

        let samples_f32: Vec<f32> = packet.samples
            .into_iter()
            .map(|raw_sample| (raw_sample - self.cached_offset) as f32 * self.cached_scale_factor)
            .collect();

        let output_packet = Packet {
            header: packet.header,
            samples: samples_f32,
        };

        Ok(Some(output_packet))
    }
}
```

### Step 2: Prioritize Developer Experience

*   **Action:** Begin design and implementation of the `stage_def!` macro, as discussed in the original design notes. This will reduce boilerplate for all stages and should be developed in parallel with the pipeline refactor.

---

## 4. Testing Plan (v2)

*   **Unit Test:** Test the `ToVoltage` stage with various `SensorMeta` configurations.
*   **Property-Based Test:** Use `proptest` or `quickcheck` to create a test that generates random `SensorMeta` configurations and raw `i32` values, asserting that the `i32 -> f32 -> i32` round-trip conversion is within an acceptable tolerance. This will prevent silent regressions in scaling logic.
*   **Integration Test:** The end-to-end test (`driver -> ToVoltage -> Filter`) remains critical. It should assert that the `Arc<SensorMeta>` pointer is identical before and after the `ToVoltage` stage using `Arc::ptr_eq`.

---

## 5. Open Architectural Decisions & Defaults

This section documents key strategic questions and proposes default answers.

1.  **Error Handling:** How should `ToVoltage` handle scaling overflows or `NaN` results?
    *   **Default:** For initial implementation, **drop the packet and log an error**. This is the safest option. A future enhancement could be to add an error flag to the packet header.

2.  **Dynamic Gain Switching:** How to handle rapid config changes from the driver?
    *   **Default:** The current "sticky cache" design handles this correctly by recalculating on every pointer change. If this proves to be a performance issue due to excessive "pointer churn," a debouncing mechanism can be added to the driver later. This is not a concern for the initial implementation.

3.  **CI Benchmark Gate:** What is the primary performance metric?
    *   **Default:** **End-to-end latency** for a 1-second, 250 Hz, 8-channel packet through a standard `ToVoltage -> Filter -> Sink` pipeline. A regression of >5% should fail the build. This is a user-centric metric that captures the overall system performance.

---

## 6. Updated Implementation Checklist

1.  [ ] **`data.rs`**: Implement the v2 `SensorMeta` and `PacketHeader` structs.
2.  [ ] **`stages/to_voltage.rs`**: Implement the v2 `ToVoltage` stage with the sticky cache.
3.  [ ] **`sensors/.../driver.rs`**: Update a driver to produce packets with the v2 `SensorMeta`.
4.  [ ] **`tests.rs`**: Implement the property-based test for the scaling logic.
5.  [ ] **`tests.rs`**: Implement the end-to-end integration test.
6.  [ ] **Roadmap:** Begin design of the `stage_def!` macro.
7.  [ ] **`README.md`**: Update documentation to link to `architecture_discussion.md` and this plan.