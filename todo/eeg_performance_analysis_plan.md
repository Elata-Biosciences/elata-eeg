# EEG System Performance Analysis Plan (Updated 6/4/2025)

This document outlines a plan to analyze the performance of the EEG system, from data acquisition through various processing stages, including applet-specific DSP like FFT. This version reflects findings from the initial information gathering phase.

## I. Information Gathering & Context Refinement (Completed)

The initial information gathering phase has been completed. Key findings include:
*   The `daemon` process is responsible for primary IIR filtering, configured via `DaemonConfig.filter_config`.
*   The `driver`'s `AdcConfig` does not currently specify IIR filter parameters for its internal `SignalProcessor`. The exact nature of filtering within the `driver` needs further clarification during performance analysis.
*   Applet-specific DSP (e.g., Brain Waves FFT) runs as a separate server process on port `8081`, spawned by the `daemon` if the corresponding feature flag is enabled. This DSP server receives data from a broadcast channel within the `daemon`.
*   The Kiosk UI consumes the main filtered EEG data from the `daemon`'s `/ws/eeg/data__basic_voltage_filter` endpoint (JSON) and applet-specific data from dedicated WebSocket endpoints like `/applet/brain_waves/data` (served by the separate DSP server).

The detailed understanding derived from this phase is presented below.

## II. Consolidated Understanding of EEG System Data Flow & Processing

1.  **Driver (`eeg_driver` crate on RPi5):**
    *   Interfaces with EEG hardware (e.g., ADS1299) via SPI.
    *   Configured by `AdcConfig` ([`driver/src/board_drivers/types.rs:39`](../../driver/src/board_drivers/types.rs:39-46)), which specifies `sample_rate`, active `channels`, `gain`, `batch_size` (for driver), and `Vref`.
    *   Acquires raw ADC samples.
    *   Converts raw samples to voltage.
    *   **`AdcConfig` ([`driver/src/board_drivers/types.rs:39`](../../driver/src/board_drivers/types.rs:39-46)) does *not* currently specify IIR filter parameters (e.g., high-pass, low-pass cutoffs).**
    *   **The `SignalProcessor` ([`driver/src/dsp/filters.rs:96`](../../driver/src/dsp/filters.rs:96)) in the `driver` is currently **bypassed** due to a "DSP refactor plan" (as seen in commented-out code in [`driver/src/eeg_system/mod.rs`](../../driver/src/eeg_system/mod.rs:1)).
    *   Therefore, the `driver` currently performs **no DSP filtering**; it only converts raw ADC samples to voltage. The `daemon` is responsible for primary IIR filtering.
    *   If the `SignalProcessor` were active, it would expect explicit filter parameters (e.g., `dsp_high_pass_cutoff`, `dsp_low_pass_cutoff`) to be passed to its `new`/`reset` methods, which are not currently part of `AdcConfig`.
    *   Emits `ProcessedData` (batches of voltage samples) to the `daemon` via an MPSC channel.

2.  **Daemon (`daemon` process on RPi5):**
    *   Receives `ProcessedData` from the `driver`.
    *   **Applies IIR Filters**: The `daemon` itself is responsible for the primary IIR filtering (high-pass, low-pass, notch) based on `FilterConfig` within `DaemonConfig` ([`daemon/src/config.rs:6-14`](../../daemon/src/config.rs:6-14), [`daemon/src/config.rs:39-40`](../../daemon/src/config.rs:39-40)).
    *   **CSV Recording**: If active, writes the (daemon-filtered) data to CSV files. Configured by `max_recording_length_minutes` and `recordings_directory` in `DaemonConfig`.
    *   **`/ws/eeg/data__basic_voltage_filter` WebSocket Streaming (Port 8080)**:
        *   Transforms data (daemon-filtered `ProcessedData`) into a JSON format.
        *   Streams this JSON data to Kiosk clients (e.g., [`kiosk/src/components/EegDataHandler.tsx`](../../kiosk/src/components/EegDataHandler.tsx)).
        *   The `batch_size` in `DaemonConfig` likely influences this streaming.
    *   **`/eeg` WebSocket Streaming (Port 8080)**:
        *   The [`daemon/src/main.rs`](../../daemon/src/main.rs:54-55) also sets up a broadcast channel `tx_eeg_batch_data` which is used by the `/eeg` endpoint ([`daemon/src/server.rs:122-125`](../../daemon/src/server.rs:122-125) from `daemon/ai_prompt.md`) to send `EegBatchData` in a custom binary format. It's also the source for the applet DSP.
    *   **Applet DSP Service (e.g., Brain Waves FFT - Port 8081):**
        *   If the `brain_waves_fft_feature` is enabled, the `daemon` spawns a *separate Warp server process* on port `8081` ([`daemon/src/main.rs:132-136`](../../daemon/src/main.rs:132-136)).
        *   This separate server is managed by the `elata_dsp_brain_waves_fft` library ([`dsp/brain_waves_fft/src/lib.rs`](../../dsp/brain_waves_fft/src/lib.rs)).
        *   It subscribes to the `tx_eeg_batch_data` broadcast channel (containing `EegBatchData` from the daemon, likely *after* daemon-level IIR filtering) and `AdcConfig` updates from the main daemon.
        *   Performs FFT calculations based on its internal parameters (`FFT_WINDOW_DURATION_SECONDS`, `FFT_WINDOW_SLIDE_SECONDS`) and the received `sample_rate`.
        *   Serves FFT results via the `/applet/brain_waves/data` WebSocket endpoint.

