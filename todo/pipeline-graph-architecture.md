# EEG Data Pipeline: Dataflow Graph Architecture

## Overview

This document describes a flexible, future-proof architecture for EEG data processing using a dataflow graph (pipeline) approach. It covers rationale, design, code/config examples, implementation details, and best practices for both developers and data scientists.

**Key Concepts:**
- **Graph-Based Pipelines:** Pipelines are modeled as directed acyclic graphs (DAGs), allowing stages to be shared, fanned out, and referenced by multiple endpoints.
- **Explicit Data Contracts:** Each sink (CSV, WebSocket, etc.) specifies the exact data schema/fields it expects, ensuring clarity and interoperability.
- **Runtime Introspection & Control:** The system exposes pipeline configs, stage parameters, and runtime state via API for transparency and UI integration.
- **Stage-Level Locking:** Locking and mutability are managed at the pipeline stage instance level—each stage is unique, even if it performs the same task for different outputs.
- **Versioning & Error Handling:** Pipeline configs are versioned for reproducibility, and error handling strategies are defined for robust operation.

---

## 1. Rationale

- **Transparency:** Every stage and parameter is explicit and versioned with each session.
- **Reproducibility:** All settings (acquisition, filtering, endpoints) are saved, enabling exact reproduction of experiments.
- **Extensibility:** New endpoints (e.g., CSV, WebSocket, OSC) and processing stages can be added without major refactoring.
- **Efficiency:** Shared stages (e.g., filtering) are computed once, with outputs fanned out to multiple endpoints.

---

## 2. Architecture Diagram

```mermaid
graph TD
    A[ADS1299 Acquire (sps=500, gain=24)] --> B[To Voltage (vref=4.5)]
    B --> C[Filtering (lowpass=0.5)]
    C --> D[Display (WebSocket ws://filtered_data)]
    C --> E[CSV Record]
    B --> F[CSV Record (raw)]
```

- **Note:** Filtering is performed once, then fanned out to both display and CSV record. Raw voltage can also be recorded.

---

## 3. Pipeline Config Example: From Linear to Graph-Based

### Notes on Pipeline Structure

The initial approach models each pipeline as a linear sequence of stages. However, to support shared computation and true fan-out, a graph-based (DAG) config is recommended.

#### Linear Example (for reference)
```
two pipelines
1) signal display pipeline...
 - acquire -> to_voltage -> filtering -> display
2) csv record pipeline...
 - acquire -> to_voltage -> filter -> save this
 - acquire -----------------------^

so if both pipelines at once, it's like this:
 - acquire -> to_voltage -> filtering -> display
 - acquire -> to_voltage -> filtering -> save raw and filtered data
 - acquire -----------------------^
so filtering should be done once, but goes to two different end points
```

#### Graph-Based JSON Example

Stages are defined once and referenced by name, allowing explicit fan-out and deduplication:

```json
{
  "stages": [
    {
      "name": "acquire1",
      "type": "acquire",
      "params": {"sps": 500, "gain": 24}
    },
    {
      "name": "to_voltage1",
      "type": "to_voltage",
      "params": {"vref": 4.5},
      "inputs": ["acquire1"]
    },
    {
      "name": "filter1",
      "type": "filter",
      "params": {"lowpass": 0.5},
      "inputs": ["to_voltage1"]
    },
    {
      "name": "display_ws",
      "type": "websocket_sink",
      "params": {
        "endpoint": "ws://filtered_data",
        "fields": ["timestamp", "filtered_channels"],
        "format": "json"
      },
      "inputs": ["filter1"]
    },
    {
      "name": "csv_filtered",
      "type": "csv_sink",
      "params": {
        "path": "filtered.csv",
        "fields": ["timestamp", "filtered_channels"]
      },
      "inputs": ["filter1"]
    },
    {
      "name": "csv_raw",
      "type": "csv_sink",
      "params": {
        "path": "raw.csv",
        "fields": ["timestamp", "raw_channels"]
      },
      "inputs": ["to_voltage1"]
    }
  ]
}
```

**Key Points:**
- Each stage is uniquely named and can be referenced as an input by multiple downstream stages.
- Sinks (CSV, WebSocket) specify their data schema/fields explicitly.
- This model supports deduplication, fan-out, and clear data contracts.


- **Shared Stages:** The runtime can optimize so that `acquire` and `to_voltage` are only run once per unique parameter set, and their outputs are fanned out to all consumers.

- **Stage-Level Locking:** Each pipeline stage instance is unique and can be locked independently. If a stage is in use (e.g., during recording), its parameters cannot be changed until it is idle. This ensures data integrity and reproducibility. If a change is attempted, the system can either block the change or stop the affected pipeline(s).
  _Note: This approach is chosen for clarity and safety, but future implementations could explore more granular or optimistic locking strategies. If ambiguity remains, document the current thinking and open questions for future contributors._

