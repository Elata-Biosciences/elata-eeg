this is an architecture doc for the ai to understand the context of this directory rapidly

# EEG Driver Architecture

## 1. Introduction

**Purpose**: This document outlines the architecture of the `driver` sub-project. The `driver` project is a Rust-based system responsible for interfacing with EEG (Electroencephalogram) hardware, specifically the Texas Instruments ADS1299 EVM (Evaluation Module) connected to a Raspberry Pi 5. Its primary functions are to configure the EEG hardware, acquire raw EEG data, perform initial DSP (Digital Signal Processing) such as filtering, and provide this processed data to upstream consumers (e.g., the `daemon` process also found in this repository).

**Key Technologies**:
*   **Rust**: The programming language used for its performance, safety, and concurrency features.
*   **`tokio`**: An asynchronous runtime for Rust, used here to manage concurrent operations like data acquisition from the hardware, signal processing, and communication with client code.
*   **`rppal`**: A Rust library for Raspberry Pi General Purpose Input/Output (GPIO) and Serial Peripheral Interface (SPI) communication. Essential for interacting with the ADS1299 EVM.
*   **`biquad`**: A Rust crate providing implementations for IIR (Infinite Impulse Response) filters, used for DSP tasks like high-pass, low-pass, and notch filtering.

## 2. Core Concepts & Data Flow

**Overall Data Flow**:

1.  **EEG Electrodes**: Capture analog bio-potentials (voltage fluctuations) from the subject's scalp.
2.  **ADS1299 EVM Board**: This is the analog front-end (AFE). It conditions (amplifies, filters minimally) and digitizes the analog signals from the electrodes into digital data.
3.  **Raspberry Pi 5 (running the `driver` code)**: This is the host controller. It communicates with the ADS1299 EVM via the SPI bus and GPIO lines.
    *   **Configuration**: Sends commands via SPI to configure the ADS1299 (e.g., set sample rate, PGA gain for each channel, active channels, bias electrode settings, reference voltage).
    *   **Data Ready (DRDY) Monitoring**: Monitors a dedicated GPIO pin connected to the ADS1299's DRDY output. A transition on this pin (typically high-to-low) signals that the ADS1299 has a new set of samples ready for retrieval.
    *   **Data Retrieval**: Upon detecting a DRDY signal, the driver reads the digitized EEG data packets from the ADS1299 via SPI.
4.  The **`driver` software** (this project) processes the raw data received from the ADS1299:
    *   **Parsing**: Parses the multi-byte data packets. Each packet typically contains a status word and 24-bit (3-byte) samples for each active channel.
    *   **Conversion**: Converts the raw integer ADC (Analog-to-Digital Converter) counts from each channel into floating-point voltage values. This conversion uses the known reference voltage (VREF) of the ADS1299 and the programmed gain for that channel.
    *   **DSP**: *Note: The `SignalProcessor` ([`driver/src/dsp/filters.rs:96`](./src/dsp/filters.rs:96)) is designed to apply digital signal processing filters (e.g., high-pass, low-pass, notch). However, its integration into `EegSystem` is currently commented out as part of a DSP refactor plan. Therefore, the driver presently passes voltage-converted data without this DSP step.*
    *   **Batching**: Collects processed samples into batches, typically with an associated timestamp indicating the time of the first sample in the batch.
5.  **Upstream Consumer** (e.g., the `daemon` application): Receives these processed EEG data batches. The [`EegSystem`](./src/eeg_system/mod.rs:85) provides an asynchronous MPSC (Multi-Producer, Single-Consumer) channel (`tokio::sync::mpsc::Receiver`) for this purpose.

**Mermaid Diagram**:
```mermaid
sequenceDiagram
    participant Electrodes
    participant ADS1299_EVM
    participant Driver_RPi5 [Driver (Raspberry Pi 5)]
    participant Upstream_Consumer [Upstream Consumer (e.g., Daemon)]

    Electrodes->>ADS1299_EVM: Analog EEG Signals
    Driver_RPi5->>ADS1299_EVM: Configure Hardware (SPI Commands)
    ADS1299_EVM-->>Driver_RPi5: Data Ready Signal (DRDY GPIO Interrupt)
    Driver_RPi5->>ADS1299_EVM: Request Data Read (SPI Commands)
    ADS1299_EVM-->>Driver_RPi5: Raw Digital EEG Data (SPI Transfer)
    Driver_RPi5->>Driver_RPi5: Process Data (Convert to Voltage; Filtering via SignalProcessor currently bypassed)
    Driver_RPi5->>Upstream_Consumer: Processed EEG Data Batches (via MPSC Channel)
```

