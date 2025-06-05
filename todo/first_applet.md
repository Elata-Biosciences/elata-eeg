# Plan: "Brain Waves" FFT Applet (Backend First, Manual Setup)

This plan outlines the steps to create the first EEG applet, "Brain Waves," focusing on backend Rust DSP implementation and manual integration into the daemon. This approach is adopted due to the current unavailability of `eeg-applet-cli` and `build_dsp.py` scripts.

**Overall Goal:** Implement a new "brain_waves" applet that performs FFT-based spectral analysis using a dedicated Rust DSP crate. The results will be exposed via a WebSocket endpoint by the daemon.

**Phase 1: Core DSP Crate and Applet Manifest (Manual Setup)**

1.  **Manually Create Rust DSP Crate Structure (`dsp/brain_waves_fft`):**
    *   **Action:** Create the following directory and file structure:
        ```
        dsp/
        └── brain_waves_fft/
            ├── Cargo.toml
            └── src/
                └── lib.rs
        ```
    *   **`dsp/brain_waves_fft/Cargo.toml` Content:**
        ```toml
        [package]
        name = "elata_dsp_brain_waves_fft"
        version = "0.1.0"
        edition = "2021"

        [dependencies]
        # For FFT. Consider `rustfft`.
        # rustfft = "6.1"
        # For windowing functions if implementing Welch's method (e.g., Hann).
        # ndarray = "0.15"

        [lib]
        name = "elata_dsp_brain_waves_fft" # Ensure this matches the package name for clarity
        path = "src/lib.rs"
        ```
    *   **`dsp/brain_waves_fft/src/lib.rs` Implementation:**
        *   Define a public function, e.g., `pub fn process_eeg_data(data: &[f32], sample_rate: f32) -> Result<(Vec<f32>, Vec<f32>), String>`.
            *   `data`: Slice of raw EEG data for a single channel.
            *   `sample_rate`: Sample rate of the EEG data.
        *   **Initial Implementation Focus:**
            *   Perform a basic FFT (e.g., using `rustfft` if added as a dependency).
            *   Calculate the power spectrum (magnitude squared of FFT results).
            *   Generate corresponding frequency bins.
        *   **Future Enhancements (Post-Initial Backend):**
            *   Implement Welch's method (segmentation, windowing, averaging).
            *   Apply a log10 transform to the power spectrum.
        *   Return a `Result` containing a tuple: `(power_spectrum_vector, frequency_bins_vector)`, or an error string.

2.  **Manually Create Applet Structure & Manifest (`applets/brain_waves`):**
    *   **Action:** Create the directory `applets/brain_waves/`.
    *   **`applets/brain_waves/manifest.json` Content:**
        ```json
        {
          "id": "brain_waves_fft_applet",
          "version": "0.1.0",
          "name": "Brain Waves FFT (Backend Dev)",
          "description": "Displays real-time brain wave activity using spectral analysis. Backend development version.",
          "dsp": "brain_waves_fft",
          "dspWebSocketPath": "/applet/brain_waves/data",
          "requiredKioskApiVersion": "1.0.0"
        }
        ```
    *   **Action:** Create an empty directory `applets/brain_waves/ui/` as a placeholder for future UI components.

**Phase 2: Daemon Integration (Manual Setup)**

1.  **Manually Update `daemon/Cargo.toml`:**
    *   **Add Dependency:** Add `elata_dsp_brain_waves_fft` as an optional path dependency:
        ```toml
        # In [dependencies] section
        elata_dsp_brain_waves_fft = { path = "../dsp/brain_waves_fft", optional = true }
        ```
    *   **Workspace Member:** Ensure `../dsp/brain_waves_fft` is included in the `[workspace.members]` list.
    *   **Feature Definition:** Define a Cargo feature to enable this DSP module. This allows selective compilation.
        ```toml
        [features]
        # default = [] # (if not already defined)
        # applets = ["brain_waves_fft_feature"] # Example if using a master 'applets' feature
        brain_waves_fft_feature = ["dep:elata_dsp_brain_waves_fft"]
        ```
        *(Note: Adapt feature naming and structure to align with any existing patterns in `daemon/Cargo.toml` for applets/DSPs, as outlined in `todo/applets.md`.)*

2.  **Manually Modify Daemon Source Code (e.g., `daemon/src/main.rs` or relevant applet handling module):**
    *   **Conditional Compilation:** Use `#[cfg(feature = "brain_waves_fft_feature")]` to conditionally compile and use the `elata_dsp_brain_waves_fft` crate.
        ```rust
        // At the top of the relevant module
        #[cfg(feature = "brain_waves_fft_feature")]
        use elata_dsp_brain_waves_fft; // Or specific items like: elata_dsp_brain_waves_fft::process_eeg_data;

        // ...
        // Within the WebSocket handler for the "/applet/brain_waves/data" path
        #[cfg(feature = "brain_waves_fft_feature")]
        fn handle_brain_waves_request(/* parameters like WebSocket sender, raw EEG data source */) {
            // 1. Obtain raw EEG data chunk for a channel and the sample rate.
            //    (How the daemon gets this data needs to be determined based on existing data flow from the driver).
            //    let eeg_chunk: Vec<f32> = ...;
            //    let sample_rate: f32 = ...;

            // 2. Call the DSP function:
            //    match elata_dsp_brain_waves_fft::process_eeg_data(&eeg_chunk, sample_rate) {
            //        Ok((power_spectrum, frequency_bins)) => {
            //            // 3. Serialize the result (e.g., to JSON).
            //            //    let response = format!("{{\"power\": {:?}, \"freq\": {:?}}}", power_spectrum, frequency_bins);
            //            // 4. Send the serialized data over the WebSocket to the client.
            //        }
            //        Err(e) => {
            //            // Handle error, e.g., log it and/or send an error message over WebSocket.
            //            eprintln!("Error processing EEG data for brain_waves applet: {}", e);
            //        }
            //    }
        }
        ```
    *   **WebSocket Endpoint:** Implement the WebSocket server logic for the `/applet/brain_waves/data` path. This involves:
        *   Accepting WebSocket connections.
        *   Orchestrating the flow of EEG data to the `process_eeg_data` function.
        *   Sending processed results (or errors) to connected clients.

**Phase 3: Basic Kiosk UI for Testing (Placeholder - Post Backend)**

*   This phase will be addressed after the backend is functional.
*   It will involve creating a minimal React component in `applets/brain_waves/ui/` to connect to the WebSocket endpoint and display/log received data, primarily for testing and verification of the backend.

**Next Steps (After this plan is saved):**

1.  Begin implementation of Phase 1: Create the `dsp/brain_waves_fft` crate structure and initial `lib.rs` and `Cargo.toml`.
2.  Create the `applets/brain_waves/manifest.json`.
3.  Proceed to Phase 2: Integrate into the `daemon`.

This plan will be tracked in `todo/first_applet.md`.