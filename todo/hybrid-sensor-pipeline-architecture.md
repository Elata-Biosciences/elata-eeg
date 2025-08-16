# Hybrid Sensor Pipeline Architecture

This document outlines a new hybrid architecture for the sensor processing pipeline, designed to meet the high-performance, real-time requirements of the project. It addresses the key issues raised in the `pipeline_performance.md` review by introducing a dedicated high-performance data path while retaining the flexibility of the existing graph-based system.

## 1. Core Principles

The new architecture is based on the following principles:

*   **Zero-Copy Data Flow:** Data packets are pre-allocated from a memory pool and passed by reference through the pipeline. This eliminates heap allocations and memory copies during processing.
*   **Backpressure:** Bounded, lock-free queues are used for communication between stages, providing natural backpressure and preventing memory exhaustion.
*   **Separation of Concerns:** The high-performance data path is separated from the general-purpose graph management and control logic.
*   **Incremental Adoption:** The new architecture can be adopted incrementally, allowing for a smooth transition from the existing system.

## 2. Architectural Overview

The hybrid architecture consists of two main components:

1.  **The Control Plane:** This is the existing `PipelineGraph` and `PipelineRuntime`, responsible for configuring, managing, and monitoring the pipeline. It will continue to use `tokio` channels for control messages and low-frequency events.
2.  **The Data Plane:** This is a new, high-performance data path responsible for the real-time processing of sensor data. It uses a pre-allocated memory pool, bounded lock-free queues, and a single-threaded execution model for the critical path.

The following Mermaid diagram illustrates the new architecture:

```mermaid
graph TD
    subgraph Control Plane (Existing Tokio-based Runtime)
        A[PipelineRuntime] -- Manages --> B{PipelineGraph};
        B -- Configures --> C{Stage Instances};
    end

    subgraph Data Plane (New High-Performance Path)
        D[Memory Pool] -- Allocates --> E(Data Packets);
        F[Acquisition Stage] -- Pushes to --> G{Queue 1};
        G -- Pops from --> H[Filter Stage];
        H -- Pushes to --> I{Queue 2};
        I -- Pops from --> J[Sink Stage];
    end

    A -- "Sends control messages (e.g., start, stop)" --> F;
    A -- " " --> H;
    A -- " " --> J;

    F -- "Returns packet to" --> D;
    H -- " " --> D;
    J -- " " --> D;

    style D fill:#f9f,stroke:#333,stroke-width:2px
    style G fill:#ccf,stroke:#333,stroke-width:2px
    style I fill:#ccf,stroke:#333,stroke-width:2px
```

## 3. Key Components

### 3.1. Memory Pool

A pre-allocated slab or arena allocator will be used to manage a pool of `EegPacket` instances. This will eliminate heap allocations during runtime and reduce memory fragmentation. Stages will "check out" a packet from the pool, process it, and then "check it back in" when they are done.

### 3.2. Bounded, Lock-Free Queues

Communication between stages in the data plane will use bounded, lock-free queues (e.g., from the `crossbeam` or `rtrb` crates). This provides a high-performance, thread-safe mechanism for data exchange with built-in backpressure.

### 3.3. Data Packet Lifecycle

1.  The acquisition stage requests a new packet from the memory pool.
2.  The packet is filled with sensor data.
3.  A reference to the packet is pushed into the queue for the next stage.
4.  The next stage pops the reference, processes the data (potentially in-place), and pushes it to the following queue.
5.  The final stage (the sink) processes the data and then returns the packet to the memory pool.

### 3.4. `PipelineData` Refactoring

The `PipelineData` enum will be refactored to support the new data flow. The `RawEeg`, `FilteredEeg`, and `Fft` variants will hold a special smart pointer (e.g., a custom `PoolPtr`) that manages the lifecycle of the packet within the memory pool, instead of an `Arc`.

## 4. Benefits

*   **Predictable Performance:** The elimination of runtime allocations and the use of lock-free queues will lead to more predictable, low-latency performance.
*   **Memory Safety:** The bounded queues and memory pool prevent unbounded memory growth.
*   **Flexibility:** The hybrid approach retains the flexibility of the existing graph-based system for configuration and control.
*   **Testability:** The separation of the data plane and control plane simplifies testing.

## 5. Next Steps

The next step is to create a detailed implementation plan, followed by a transition plan to migrate the existing pipeline to the new architecture.