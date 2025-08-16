# Pipeline Transition Plan

This document outlines the plan for transitioning the existing sensor pipeline to the new hybrid architecture. The transition is designed to be incremental, allowing for a gradual rollout and minimizing risk.

## 1. Phase 1: Coexistence

The first phase focuses on allowing the new data plane to coexist with the existing `tokio`-based runtime.

### 1.1. Bridge Stages

Two special "bridge" stages will be created:

*   **`ToDataPlane`:** This stage will run in the `tokio` runtime and will be responsible for converting `Arc`-based `PipelineData` into `Packet`-based `PipelineData`. It will acquire a packet from the `MemoryPool`, copy the data, and then push it into a `StageQueue` for the data plane.
*   **`FromDataPlane`:** This stage will run in the `tokio` runtime and will be responsible for converting `Packet`-based `PipelineData` back into `Arc`-based `PipelineData`. It will pop data from a `StageQueue` and copy it into a new `Arc`-based structure.

### 1.2. Initial Integration

The new `HybridRuntime` will be integrated into the main application, but it will initially only run a simple "passthrough" data plane that consists of a `ToDataPlane` stage, a single data plane stage (e.g., a no-op filter), and a `FromDataPlane` stage. This will allow for testing the core data plane functionality without affecting the existing pipeline.

## 2. Phase 2: Incremental Migration

The second phase focuses on migrating existing stages to the new data plane.

### 2.1. Stage-by-Stage Migration

Stages will be migrated one at a time, starting with the most performance-critical stages (e.g., the FIR filter). For each stage, a new `DataPlaneStage` implementation will be created, and the pipeline configuration will be updated to use the new implementation.

### 2.2. Performance Monitoring

Throughout the migration process, performance will be closely monitored to ensure that the new data plane is delivering the expected benefits. The existing metrics system will be extended to collect metrics from the data plane, including queue depths, packet acquisition times, and processing latencies.

## 3. Phase 3: Full Transition

The final phase focuses on completing the transition to the new architecture.

### 3.1. Removal of Bridge Stages

Once all performance-critical stages have been migrated to the data plane, the bridge stages will be removed, and the data plane will become the primary path for all high-frequency data. The `tokio`-based runtime will still be used for control, configuration, and low-frequency event handling.

### 3.2. Deprecation of Old `Stage` Trait

The old `PipelineStage` trait will be deprecated and eventually removed, and all new stages will be implemented using the `DataPlaneStage` trait.

## 4. Transition Timeline

The transition will be carried out over several sprints, with each sprint focusing on a specific set of stages. A detailed timeline will be created in the project management tool, but the high-level phases are as follows:

*   **Sprint 1-2:** Implement Phase 1 (Coexistence).
*   **Sprint 3-6:** Implement Phase 2 (Incremental Migration), starting with the FIR filter and then moving to other stages.
*   **Sprint 7:** Implement Phase 3 (Full Transition).

This phased approach will ensure a smooth and controlled transition to the new architecture, delivering performance improvements incrementally while minimizing risk.