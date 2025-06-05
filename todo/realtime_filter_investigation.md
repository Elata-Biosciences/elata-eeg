# Real-Time EEG Filter Investigation (ADS1299 & DSP)

Date: 2025-05-27

## 1. User Query Summary

The user inquired about the real-time filter functionality associated with the ADS1299 driver, specifically:
*   Whether the filter (options: Off, 60Hz, 50Hz) works.
*   What the default filter setting is.
*   Why changing the filter setting doesn't appear to have a UI impact.

## 2. Investigation Findings

### 2.1. Filter Existence and Functionality
*   **Filter Exists:** A real-time powerline filter (for 50Hz, 60Hz, or "off") is implemented in the system.
*   **Software DSP Filter:** This filter is **not** a hardware feature of the ADS1299 chip itself. It's a software-based DSP (Digital Signal Processing) filter applied by the `SignalProcessor` located in [`driver/src/dsp/filters.rs`](../driver/src/dsp/filters.rs:1).
*   **Configuration Mechanism:** The filter is configured via the `powerline_filter_hz: Option<u32>` field within the `AdcConfig` struct, defined in [`driver/src/board_drivers/types.rs:51`](../driver/src/board_drivers/types.rs:51).
    *   `Some(50)`: Enables a 50Hz notch filter.
    *   `Some(60)`: Enables a 60Hz notch filter.
    *   `None`: Disables the powerline notch filter.

### 2.2. Default Filter Setting
*   The **default setting for the powerline filter is 60Hz**.
    *   This is specified in the `Default` implementation for `AdcConfig` in [`driver/src/board_drivers/types.rs:65`](../driver/src/board_drivers/types.rs:65).
    *   This is also the value currently set in the project's root [`config.json:9`](../config.json:9) (`"powerline_filter_hz": 60`).

### 2.3. Reason for "No UI Impact" When Changing Dynamically
*   **Daemon Startup Configuration:** The `daemon` process correctly loads the `powerline_filter_hz` value from [`config.json`](../config.json:1) upon startup. This value is used to initialize the `AdcConfig` ([`daemon/src/main.rs:65`](../daemon/src/main.rs:65)), which in turn configures the `EegSystem` and the `SignalProcessor` with the specified filter.
    *   Therefore, changes made to [`config.json`](../config.json:1) *will* take effect if the daemon is restarted.
*   **Kiosk UI Display:** The Kiosk UI (specifically [`kiosk/src/components/EegConfig.tsx`](../kiosk/src/components/EegConfig.tsx:1)) correctly receives and displays the active filter configuration from the daemon via a WebSocket connection (`ws://${wsHost}:8080/config`).
*   **Missing Dynamic Update Link:** The primary reason for the lack of UI impact when attempting to change the filter *dynamically* (while the application is running) is that **the Kiosk UI currently does not implement functionality to send filter change commands back to the daemon.**
    *   Searches for WebSocket `send` operations related to configuration changes in the Kiosk components yielded no results.
    *   The `EegConfig.tsx` component is set up to receive and display configuration, but not to transmit changes for this particular setting.

## 3. System Flow Diagrams

### 3.1. Current Initialization Flow (Works at Startup)
```mermaid
graph TD
    A[config.json e.g., powerline_filter_hz: 60] --reads--> B(Daemon Startup);
    B --uses value to create--> C(AdcConfig { powerline_filter_hz: Some(60) });
    C --initializes--> D(Driver/EegSystem);
    D --initializes--> E(DSP SignalProcessor with 60Hz Notch Filter);
    E --filters incoming data--> F[Filtered EEG Data];
    F --sends to--> G(Kiosk UI via WebSocket);
    G --displays data & current config (shows 60Hz)--> H(User Sees 60Hz Filter Active);
```

### 3.2. Problem: Attempting Dynamic Change from UI (Missing Communication Link)
```mermaid
graph TD
    I(User tries to change filter in UI to 50Hz) --> J(Kiosk UI Component);
    J --X No command is sent to backend X--> K(Daemon);
    K --continues with its initial config--> L(Driver/EegSystem still using 60Hz filter);
    L --data is still filtered at 60Hz--> M[Filtered EEG Data (at 60Hz)];
    M --sends to--> J;
    J --displays data (still 60Hz filtered) & config (still shows 60Hz from daemon)--> I;
```

## 4. Proposed Plan for Enabling Dynamic UI Control

To enable users to change the powerline filter setting dynamically from the Kiosk UI, the following modifications are proposed:

### 4.1. Kiosk UI (e.g., `kiosk/src/components/`)
1.  **UI Element:** Ensure a UI element (e.g., dropdown, radio buttons) exists for selecting the desired filter (50Hz, 60Hz, Off).
2.  **Send Command:** When the user changes the selection, the UI component should send a command to the daemon via WebSocket. This command should specify the new desired `powerline_filter_hz` value (e.g., `{"command": "set_powerline_filter", "value": 50}` or `{"command": "set_powerline_filter", "value": null}` for "Off").
    *   This might involve using the existing WebSocket connection in [`kiosk/src/context/CommandWebSocketContext.tsx`](../kiosk/src/context/CommandWebSocketContext.tsx:1) or establishing a dedicated message type for configuration changes if appropriate.

### 4.2. Daemon (`daemon/src/`)
1.  **WebSocket Handler:** In [`daemon/src/server.rs`](../daemon/src/server.rs:1) (or a dedicated command handling module), add a handler for the new "set_powerline_filter" WebSocket command.
2.  **Update Active Configuration:**
    *   Parse the incoming command to extract the new filter value.
    *   Update the daemon's active `AdcConfig`. This might involve modifying the shared `Arc<Mutex<AdcConfig>>` that seems to be in use ([`daemon/src/main.rs:69`](../daemon/src/main.rs:69)).
3.  **Reconfigure `EegSystem`:**
    *   A mechanism to tell the running `EegSystem` to reconfigure itself with the updated `AdcConfig` is needed. This might involve:
        *   Adding a new method to `EegSystem` (e.g., `async fn reconfigure(&mut self, new_config: AdcConfig) -> Result<(), DriverError>`).
        *   This `reconfigure` method would then call `self.signal_processor.reset(...)` with the new filter settings.
4.  **Broadcast Updated Config:** After successfully applying the change, the daemon should broadcast the *entire updated* `AdcConfig` object to all connected Kiosk clients via the `/config` WebSocket endpoint. This ensures all UIs reflect the current state.

### 4.3. Driver (`driver/src/`)
1.  **`EegSystem::reconfigure` (New Method):**
    *   Implement the `reconfigure` method in [`driver/src/eeg_system/mod.rs`](../driver/src/eeg_system/mod.rs:1).
    *   This method should take the new `AdcConfig`.
    *   It should update its internal copy of the config.
    *   Crucially, it must call `self.signal_processor.reset(new_config.sample_rate, /* num_channels */, new_config.dsp_high_pass_cutoff_hz, new_config.dsp_low_pass_cutoff_hz, new_config.powerline_filter_hz)`. The number of channels might need to be fetched or passed correctly.
    *   Handle any potential errors during reconfiguration.

## 5. Next Steps
*   Discuss this plan.
*   If approved, proceed to implement the changes, likely starting with the daemon-side command handling and `EegSystem` reconfiguration, followed by the Kiosk UI changes.