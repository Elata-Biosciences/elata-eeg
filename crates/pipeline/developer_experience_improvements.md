# Improving the Pipeline Developer Experience

This document provides a detailed breakdown of the recommendations for making the pipeline system more approachable for new contributors. The goal is to keep the high-performance core while significantly reducing the cognitive overhead required to build and maintain pipeline stages.

---

## 1. Create a `Stage Developer's Guide`

A dedicated guide is the most effective way to onboard new developers. It should be a new file, `TUTORIAL.md`, in the `crates/pipeline` directory.

### Proposed `TUTORIAL.md` Structure

1.  **The Big Picture (In 30 Seconds):** A brief, high-level overview. "The pipeline runs in parallel threads. You write 'stages' that process data. You don't need to worry about threads; the system handles it for you."
2.  **Your First Stage: A Simple Pass-Through:** A complete, step-by-step walkthrough of creating a stage that does nothing but pass data through. This builds confidence.
3.  **The `process` Function: Your World:** Explain that 99% of a stage developer's work happens inside the `process` function.
4.  **Stage Cookbook: Common Recipes:** Practical, copy-pasteable examples for common tasks.

### Cookbook Example: A Low-Pass Filter

Here's what a "Cookbook" entry might look like.

> #### **Recipe: Filtering Data**
>
> Let's say you want to create a simple low-pass filter. You only care about the `Voltage` data packets.
>
> **1. Define your stage struct and descriptor:**
>
> ```rust
> // In your plugin crate, e.g., `crates/pipeline/src/stages/filter.rs`
> use pipeline_core::{PacketView, RtPacket};
> use plugin_api::{simple_stage, StageDesc, StageResult};
> use std::sync::Arc;
>
> // This struct will hold your filter's state.
> pub struct LowPassFilter {
>     alpha: f32,
>     last_value: f32,
> }
>
> // The descriptor for your stage in the YAML config.
> #[derive(Serialize, Deserialize)]
> pub struct LowPassFilterDesc {
>     pub cutoff_hz: f32,
> }
> ```
>
> **2. Implement the `StageImpl` trait:**
>
> ```rust
> // Using the simple_stage! macro to handle boilerplate
> simple_stage!(LowPassFilter, LowPassFilter(desc), {
>     // This is the `process` block.
>     // We only care about Voltage packets. Ignore everything else.
>     let PacketView::Voltage { header, data } = PacketView::from(&*pkt) else {
>         return Ok(Some(pkt)); // Pass non-voltage packets through untouched
>     };
>
>     // Create a new buffer for the filtered data.
>     // The `map_samples` helper allocates a new vector from the pool.
>     let new_pkt = pkt.map_samples(|_header, samples| {
>         let mut filtered_data = Vec::with_capacity(samples.len());
>         for &sample in samples {
>             let new_value = self.last_value + self.alpha * (sample - self.last_value);
>             self.last_value = new_value;
>             filtered_data.push(new_value);
>         }
>         filtered_data
>     });
>
>     // Return the new packet.
>     Ok(Some(Arc::new(new_pkt)))
> });
>
> // You also need to implement the `new` function to initialize the stage.
> impl StageImpl for LowPassFilter {
>     fn new(desc: &StageDesc, ctx: &StageInitCtx) -> anyhow::Result<Box<dyn StageImpl>> {
>         let StageDesc::LowPassFilter(params) = desc else {
>             anyhow::bail!("Expected LowPassFilter descriptor");
>         };
>
>         // Calculate the filter coefficient
>         let sample_rate = ctx.sample_rate(); // Get sample rate from context
>         let rc = 1.0 / (2.0 * std::f32::consts::PI * params.cutoff_hz);
>         let dt = 1.0 / sample_rate;
>         let alpha = dt / (rc + dt);
>
>         Ok(Box::new(LowPassFilter {
>             alpha,
>             last_value: 0.0,
>         }))
>     }
>     // ... process and descriptor methods handled by the macro
> }
> ```

---

## 2. Enhance Stage Creation Boilerplate

The `simple_stage!` macro is good, but a `#[derive(Stage)]` procedural macro would be even better.

### Discussion: `proc-macro` vs. `declarative macro`

A procedural macro can inspect the code it's attached to, allowing for more powerful and "magical" features.

### Proposed `#[derive(Stage)]` Implementation

The goal is to let a developer write only the logic they care about.

