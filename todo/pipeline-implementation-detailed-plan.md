# Pipeline Implementation - Detailed Plan

This document provides a detailed plan for implementing the new hybrid sensor pipeline architecture. It builds upon the concepts outlined in `hybrid-sensor-pipeline-architecture.md` and provides a concrete roadmap for implementation.

## 1. Phase 1: Memory Pool and Data Packet

The first phase focuses on creating the core data structures for the new data plane.

### 1.1. `Packet<T>` Smart Pointer

A custom smart pointer, `Packet<T>`, will be created to manage the lifecycle of data packets within the memory pool. It will have the following characteristics:

*   It will wrap a `&'static mut T`, providing access to the underlying data.
*   It will implement `Deref` and `DerefMut` for easy access to the data.
*   It will implement `Drop` to automatically return the packet to the memory pool when it goes out of scope.
*   It will be `Send` but not `Sync`, as each packet should only be owned by one stage at a time.

### 1.2. `MemoryPool<T>`

A `MemoryPool<T>` will be created to manage the allocation and deallocation of packets. It will have the following API:

```rust
pub struct MemoryPool<T> {
    // ...
}

impl<T> MemoryPool<T> {
    /// Creates a new memory pool with a fixed capacity.
    pub fn new(capacity: usize) -> Self;

    /// Acquires a new packet from the pool, blocking if none are available.
    pub async fn acquire(&self) -> Packet<T>;

    /// Tries to acquire a new packet from the pool, returning `None` if none are available.
    pub fn try_acquire(&self) -> Option<Packet<T>>;

    /// Returns a packet to the pool. This will be called by the `Drop` implementation of `Packet<T>`.
    fn release(&self, packet: &'static mut T);
}
```

The underlying implementation will use a lock-free queue (e.g., `crossbeam::queue::ArrayQueue`) to manage the free list of packets.

### 1.3. `PipelineData` Refactoring

The `PipelineData` enum will be refactored to use `Packet<T>` for the high-performance data types:

```rust
pub enum PipelineData {
    RawEeg(Packet<EegPacket>),
    FilteredEeg(Packet<FilteredEegPacket>),
    Fft(Packet<FftPacket>),
    // ... other variants remain the same
}
```

## 2. Phase 2: Bounded, Lock-Free Queues

The second phase focuses on the communication channels between stages.

### 2.1. `StageQueue<T>`

A new queue type, `StageQueue<T>`, will be created as a wrapper around a bounded, lock-free queue (e.g., `rtrb::RingBuffer`). It will provide a simple `push`/`pop` interface for `Packet<T>` instances.

### 2.2. `Input` and `Output` Traits

To abstract the queueing mechanism, `Input` and `Output` traits will be defined:

```rust
#[async_trait]
pub trait Input<T> {
    async fn recv(&mut self) -> Option<T>;
}

#[async_trait]
pub trait Output<T> {
    async fn send(&mut self, data: T);
}
```

The `StageQueue` will implement these traits.

## 3. Phase 3: New `Stage` Trait and Runtime

The final phase focuses on integrating the new components into the pipeline runtime.

### 3.1. `DataPlaneStage` Trait

A new `DataPlaneStage` trait will be defined for stages that operate on the high-performance data path:

```rust
#[async_trait]
pub trait DataPlaneStage: Send + Sync {
    type Input;
    type Output;

    async fn run(
        &mut self,
        input: impl Input<Self::Input>,
        output: impl Output<Self::Output>,
        // ... other context, e.g., cancellation token
    );
}
```

### 3.2. `HybridRuntime`

A new `HybridRuntime` will be created to manage the execution of the data plane. It will be responsible for:

*   Instantiating the `MemoryPool`.
*   Creating the `StageQueue` instances.
*   Spawning a dedicated thread for the data plane.
*   Running the `DataPlaneStage` implementations in a tight loop.

### 3.3. Integration with `PipelineGraph`

The `PipelineGraph` will be extended to support the new `DataPlaneStage`s. A new `stage_type` (e.g., `data_plane`) will be introduced to distinguish them from the existing `tokio`-based stages. The `HybridRuntime` will be responsible for executing these stages, while the `PipelineRuntime` will continue to manage the overall graph.

## 4. Implementation Checklist

- [ ] Implement `Packet<T>` smart pointer.
- [ ] Implement `MemoryPool<T>`.
- [ ] Refactor `PipelineData` to use `Packet<T>`.
- [ ] Implement `StageQueue<T>`.
- [ ] Define `Input` and `Output` traits.
- [ ] Define `DataPlaneStage` trait.
- [ ] Implement `HybridRuntime`.
- [ ] Update `PipelineGraph` to support `data_plane` stages.
- [ ] Create example stages using the new `DataPlaneStage` trait.
- [ ] Write comprehensive tests for all new components.