3.  **Kiosk UI (Client Machine):**
    *   **`EegDataHandler.tsx`**: Connects to `/ws/eeg/data__basic_voltage_filter` on port `8080` to receive and display the main (daemon-filtered) EEG waveforms. Client-side FFT calculation in this component appears to be disabled/removed.
    *   **`AppletFftRenderer.tsx`** (for Brain Waves applet): Connects to `/applet/brain_waves/data` on port `8081` to receive and display FFT data from the dedicated applet DSP service. Lifecycle is managed by this component.

## III. Updated Data Flow Diagram

```mermaid
graph TD
    subgraph Raspberry Pi 5
        subgraph Driver Process (driver)
            A[EEG Hardware Interface (ADS1299 via SPI/DRDY)] --> B{Raw ADC Data};
            B --> C[Parse & Convert to Voltage];
            C --> C_DSP[Driver: SignalProcessor (Currently Bypassed - No DSP in Driver)];
            C_DSP -- ProcessedData (Driver Output) --> E(MPSC Channel to Daemon);
        end

        subgraph Daemon Process (daemon - Port 8080)
            E --> F{Received ProcessedData};
            F --> F_DSP[Daemon: IIR Filters (based on DaemonConfig.filter_config)];
            F_DSP -- DaemonFilteredData --> G[Optional: CSV Recorder];
            G -- CSV Files --> H[Disk Storage (SD Card)];

            F_DSP -- DaemonFilteredData_ForWS --> I_JSON[Transform to JSON for /ws/eeg/data__basic_voltage_filter];
            I_JSON --> K_JSON[WebSocket Server: /ws/eeg/data__basic_voltage_filter (JSON Stream)];

            F_DSP -- EegBatchData_ForBroadcast --> J[Broadcast Channel for /eeg & Applet DSP];
            J --> K_Binary[WebSocket Server: /eeg (Binary EegBatchData Stream)];
        end

        subgraph Applet DSP Process (e.g., brain_waves_fft - Port 8081, if feature enabled)
            J -- EegBatchData (from Daemon Broadcast) --> M_AppletDSP[Applet DSP: e.g., brain_waves_fft (FFT Calc)];
            M_AppletDSP -- AppletSpecificResults (e.g., FFT) --> N[Broadcast Channel for Applet Data (internal to Applet DSP server)];
            N --> O_Applet[WebSocket Server: /applet/brain_waves/data (Applet Stream)];
        end
    end

    subgraph Kiosk UI (Client Machine)
        K_JSON -- Filtered EEG Data (JSON) --> P[EegDataHandler & Renderer for main waveforms];
        K_Binary -- EEG Data Stream (Binary) --> P_Binary_Consumer_Note["Note: /eeg binary stream consumer in Kiosk? (If any, besides applets)"];
        O_Applet -- Applet FFT Data Stream --> Q[AppletFftRenderer];
    end

    style Driver Process fill:#lightblue,stroke:#333,stroke-width:2px
    style "Daemon Process (daemon - Port 8080)" fill:#lightgreen,stroke:#333,stroke-width:2px
    style "Applet DSP Process (e.g., brain_waves_fft - Port 8081, if feature enabled)" fill:#lightcoral,stroke:#333,stroke-width:2px
    style "Kiosk UI (Client Machine)" fill:#lightyellow,stroke:#333,stroke-width:2px

    classDef bottleneck fill:#ffcccc,stroke:#cc0000,stroke-width:2px;
    class C_DSP,F_DSP,M_AppletDSP,G bottleneck;
```