**Key Data Structures**:
*   [`AdcConfig`](./src/board_drivers/types.rs:39) (from [`driver/src/board_drivers/types.rs`](./src/board_drivers/types.rs:1)): The central configuration struct. It defines parameters like target sample rate, list of active channels, per-channel gain settings, the type of board driver to use (e.g., `Ads1299`, `Mock`), reference voltage (Vref), and data batch size. *Note: It does not currently include fields for DSP filter cutoff frequencies.*
*   [`AdcData`](./src/board_drivers/types.rs:72) (from [`driver/src/board_drivers/types.rs`](./src/board_drivers/types.rs:1)): Represents a single raw sample or a small, minimally processed block of data from an ADC channel, typically before full voltage conversion or filtering.
*   [`ProcessedData`](./src/lib.rs:12) (from [`driver/src/lib.rs`](./src/lib.rs:1)): Represents a batch of fully processed EEG data. This typically includes a timestamp for the batch, and a collection of voltage values for each active channel for the duration of the batch. This is the primary data type consumed by clients of the driver.
*   [`DriverEvent`](./src/board_drivers/types.rs:14) (from [`driver/src/board_drivers/types.rs`](./src/board_drivers/types.rs:1)): An enum representing events that a board driver might emit, such as `DataReady` (though often handled internally), `Error`, or `StatusChange`.
*   [`DriverError`](./src/board_drivers/types.rs:80) (from [`driver/src/board_drivers/types.rs`](./src/board_drivers/types.rs:1)): An enum used for error handling within the driver modules. It abstracts various underlying issues like I/O errors, SPI communication failures, or hardware-specific problems.
*   [`SignalProcessor`](./src/dsp/filters.rs:96) (from [`driver/src/dsp/filters.rs`](./src/dsp/filters.rs:1)): A struct that encapsulates the DSP pipeline. It is designed to manage a set of digital filters (high-pass, low-pass, notch) for each channel and apply them to the incoming voltage-converted samples. *Note: Its usage in `EegSystem` is currently commented out.*

## 3. Driver Architecture Overview

The driver is architected with several layers of abstraction to separate concerns and allow for potential future expansion to other hardware.

*   **Main Public Interface (`EegSystem`)**:
    *   Defined in [`driver/src/eeg_system/mod.rs`](./src/eeg_system/mod.rs:85).
    *   This struct serves as the primary entry point and orchestrator for applications using the EEG driver.
    *   It provides public methods like `new(config: AdcConfig)`, `start()`, `stop()`, `reconfigure(new_config: AdcConfig)`, and `shutdown()`.
    *   Upon creation (`new`), it initializes and holds an instance of a specific `AdcDriver` (e.g., `Ads1299Driver` or `MockDriver`) based on the provided `AdcConfig`.
    *   It is designed to manage an instance of the [`SignalProcessor`](./src/dsp/filters.rs:96) for applying DSP to the data, *however, this functionality is currently commented out in [`driver/src/eeg_system/mod.rs`](./src/eeg_system/mod.rs:1) as part of a DSP refactor plan.*
    *   It spawns `tokio` tasks for the data acquisition loop (managed by the `AdcDriver`) and the data processing pipeline.
    *   The `new` method returns a `tokio::sync::mpsc::Receiver<ProcessedData>`, which the client application uses to receive batches of processed EEG data.