---

## 4. Rust Implementation Sketch

### 4.1. Pipeline Stage Trait

```rust
use async_trait::async_trait;

#[async_trait]
pub trait PipelineStage: Send + Sync {
    type Input: Send + Sync + 'static;
    type Output: Send + Sync + 'static;

    async fn process(&mut self, input: Self::Input) -> Self::Output;
    fn name(&self) -> &'static str;
}
```

### 4.2. Example Stage Struct

```rust
pub struct ToVoltage {
    vref: f32,
}

#[async_trait]
impl PipelineStage for ToVoltage {
    type Input = RawEegData;
    type Output = VoltageData;

    async fn process(&mut self, input: RawEegData) -> VoltageData {
        // ... conversion logic ...
    }
    fn name(&self) -> &'static str { "to_voltage" }
}
```

### 4.3. Pipeline Graph Construction

- Parse the config/DSL.
- Deduplicate stages with identical parameters.
- Wire stages with bounded channels (`tokio::sync::mpsc`).
- Spawn each stage as an async task.
- Endpoints (display, CSV, etc.) are terminal nodes.

---

## 5. Dynamic Endpoints & Hot Swapping

- **Add/Remove Endpoints:** When a user enables/disables a plugin/applet (e.g., CSV record), the pipeline graph is updated. Only active endpoints consume resources.
- **No Dead Branches:** If an endpoint is removed, upstream stages are only kept alive if still needed by other endpoints.

---

## 6. Parameterization & Session Metadata

- All stage parameters are explicit in the config and can be saved with each recording.
- Session metadata includes the full pipeline config, ensuring reproducibility.

---

## 7. Exposing Endpoints

- Each endpoint (WebSocket, OSC, UDP, CSV, etc.) is a terminal node with its own route/URI and parameters.
- The pipeline can expose its structure and parameters to endpoints, so clients and recorders always know the full context.
- **Runtime Introspection:** The system should provide an API (REST, WebSocket, etc.) to query the current pipeline graph, stage parameters, and runtime state. This enables advanced UI features, live monitoring, and remote debugging.
- **Config Versioning:** Every pipeline config should be versioned and saved with each recording session, ensuring full reproducibility and traceability.
- **Error Handling:** Define how errors in one stage affect downstream stages and the overall pipeline. For now, the system should surface errors to the UI and halt affected pipelines, but future work could explore more granular error recovery.

---

## 8. Data Science & Reproducibility

- Every recording is traceable to its exact acquisition and processing settings.
- Enables future analysis, comparison, and publication with full provenance.

---

## 9. User Experience & DSL Design

- **Simple Mode:** Predefined pipelines for common use cases.
- **Advanced Mode:** Custom pipelines via JSON/DSL.
- **Documentation:** Provide clear docs and error messages. Consider a visual editor for pipelines.

---

## 10. Best Practices

- Keep stage implementations modular and stateless where possible.
- Use bounded channels to avoid unbounded memory growth.
- Log all pipeline configs and parameters with each session.
- Validate pipeline configs at startup and provide clear errors.
- Consider versioning the pipeline config format.

---

## 11. Example: Minimal Pipeline Runtime (Pseudocode)

```rust
// Parse config
let config = load_pipeline_config("pipeline.json");

// Build graph, deduplicate stages
let graph = build_pipeline_graph(&config);

// For each stage, spawn async task
for stage in graph.stages() {
    tokio::spawn(run_stage(stage));
}

// Endpoints consume data and expose via WebSocket, CSV, etc.
```

---

## 12. Migration Path

- Start with a static pipeline, but design stages as composable units.
- Gradually introduce config-driven graph construction.
- Add endpoint/plugin support as needed.

---

## 13. Summary Table

| Approach         | Flexibility | Reproducibility | Complexity | Data Science Friendliness |
|------------------|-------------|-----------------|------------|--------------------------|
| Static Pipeline  | Low         | Medium          | Low        | Medium                   |
| Dataflow Graph   | High        | High            | Medium     | High                     |

---

## 14. References & Further Reading

- [Dataflow Programming](https://en.wikipedia.org/wiki/Dataflow_programming)
- [Tokio mpsc channels](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html)
- [async_trait crate](https://docs.rs/async-trait/latest/async_trait/)

---

## 15. FAQ

**Q: What if two endpoints need the same filtered data?**  
A: The runtime deduplicates stages, so filtering is only performed once per unique parameter set.

**Q: Can I change pipeline parameters at runtime?**  
A: Yes, but the pipeline will be rebuilt and restarted to ensure consistency.

**Q: How are errors handled?**  
A: Each stage should propagate errors upstream, and the runtime should log and surface errors clearly.

---

## 16. Contact

For questions or contributions, see [`plugins/README.md`](../plugins/README.md) or contact the core development team.