## IV. Refined Plan for Performance Analysis & Bottleneck Identification (Next Steps)

This refined plan incorporates our updated understanding.

**Summary of Investigations (as of 6/4/2025 session):**

*   **Driver `SignalProcessor` Configuration (IV.1 Action Item):**
    *   **Investigated & Answered:** The `SignalProcessor` in the `driver` is currently **bypassed** (code related to its use in [`driver/src/eeg_system/mod.rs`](../../driver/src/eeg_system/mod.rs:1) is commented out).
    *   `AdcConfig` ([`driver/src/board_drivers/types.rs:39`](../../driver/src/board_drivers/types.rs:39-46)) does not contain filter cutoff parameters.
    *   The `SignalProcessor` itself ([`driver/src/dsp/filters.rs:105`](../../driver/src/dsp/filters.rs:105)) is designed to take explicit cutoff frequencies if it were active.
    *   Therefore, the `driver` currently performs no DSP filtering. The `daemon` is responsible for primary IIR filtering.
    *   The corresponding "Open Question" in Section VII has been marked as answered.
    *   The `driver/ai_prompt.md` has been updated to reflect these findings.

*   **`batch_size` Relationship (Driver vs. Daemon) (VII. Open Question):**
    *   **Investigated & Answered:**
        *   `AdcConfig.batch_size` (Driver): Determines samples bundled into one `ProcessedData` object sent from `driver` to `daemon`.
        *   `DaemonConfig.batch_size` (Daemon): Primarily used to re-chunk data for the unfiltered `/eeg` WebSocket. Not directly used for the filtered `/ws/eeg/data__basic_voltage_filter` WebSocket or CSV recording, which process data based on the incoming `AdcConfig.batch_size`.
    *   The corresponding "Open Question" in Section VII has been marked as answered.

*   **Performance Impact of WebSocket Streams (VII. Open Question):**
    *   **Partially Addressed (Qualitatively):** Inferences made based on data types (JSON vs. binary) and processing (filtered vs. unfiltered).
    *   **Remaining:** Quantitative performance impact (CPU, network, latency) requires actual profiling and measurement, which is outside the scope of pure code analysis.

*   **Other Items in Section IV:** Most other specific metric collection and profiling tasks listed in Section IV (e.g., CPU cycles for specific operations, SPI throughput, detailed latency measurements for various stages) remain to be performed through direct measurement and profiling tools.

**IV.1. Driver Performance (Acquisition & Initial Processing on RPi5):**
    *   **Focus**: CPU usage of the `driver` process, particularly the `EegSystem` and its internal `SignalProcessor` ([`driver/src/dsp/filters.rs`](../../driver/src/dsp/filters.rs)).
        *   **Action (Completed for Configuration Aspect)**: Investigation revealed that the `SignalProcessor` in the `driver` is currently bypassed (code commented out in [`driver/src/eeg_system/mod.rs`](../../driver/src/eeg_system/mod.rs:1)). `AdcConfig` ([`driver/src/board_drivers/types.rs:39`](../../driver/src/board_drivers/types.rs:39-46)) does not contain filter cutoff parameters. The `SignalProcessor` itself ([`driver/src/dsp/filters.rs:105`](../../driver/src/dsp/filters.rs:105)) expects explicit cutoff frequencies if it were to be used. Thus, the `driver` currently performs no filtering.
        *   **Next Action (Performance Aspect, if re-enabled)**: If `SignalProcessor` were to be re-enabled in the driver, its CPU load would need to be measured.
    *   **Metrics**:
        *   Latency from hardware DRDY to `ProcessedData` being available to the `daemon`.
        *   CPU cycles/time per sample/channel for any processing done by the `driver`'s `SignalProcessor`.
        *   SPI communication throughput and latency.
    *   **Concerns**: Impact of high sample rates/channel counts on RPi5 CPU. Overhead of `rppal`.