```rust
use plugin_api::Stage;

// The developer writes this:
#[derive(Stage)]
#[stage(
    desc = "LowPassFilterDesc",
    input = "RtPacket::Voltage",
    output = "RtPacket::Voltage"
)]
pub struct LowPassFilter {
    #[stage_param]
    cutoff_hz: f32,

    alpha: f32,
    last_value: f32,
}

impl LowPassFilter {
    // The `new` function is now just for custom logic.
    // The macro handles boilerplate parameter extraction.
    pub fn new(&mut self, ctx: &StageInitCtx) -> anyhow::Result<()> {
        let sample_rate = ctx.sample_rate();
        let rc = 1.0 / (2.0 * std::f32::consts::PI * self.cutoff_hz);
        let dt = 1.0 / sample_rate;
        self.alpha = dt / (rc + dt);
        Ok(())
    }

    // The `process` function gets the correct data type automatically.
    pub fn process(&mut self, data: &mut [f32], header: &PacketHeader) -> StageResult {
        for sample in data.iter_mut() {
            let new_value = self.last_value + self.alpha * (*sample - self.last_value);
            *sample = new_value;
            self.last_value = new_value;
        }
        Ok(None) // Modify in-place, so no new packet to return
    }
}
```

The `#[derive(Stage)]` macro would auto-generate the full `StageImpl` trait implementation, including:
*   The `new` function boilerplate.
*   The `process` function wrapper that performs the pattern matching on `RtPacket` and calls the user's `process` with the correct slice.
*   The `descriptor` function.

---

## 3. Improve Configuration & Introspection

### Better YAML Error Messages

When a pipeline fails to build, the error should be precise.

**Current (potential) error:**
`Error: Failed to build pipeline: Invalid stage configuration`

**Proposed error:**
```
Error: Failed to build pipeline from 'pipelines/eeg_processing.yaml'

  Invalid configuration for stage 'notch_filter':
  Missing required field: 'hz'

  at pipelines/eeg_processing.yaml:15:5
  |
  15 |   - name: "notch_filter"
  |     ^------------
  16 |     type: "Notch"
  17 |     # Missing 'hz' parameter
```

This requires building a more robust parser in the `daemon` that tracks line/column information from the YAML file.

### `GET /pipeline/graph` Endpoint

This endpoint would provide live introspection of the running pipeline.

**Request:**
`GET http://localhost:9000/pipeline/graph`

**Response (JSON):**
```json
{
  "nodes": [
    { "id": "eeg_source", "type": "eeg_source", "params": { "sample_rate": 1000 } },
    { "id": "to_voltage", "type": "to_voltage", "params": {} },
    { "id": "websocket_sink", "type": "websocket_sink", "params": { "topic": "eeg_data" } },
    { "id": "csv_sink", "type": "csv_sink", "params": { "path": "recording.csv" } }
  ],
  "edges": [
    { "from": "eeg_source", "to": "to_voltage", "label": "raw_data" },
    { "from": "to_voltage", "to": "websocket_sink", "label": "voltage_data" },
    { "from": "to_voltage", "to": "csv_sink", "label": "voltage_data" }
  ]
}
```
This JSON can be used to dynamically render a graph in a UI, which would be an invaluable debugging tool.

---

## 4. "Sanity Check" Test Suite

A contributor needs a simple way to verify their stage works correctly in a minimal pipeline.

### Example Integration Test

```rust
// In `crates/pipeline/tests/stage_integration_tests.rs`

#[test]
fn test_to_voltage_stage_produces_correct_output() {
    // 1. Define a minimal pipeline configuration as a string
    let yaml_config = r#"
    stages:
      - name: "source"
        type: "MockEegSource"
        params:
          data: [[100, -100]] # One packet with two samples
          vref: 5.0
          gain: 1.0
      - name: "converter"
        type: "ToVoltage"
        inputs: ["source.raw_data"]
    "#;

    // 2. Build and run the pipeline
    let mut pipeline = Pipeline::from_yaml(yaml_config).unwrap();
    let output = pipeline.run_once().unwrap(); // Run for one cycle

    // 3. Assert the output is correct
    let output_packet = output.get("converter.voltage_data").unwrap();
    let PacketView::Voltage { data, .. } = PacketView::from(output_packet) else {
        panic!("Expected voltage packet");
    };

    // V = (sample * Vref) / (gain * (2^23 - 1))
    let vref = 5.0;
    let gain = 1.0;
    let scale_factor = vref / (gain * 8388607.0);
    let expected_voltage_pos = 100.0 * scale_factor;
    let expected_voltage_neg = -100.0 * scale_factor;

    assert!((data[0] - expected_voltage_pos).abs() < 1e-9);
    assert!((data[1] - expected_voltage_neg).abs() < 1e-9);
}
```

This kind of test provides a clear, working example of how to use a stage and verifies its correctness in a controlled environment.