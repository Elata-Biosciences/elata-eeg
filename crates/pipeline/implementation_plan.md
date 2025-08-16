# Implementation Plan: Improving Developer Experience

This document outlines a phased implementation plan for the developer experience enhancements detailed in `developer_experience_improvements.md`. The goal is to deliver value incrementally, starting with the highest-impact, lowest-effort tasks.

---

## Phase 1: Quick Wins (Immediate Impact)

**Goal:** Lower the barrier to entry for new contributors *now*.

1.  **Write the `TUTORIAL.md`:**
    *   **Task:** Create the `crates/pipeline/TUTORIAL.md` file as outlined in the documentation.
    *   **Focus:** The "Pass-Through" and "Low-Pass Filter" examples are critical.
    *   **Effort:** Low. This is primarily a documentation task.
    *   **Impact:** High. It immediately gives new contributors a starting point.

2.  **Improve YAML Error Messages:**
    *   **Task:** Modify the pipeline builder in `crates/daemon/src/server.rs` (or similar) to use a YAML parser that retains line/column information (e.g., `serde_yaml::with_positions`).
    *   **Focus:** Wrap the parsing and stage-building logic in a `Result` that includes contextual error information.
    *   **Effort:** Medium. Requires modifying the configuration loading and error handling paths.
    *   **Impact:** High. Prevents frustrating debugging sessions caused by simple typos in the config.

---

## Phase 2: Core Abstractions (Reducing Boilerplate)

**Goal:** Make writing stages fundamentally easier and more declarative.

1.  **Develop the `#[derive(Stage)]` Proc-Macro:**
    *   **Task:** Create a new crate, `plugin_api_macros`, to house the procedural macro.
    *   **Focus:** Start with the basic functionality: generating the `StageImpl` trait and the `new` function boilerplate. The attribute parsing (`#[stage(...)]`) is the most complex part.
    *   **Effort:** High. Procedural macros are complex and require a solid understanding of Rust's token streams and syntax trees.
    *   **Impact:** High. This is the most significant long-term improvement for developer ergonomics.

2.  **Refactor Existing Stages:**
    *   **Task:** Once the proc-macro is available, refactor one or two existing stages (e.g., `ToVoltage`, `CsvSink`) to use it.
    *   **Focus:** This serves as a validation of the macro's design and provides clear examples of its use.
    *   **Effort:** Low.

---

## Phase 3: Advanced Tooling (Enhancing Debuggability)

**Goal:** Provide powerful tools for introspection and validation.

1.  **Implement the `GET /pipeline/graph` Endpoint:**
    *   **Task:** Add a new route to the `axum` server in `crates/daemon/src/server.rs`.
    *   **Focus:** The handler for this route will need to query the running pipeline executor for its graph structure and serialize it to the specified JSON format. This may require adding a new message type to the control channel for the executor.
    *   **Effort:** Medium.
    *   **Impact:** Medium. Invaluable for debugging and for building future UI tools.

2.  **Create the "Sanity Check" Test Suite:**
    *   **Task:** Create a new test file, `crates/pipeline/tests/stage_integration_tests.rs`.
    *   **Focus:** Add 2-3 tests for core stages, following the example in the documentation. This establishes the pattern for future contributors.
    *   **Effort:** Medium.
    *   **Impact:** High. Increases code quality and contributor confidence.

---

## Recommended Order of Operations

1.  `TUTORIAL.md` (Phase 1)
2.  YAML Error Messages (Phase 1)
3.  Sanity Check Test Suite (Phase 3) - *Moved up, as it helps validate all other changes.*
4.  `#[derive(Stage)]` Proc-Macro (Phase 2)
5.  Refactor Existing Stages (Phase 2)
6.  `GET /pipeline/graph` Endpoint (Phase 3)