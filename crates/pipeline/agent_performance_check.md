# Elata EEG: Data Pipeline Performance & Architecture Guardrails

## 1. Purpose

This document establishes the core architectural principles and performance requirements for the Elata EEG real-time data pipeline. Its purpose is to prevent design erosion and performance regressions as the codebase evolves.

**All code changes affecting the `sensors`, `boards`, `daemon`, or `pipeline` crates MUST be checked against these guardrails during code review.**

---

## 2. Core Principles

1.  **The Data Path is Sacred:** The path a data packet takes from the sensor driver to a pipeline sink is the most critical part of the system. Performance, predictability, and low latency are the highest priorities here.
2.  **Block, Don't Poll:** Threads waiting for data or events must consume zero CPU. This is non-negotiable. Polling with `thread::sleep` is strictly forbidden in the data path.
3.  **Static Over Dynamic:** The compiler is our most powerful optimization tool. The architecture must favor static dispatch (`enums`, generics) over dynamic dispatch (`Box<dyn Trait>`, `Box<dyn Any>`) wherever possible to allow for maximum compiler optimization.
4.  **Enforce Backpressure:** The system must be resilient to downstream slowness. Data producers must block if a consumer is not ready. This prevents uncontrolled memory growth and makes system behavior under load predictable.

---

## 3. Performance Checklist & Guardrails

### 3.1. Threading & Concurrency

| Rule                                                                                             | Rationale                                                                                             |
| ------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------- |
| **[MUST]** Use blocking receives (`recv`, `recv_timeout`) for inter-thread communication.          | Avoids inefficient "busy-wait" loops, minimizes CPU usage, and reduces latency.                       |
| **[MUST NOT]** Use `thread::sleep` as a mechanism for waiting on data.                             | Introduces artificial latency and is an unreliable synchronization primitive.                         |
| **[MUST]** Use bounded channels (`mpsc::sync_channel`) for all data transfer between threads.      | Enforces backpressure, preventing memory exhaustion if a consumer thread is slow.                     |
| **[MUST]** Run the hardware acquisition thread(s) at an elevated, real-time OS priority.           | Guarantees that time-critical hardware reads are not delayed by other processes.                      |
| **[MUST NOT]** Use `Mutex` for data structures that are only accessed by a single thread.          | `Mutex` adds unnecessary locking overhead. Use `RefCell` or direct ownership in single-threaded contexts. |

### 3.2. Data Pipeline & Processing

| Rule                                                                                             | Rationale                                                                                             |
| ------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------- |
| **[MUST NOT]** Pass data between pipeline stages using `Box<dyn Any>`.                            | Prevents compiler optimizations and incurs significant runtime overhead from dynamic dispatch/downcasting. |
| **[MUST]** Use a concrete `enum` of packet types for data flowing through the pipeline.            | Enables static dispatch, allowing the compiler to aggressively optimize the processing path.           |
| **[MUST NOT]** Perform heap allocations (e.g., `Vec::new`, `.collect()`) for every data packet.    | Causes memory churn and allocator pressure. Reuse buffers or use in-place operations.                 |
| **[MUST]** Move all data transformations (e.g., `i32`->`f32` conversion) into the pipeline itself. | The main event loop should only be a simple, low-overhead message forwarder.                          |

### 3.3. Hardware Drivers & Synchronization

| Rule                                                                                             | Rationale                                                                                             |
| ------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------- |
| **[MUST]** Synchronize multi-sensor data using a strict, time-gated mechanism.                   | Prevents data stream de-synchronization. Stale packets from a delayed sensor must be discarded.       |
| **[MUST]** Implement a consecutive error counter in hardware drivers.                              | A driver should not loop indefinitely on errors. It must enter a failed state after a threshold.      |