**IV.2. Daemon Performance (Core Tasks on RPi5 - Port 8080):**
    *   **IIR Filtering**:
        *   **Focus**: CPU usage of the daemon applying IIR filters based on `DaemonConfig.filter_config`. This is now understood to be a primary filtering stage.
        *   **Metrics**: CPU cycles/time per sample/channel for daemon-side IIR filtering.
        *   **Concerns**: Complexity of filters (order, type) vs. RPi5 CPU capacity.
    *   **CSV Recording**:
        *   **Focus**: Disk I/O for writing (daemon-filtered) data.
        *   **Metrics**: Time to write batches, impact on other tasks. CPU overhead of CSV formatting.
        *   **Concerns**: SD card write speed.
    *   **`/ws/eeg/data__basic_voltage_filter` WebSocket Streaming (JSON)**:
        *   **Focus**: Network throughput, CPU for data transformation (to JSON) and serialization.
        *   **Metrics**: End-to-end latency to Kiosk. Max sustainable data rate.
        *   **Concerns**: JSON serialization overhead.
    *   **`/eeg` WebSocket Streaming (Binary `EegBatchData`)**:
        *   **Focus**: Network throughput, CPU for binary serialization ([`daemon/src/server.rs:85`](../../daemon/src/server.rs:85) `create_eeg_binary_packet`).
        *   **Metrics**: Max sustainable data rate.
        *   **Concerns**: Overhead of custom binary format if complex.

**IV.3. Applet DSP Performance (e.g., Brain Waves FFT on RPi5 - Port 8081):**
    *   **Focus**: CPU and memory usage of the separate applet DSP server process (e.g., `elata_dsp_brain_waves_fft`).
    *   **Metrics**:
        *   Time to compute FFT per data window/batch (considering `FFT_WINDOW_DURATION_SECONDS`, `FFT_WINDOW_SLIDE_SECONDS`, and `sample_rate`).
        *   Impact on overall RPi5 CPU/memory when this DSP server is active.
        *   Latency of data from `daemon` broadcast to FFT results available on `/applet/.../data` WebSocket.
    *   **Concerns**: This remains a **potential major performance hotspot** due to FFT complexity, especially with multiple channels or high data rates/sample rates.

**IV.4. Overall System Throughput & Latency:**
    *   **Metrics**:
        *   End-to-end latency: Electrodes to Kiosk UI (for main filtered stream via `/ws/eeg/data__basic_voltage_filter` and for applet-processed stream via `/applet/.../data`).
        *   Maximum sustainable `sample_rate` and channel count before system overload.
    *   **Concerns**: Resource contention (CPU, memory, disk I/O, network) on RPi5. Interaction effects between the `driver`, main `daemon` (filtering, WS), and separate applet DSP server(s).

## V. Potential Optimizations & Recommendations (Initial High-Level - To be reviewed after Section IV)

*   **Offloading Intensive DSP**: If applet DSP (like FFT) running on the RPi5 is a significant bottleneck:
    *   Consider client-side processing in the Kiosk UI (using JavaScript or WebAssembly compiled from Rust). This shifts CPU load to the client machine.
    *   Evaluate if a more powerful dedicated co-processor for DSP is feasible (though likely outside scope of software-only changes).
*   **Driver Optimizations**:
    *   Clarify and potentially optimize filter application in the `driver`'s `SignalProcessor`.
    *   Review `biquad` filter implementations for efficiency if used in driver.
    *   Ensure optimal use of `tokio` for asynchronous operations.
    *   Profile SPI communication for any unexpected latencies.
*   **Daemon Optimizations**:
    *   **IIR Filtering**: Optimize filter implementations (`biquad` usage).
    *   **CSV Writing**: Ensure robust buffering and fully asynchronous writes to minimize impact on real-time tasks.
    *   **WebSocket Streaming**: Optimize serialization (JSON and binary), consider compression for WebSocket data if bandwidth is a constraint (trade-off with CPU).
    *   **Task Management**: Efficiently manage concurrent tasks, especially if multiple applets with server-side DSP might run.