*   **Board Driver Abstraction (`AdcDriver` Trait)**:
    *   Defined in [`driver/src/board_drivers/types.rs`](./src/board_drivers/types.rs:125).
    *   This is a `pub trait AdcDriver: Send + Sync + 'static` that specifies the common interface that all supported ADC hardware drivers must implement. This allows `EegSystem` to be generic over the actual hardware being used.
    *   Key methods defined by the trait include `start_acquisition(&mut self, data_tx: mpsc::Sender<Vec<i32>>)`, `stop_acquisition(&mut self)`, `get_status(&self) -> DriverStatus`, `get_config(&self) -> AdcConfig`, and `shutdown(&mut self)`. The `start_acquisition` method takes a sender channel to forward raw (but channel-separated and integer) data.
    *   The `create_driver` factory function, also in [`driver/src/board_drivers/types.rs`](./src/board_drivers/types.rs:136), is responsible for instantiating the correct concrete `AdcDriver` implementation (e.g., `Ads1299Driver`, `MockDriver`) based on the `AdcConfig.board_driver` field.

*   **ADS1299 Specific Implementation (`Ads1299Driver`)**:
    *   This is the concrete `AdcDriver` implementation for the Texas Instruments ADS1299 and ADS1299EVB evaluation module. It's located in [`driver/src/board_drivers/ads1299/driver.rs`](./src/board_drivers/ads1299/driver.rs:28).
    *   It implements all methods required by the `AdcDriver` trait, handling the specifics of ADS1299 communication and control.
    *   **Key Internal Modules & Responsibilities within `ads1299/`**:
        *   [`acquisition.rs`](./src/board_drivers/ads1299/acquisition.rs): Manages the core data acquisition loop. This involves setting up a `tokio` task that listens for DRDY interrupts (via an `InputPinDevice` abstraction from [`spi.rs`](./src/board_drivers/ads1299/spi.rs:1)) and, upon interrupt, reads a full data frame from the ADS1299 via SPI (using an `SpiDevice` abstraction from [`spi.rs`](./src/board_drivers/ads1299/spi.rs:1)). The raw data is then sent through an internal MPSC channel provided by `EegSystem` for further processing.
        *   [`spi.rs`](./src/board_drivers/ads1299/spi.rs): Abstracts low-level SPI communication and GPIO pin interactions (for DRDY, RESET, PWDN pins) using the `rppal` crate. It defines `SpiDevice` and `InputPinDevice` traits, which wrap `rppal` components. This abstraction aids in testability (e.g., allowing mock SPI/GPIO devices) and encapsulates `rppal` usage. Includes `init_spi()` and `init_drdy_pin()` for hardware setup.
        *   [`registers.rs`](./src/board_drivers/ads1299/registers.rs): Contains definitions for ADS1299 register addresses, default values, and bitfield constants. It also includes helper functions to convert high-level configuration parameters (like gain settings, sample rates) into the specific bitmasks required for writing to ADS1299 registers (e.g., `gain_to_reg_mask()`, `sps_to_reg_mask()`).
        *   [`helpers.rs`](./src/board_drivers/ads1299/helpers.rs): Provides various utility functions specific to ADS1299 data handling. This includes functions like `ch_sample_to_raw()` (to convert the 3-byte, 2's complement sample from an ADS1299 channel into an `i32` value) and `ch_raw_to_voltage()` (to convert this raw integer into a floating-point voltage value, considering VREF and gain). Also includes `current_timestamp_micros()`.
        *   [`builder.rs`](./src/board_drivers/ads1299/builder.rs): Implements the builder pattern (`Ads1299DriverBuilder`) for constructing `Ads1299Driver` instances. This allows for a more fluent and potentially more complex configuration process if needed.
        *   [`driver.rs`](./src/board_drivers/ads1299/driver.rs) (the main file for this module): Contains the `Ads1299Driver` struct definition, its `new()` constructor, the implementation of `AdcDriver` trait methods, and chip-specific control logic like `initialize_chip()`, `reset_chip()`, `read_register()`, `write_register()`, and sending ADS1299 commands (e.g., `RDATAC`, `SDATAC`).
        *   [`error.rs`](./src/board_drivers/ads1299/error.rs): Defines error types specific to ADS1299 operations, often wrapping lower-level `std::io::Error` or `rppal::gpio::Error` / `rppal::spi::Error` with more context relevant to the ADS1299 interaction.
        *   [`README.md`](./src/board_drivers/ads1299/README.md): **Crucial reference document.** Contains detailed hardware setup instructions, jumper configurations, pinout diagrams for connecting the ADS1299 EVM to a Raspberry Pi, and a summary of default register settings. Essential for anyone working directly with the hardware.

*   **Mock Driver Implementation (`MockDriver`)**:
    *   Located in [`driver/src/board_drivers/mock/driver.rs`](./src/board_drivers/mock/driver.rs:1) (with data generation logic in [`driver/src/board_drivers/mock/mock_data_generator.rs`](./src/board_drivers/mock/mock_data_generator.rs:1)).
    *   Provides a mock implementation of the `AdcDriver` trait. This is invaluable for development and testing of `EegSystem` and upstream consumers without needing physical EEG hardware.
    *   It generates synthetic EEG-like data (e.g., sine waves with configurable frequencies and amplitudes per channel) via the `MockDataGenerator`.

*   **DSP (Digital Signal Processing) - `SignalProcessor`**:
    *   The `SignalProcessor` struct is defined in [`driver/src/dsp/filters.rs`](./src/dsp/filters.rs:96).
    *   *Current Status: The instantiation and use of `SignalProcessor` within `EegSystem` ([`driver/src/eeg_system/mod.rs`](./src/eeg_system/mod.rs:1)) are currently commented out due to a "DSP refactor plan." Therefore, no DSP filtering is actively performed by the `SignalProcessor` in the driver at this time.*
    *   *Design Intent:* `SignalProcessor` is designed to be initialized directly with parameters such as `sample_rate`, `num_channels`, `dsp_high_pass_cutoff`, `dsp_low_pass_cutoff`, and `powerline_filter_hz` (see [`driver/src/dsp/filters.rs:105`](./src/dsp/filters.rs:105)). The `AdcConfig` struct ([`driver/src/board_drivers/types.rs:39`](./src/board_drivers/types.rs:39)) currently provides `sample_rate` and `num_channels` (implicitly via `config.channels.len()`) but lacks fields for the explicit filter cutoff frequencies. If `SignalProcessor` were to be re-enabled, `AdcConfig` would likely need to be extended to include these missing filter parameters, or they would need to be sourced from elsewhere.
    *   It maintains a set of digital filters (instances of `biquad::DirectForm2Transposed` from the `biquad` crate) for each active EEG channel.
    *   It provides methods like `process_sample()` and `process_chunk()` which *would be* called by `EegSystem` to apply these filters to the voltage-converted data.

*   **Configuration Management**:
    *   The primary mechanism for configuration is the [`AdcConfig`](./src/board_drivers/types.rs:39) struct.
    *   An `AdcConfig` instance is passed to `EegSystem::new()` to initialize the system. The configuration can be updated at runtime (if supported by the hardware and driver) via `EegSystem::reconfigure()`.
    *   `EegSystem` passes relevant parts of this configuration (like `sample_rate`, `channels`) to the instantiated `AdcDriver` (which then configures the actual hardware). *Currently, it does not pass filter cutoff configurations to `SignalProcessor` because `AdcConfig` lacks these fields and the `SignalProcessor`'s usage is commented out in `EegSystem`.*
    *   For the `Ads1299Driver`, the `initialize_chip` method uses the `AdcConfig` to program the various ADS1299 registers to match the desired settings (sample rate, gain, channel modes, bias, etc.).

*   **Error Handling**:
    *   The system primarily uses the [`DriverError`](./src/board_drivers/types.rs:80) enum as a common error type for operations within `AdcDriver` implementations and related modules.
    *   Methods in `EegSystem` and `AdcDriver` that can fail typically return `Result<T, DriverError>` or `Result<T, Box<dyn std::error::Error>>` (for broader compatibility at the `EegSystem` public API level).
    *   Errors from underlying operations (e.g., SPI communication failures from `rppal`, I/O errors, invalid configuration parameters) are wrapped into these more specific error types to provide better context to the caller.

## 4. Key Directories & Their Purpose

*   `driver/src/`: Root of the library and binary source code.
    *   [`lib.rs`](./src/lib.rs): The main library entry point. It defines the public API of the `eeg_driver` crate, including `ProcessedData`, and re-exports key components like `EegSystem`, `AdcConfig`, and `DriverType`.
    *   [`main.rs`](./src/main.rs): An example binary entry point. This demonstrates how to use the `eeg_driver` library to initialize the system, start acquisition, and receive data. It's useful for standalone testing or as a basic example for integrators.

*   `driver/src/eeg_system/`:
    *   [`mod.rs`](./src/eeg_system/mod.rs): Contains the `EegSystem` struct, which is the main orchestrator and public interface for the driver.
    *   [`tests.rs`](./src/eeg_system/tests.rs): Contains unit and integration tests for the `EegSystem`, often utilizing the `MockDriver`.

*   `driver/src/board_drivers/`: This directory houses the core abstractions for board drivers and the specific implementations for different ADC hardware.
    *   [`types.rs`](./src/board_drivers/types.rs): Defines the fundamental `AdcDriver` trait, common data structures like `AdcConfig`, `AdcData`, `DriverStatus`, `DriverEvent`, `DriverError`, the `DriverType` enum (to select between different hardware drivers), and the `create_driver` factory function.
    *   `ads1299/`: A submodule containing all code specific to the Texas Instruments ADS1299 driver. This includes its own `driver.rs`, `acquisition.rs`, `spi.rs`, `registers.rs`, `helpers.rs`, `builder.rs`, `error.rs`, and `README.md`.
    *   `mock/`: A submodule for the `MockDriver`. Contains `driver.rs` for the mock implementation and `mock_data_generator.rs` for creating synthetic EEG data.

*   `driver/src/dsp/`: Contains modules related to Digital Signal Processing.
    *   [`filters.rs`](./src/dsp/filters.rs): Implements the `SignalProcessor` struct and various digital filter types (HighPass, LowPass, Notch) using the `biquad` crate.
    *   [`mod.rs`](./src/dsp/mod.rs): Declares the `filters` module, making `SignalProcessor` available to other parts of the driver (primarily `EegSystem`).

## 5. How to Approach Common Tasks

*   **Adding Support for a New ADC Board**:
    1.  Create a new subdirectory under `driver/src/board_drivers/` (e.g., `driver/src/board_drivers/new_board_xyz/`).
    2.  Inside this new directory, create at least a `driver.rs` file. Implement the [`AdcDriver`](./src/board_drivers/types.rs:125) trait for your new hardware. This will involve writing code to handle its specific communication protocol (e.g., SPI, I2C), configure its registers, manage its data acquisition sequence, and handle its specific error conditions.
    3.  Add any necessary helper modules within your `new_board_xyz/` directory (e.g., for register definitions, communication protocol helpers, data parsing specific to that board).
    4.  Add a new variant to the `DriverType` enum in [`driver/src/board_drivers/types.rs`](./src/board_drivers/types.rs:32) to represent your new board.
    5.  Update the `create_driver` factory function in [`driver/src/board_drivers/types.rs`](./src/board_drivers/types.rs:136) to include a match arm for your new `DriverType` variant. This arm should instantiate and return your new driver.
    6.  Consider if any new fields are needed in [`AdcConfig`](./src/board_drivers/types.rs:39) if your new board has unique configuration parameters not covered by the existing fields.
    7.  Write comprehensive tests for your new driver, potentially including mock hardware interactions if possible.

*   **Modifying ADS1299 Configuration Logic (e.g., supporting a new ADS1299 feature or changing default behavior)**:
    1.  Identify the ADS1299 registers involved by carefully consulting the official ADS1299 datasheet.
    2.  If new register addresses, bitfield definitions, or helper functions for register manipulation are needed, add them to [`driver/src/board_drivers/ads1299/registers.rs`](./src/board_drivers/ads1299/registers.rs).
    3.  If a new high-level configuration option is needed for users of the driver, update the [`AdcConfig`](./src/board_drivers/types.rs:39) struct (and ensure the `Ads1299Driver`'s `get_config` method reflects this).
    4.  Modify the `initialize_chip` method (or other relevant methods like `write_register`, or add new specific configuration methods) in [`driver/src/board_drivers/ads1299/driver.rs`](./src/board_drivers/ads1299/driver.rs) to use the new configuration option to correctly set the ADS1299 registers.
    5.  Update the [`driver/src/board_drivers/ads1299/README.md`](./src/board_drivers/ads1299/README.md) if hardware jumper settings or wiring is affected.

*   **Changing DSP Filtering (e.g., adding a new type of filter, modifying filter parameters, or changing the filter chain)**:
    1.  Modifications would primarily occur in the `SignalProcessor` struct and its associated filter implementations within [`driver/src/dsp/filters.rs`](./src/dsp/filters.rs).
    2.  This might involve adding new filter types (if supported by the `biquad` crate, or by implementing a new `Biquad` or filter structure) or changing how existing filters are configured (e.g., order, Q-factor) or chained.
    3.  If new configuration parameters are needed for the DSP stage (e.g., a Q-factor for a notch filter, or parameters for a new filter type), these would typically be added to [`AdcConfig`](./src/board_drivers/types.rs:39). *If re-enabling `SignalProcessor` in `EegSystem`, ensure that `SignalProcessor::new()` ([`driver/src/dsp/filters.rs:105`](./src/dsp/filters.rs:105)) and `SignalProcessor::reset()` ([`driver/src/dsp/filters.rs:203`](./src/dsp/filters.rs:203)) are correctly called with these parameters, and that `EegSystem` is updated to pass them from the (modified) `AdcConfig`.*
    4.  Update tests in [`driver/src/eeg_system/tests.rs`](./src/eeg_system/tests.rs) (like `test_signal_processing`) to verify the new DSP behavior.

*   **Debugging Hardware Communication Issues (specifically with ADS1299)**:
    1.  Add extensive logging (e.g., using the `log` crate with `log::debug!`, `log::trace!`, or simple `println!` statements for quick checks) in key areas:
        *   [`driver/src/board_drivers/ads1299/spi.rs`](./src/board_drivers/ads1299/spi.rs): Log data being written to and read from SPI.
        *   [`driver/src/board_drivers/ads1299/acquisition.rs`](./src/board_drivers/ads1299/acquisition.rs): Log DRDY events, timing of reads, and raw data frames.
        *   [`driver/src/board_drivers/ads1299/driver.rs`](./src/board_drivers/ads1299/driver.rs): Log register read/write operations (addresses and values), command sequences, and state transitions.
    2.  Thoroughly double-check all physical hardware connections (SPI lines: SCLK, MOSI, MISO, CS; GPIO lines: DRDY, RESET, PWDN; power lines: DVDD, AVDD, DGND, AGND) against the pinout specified in [`driver/src/board_drivers/ads1299/README.md`](./src/board_drivers/ads1299/README.md) and the ADS1299 EVM schematics.
    3.  Verify SPI communication parameters (mode, clock speed, bit order) in [`driver/src/board_drivers/ads1299/spi.rs`](./src/board_drivers/ads1299/spi.rs) against the ADS1299 datasheet requirements.
    4.  If available, use a logic analyzer to inspect the SPI signals (SCLK, MOSI, MISO, CS) and the DRDY line in real-time. This can reveal issues with signal integrity, timing, or incorrect data transfers.
    5.  Utilize the test functions in [`driver/src/board_drivers/ads1299/test.rs`](./src/board_drivers/ads1299/test.rs) for isolated register read/write tests if the hardware setup allows direct execution of these tests.

*   **Understanding Raw Data Format from ADS1299**:
    1.  Refer to the `read_data_from_spi` function (or similar logic) within [`driver/src/board_drivers/ads1299/acquisition.rs`](./src/board_drivers/ads1299/acquisition.rs) or [`driver/src/board_drivers/ads1299/helpers.rs`](./src/board_drivers/ads1299/helpers.rs) to see how bytes are read from the SPI bus and assembled into individual channel samples.
    2.  The ADS1299 typically outputs a status word (24 bits) followed by 24-bit (3-byte), two's complement samples for each active channel in a data frame. Consult the "Data Format" section of the official ADS1299 datasheet for the precise structure and interpretation of these frames.

*   **Modifying the `EegSystem` Behavior (e.g., changing batching strategy, error handling at a higher level)**:
    1.  Changes to the overall lifecycle management, data batching logic, how drivers and signal processors are orchestrated, or top-level error handling and recovery would typically be made within the [`EegSystem`](./src/eeg_system/mod.rs:85) implementation in [`driver/src/eeg_system/mod.rs`](./src/eeg_system/mod.rs).