*   **Configuration Tuning**: Provide guidance on realistic sample rates, channel counts, and DSP complexity suitable for the RPi5's capabilities to manage user expectations and prevent system overload.
*   **Resource Monitoring**: Implement or recommend tools for monitoring RPi5 CPU, memory, disk I/O, and network usage to help identify bottlenecks during operation.

## VI. Final Report Structure (Deliverable - To be reviewed after Section IV)

1.  **Executive Summary**: Overview of system performance, key findings, and top recommendations.
2.  **Detailed Data Flow and Processing Architecture**: Updated analysis and diagrams (as presented here).
3.  **Performance Analysis by Component**:
    *   Driver (Acquisition, Initial Processing).
    *   Daemon (IIR Filtering, Core tasks: CSV, `/ws/eeg/data__basic_voltage_filter` & `/eeg` WebSockets).
    *   Applet DSP (e.g., FFT on Port 8081).
4.  **Identified Bottlenecks and Resource Constraints**: Detailed discussion of limitations on the RPi5 (CPU, Disk I/O, Network, Memory).
5.  **Recommendations for Optimization**: Prioritized list of actionable recommendations.
6.  **Open Questions / Areas for Deeper Investigation**: Suggestions for further profiling, testing, or measurements needed for more precise analysis.

## VII. Open Questions / Areas for Deeper Investigation (To be reviewed after Section IV)

*   **[Answered]** What specific filtering, if any, is performed by the `driver`'s `SignalProcessor` and how is it configured?
    *   *Finding (6/4/2025): The `SignalProcessor` in the `driver` is currently **bypassed** (code related to its use in [`driver/src/eeg_system/mod.rs`](../../driver/src/eeg_system/mod.rs:1) is commented out). `AdcConfig` ([`driver/src/board_drivers/types.rs:39`](../../driver/src/board_drivers/types.rs:39-46)) does not contain filter cutoff parameters. The `SignalProcessor` itself ([`driver/src/dsp/filters.rs:105`](../../driver/src/dsp/filters.rs:105)) is designed to take explicit cutoff frequencies if it were active. Therefore, the `driver` currently performs no DSP filtering.*
*   **[Partially Addressed - Profiling Needed]** What is the performance impact of the two different EEG data WebSocket streams from the daemon (`/ws/eeg/data__basic_voltage_filter` vs. `/eeg`)?
    *   *Qualitative Analysis (6/4/2025): `/ws/eeg/data__basic_voltage_filter` (JSON, filtered) sends larger chunks based on `AdcConfig.batch_size`, includes daemon-side filtering and JSON serialization overhead. `/eeg` (Binary, unfiltered) sends chunks based on `DaemonConfig.batch_size`, has no daemon filtering for this stream, and uses more efficient binary serialization. Quantitative impact requires profiling.*
*   **[Answered]** How does the `batch_size` in `AdcConfig` (driver) relate to the `batch_size` in `DaemonConfig` (daemon) and affect overall data flow and latency?
    *   *Finding (6/4/2025):*
        *   **`AdcConfig.batch_size` (Driver):** Determines the number of hardware samples bundled into a single `ProcessedData` object sent from the `driver` to the `daemon`. This introduces an initial latency of (`AdcConfig.batch_size` / `sample_rate`) before data reaches the `daemon`.
        *   **`DaemonConfig.batch_size` (Daemon):**
            *   **Used for `/eeg` (unfiltered) WebSocket:** The `daemon` receives `ProcessedData` (containing `AdcConfig.batch_size` samples) and re-chunks it into smaller batches of `DaemonConfig.batch_size` for this specific WebSocket stream.
            *   **Not directly used for `/ws/eeg/data__basic_voltage_filter` (filtered) WebSocket:** This endpoint sends the entire filtered block of `AdcConfig.batch_size` samples in one go.
            *   **Not used for CSV recording:** CSV recording writes individual samples.
        *   **Overall Latency Impact:**
            *   The primary latency from batching before daemon processing is due to `AdcConfig.batch_size`.
            *   For the `/eeg` stream, `DaemonConfig.batch_size` influences the size/frequency of WebSocket messages *after* the initial driver latency.
            *   For the filtered stream, `AdcConfig.batch_size` dictates the WebSocket message payload size.
*   What is the performance impact of the two different EEG data WebSocket streams from the daemon (`/